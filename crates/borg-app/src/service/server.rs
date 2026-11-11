//! The service itself: it keeps a PTY running for each named session, answers the calls that
//! paired computers make through borg cloud, and serves the terminal clients that attach.

use std::{
    collections::{HashSet, hash_map::Entry},
    fs,
    ops::RangeInclusive,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    sync::{Arc, atomic::AtomicUsize},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, ensure};
use borg_core::{AgentEvent, Msg};
use borg_term::{GridSize, Pty, PtyEvent};
use rustix::process::{Pid, WaitOptions, waitpid};
use serde::{Deserialize, Serialize};
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
    CHILD_SCROLLBACK, Child, Children, Controller, LOCK, ON_DEMAND, REPLAY_LIMIT, RUN, SERVICE_COMMAND, START_CALL,
    adopt_child, attach,
    carry::{self, Handed, Terminal},
    forward, frame_when_ready, guard,
    members::{self, Members, Request},
    mirror, service_runtime, service_socket, spawn_child,
    wire::{Place, REGISTER, Registration, ToMember},
};
use crate::{
    agent::{Call, Reply},
    config::{Config, ConfigDir},
    launch,
    link::{HostUpdating, LinkServer},
    relaunch::Carried,
    remote_console,
    session::StateDir,
    sessions,
};

/// How often the service reads its settings again and starts the sessions' PTYs.
const REFRESH_EVERY: Duration = Duration::from_secs(2);
/// How long a client has to say what it wants.
const REQUEST_WAIT: Duration = Duration::from_secs(2);
/// How long a service Borg started for itself waits, with no session left, before it ends.
const IDLE_GRACE: Duration = Duration::from_secs(30);
/// The request a kept service makes of one started on demand, to take its place.
const HAND_OVER: &str = "hand-over";
/// The request to start again on the newer Borg an update put in the program file.
pub(super) const RELAUNCH: &str = "relaunch";
/// How long the answer to an update has to reach the paired computer before the service moves.
const ANSWER_GRACE: Duration = Duration::from_secs(2);
/// How long a kept service waits for one started on demand to make way.
const HAND_OVER_WAIT: Duration = Duration::from_secs(5);
const HAND_OVER_POLL: Duration = Duration::from_millis(100);
/// The screen sizes a client may ask for.
const COLUMNS: RangeInclusive<u64> = 20..=500;
const PRIVATE_SOCKET: u32 = 0o600;
const ROWS: RangeInclusive<u64> = 8..=200;

/// How long the service runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Lifetime {
    /// Installed, or run by a supervisor: it runs until stopped.
    Kept,
    /// Started by a Borg: it ends once no session has used it for a while, and outlives the
    /// terminal it was started from.
    OnDemand,
}

/// Runs the service, on what the previous service carried when an update started this one in
/// its place.
pub(super) async fn run(lifetime: Lifetime, carried: Option<Carried>) -> Result<()> {
    let state = StateDir::locate().context("set HOME or BORG_STATE_DIR for the service")?;
    let handed = carried
        .and_then(|carried| carry::unpack(carried).inspect_err(|error| eprintln!("borg service: {error:#}")).ok());
    let (lock, terminals) = match handed {
        Some(handed) => {
            collect(handed.guard);
            (fs::File::from(handed.lock), handed.terminals)
        }
        None => (take_lock(&state, lifetime).await?, Vec::new()),
    };
    let (mut service, mut inbox) = Service::start(state, lock, lifetime)?;
    service.adopt(terminals);
    let mut tick = tokio::time::interval(REFRESH_EVERY);
    let mut idle = Idle::default();
    loop {
        tokio::select! {
            _ = tick.tick() => {
                service.refresh()?;
                if lifetime == Lifetime::OnDemand && idle.over(service.in_use(), Instant::now()) {
                    return Ok(());
                }
            }
            Some((id, event)) = inbox.events.recv() => service.pty_event(&id, event),
            Some((call, reply)) = inbox.calls.recv() => service.answer(call, reply),
            Some(id) = inbox.removed.recv() => {
                service.removing.remove(&id);
            }
            Ok((stream, _)) = inbox.listener.accept() => if crate::agent::same_user(&stream) {
                let _ = service.serve(stream).await;
                if service.handed_over {
                    return Ok(());
                }
            },
            Some(version) = inbox.relaunches.recv() => {
                let error = service.move_to(&version);
                eprintln!("borg service: {error:#}");
            }
            Some((session, request)) = inbox.requests.recv() => service.request(&session, request),
            Some(notice) = inbox.notices.recv() => service.notice(notice),
        }
    }
}

