//! Sending one message to one place over HTTPS, and the heartbeat. A place's secrets never
//! appear in what Borg says went wrong: its token, and the part of its address that works as a
//! key, such as an ntfy topic, a Discord or Slack webhook's path or a healthchecks.io check.

use std::time::{Duration, SystemTime};

use serde_json::json;
use ureq::tls::{RootCerts, TlsConfig};

use super::{Channel, Envelope, NO_TOPIC, Refused, Settings, ntfy_parts, seconds, signature, text};

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
pub fn send(agent: &ureq::Agent, settings: &Settings, channel: Channel, body: &str) -> Result<(), Refused> {
    send_through(agent, settings, channel, body, TELEGRAM)
}

/// Sends as [`send`] does, with Telegram's API at `telegram`.
pub fn send_through(
    agent: &ureq::Agent,
    settings: &Settings,
    channel: Channel,
    body: &str,
    telegram_api: &str,
) -> Result<(), Refused> {
    let envelope: Envelope = serde_json::from_str(body).map_err(|error| Refused::said(error.to_string()))?;
    let sent = match channel {
        Channel::Telegram => {
            let Some(telegram) = &settings.telegram else { return Ok(()) };
            let Some((title, lines)) = text(&envelope) else { return Ok(()) };
            let url = format!("{telegram_api}/bot{}/sendMessage", telegram.token);
            to_telegram(agent, &url, &telegram.chat, &format!("{title}\n{lines}"))
        }
        Channel::Ntfy => {
            let Some(ntfy) = &settings.ntfy else { return Ok(()) };
            let Some((title, lines)) = text(&envelope) else { return Ok(()) };
            to_ntfy(agent, &ntfy.url, ntfy.token.as_deref(), &title, &lines, envelope.event.priority())
        }
        Channel::Webhook => {
            let Some(webhook) = &settings.webhook else { return Ok(()) };
            to_webhook(agent, &webhook.url, webhook.secret.as_deref(), envelope.event.kind(), body)
        }
    };
    sent.map_err(|refused| hide(refused, &secrets(settings, channel)))
}

/// What must never show in an error about this place.
fn secrets(settings: &Settings, channel: Channel) -> Vec<&str> {
    let (token, url) = match channel {
        Channel::Telegram => (settings.telegram.as_ref().map(|telegram| telegram.token.as_str()), None),
        Channel::Ntfy => {
            let ntfy = settings.ntfy.as_ref();
            (ntfy.and_then(|ntfy| ntfy.token.as_deref()), ntfy.map(|ntfy| ntfy.url.as_str()))
        }
        Channel::Webhook => {
            let webhook = settings.webhook.as_ref();
            (webhook.and_then(|webhook| webhook.secret.as_deref()), webhook.map(|webhook| webhook.url.as_str()))
        }
    };
    token.into_iter().chain(url.map(private)).collect()
}

/// The part of an address after its host, which services such as Discord, Slack, ntfy and
/// healthchecks.io treat as the key.
fn private(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    rest.split_once('/').map_or("", |(_, path)| path.trim_end_matches('/'))
}

pub fn to_telegram(agent: &ureq::Agent, url: &str, chat: &str, message: &str) -> Result<(), Refused> {
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
) -> Result<(), Refused> {
    let (server, topic) = ntfy_parts(url).ok_or_else(|| Refused::said(NO_TOPIC))?;
    let body = json!({ "topic": topic, "title": title, "message": message, "priority": priority }).to_string();
    let request = agent.post(server).header("Content-Type", JSON);
    let request = match token {
        Some(token) => request.header("Authorization", format!("Bearer {token}")),
        None => request,
    };
    answer(request.send(body))
}

pub fn to_webhook(agent: &ureq::Agent, url: &str, secret: Option<&str>, kind: &str, body: &str) -> Result<(), Refused> {
    let request = agent.post(url).header("Content-Type", JSON).header(EVENT_HEADER, kind);
    let request = match secret {
        Some(secret) => request.header(SIGNATURE_HEADER, signature(secret, seconds(SystemTime::now()), body)),
        None => request,
    };
    answer(request.send(body))
}

/// Tells the heartbeat service the watcher runs, at the address with `/fail` added when
/// something is wrong.
pub fn heartbeat(agent: &ureq::Agent, url: &str, fine: bool, body: &str) -> Result<(), Refused> {
    let address = if fine { url.to_owned() } else { format!("{}{FAIL_SUFFIX}", url.trim_end_matches('/')) };
    answer(agent.post(&address).header("Content-Type", TEXT).send(body))
        .map_err(|refused| hide(refused, &[private(url)]))
}

fn answer(sent: Result<ureq::http::Response<ureq::Body>, ureq::Error>) -> Result<(), Refused> {
    let mut response = sent.map_err(|error| Refused::said(error.to_string()))?;
    let status = response.status();
    if status.is_success() {
        return Ok(());
    }
    let said = response.body_mut().with_config().limit(ANSWER_BYTES).read_to_string().unwrap_or_default();
    let said: String = said.chars().take(ANSWER_CHARS).collect();
    Err(Refused { status: Some(status.as_u16()), said: said.trim().to_owned() })
}

/// What went wrong, with the secrets taken out, the longest first so a short one inside a longer
/// one cannot break it up before it is found.
fn hide(mut refused: Refused, secrets: &[&str]) -> Refused {
    let mut secrets: Vec<&str> = secrets.iter().copied().filter(|secret| !secret.is_empty()).collect();
    secrets.sort_by_key(|secret| std::cmp::Reverse(secret.len()));
    for secret in secrets {
        refused.said = refused.said.replace(secret, HIDDEN);
    }
    refused
}
