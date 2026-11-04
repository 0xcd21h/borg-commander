//! Alerts that leave the machine as they happen. A copy that has arrived somewhere else is out
//! of reach of whoever takes this machine over later, and a heartbeat tells a service such as
//! healthchecks.io that the watcher still runs, so its silence is news too.
//!
//! Every message names this machine and carries a number one higher than the one before it to
//! the same place, with the SHA-256 of that earlier message, so whoever keeps them can tell when
//! one went missing or was changed on the way. The numbers, the outbox and the tokens all live
//! on this machine: someone who controls it can drop what has not left yet and carry the chain
//! on without a gap. Messages wait in an owner-only outbox in the state folder until the place
//! they go has taken them.
//!
//! What a message says and when Borg tries again are pure functions; `deliver` sends them.

use std::{
    collections::{BTreeMap, VecDeque},
    fmt, fs,
    io::Read,
    path::{Path, PathBuf},
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, Sender},
    },
    thread,
    time::{Duration, SystemTime},
};

use anyhow::Result;
use borg_core::{Alert, Severity, WATCH_OUTBOX, WatchRule, WatchSource};
use borg_i18n::tr;
use borg_rules::Level;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

mod deliver;

/// Messages the outbox holds before it gives up on the oldest.
const OUTBOX_LIMIT: usize = 500;
/// How long Borg keeps trying to send one message.
const GIVE_UP_AFTER: Duration = Duration::from_secs(48 * 60 * 60);
/// Waits after each failed try, the last one repeated.
const RETRY_WAITS: [Duration; 5] = [
    Duration::from_secs(5),
    Duration::from_secs(30),
    Duration::from_secs(2 * 60),
    Duration::from_secs(10 * 60),
    Duration::from_secs(30 * 60),
];
/// A message still waiting after this long means a place stopped taking them.
const LATE: Duration = Duration::from_secs(30 * 60);
/// Minutes between heartbeats unless the configuration says otherwise.
pub const HEARTBEAT_MINUTES: u64 = 5;
/// How long the sender waits for news when nothing is due.
const IDLE: Duration = Duration::from_secs(60);
/// Bytes of the outbox Borg reads.
const OUTBOX_BYTES: u64 = 16 * 1024 * 1024;
/// Connections a message names; the alert in the log keeps them all.
const CONNECTIONS_SENT: usize = 8;
/// Characters of a command line a message carries.
const COMMAND_CHARS: usize = 1000;
/// Rule matches a message names.
pub const MATCHES_SENT: usize = 8;
/// Why the configuration refuses an address. It stays in English, as the configuration's other
/// errors do, since the language is set in the same file.
pub const UNSAFE_ADDRESS: &str = "alerts go to https:// addresses, or to http:// on this machine itself";
pub const NO_TOPIC: &str = "an ntfy address ends with its topic, such as https://ntfy.sh/my-topic";

/// Where alerts go, from `[watch.notify]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// How messages name this machine; its host name otherwise.
    pub name: Option<String>,
    /// The least an alert's level must be for it to be sent.
    pub level: Level,
    pub telegram: Option<Telegram>,
    pub ntfy: Option<Ntfy>,
    pub webhook: Option<Webhook>,
    pub heartbeat: Option<Heartbeat>,
}

