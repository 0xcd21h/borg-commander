//! The service itself: it keeps a PTY running for each named session, answers the calls that
//! paired computers make through borg cloud, and serves the terminal clients that attach.

use std::{
    collections::{HashSet, hash_map::Entry},
    fs,
    ops::RangeInclusive,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    sync::{Arc, atomic::AtomicUsize},
    time::Duration,
};

use anyhow::{Context, Result};
use borg_core::Msg;
use borg_term::{GridSize, PtyEvent};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
    sync::{
        mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel},
        oneshot,
    },
};

use super::{
    Child, Children, Controller, LOCK, REPLAY_LIMIT, attach, frame_when_ready, mirror, service_runtime, service_socket,
    spawn_child,
};
use crate::{
    agent::{Call, Reply, Session},
    config::{Config, ConfigDir},
    launch,
    link::LinkServer,
    remote_console,
    session::StateDir,
    sessions,
};

/// How often the service reads its settings again and starts the sessions' PTYs.
const REFRESH_EVERY: Duration = Duration::from_secs(2);
/// How long a client has to say what it wants.
const REQUEST_WAIT: Duration = Duration::from_secs(2);
/// The screen sizes a client may ask for.
const COLUMNS: RangeInclusive<u64> = 20..=500;
const ROWS: RangeInclusive<u64> = 8..=200;

pub(super) async fn run() -> Result<()> {
    let (mut service, mut inbox) = Service::start()?;
    let mut tick = tokio::time::interval(REFRESH_EVERY);
    loop {
        tokio::select! {
            _ = tick.tick() => service.refresh()?,
            Some((id, event)) = inbox.events.recv() => service.pty_event(&id, event),
            Some((call, reply)) = inbox.calls.recv() => service.answer(call, reply),
            Some(id) = inbox.removed.recv() => {
                service.removing.remove(&id);
            }
            Ok((stream, _)) = inbox.listener.accept() => service.serve(stream).await?,
            Some(_notice) = inbox.notices.recv() => {}
        }
    }
}

/// What the service keeps while it runs.
struct Service {
    state: StateDir,
    runtime: PathBuf,
    config: Config,
    cloud: LinkServer,
    consoles: remote_console::Server,
    events: UnboundedSender<(String, PtyEvent)>,
    children: Children,
    /// Sessions being removed, whose PTYs must not start again meanwhile.
    removing: HashSet<String>,
    removed: UnboundedSender<String>,
    /// Held while the service runs, so that only one does.
    _lock: fs::File,
}

/// Where the service's work comes from: its clients, borg cloud, the PTYs and the removals.
struct Inbox {
    listener: UnixListener,
    calls: UnboundedReceiver<(Call, oneshot::Sender<Reply>)>,
    notices: UnboundedReceiver<Msg>,
    events: UnboundedReceiver<(String, PtyEvent)>,
    removed: UnboundedReceiver<String>,
}

impl Service {
    /// Takes the service's lock and its socket, and starts the link to borg cloud.
    fn start() -> Result<(Self, Inbox)> {
        let state = StateDir::locate().context("set HOME or BORG_STATE_DIR for the service")?;
        fs::create_dir_all(state.path())?;
        let runtime = launch::private_dir(&service_runtime()?)?;
        let lock =
            fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(state.path().join(LOCK))?;
        lock.try_lock().context("a Borg service is already running")?;
        let socket = service_socket();
        let _ = fs::remove_file(&socket);
        let listener = UnixListener::bind(&socket)?;
        fs::set_permissions(&socket, fs::Permissions::from_mode(0o600))?;
        let config = ConfigDir::locate().map(|dir| dir.load()).transpose()?.unwrap_or_default();
        let (calls_tx, calls) = unbounded_channel();
        let (messages, notices) = unbounded_channel();
        let mut cloud = LinkServer::new(
            Some(state.path().to_path_buf()),
            runtime.clone(),
            None,
            relay(&config),
            calls_tx,
            messages,
            Arc::new(AtomicUsize::new(0)),
        );
        cloud.set_background_service();
        let consoles = remote_console::Server::new(borg_term::user_shell(), runtime.clone());
        let (events, events_rx) = unbounded_channel();
        let (removed, removed_rx) = unbounded_channel();
        let service = Self {
            state,
            runtime,
            config,
            cloud,
            consoles,
            events,
            children: Children::new(),
            removing: HashSet::new(),
            removed,
            _lock: lock,
        };
        Ok((service, Inbox { listener, calls, notices, events: events_rx, removed: removed_rx }))
    }

