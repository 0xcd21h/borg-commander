//! Sending one message to one place over HTTPS, and the heartbeat. A place's secret never
//! appears in what Borg says went wrong.

use std::time::{Duration, SystemTime};

use serde_json::json;
use ureq::tls::{RootCerts, TlsConfig};

use super::{Channel, Envelope, Settings, seconds, signature, text};

const TIMEOUT: Duration = Duration::from_secs(10);
const TELEGRAM: &str = "https://api.telegram.org";
const USER_AGENT: &str = concat!("borg-commander/", env!("CARGO_PKG_VERSION"));
const JSON: &str = "application/json";
const TEXT: &str = "text/plain; charset=utf-8";
const SIGNATURE_HEADER: &str = "X-Borg-Signature";
const EVENT_HEADER: &str = "X-Borg-Event";
/// What healthchecks.io and services like it take as "something is wrong".
const FAIL_SUFFIX: &str = "/fail";
/// Characters of a refusal Borg keeps, enough for the reason a place gives.
const ANSWER_CHARS: usize = 200;
/// Bytes of an answer Borg reads.
const ANSWER_BYTES: u64 = 4 * 1024;
const HIDDEN: &str = "…";

pub fn agent() -> ureq::Agent {
    let tls = TlsConfig::builder().root_certs(RootCerts::PlatformVerifier).build();
    ureq::Agent::config_builder()
        .tls_config(tls)
        .timeout_global(Some(TIMEOUT))
        .http_status_as_error(false)
        .user_agent(USER_AGENT)
        .build()
        .new_agent()
}

/// Sends one message, given as the envelope's exact body, to one place.
pub fn send(agent: &ureq::Agent, settings: &Settings, channel: Channel, body: &str) -> Result<(), String> {
    send_through(agent, settings, channel, body, TELEGRAM)
}

/// Sends as [`send`] does, with Telegram's API at `telegram`.
pub fn send_through(
    agent: &ureq::Agent,
    settings: &Settings,
    channel: Channel,
    body: &str,
    telegram_api: &str,
) -> Result<(), String> {
    let envelope: Envelope = serde_json::from_str(body).map_err(|error| error.to_string())?;
    match channel {
        Channel::Telegram => {
            let Some(telegram) = &settings.telegram else { return Ok(()) };
            let Some((title, lines)) = text(&envelope) else { return Ok(()) };
            let url = format!("{telegram_api}/bot{}/sendMessage", telegram.token);
            to_telegram(agent, &url, &telegram.chat, &format!("{title}\n{lines}"))
                .map_err(|error| hide(&error, &telegram.token))
        }
        Channel::Ntfy => {
            let Some(ntfy) = &settings.ntfy else { return Ok(()) };
            let Some((title, lines)) = text(&envelope) else { return Ok(()) };
            to_ntfy(agent, &ntfy.url, ntfy.token.as_deref(), &title, &lines, envelope.event.priority())
                .map_err(|error| ntfy.token.as_deref().map_or(error.clone(), |token| hide(&error, token)))
        }
        Channel::Webhook => {
            let Some(webhook) = &settings.webhook else { return Ok(()) };
            to_webhook(agent, &webhook.url, webhook.secret.as_deref(), envelope.event.kind(), body)
        }
    }
}

pub fn to_telegram(agent: &ureq::Agent, url: &str, chat: &str, message: &str) -> Result<(), String> {
    let body = json!({ "chat_id": chat, "text": message, "disable_web_page_preview": true }).to_string();
    answer(agent.post(url).header("Content-Type", JSON).send(body))
}

/// Publishes through ntfy's JSON form, which carries any text in the title: the topic is the
/// last part of the address and the server the rest.
pub fn to_ntfy(
    agent: &ureq::Agent,
    url: &str,
    token: Option<&str>,
    title: &str,
    message: &str,
    priority: u8,
) -> Result<(), String> {
    let (server, topic) = url.trim_end_matches('/').rsplit_once('/').ok_or_else(|| url.to_owned())?;
    let body = json!({ "topic": topic, "title": title, "message": message, "priority": priority }).to_string();
    let request = agent.post(server).header("Content-Type", JSON);
    let request = match token {
        Some(token) => request.header("Authorization", format!("Bearer {token}")),
        None => request,
    };
    answer(request.send(body))
}

pub fn to_webhook(agent: &ureq::Agent, url: &str, secret: Option<&str>, kind: &str, body: &str) -> Result<(), String> {
    let request = agent.post(url).header("Content-Type", JSON).header(EVENT_HEADER, kind);
    let request = match secret {
        Some(secret) => request.header(SIGNATURE_HEADER, signature(secret, seconds(SystemTime::now()), body)),
        None => request,
    };
    answer(request.send(body))
}

/// Tells the heartbeat service the watcher runs, at the address with `/fail` added when
/// something is wrong.
pub fn heartbeat(agent: &ureq::Agent, url: &str, fine: bool, body: &str) -> Result<(), String> {
    let url = if fine { url.to_owned() } else { format!("{}{FAIL_SUFFIX}", url.trim_end_matches('/')) };
    answer(agent.post(&url).header("Content-Type", TEXT).send(body))
}

fn answer(sent: Result<ureq::http::Response<ureq::Body>, ureq::Error>) -> Result<(), String> {
    let mut response = sent.map_err(|error| error.to_string())?;
    let status = response.status();
    if status.is_success() {
        return Ok(());
    }
    let said = response.body_mut().with_config().limit(ANSWER_BYTES).read_to_string().unwrap_or_default();
    let said: String = said.chars().take(ANSWER_CHARS).collect();
    Err(format!("HTTP {}: {}", status.as_u16(), said.trim()))
}

/// What went wrong, with the secret taken out.
fn hide(error: &str, secret: &str) -> String {
    if secret.is_empty() { error.to_owned() } else { error.replace(secret, HIDDEN) }
}