impl Default for Settings {
    fn default() -> Self {
        Self { name: None, level: Level::Warning, telegram: None, ntfy: None, webhook: None, heartbeat: None }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Telegram {
    pub token: String,
    pub chat: String,
}

/// A topic's whole address, such as `https://ntfy.sh/my-topic`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ntfy {
    pub url: String,
    pub token: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Webhook {
    pub url: String,
    /// The key each message is signed with.
    pub secret: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heartbeat {
    pub url: String,
    pub every: Duration,
}

impl Settings {
    /// The places messages go.
    pub fn channels(&self) -> Vec<Channel> {
        let set = [
            (self.telegram.is_some(), Channel::Telegram),
            (self.ntfy.is_some(), Channel::Ntfy),
            (self.webhook.is_some(), Channel::Webhook),
        ];
        set.into_iter().filter(|(on, _)| *on).map(|(_, channel)| channel).collect()
    }

    /// Whether anything leaves the machine at all.
    pub fn any(&self) -> bool {
        !self.channels().is_empty() || self.heartbeat.is_some()
    }

    /// Whether an alert of this level is sent.
    pub fn takes(&self, level: Level) -> bool {
        level <= self.level
    }
}

/// Whether an address may carry alerts: HTTPS anywhere, or plain HTTP to this machine itself.
pub fn safe_address(url: &str) -> bool {
    let Ok(uri) = url.parse::<ureq::http::Uri>() else { return false };
    let Some(host) = uri.host().filter(|host| !host.is_empty()) else { return false };
    match uri.scheme_str().map(str::to_ascii_lowercase).as_deref() {
        Some("https") => true,
        Some("http") => matches!(host, "localhost" | "127.0.0.1" | "::1" | "[::1]"),
        _ => false,
    }
}

/// An ntfy address split into its server, which takes JSON at its root, and its topic, the
/// last part of the path. None when the address names no topic or carries a query.
pub fn ntfy_parts(url: &str) -> Option<(&str, &str)> {
    let trimmed = url.trim_end_matches('/');
    let (_, rest) = trimmed.split_once("://")?;
    if rest.contains(['?', '#']) || !rest.contains('/') {
        return None;
    }
    let (server, topic) = trimmed.rsplit_once('/')?;
    (!topic.is_empty()).then_some((server, topic))
}

/// A place messages go.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Channel {
    Telegram,
    Ntfy,
    Webhook,
}

impl Channel {
    pub fn label(self) -> &'static str {
        match self {
            Self::Telegram => "Telegram",
            Self::Ntfy => "ntfy",
            Self::Webhook => tr!(notify_channel_webhook),
        }
    }
}

/// What the watcher has to say.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Event {
    Alert(Box<AlertReport>),
    /// What the examiner found in the program an alert is about. Only the webhook gets these.
    Evidence {
        alert: u64,
        level: Level,
        program_hash: Option<String>,
        matches: Vec<String>,
    },
    Started {
        source: WatchSource,
        freezing: bool,
    },
    /// Freezing began, after the learning period and its review.
    Armed,
    /// Sent by `borg service watch notify`, outside the chain.
    Test,
}

impl Event {
    fn goes_to(&self, channel: Channel) -> bool {
        !matches!(self, Self::Evidence { .. }) || channel == Channel::Webhook
    }

    /// What the webhook's event header says.
    fn kind(&self) -> &'static str {
        match self {
            Self::Alert(_) => "alert",
            Self::Evidence { .. } => "evidence",
            Self::Started { .. } => "started",
            Self::Armed => "armed",
            Self::Test => "test",
        }
    }

    fn priority(&self) -> u8 {
        match self {
            Self::Alert(alert) => match alert.level {
                Level::Alert => 5,
                Level::Warning => 4,
                Level::Notice => 3,
            },
            _ => 3,
        }
    }
}

/// One alert as a message carries it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlertReport {
    /// The alert's number in this machine's log.
    pub id: u64,
    pub rule: WatchRule,
    pub level: Level,
    /// The rule's name and why it fired, in the language Borg speaks here.
    pub title: String,
    pub reason: String,
    pub response: String,
    pub frozen: bool,
    pub pid: u32,
    pub name: String,
    pub program: Option<String>,
    pub command: String,
    pub user: String,
    pub uid: u32,
    pub parent: Option<String>,
    /// The command lines of the processes above it, nearest first.
    pub ancestors: Vec<String>,
    pub connections: Vec<String>,
}