/// The lock only one service holds at a time. A kept service asks one started on demand to
/// make way, and waits for it to go.
async fn take_lock(state: &StateDir, lifetime: Lifetime) -> Result<fs::File> {
    fs::create_dir_all(state.path())?;
    let lock =
        fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(state.path().join(LOCK))?;
    if lock.try_lock().is_ok() {
        return Ok(lock);
    }
    ensure!(lifetime == Lifetime::Kept && hand_over().await, "a Borg service is already running");
    let deadline = Instant::now() + HAND_OVER_WAIT;
    while Instant::now() < deadline {
        tokio::time::sleep(HAND_OVER_POLL).await;
        if lock.try_lock().is_ok() {
            return Ok(lock);
        }
    }
    anyhow::bail!("the Borg service started on demand did not make way")
}

/// Asks the running service to make way, which one started on demand does.
async fn hand_over() -> bool {
    let reply = tokio::time::timeout(REQUEST_WAIT, super::request_json(json!({ "mode": HAND_OVER }))).await;
    matches!(reply, Ok(Ok(reply)) if reply["ok"] == true)
}

/// Collects the guard of the move that started this service, once it ends.
fn collect(guard: Option<u32>) {
    let Some(guard) = guard.and_then(|guard| i32::try_from(guard).ok()).and_then(Pid::from_raw) else { return };
    std::thread::spawn(move || waitpid(Some(guard), WaitOptions::empty()));
}

/// Installs the latest release when it is newer than the Borg in the program file, and says
/// which version the service runs next.
async fn install_latest() -> Result<HostUpdating, String> {
    let current = crate::relaunch::installed_version().await.unwrap_or_else(|| crate::update::VERSION.to_owned());
    tokio::task::spawn_blocking(move || crate::update::install_newer(&current))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| format!("{error:#}"))?;
    let installed = crate::relaunch::installed_version().await;
    Ok(match newer(installed.as_deref(), crate::update::VERSION) {
        Ok(version) => HostUpdating { version, moving: true },
        Err(_) => HostUpdating { version: crate::update::VERSION.to_owned(), moving: false },
    })
}

/// The version the service moves to: the one in the program file, when it is newer than the
/// one running.
pub(super) fn newer(installed: Option<&str>, running: &str) -> Result<String, String> {
    let installed = installed.ok_or_else(|| borg_i18n::tr!(relaunch_unknown_version).to_owned())?;
    match (installed.parse::<borg_update::Version>(), running.parse::<borg_update::Version>()) {
        (Ok(new), Ok(old)) if new > old => Ok(installed.to_owned()),
        _ => Err(borg_i18n::tr!(relaunch_not_newer, version = running)),
    }
}

/// What the newer Borg runs with, to run as this service does.
fn run_args(lifetime: Lifetime) -> Vec<std::ffi::OsString> {
    let on_demand = (lifetime == Lifetime::OnDemand).then_some(ON_DEMAND);
    [SERVICE_COMMAND, RUN].into_iter().chain(on_demand).map(Into::into).collect()
}

/// Since when no session has used the service.
#[derive(Default)]
struct Idle(Option<Instant>);

