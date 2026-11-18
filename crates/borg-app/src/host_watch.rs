//! Asks every paired host how it is doing while the hosts screen is open: every few seconds, a
//! few hosts at a time, over one channel per host kept open between rounds. Remembers when each
//! host last answered, so an offline host's card can say since when.

use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Result, bail};
use borg_core::{HOSTS_EVERY, HostHeard, HostReply, Msg, PairedHost, RemoteHostInfo};
use futures::{StreamExt, stream};
use serde_json::json;
use tokio::{
    sync::{Notify, mpsc::UnboundedSender},
    task::JoinHandle,
};

use crate::{
    agent::{Call, Reply},
    link,
    remote::HostInfo,
    session::StateDir,
};

/// How long a host has to answer before its card says it is not answering.
const ANSWER_WAIT: Duration = Duration::from_secs(5);
/// How many hosts are asked at once.
const AT_ONCE: usize = 8;
/// How often the times hosts last answered are written down while the screen stays open.
const SAVE_EVERY: Duration = Duration::from_secs(60);
/// When each paired host last answered, in the state folder.
const SEEN_FILE: &str = "hosts-seen.json";

pub struct HostWatch {
    messages: UnboundedSender<Msg>,
    task: Option<JoinHandle<()>>,
    refresh: Arc<Notify>,
}

impl HostWatch {
    pub fn new(messages: UnboundedSender<Msg>) -> Self {
        Self { messages, task: None, refresh: Arc::default() }
    }

    /// Starts asking the hosts, or stops.
    pub fn watch(&mut self, on: bool) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
        if on {
            self.refresh = Arc::default();
            self.task = Some(tokio::spawn(poll(self.messages.clone(), self.refresh.clone())));
        }
    }

    /// Asks the hosts again now, or as soon as the round under way ends.
    pub fn refresh(&self) {
        if self.task.is_some() {
            self.refresh.notify_one();
        }
    }
}

impl Drop for HostWatch {
    fn drop(&mut self) {
        self.watch(false);
    }
}

async fn poll(messages: UnboundedSender<Msg>, refresh: Arc<Notify>) {
    let Some(state) = StateDir::locate() else {
        let _ = messages.send(Msg::HostsListed(Ok(Vec::new())));
        return;
    };
    let mut seen = Seen::load(state.path());
    let mut kept: HashMap<String, link::Remote> = HashMap::new();
    loop {
        let servers = link::servers(state.path());
        seen.keep(&servers);
        let hosts = servers
            .iter()
            .map(|server| PairedHost {
                identity: server.key.clone(),
                machine: server.name.clone(),
                relay: server.relay.clone(),
                last_seen: seen.get(&server.key),
            })
            .collect();
        if messages.send(Msg::HostsListed(Ok(hosts))).is_err() {
            return;
        }
        let jobs: Vec<_> = servers
            .into_iter()
            .map(|server| {
                let remote = kept.remove(&server.key);
                (server, remote)
            })
            .collect();
        kept.clear();
        let mut asked = stream::iter(jobs.into_iter().map(|(server, remote)| ask(state.path(), server, remote)))
            .buffer_unordered(AT_ONCE);
        while let Some((identity, heard, remote)) = asked.next().await {
            if let HostHeard::Answered(reply) = &heard {
                seen.mark(&identity, reply.seen);
            }
            if let Some(remote) = remote {
                kept.insert(identity.clone(), remote);
            }
            if messages.send(Msg::HostAnswered { identity, answer: heard }).is_err() {
                return;
            }
        }
        drop(asked);
        seen.save_when_due();
        tokio::select! {
            () = tokio::time::sleep(HOSTS_EVERY) => {}
            () = refresh.notified() => {}
        }
    }
}

/// One host's answer, and the channel to ask it again over.
async fn ask(
    state: &Path,
    server: link::Server,
    kept: Option<link::Remote>,
) -> (String, HostHeard, Option<link::Remote>) {
    let result = tokio::time::timeout(ANSWER_WAIT, answer(state, &server, kept)).await;
    let (heard, remote) = match result {
        Err(_) => (HostHeard::Silent, None),
        Ok(Err(error)) if offline(&error) => (HostHeard::Offline, None),
        Ok(Err(error)) => (HostHeard::Failed(format!("{error:#}")), None),
        Ok(Ok((reply, remote))) => (HostHeard::Answered(Box::new(reply)), Some(remote)),
    };
    (server.key, heard, remote)
}

/// Asks over the kept channel, and over a new one when the kept one is gone.
async fn answer(state: &Path, server: &link::Server, kept: Option<link::Remote>) -> Result<(HostReply, link::Remote)> {
    let call = Call::new("host_info", json!({ link::OVERVIEW_ARG: true }));
    if let Some(mut remote) = kept {
        let start = Instant::now();
        if let Ok(reply) = remote.call(&call).await {
            return Ok((read(server, &reply, start.elapsed())?, remote));
        }
    }
    let mut remote = link::Remote::open(state, server).await?;
    let start = Instant::now();
    let reply = remote.call(&call).await?;
    Ok((read(server, &reply, start.elapsed())?, remote))
}