impl AlertReport {
    pub fn of(alert: &Alert) -> Self {
        let process = &alert.process;
        Self {
            id: alert.id,
            rule: alert.rule(),
            level: alert.level(),
            title: alert.rule().label().to_owned(),
            reason: alert.finding.reason(),
            response: alert.response.label().to_owned(),
            frozen: alert.frozen(),
            pid: process.key.pid,
            name: process.name.clone(),
            program: process.program.as_ref().map(|program| program.to_string_lossy().into_owned()),
            command: shortened(&process.command_line()),
            user: process.user.clone(),
            uid: process.uid,
            parent: alert.parent().map(str::to_owned),
            ancestors: process.ancestors.iter().map(|ancestor| shortened(&ancestor.command)).collect(),
            connections: alert.evidence.connections.iter().take(CONNECTIONS_SENT).cloned().collect(),
        }
    }
}

fn shortened(text: &str) -> String {
    match text.char_indices().nth(COMMAND_CHARS) {
        Some((end, _)) => format!("{}…", &text[..end]),
        None => text.to_owned(),
    }
}

/// A message as it leaves the machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Envelope {
    pub host: String,
    /// One higher than the message before to the same place; the first is 1.
    pub seq: u64,
    /// Seconds since the Unix epoch.
    pub at: u64,
    /// The SHA-256 of the message before to the same place, in hex; empty for the first.
    pub previous: String,
    /// Messages Borg has given up on so far, for every place together.
    pub dropped: u64,
    pub event: Event,
}

/// The last message written to one place.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chain {
    pub written: u64,
    pub last: String,
}

/// A message on its way to one place.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Waiting {
    pub channel: Channel,
    pub seq: u64,
    /// The message exactly as it is sent, which the next message's `previous` is the digest of.
    pub body: String,
    pub failures: u32,
    pub due: SystemTime,
    pub since: SystemTime,
}

/// Why a place did not take a message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Refused {
    /// The HTTP status the place answered with, when an answer came.
    pub status: Option<u16>,
    /// What the place said, or why no answer came, with the place's secrets taken out.
    pub said: String,
}

impl Refused {
    pub fn said(said: impl Into<String>) -> Self {
        Self { status: None, said: said.into() }
    }
}

impl fmt::Display for Refused {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.status {
            Some(status) if self.said.is_empty() => write!(formatter, "HTTP {status}"),
            Some(status) => write!(formatter, "HTTP {status}: {}", self.said),
            None => formatter.write_str(&self.said),
        }
    }
}

/// Messages waiting to leave, and the chain each place has seen so far.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Outbox {
    pub chains: BTreeMap<Channel, Chain>,
    pub waiting: VecDeque<Waiting>,
    pub dropped: u64,
    /// What went wrong the last time a place refused a message, until it takes one again.
    #[serde(default)]
    pub errors: BTreeMap<Channel, Refused>,
}

impl Outbox {
    /// Writes an event into the chain of every place that takes it, to be sent from now on.
    pub fn add(&mut self, host: &str, event: &Event, channels: &[Channel], now: SystemTime) -> Result<()> {
        for &channel in channels.iter().filter(|channel| event.goes_to(**channel)) {
            let chain = self.chains.entry(channel).or_default();
            let envelope = Envelope {
                host: host.to_owned(),
                seq: chain.written + 1,
                at: seconds(now),
                previous: chain.last.clone(),
                dropped: self.dropped,
                event: event.clone(),
            };
            let body = serde_json::to_string(&envelope)?;
            *chain = Chain { written: envelope.seq, last: digest(body.as_bytes()) };
            self.waiting.push_back(Waiting { channel, seq: envelope.seq, body, failures: 0, due: now, since: now });
        }
        while self.waiting.len() > OUTBOX_LIMIT {
            self.waiting.pop_front();
            self.dropped += 1;
        }
        Ok(())
    }

    /// Forgets the messages and refusals of places the configuration no longer names. A place
    /// taken out on purpose is no reason for the heartbeat to fail.
    pub fn keep_only(&mut self, channels: &[Channel]) {
        self.waiting.retain(|waiting| channels.contains(&waiting.channel));
        self.errors.retain(|channel, _| channels.contains(channel));
    }