impl Idle {
    /// Whether the service has had no session for the whole grace period.
    fn over(&mut self, in_use: bool, now: Instant) -> bool {
        if in_use {
            self.0 = None;
            return false;
        }
        now.duration_since(*self.0.get_or_insert(now)) >= IDLE_GRACE
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
    /// The process watcher, while the settings ask for it.
    watcher: Option<crate::watch::Handle>,
    /// The Borgs registered with the service, one for each running session.
    members: Members,
    requests: UnboundedSender<(String, Request)>,
    /// The session whose Borg asked for the pairing link shown last, which approves it.
    pairing: Option<String>,
    lifetime: Lifetime,
    /// A kept service took over, so this one, started on demand, ends.
    handed_over: bool,
    /// Where a request to start again on a newer Borg goes, which the service does between
    /// requests.
    relaunches: UnboundedSender<String>,
    /// Held while the service runs, so that only one does.
    lock: fs::File,
}

/// Where the service's work comes from: its clients, borg cloud, the PTYs and the removals.
struct Inbox {
    listener: UnixListener,
    calls: UnboundedReceiver<(Call, oneshot::Sender<Reply>)>,
    notices: UnboundedReceiver<Msg>,
    events: UnboundedReceiver<(String, PtyEvent)>,
    removed: UnboundedReceiver<String>,
    requests: UnboundedReceiver<(String, Request)>,
    relaunches: UnboundedReceiver<String>,
}

impl Service {
    /// Opens the service's socket, with its lock held, and starts the link to borg cloud.
    fn start(state: StateDir, lock: fs::File, lifetime: Lifetime) -> Result<(Self, Inbox)> {
        let runtime = launch::private_dir(&service_runtime()?)?;
        let listener = listen()?;
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
        let members = Members::default();
        cloud.set_members(members.clone());
        let consoles = remote_console::Server::new(
            borg_term::user_shell(),
            crate::program::path()?.to_path_buf(),
            runtime.clone(),
        );
        let (events, events_rx) = unbounded_channel();
        let (removed, removed_rx) = unbounded_channel();
        let (requests, requests_rx) = unbounded_channel();
        let (relaunches, relaunches_rx) = unbounded_channel();
        let watcher = crate::watch::start(config.watch());
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
            watcher,
            members,
            requests,
            pairing: None,
            lifetime,
            handed_over: false,
            relaunches,
            lock,
        };
        let inbox = Inbox {
            listener,
            calls,
            notices,
            events: events_rx,
            removed: removed_rx,
            requests: requests_rx,
            relaunches: relaunches_rx,
        };
        Ok((service, inbox))
    }

    /// Starts the newer Borg in place of this service, carrying its lock and the sessions'
    /// terminals, with a guard to come back if the newer Borg does not come up. The newer Borg
    /// opens the service's socket again, and Borgs registered with it register again. It
    /// returns only when the start failed, and the service then runs on as before.
    fn move_to(&mut self, version: &str) -> anyhow::Error {
        let mut guard = guard::start(version, self.lifetime, self.cloud.reachable());
        let error = match self.hand(guard.as_ref().map(std::process::Child::id)) {
            Ok(carried) => {
                let Err(error) = crate::relaunch::exec(&run_args(self.lifetime), &carried);
                match carry::unpack(carried) {
                    Ok(handed) => self.adopt(handed.terminals),
                    Err(error) => eprintln!("borg service: {error:#}"),
                }
                error
            }
            Err(error) => error,
        };
        if let Some(mut guard) = guard.take() {
            let _ = guard.kill();
            std::thread::spawn(move || guard.wait());
        }
        error
    }

    /// The service taken apart for the move: its lock, and each session's terminal, which
    /// stops being read here.
    fn hand(&mut self, guard: Option<u32>) -> Result<Carried> {
        let lock = self.lock.try_clone()?.into();
        let children: Vec<_> = self.children.iter().map(|(id, child)| (id.clone(), child.clone())).collect();
        let mut terminals = Vec::new();
        for (session, child) in children {
            let mut child = child.lock().expect("child lock");
            let screen = child.emulator.carry_ansi(CHILD_SCROLLBACK);
            let size = GridSize { cols: child.emulator.screen().cols(), rows: child.emulator.screen().rows() };
            let sequence = child.sequence;
            match child.pty.detach() {
                Ok(detached) => terminals.push(Terminal { session, detached, size, sequence, screen }),
                Err(error) => {
                    drop(child);
                    self.adopt(terminals);
                    return Err(error);
                }
            }
        }
        carry::pack(Handed { lock, guard, terminals })
    }