    /// Follows a change of relay in the settings, keeps the link to borg cloud as the plugins
    /// say, and keeps a PTY running for each named session that is not archived.
    fn refresh(&mut self) -> Result<()> {
        if let Ok(Some(current)) = ConfigDir::locate().map(|dir| dir.load()).transpose() {
            if current.relay != self.config.relay {
                self.cloud.set_relay(relay(&current));
            }
            self.config = current;
        }
        self.cloud.sync(self.config.plugins.as_ref().is_none_or(|plugins| plugins.agent_cloud));
        let named: Vec<_> =
            sessions::ensure_default(&self.state)?.into_iter().filter(|session| !session.archived).collect();
        let active: HashSet<_> = named.iter().map(|session| session.id.as_str()).collect();
        self.children.retain(|id, _| active.contains(id.as_str()));
        for session in named {
            if self.removing.contains(&session.id) {
                continue;
            }
            if let Entry::Vacant(entry) = self.children.entry(session.id) {
                match spawn_child(entry.key(), self.events.clone()) {
                    Ok(child) => {
                        entry.insert(child);
                    }
                    Err(error) => eprintln!("borg service: could not start {}: {error:#}", session.name),
                }
            }
        }
        Ok(())
    }

    /// A session's PTY wrote, or closed.
    fn pty_event(&mut self, id: &str, event: PtyEvent) {
        match event {
            PtyEvent::Output(bytes) => {
                if let Some(child) = self.children.get(id) {
                    feed(&mut child.lock().expect("child lock"), bytes);
                }
            }
            PtyEvent::Closed => {
                if let Some(child) = self.children.remove(id) {
                    let _ = child.lock().expect("child lock").output.send(Vec::new());
                }
            }
        }
    }

    /// A call from borg cloud: removing a session, a console's, a mirror's, or else one for the
    /// named session opened last.
    fn answer(&mut self, call: Call, reply: oneshot::Sender<Reply>) {
        if call.tool == "session_delete" {
            self.delete_session(call, reply);
        } else if call.tool.starts_with("console_") {
            self.consoles.handle(call, reply);
        } else if call.tool == "mirror_frame"
            && let Some(child) = call.args["session"].as_str().and_then(|id| self.children.get(id)).cloned()
        {
            tokio::spawn(async move {
                let _ = reply.send(frame_when_ready(call, child).await);
            });
        } else if call.tool.starts_with("mirror_") {
            let _ = reply.send(mirror(&call, &self.children));
        } else {
            tokio::spawn(answer_in_session(self.state.path().to_path_buf(), call, reply));
        }
    }

    /// Removes a session other than the Default one: its PTY stops, and it may start again only
    /// once the session is gone.
    fn delete_session(&mut self, call: Call, reply: oneshot::Sender<Reply>) {
        let id = call.args["id"].as_str().unwrap_or_default().to_owned();
        let exists = sessions::list(&self.state).is_ok_and(|sessions| sessions.iter().any(|session| session.id == id));
        if id == "default" || !exists {
            let _ =
                reply.send(Reply::error("The Default session is protected, or the requested session no longer exists"));
            return;
        }
        if !self.removing.insert(id.clone()) {
            let _ = reply.send(Reply::error("This session is already being removed"));
            return;
        }
        let child = self.children.remove(&id);
        let removed = self.removed.clone();
        let state = self.state.clone();
        let runtime = self.runtime.clone();
        let consoles = self.consoles.clone();
        tokio::spawn(async move {
            let result = async {
                if let Some(child) = child {
                    tokio::task::spawn_blocking(move || child.lock().expect("child lock").pty.stop()).await??;
                }
                sessions::delete_remote(&state, &runtime, &id, None).await
            }
            .await;
            let answer = match result {
                Ok(()) => {
                    consoles.close_session(&id);
                    Reply::ok("Session removed")
                }
                Err(error) => Reply::error(format!("{error:#}")),
            };
            let _ = reply.send(answer);
            let _ = removed.send(id);
        });
    }