fn read(server: &link::Server, reply: &Reply, took: Duration) -> Result<HostReply> {
    if !reply.ok {
        bail!("{}", reply.text);
    }
    let host: HostInfo = serde_json::from_str(&reply.text)?;
    let mut info = RemoteHostInfo {
        relay: server.relay.clone(),
        latency_ms: Some(u64::try_from(took.as_millis()).unwrap_or(u64::MAX)),
        ..Default::default()
    };
    host.fill(&mut info);
    let figures = host.figures.clone();
    let sessions = crate::remote::session_choices(server, &info, host.sessions);
    Ok(HostReply { info, sessions, figures, seen: SystemTime::now() })
}

/// Whether the relay said the host is not connected to it.
fn offline(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| cause.downcast_ref::<borg_link::relay::Offline>().is_some())
}

/// When each paired host last answered, by its key, in seconds since 1970.
struct Seen {
    path: PathBuf,
    times: BTreeMap<String, u64>,
    saved: Instant,
    changed: bool,
}

impl Seen {
    fn load(state: &Path) -> Self {
        let path = state.join(SEEN_FILE);
        let times = std::fs::read(&path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
        Self { path, times, saved: Instant::now(), changed: false }
    }

    fn get(&self, key: &str) -> Option<SystemTime> {
        self.times.get(key).map(|seconds| UNIX_EPOCH + Duration::from_secs(*seconds))
    }

    fn mark(&mut self, key: &str, at: SystemTime) {
        let seconds = at.duration_since(UNIX_EPOCH).map(|since| since.as_secs()).unwrap_or_default();
        self.times.insert(key.to_owned(), seconds);
        self.changed = true;
    }

    /// Forgets the hosts no longer paired.
    fn keep(&mut self, servers: &[link::Server]) {
        let before = self.times.len();
        self.times.retain(|key, _| servers.iter().any(|server| &server.key == key));
        self.changed |= self.times.len() != before;
    }

    fn save_when_due(&mut self) {
        if self.saved.elapsed() >= SAVE_EVERY {
            self.save();
        }
    }

    fn save(&mut self) {
        if !self.changed {
            return;
        }
        let Ok(bytes) = serde_json::to_vec_pretty(&self.times) else { return };
        if crate::store::write_privately(&self.path, &bytes).is_ok() {
            self.changed = false;
            self.saved = Instant::now();
        }
    }
}

impl Drop for Seen {
    fn drop(&mut self) {
        self.save();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server(key: &str) -> link::Server {
        link::Server {
            name: key.to_owned(),
            relay: "wss://relay.example".into(),
            key: key.to_owned(),
            computer: None,
            offered: None,
        }
    }

    #[test]
    fn seen_times_are_kept_for_paired_hosts_only() {
        let dir = tempfile::tempdir().unwrap();
        let at = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        {
            let mut seen = Seen::load(dir.path());
            seen.mark("a", at);
            seen.mark("b", at);
            seen.keep(&[server("a")]);
        }
        let seen = Seen::load(dir.path());
        assert_eq!(seen.get("a"), Some(at));
        assert_eq!(seen.get("b"), None);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.path().join(SEEN_FILE)).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn a_relay_that_says_offline_marks_the_host_offline() {
        let error = anyhow::Error::from(borg_link::relay::Offline).context("borg on web-01 is not reachable");
        assert!(offline(&error));
        assert!(!offline(&anyhow::anyhow!("the server closed the connection")));
    }

    #[test]
    fn a_reply_with_figures_carries_them_and_the_sessions() {
        let figures = borg_core::HostFigures { cores: 4, cpu: Some(3_100), ..Default::default() };
        let text = json!({
            "version": "0.12.0",
            "os": "linux",
            "arch": "x86_64",
            "service": true,
            "sessions": [{"id": "s1", "name": "main", "terminal": false}],
            "overview": figures,
        })
        .to_string();
        let reply = Reply::ok(text);
        let read = read(&server("k"), &reply, Duration::from_millis(38)).unwrap();
        assert_eq!(read.figures, Some(figures));
        assert_eq!(read.info.version.as_deref(), Some("0.12.0"));
        assert_eq!(read.info.platform.as_deref(), Some("linux / x86_64"));
        assert_eq!(read.info.latency_ms, Some(38));
        assert_eq!(read.sessions.len(), 1);
        assert_eq!(read.sessions[0].session.as_deref(), Some("s1"));
    }

    #[test]
    fn an_older_host_answers_without_figures() {
        let text = json!({"version": "0.11.4", "os": "linux", "arch": "aarch64", "service": false, "sessions": []})
            .to_string();
        let reply = Reply::ok(text);
        let read = read(&server("k"), &reply, Duration::ZERO).unwrap();
        assert_eq!(read.figures, None);
        assert!(read.sessions.is_empty());
    }
}