    /// The oldest message waiting for this place, which goes before any other.
    pub fn first(&self, channel: Channel) -> Option<usize> {
        self.waiting.iter().position(|waiting| waiting.channel == channel)
    }

    /// When the next try is due, if anything waits.
    pub fn next_due(&self, channels: &[Channel]) -> Option<SystemTime> {
        channels.iter().filter_map(|channel| self.first(*channel)).map(|index| self.waiting[index].due).min()
    }

    pub fn sent(&mut self, index: usize) {
        if let Some(waiting) = self.waiting.remove(index) {
            self.errors.remove(&waiting.channel);
        }
    }

    /// A place refused the message: it waits longer each time, until Borg gives up on it.
    pub fn failed(&mut self, index: usize, error: Refused, now: SystemTime) {
        let Some(waiting) = self.waiting.get_mut(index) else { return };
        self.errors.insert(waiting.channel, error);
        waiting.failures = waiting.failures.saturating_add(1);
        waiting.due = now + retry_wait(waiting.failures);
        if now.duration_since(waiting.since).is_ok_and(|age| age >= GIVE_UP_AFTER) {
            self.waiting.remove(index);
            self.dropped += 1;
        }
    }

    /// Whether a message has waited so long that a place must have stopped taking them.
    pub fn late(&self, now: SystemTime) -> bool {
        self.waiting.iter().any(|waiting| now.duration_since(waiting.since).is_ok_and(|age| age >= LATE))
    }
}

/// How long a message waits after its `failures`-th failed try.
pub fn retry_wait(failures: u32) -> Duration {
    let place = (failures.max(1) as usize - 1).min(RETRY_WAITS.len() - 1);
    RETRY_WAITS[place]
}

pub fn seconds(time: SystemTime) -> u64 {
    time.duration_since(SystemTime::UNIX_EPOCH).map_or(0, |since| since.as_secs())
}

pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// The webhook's signature header: when it was sent and the HMAC-SHA256 of that time, a dot
/// and the body, under the shared secret.
pub fn signature(secret: &str, at: u64, body: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC takes a key of any length");
    mac.update(format!("{at}.{body}").as_bytes());
    format!("t={at},v1={:x}", mac.finalize().into_bytes())
}

/// What a person reads: a title and the lines below it. The webhook gets the envelope itself.
pub fn text(envelope: &Envelope) -> Option<(String, String)> {
    let host = envelope.host.as_str();
    let (title, mut lines) = match &envelope.event {
        Event::Alert(alert) => {
            let level = Severity::from(alert.level).label();
            let title = tr!(notify_alert_title, host = host, level = level, rule = alert.title);
            let program = alert.program.as_deref().unwrap_or(&alert.name);
            let mut lines = vec![
                alert.reason.clone(),
                tr!(notify_alert_process, program = program, pid = alert.pid, user = alert.user),
            ];
            if let Some(parent) = &alert.parent {
                lines.push(tr!(notify_alert_parent, parent = parent));
            }
            lines.push(tr!(notify_alert_command, command = alert.command));
            lines.push(tr!(notify_alert_response, response = alert.response));
            (title, lines)
        }
        Event::Evidence { .. } => return None,
        Event::Started { source, freezing } => {
            let freezing = if *freezing { tr!(notify_started_freezing) } else { tr!(notify_started_waiting) };
            (
                tr!(notify_started_title, host = host),
                vec![tr!(notify_started, source = source.label()), freezing.into()],
            )
        }
        Event::Armed => (tr!(notify_armed_title, host = host), vec![tr!(notify_armed).into()]),
        Event::Test => (tr!(notify_test_title, host = host), vec![tr!(notify_test).into()]),
    };
    if envelope.seq > 0 {
        lines.push(format!("#{}", envelope.seq));
    }
    Some((title, lines.join("\n")))
}