    /// A terminal client's request, a line of JSON: to attach to a session, to pair, which
    /// sessions run, or a new size for a session's screen.
    async fn serve(&mut self, stream: UnixStream) -> Result<()> {
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        if tokio::time::timeout(REQUEST_WAIT, reader.read_line(&mut line)).await.is_err() {
            return Ok(());
        }
        let request: Value = serde_json::from_str(&line).unwrap_or(Value::Null);
        let mut stream = reader.into_inner();
        match request["mode"].as_str() {
            Some("attach") => self.attach_client(stream, &request).await,
            Some("pair") => {
                let reply = self
                    .cloud
                    .pair()
                    .map(|link| json!({"ok": true, "link": link}))
                    .unwrap_or_else(|error| json!({"ok": false, "error": format!("{error:#}")}));
                stream.write_all(format!("{reply}\n").as_bytes()).await?;
            }
            Some("status") => {
                let reply = json!({"ok": true, "sessions": self.children.keys().collect::<Vec<_>>()});
                stream.write_all(format!("{reply}\n").as_bytes()).await?;
            }
            Some("resize") => {
                let ok = self.resize(&request);
                stream.write_all(format!("{}\n", json!({"ok": ok})).as_bytes()).await?;
            }
            _ => {
                let _ = stream.write_all(b"Unknown service request\n").await;
            }
        }
        Ok(())
    }

    /// Attaches a client to a session, starting the PTY of a named session that is not running.
    async fn attach_client(&mut self, mut stream: UnixStream, request: &Value) {
        let id = request["session"].as_str().unwrap_or_default();
        if !self.children.contains_key(id)
            && !self.removing.contains(id)
            && sessions::list(&self.state)
                .is_ok_and(|all| all.iter().any(|session| session.id == id && !session.archived))
            && let Ok(child) = spawn_child(id, self.events.clone())
        {
            self.children.insert(id.to_owned(), child);
        }
        if let Some(child) = self.children.get(id) {
            tokio::spawn(attach(stream, child.clone(), request["take"] == true));
        } else {
            let _ = stream.write_all(b"Session is offline\n").await;
        }
    }

    /// Resizes a session's screen when a client here controls it and the size is sensible.
    fn resize(&self, request: &Value) -> bool {
        let id = request["session"].as_str().unwrap_or_default();
        let cols = request["cols"].as_u64().unwrap_or(0);
        let rows = request["rows"].as_u64().unwrap_or(0);
        let Some(child) = self.children.get(id).filter(|_| COLUMNS.contains(&cols) && ROWS.contains(&rows)) else {
            return false;
        };
        let mut child = child.lock().expect("child lock");
        let size = GridSize { cols: cols as u16, rows: rows as u16 };
        if matches!(child.controller, Some(Controller::Local(_))) && child.pty.resize(size).is_ok() {
            child.emulator.resize(size);
            true
        } else {
            false
        }
    }
}

fn relay(config: &Config) -> Option<borg_link::Relay> {
    config.relay.as_deref().or(crate::link::DEFAULT_RELAY).and_then(borg_link::Relay::parse)
}

/// A session's output: its screen takes it and may answer the program, the latest of it stays
/// for clients that attach later, and the clients attached get it.
fn feed(child: &mut Child, bytes: Vec<u8>) {
    child.emulator.feed(&bytes);
    for answer in child.emulator.drain_replies() {
        let _ = child.pty.write(answer.as_bytes());
    }
    child.sequence = child.sequence.saturating_add(bytes.len() as u64);
    child.replay.extend(&bytes);
    while child.replay.len() > REPLAY_LIMIT {
        child.replay.pop_front();
    }
    let _ = child.output.send(bytes);
}

/// Answers a call in the named session opened last, through that session's Borg.
async fn answer_in_session(state: PathBuf, call: Call, reply: oneshot::Sender<Reply>) {
    let registry = StateDir::at(state);
    let target = sessions::list(&registry)
        .ok()
        .and_then(|all| all.into_iter().filter(|session| !session.archived).max_by_key(|session| session.last_opened));
    let answer = match target {
        Some(session) => {
            match Session::open(&service_runtime().unwrap_or_else(|_| launch::runtime_dir()), Some(&session.id)).await {
                Some(mut open) => open.call(&call).await.unwrap_or_else(|| Reply::error("session disconnected")),
                None => Reply::error("no running Borg session is ready"),
            }
        }
        None => Reply::error("no Borg session exists"),
    };
    let _ = reply.send(answer);
}