    /// Takes over sessions' terminals: into the sessions that run here, whose terminals were
    /// handed off for a move that failed, or as new sessions the previous service ran.
    fn adopt(&mut self, terminals: Vec<Terminal>) {
        for Terminal { session, detached, size, sequence, screen } in terminals {
            let adopted = match self.children.get(&session) {
                Some(child) => Pty::adopt(detached, forward(&session, self.events.clone())).map(|pty| {
                    child.lock().expect("child lock").pty = pty;
                }),
                None => adopt_child(&session, detached, size, &screen, sequence, self.events.clone()).map(|child| {
                    self.children.insert(session.clone(), child);
                }),
            };
            if let Err(error) = adopted {
                self.children.remove(&session);
                eprintln!("borg service: could not take over session {session}: {error:#}");
            }
        }
    }

    /// Follows a change of relay in the settings, keeps the link to borg cloud as the plugins
    /// say, and keeps a PTY running for each named session it keeps in the background.
    fn refresh(&mut self) -> Result<()> {
        self.reload();
        self.sync_cloud();
        let kept: Vec<_> = sessions::ensure_default(&self.state)?
            .into_iter()
            .filter(|session| session.background && !session.archived)
            .collect();
        let active: HashSet<_> = kept.iter().map(|session| session.id.as_str()).collect();
        self.children.retain(|id, _| active.contains(id.as_str()));
        for session in kept {
            if self.removing.contains(&session.id) || sessions::held(&self.state, &session.id) {
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

    /// Whether a session runs here or is registered.
    fn in_use(&self) -> bool {
        !self.children.is_empty() || !self.members.sessions().is_empty()
    }

    /// Reads the settings again, following a change of relay or watcher.
    fn reload(&mut self) {
        if let Ok(Some(current)) = ConfigDir::locate().map(|dir| dir.load()).transpose() {
            if current.relay != self.config.relay {
                self.cloud.set_relay(relay(&current));
            }
            if current.watch() != self.config.watch() {
                self.watcher = crate::watch::start(current.watch());
            }
            self.config = current;
        }
    }

    fn sync_cloud(&mut self) {
        self.cloud.sync(self.config.plugins.as_ref().is_none_or(|plugins| plugins.agent_cloud));
    }

    /// Keeps a session running in the background from now on, starting its Borg unless it runs
    /// here already. A session open in a terminal stays there.
    fn keep_running(&mut self, id: &str) -> Result<()> {
        if self.children.contains_key(id) {
            return Ok(());
        }
        ensure!(!self.removing.contains(id), "session {id} is being removed");
        ensure!(!sessions::held(&self.state, id), "session {id} is open in a terminal");
        let session = sessions::keep(&self.state, id, true)?;
        ensure!(!session.archived, "session {} is archived", session.name);
        let child = spawn_child(&session.id, self.events.clone())?;
        self.children.insert(session.id, child);
        Ok(())
    }

    /// A registered Borg's request: a pairing link, which its session then approves, or
    /// forgetting every paired computer.
    fn request(&mut self, session: &str, request: Request) {
        match request {
            Request::Pair => {
                self.reload();
                let link = self.cloud.pair().map_err(|error| format!("{error:#}"));
                if link.is_ok() {
                    self.pairing = Some(session.to_owned());
                }
                self.members.send(session, ToMember::Link { link });
                self.sync_cloud();
            }
            Request::Forget => {
                let _ = self.cloud.forget();
                self.members.broadcast(|| ToMember::Computers { names: Vec::new() });
            }
        }
    }

    /// News from borg cloud: a new pairing reaches every registered Borg.
    fn notice(&mut self, notice: Msg) {
        if let Msg::Agent(AgentEvent::Paired { computers }) = notice {
            self.members.broadcast(|| ToMember::Computers { names: computers.clone() });
        }
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
                    if !self.removing.contains(id) {
                        let _ = sessions::keep(&self.state, id, false);
                    }
                }
            }
        }
    }

    /// A call from borg cloud: removing a session, starting one, a console's, a mirror's, or
    /// else one for the session the user worked in last.
    fn answer(&mut self, call: Call, reply: oneshot::Sender<Reply>) {
        if call.tool == START_CALL {
            let id = call.args["session"].as_str().unwrap_or_default().to_owned();
            let _ = reply.send(match self.keep_running(&id) {
                Ok(()) => Reply::ok("Session running in the background"),
                Err(error) => Reply::error(format!("{error:#}")),
            });
        } else if call.tool == "session_delete" {
            self.delete_session(call, reply);
        } else if call.tool == crate::link::HOST_UPDATE_CALL {
            self.update_host(reply);
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
            let target = if call.tool == crate::link::PAIR_CALL { self.pairing.take() } else { None };
            let members = self.members.clone();
            tokio::spawn(async move {
                let answer = match target.filter(|id| members.place(id).is_some()).or_else(|| members.last_used()) {
                    Some(session) => members.ask(&session, call).await,
                    None => Reply::error("no Borg session is running"),
                };
                let _ = reply.send(answer);
            });
        }
    }