/// How the watcher is doing, as the heartbeat reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Health {
    pub source: WatchSource,
    pub seen: u64,
    pub suppressed: u64,
    /// Whether the thread that hashes programs and runs the rules over them still runs.
    pub examiner: bool,
}

/// The heartbeat: whether all is well, and what it says. Something is wrong when the examiner
/// stopped, a place stopped taking messages, or Borg gave up on one since the last heartbeat.
/// A refusal shows only as its status: what a place answered stays on this machine, since the
/// heartbeat goes to a service of its own.
pub fn beat(
    host: &str,
    health: Option<&Health>,
    outbox: &Outbox,
    dropped_before: u64,
    now: SystemTime,
) -> (bool, String) {
    let Some(health) = health else { return (true, tr!(notify_beat_starting, host = host)) };
    let mut lines = vec![
        tr!(notify_beat_watching, host = host, source = health.source.label()),
        tr!(notify_beat_seen, seen = health.seen, suppressed = health.suppressed),
    ];
    if !health.examiner {
        lines.push(tr!(notify_beat_examiner).into());
    }
    if !outbox.waiting.is_empty() || outbox.dropped > 0 {
        lines.push(tr!(notify_beat_waiting, count = outbox.waiting.len(), dropped = outbox.dropped));
    }
    for (channel, refused) in &outbox.errors {
        let why =
            refused.status.map_or_else(|| tr!(notify_beat_no_answer).to_owned(), |status| format!("HTTP {status}"));
        lines.push(format!("{}: {why}", channel.label()));
    }
    let fine = health.examiner && !outbox.late(now) && outbox.dropped == dropped_before;
    (fine, lines.join("\n"))
}

/// Held by the one sender that runs: a watcher started again with new settings waits for the
/// old sender to finish, so the two never write the outbox over each other.
static SENDING: Mutex<()> = Mutex::new(());
/// The senders started so far. One that a newer sender replaced stops sending after the
/// message in hand, keeps whatever its watcher still hands it, and ends when that watcher does.
static STARTED: AtomicU64 = AtomicU64::new(0);

/// The watcher's line to the sender, which runs on a thread of its own so a slow place never
/// holds up the watcher.
pub struct Notifier {
    settings: Settings,
    messages: Sender<Message>,
}

enum Message {
    Event(Event),
    Health(Health),
}

impl Notifier {
    /// Starts sending, if the configuration names anywhere to send to.
    pub fn start(settings: Settings, host: String, state: PathBuf) -> Option<Self> {
        if !settings.any() {
            return None;
        }
        let (messages, received) = mpsc::channel();
        let sending = settings.clone();
        let generation = STARTED.fetch_add(1, Ordering::SeqCst) + 1;
        thread::Builder::new()
            .name("borg-notify".into())
            .spawn(move || run(&sending, &host, &state, &received, generation))
            .map_err(|error| eprintln!("borg watch: starting the sender: {error}"))
            .ok()?;
        Some(Self { settings, messages })
    }

    pub fn takes(&self, level: Level) -> bool {
        self.settings.takes(level)
    }

    pub fn send(&self, event: Event) {
        let _ = self.messages.send(Message::Event(event));
    }

    pub fn health(&self, health: Health) {
        let _ = self.messages.send(Message::Health(health));
    }
}