    /// Updates this host to the latest Borg for a paired computer. The answer says which
    /// version the service moves to, and the service moves once the answer is on its way.
    fn update_host(&self, reply: oneshot::Sender<Reply>) {
        let relaunches = self.relaunches.clone();
        tokio::spawn(async move {
            match install_latest().await {
                Ok(update) => {
                    let _ = reply.send(Reply::ok(json!(update).to_string()));
                    if update.moving {
                        tokio::time::sleep(ANSWER_GRACE).await;
                        let _ = relaunches.send(update.version);
                    }
                }
                Err(error) => {
                    let _ = reply.send(Reply::error(error));
                }
            }
        });
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
        let terminal = matches!(self.members.place(&id), Some((Place::Terminal, _)));
        let members = self.members.clone();
        let removed = self.removed.clone();
        let state = self.state.clone();
        let runtime = self.runtime.clone();
        let consoles = self.consoles.clone();
        tokio::spawn(async move {
            let result = async {
                if let Some(child) = child {
                    tokio::task::spawn_blocking(move || child.lock().expect("child lock").pty.stop()).await??;
                }
                if terminal {
                    sessions::archive(&state, &id, true)?;
                    let stopped = members.ask(&id, Call::new("session_stop", json!({ "id": id }))).await;
                    if !stopped.ok {
                        sessions::archive(&state, &id, false)?;
                        anyhow::bail!("{}", stopped.text);
                    }
                }
                let removed = sessions::delete_remote(&state, &runtime, &id, None).await;
                if removed.is_err() && terminal {
                    sessions::archive(&state, &id, false)?;
                }
                removed
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

    /// A local client's request, a line of JSON: a Borg registering its session, to attach to
    /// a session or keep one running, to pair, which sessions run, or a new size for a
    /// session's screen.
    async fn serve(&mut self, stream: UnixStream) -> Result<()> {
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        if tokio::time::timeout(REQUEST_WAIT, reader.read_line(&mut line)).await.is_err() {
            return Ok(());
        }
        let request: Value = serde_json::from_str(&line).unwrap_or(Value::Null);
        let mut stream = reader.into_inner();
        match request["mode"].as_str() {
            Some(REGISTER) => match serde_json::from_value::<Registration>(request) {
                Ok(registration) => {
                    tokio::spawn(members::serve(
                        stream,
                        registration,
                        self.members.clone(),
                        self.cloud.computers(),
                        self.requests.clone(),
                    ));
                }
                Err(error) => {
                    let _ = stream.write_all(format!("{error}\n").as_bytes()).await;
                }
            },
            Some("attach") => self.attach_client(stream, &request).await,
            Some("keep") => {
                let id = request["session"].as_str().unwrap_or_default();
                let reply = match self.keep_running(id) {
                    Ok(()) => json!({"ok": true}),
                    Err(error) => json!({"ok": false, "error": format!("{error:#}")}),
                };
                stream.write_all(format!("{reply}\n").as_bytes()).await?;
            }
            Some("pair") => {
                let reply = self
                    .cloud
                    .pair()
                    .map(|link| json!({"ok": true, "link": link}))
                    .unwrap_or_else(|error| json!({"ok": false, "error": format!("{error:#}")}));
                stream.write_all(format!("{reply}\n").as_bytes()).await?;
            }
            Some("status") => {
                let terminals: Vec<String> = self
                    .members
                    .sessions()
                    .into_iter()
                    .filter_map(|(id, place)| (place == Place::Terminal).then_some(id))
                    .collect();
                let reply = json!({
                    "ok": true,
                    "sessions": self.children.keys().collect::<Vec<_>>(),
                    "terminals": terminals,
                    "kept": self.lifetime == Lifetime::Kept,
                    "version": crate::update::VERSION,
                    "reachable": self.cloud.reachable(),
                });
                stream.write_all(format!("{reply}\n").as_bytes()).await?;
            }
            Some(HAND_OVER) => {
                self.handed_over = self.lifetime == Lifetime::OnDemand;
                let reply = if self.handed_over {
                    json!({"ok": true})
                } else {
                    json!({"ok": false, "error": "this Borg service keeps running"})
                };
                stream.write_all(format!("{reply}\n").as_bytes()).await?;
            }
            Some(RELAUNCH) => {
                let reply = match newer(crate::relaunch::installed_version().await.as_deref(), crate::update::VERSION) {
                    Ok(version) => {
                        let _ = self.relaunches.send(version.clone());
                        json!({"ok": true, "version": version})
                    }
                    Err(error) => json!({"ok": false, "error": error}),
                };
                stream.write_all(format!("{reply}\n").as_bytes()).await?;
            }
            Some("resize") => {
                let ok = self.resize(&request);
                stream.write_all(format!("{}\n", json!({"ok": ok})).as_bytes()).await?;
            }
            Some("watch-act") => {
                let reply = self.watch_act(&request).await;
                stream.write_all(format!("{reply}\n").as_bytes()).await?;
            }
            _ => {
                let _ = stream.write_all(b"Unknown service request\n").await;
            }
        }
        Ok(())
    }

    /// Attaches a client to a session, keeping it running in the background first unless it
    /// runs here already.
    async fn attach_client(&mut self, mut stream: UnixStream, request: &Value) {
        let id = request["session"].as_str().unwrap_or_default();
        match self.keep_running(id) {
            Ok(()) => {
                if let Some(child) = self.children.get(id) {
                    tokio::spawn(attach(stream, child.clone(), request["take"] == true));
                }
            }
            Err(error) => {
                let _ = stream.write_all(format!("{error:#}\r\n").as_bytes()).await;
            }
        }
    }

    /// Stops, continues or ends a process for a Borg that may not signal it itself. Only the
    /// watcher does it, and only to a process it is watching.
    async fn watch_act(&self, request: &Value) -> Value {
        let Some(watcher) = &self.watcher else { return json!({"ok": false}) };
        let act = serde_json::from_value(request["act"].clone());
        let (Ok(act), Some(pid), Some(start)) = (act, request["pid"].as_u64(), request["start"].as_u64()) else {
            return json!({"ok": false});
        };
        let process = borg_core::ProcessKey { pid: pid as u32, start };
        match watcher.act(act, process).await {
            Some(outcome) => json!({"ok": true, "outcome": outcome}),
            None => json!({"ok": false}),
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

/// The service's own socket, open only to its user.
fn listen() -> Result<UnixListener> {
    let socket = service_socket();
    let _ = fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket)?;
    fs::set_permissions(&socket, fs::Permissions::from_mode(PRIVATE_SOCKET))?;
    Ok(listener)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_service_moves_only_to_a_newer_borg_in_its_program_file() {
        assert_eq!(newer(Some("0.12.0"), "0.11.0"), Ok("0.12.0".into()));
        assert!(newer(Some("0.11.0"), "0.11.0").is_err(), "the same version runs already");
        assert!(newer(Some("0.10.0"), "0.11.0").is_err(), "an older program file is no update");
        assert!(newer(None, "0.11.0").is_err(), "a program file that does not answer is no update");
    }

    #[test]
    fn the_newer_borg_runs_as_this_service_does() {
        assert_eq!(run_args(Lifetime::Kept), ["service", "run"]);
        assert_eq!(run_args(Lifetime::OnDemand), ["service", "run", "--on-demand"]);
    }

    #[test]
    fn a_service_started_on_demand_ends_after_a_grace_period_with_no_session() {
        let start = Instant::now();
        let mut idle = Idle::default();
        assert!(!idle.over(false, start), "the grace period starts with the first idle look");
        assert!(!idle.over(false, start + IDLE_GRACE / 2));
        assert!(!idle.over(true, start + IDLE_GRACE), "a session using it starts the wait over");
        assert!(!idle.over(false, start + IDLE_GRACE * 3 / 2));
        assert!(idle.over(false, start + IDLE_GRACE * 5 / 2));
    }
}