fn run(settings: &Settings, host: &str, state: &Path, received: &Receiver<Message>, generation: u64) {
    let _only = SENDING.lock().unwrap_or_else(PoisonError::into_inner);
    let current = || STARTED.load(Ordering::SeqCst) == generation;
    let channels = settings.channels();
    let agent = deliver::agent();
    let mut outbox = read_outbox(state);
    outbox.keep_only(&channels);
    let mut health = None;
    let mut next_beat = SystemTime::now();
    let mut dropped_before = outbox.dropped;
    loop {
        let now = SystemTime::now();
        let beat_due = settings.heartbeat.as_ref().filter(|_| current()).map(|_| next_beat);
        let due = outbox.next_due(&channels).filter(|_| current()).into_iter().chain(beat_due).min();
        let wait = due.map_or(IDLE, |due| due.duration_since(now).unwrap_or_default());
        let mut changed = false;
        match received.recv_timeout(wait) {
            Ok(message) => {
                for message in std::iter::once(message).chain(received.try_iter()) {
                    match message {
                        Message::Event(event) => changed |= outbox.add(host, &event, &channels, now).is_ok(),
                        Message::Health(fresh) => health = Some(fresh),
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        if changed {
            write_outbox(state, &outbox);
        }
        if send_due(settings, &agent, &channels, &mut outbox, &current) {
            write_outbox(state, &outbox);
        }
        let now = SystemTime::now();
        if let Some(heartbeat) = &settings.heartbeat
            && current()
            && now >= next_beat
        {
            let (fine, body) = beat(host, health.as_ref(), &outbox, dropped_before, now);
            if let Err(error) = deliver::heartbeat(&agent, &heartbeat.url, fine, &body) {
                eprintln!("borg watch: heartbeat: {error}");
            }
            dropped_before = outbox.dropped;
            next_beat = now + heartbeat.every;
        }
    }
}

/// Sends what is due, oldest first for each place, and stops at a place's first refusal so its
/// messages arrive in order, or as soon as `current` says a newer sender took over. Says whether
/// the outbox changed.
fn send_due(
    settings: &Settings,
    agent: &ureq::Agent,
    channels: &[Channel],
    outbox: &mut Outbox,
    current: &dyn Fn() -> bool,
) -> bool {
    let mut changed = false;
    for &channel in channels {
        while let Some(index) = outbox.first(channel) {
            let now = SystemTime::now();
            if !current() || outbox.waiting[index].due > now {
                break;
            }
            changed = true;
            match deliver::send(agent, settings, channel, &outbox.waiting[index].body) {
                Ok(()) => outbox.sent(index),
                Err(error) => {
                    outbox.failed(index, error, now);
                    break;
                }
            }
        }
    }
    changed
}

/// Sends a test message to every place at once, outside the chain, and says how each went.
pub fn test(settings: &Settings, host: &str) -> Vec<(String, Result<(), Refused>)> {
    let agent = deliver::agent();
    let envelope = Envelope {
        host: host.to_owned(),
        seq: 0,
        at: seconds(SystemTime::now()),
        previous: String::new(),
        dropped: 0,
        event: Event::Test,
    };
    let body = serde_json::to_string(&envelope).unwrap_or_default();
    let mut results: Vec<(String, Result<(), Refused>)> = settings
        .channels()
        .into_iter()
        .map(|channel| (channel.label().to_owned(), deliver::send(&agent, settings, channel, &body)))
        .collect();
    if let Some(heartbeat) = &settings.heartbeat {
        let (_, body) = beat(host, None, &Outbox::default(), 0, SystemTime::now());
        results.push((tr!(notify_heartbeat).to_owned(), deliver::heartbeat(&agent, &heartbeat.url, true, &body)));
    }
    results
}

pub fn outbox_path(state: &Path) -> PathBuf {
    state.join(WATCH_OUTBOX)
}

fn read_outbox(state: &Path) -> Outbox {
    let mut bytes = Vec::new();
    let read = fs::File::open(outbox_path(state)).and_then(|file| file.take(OUTBOX_BYTES).read_to_end(&mut bytes));
    read.ok().and_then(|_| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

fn write_outbox(state: &Path, outbox: &Outbox) {
    let written = serde_json::to_vec(outbox)
        .map_err(anyhow::Error::from)
        .and_then(|bytes| crate::store::write_privately(&outbox_path(state), &bytes));
    if let Err(error) = written {
        eprintln!("borg watch: writing the outbox: {error:#}");
    }
}

#[cfg(test)]
mod tests;
