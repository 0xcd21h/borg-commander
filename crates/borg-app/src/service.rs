//! An optional machine-wide Borg process. It keeps named Borg workspaces and their PTYs alive
//! without a terminal, and lets terminal clients attach to those same workspaces.

use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::PathBuf,
    sync::{Arc, Mutex},
};

use anyhow::{Context, Result, bail};
use base64::Engine as _;
use borg_term::{Emulator, GridSize, Pty, PtyEvent};
use crossterm::{
    event::{DisableBracketedPaste, DisableMouseCapture},
    execute,
    terminal::LeaveAlternateScreen,
};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
    sync::{
        broadcast,
        mpsc::{UnboundedSender, unbounded_channel},
    },
};

use crate::{
    agent::{Call, Reply},
    launch,
    session::StateDir,
    sessions,
};

mod carry;
mod follow;
mod guard;
mod member;
mod members;
mod server;
mod wire;

pub use follow::after_update;
pub use guard::{GUARD, run as guard};
pub use member::{Inbox, Membership};
pub use members::{Lease, Members};
pub use wire::Place;

const SOCKET: &str = "service.sock";
/// Set for the Borg the service runs for a session, which opens that session without counting
/// as a use of it and leaves borg cloud to the service.
pub const SERVICE_CHILD: &str = "BORG_SERVICE_CHILD";
/// The service's own call that keeps a session running in the background.
pub const START_CALL: &str = "_service_start";
const LOCK: &str = "service.lock";
const REPLAY_LIMIT: usize = 1024 * 1024;
/// The screen a session in the background starts with, until a client resizes it.
const CHILD_SIZE: GridSize = GridSize { cols: 100, rows: 32 };
const CHILD_SCROLLBACK: usize = 2000;
/// How many pieces of a session's output wait for a client that reads slowly.
const CHILD_OUTPUT_BACKLOG: usize = 256;
/// How much of a terminal's input or output moves at once.
const CHUNK: usize = 8192;
/// The key that leaves an attached session: Ctrl-].
const DETACH: u8 = 0x1d;
/// How long the service has to say which sessions it keeps running.
const STATUS_WAIT: std::time::Duration = std::time::Duration::from_secs(1);
/// The subcommand of the service, and its action that runs it.
const SERVICE_COMMAND: &str = "service";
const RUN: &str = "run";
/// Marks a service that Borg started for itself, which ends once no session uses it.
const ON_DEMAND: &str = "--on-demand";
/// How long a Borg waits for the service it started to answer.
const START_WAIT: std::time::Duration = std::time::Duration::from_secs(2);
const START_POLL: std::time::Duration = std::time::Duration::from_millis(50);

struct Child {
    pty: Pty,
    emulator: Emulator,
    replay: VecDeque<u8>,
    output: broadcast::Sender<Vec<u8>>,
    controller: Option<Controller>,
    next_client: u64,
    sequence: u64,
}

#[derive(Clone, PartialEq, Eq)]
enum Controller {
    Local(u64),
    Remote(String),
}

type Children = HashMap<String, Arc<Mutex<Child>>>;

fn mirror_payload(emulator: &Emulator, replay: &VecDeque<u8>, sequence: u64, after: u64) -> (bool, Vec<u8>) {
    let start = sequence.saturating_sub(replay.len() as u64);
    let reset = after == 0 || after < start || after > sequence || sequence.saturating_sub(after) > 64 * 1024;
    let bytes =
        if reset { emulator.snapshot_ansi() } else { replay.iter().skip((after - start) as usize).copied().collect() };
    (reset, bytes)
}

fn service_socket() -> PathBuf {
    StateDir::locate().map_or_else(|| launch::runtime_dir().join(SOCKET), |state| state.path().join(SOCKET))
}

/// The background service's runtime folder, inside the state folder.
pub(crate) const SERVICE_RUNTIME: &str = "runtime";

fn service_runtime() -> Result<PathBuf> {
    Ok(StateDir::locate().context("set HOME or BORG_STATE_DIR")?.path().join(SERVICE_RUNTIME))
}

pub fn runtime_dir() -> Result<PathBuf> {
    service_runtime()
}

fn spawn_child(id: &str, events: UnboundedSender<(String, PtyEvent)>) -> Result<Arc<Mutex<Child>>> {
    let program = crate::program::path()?;
    let mut command = std::process::Command::new(program);
    command.args(["--session", id]);
    command.env(SERVICE_CHILD, "1");
    command.env("BORG_RUNTIME_DIR", service_runtime()?);
    command.env("TERM", "xterm-256color");
    let pty = Pty::spawn(program, &command, CHILD_SIZE, forward(id, events))?;
    Ok(child(pty, Emulator::new(CHILD_SIZE, CHILD_SCROLLBACK), 0))
}

/// A session's terminal the previous service carried, with the screen it showed.
fn adopt_child(
    id: &str,
    detached: borg_term::Detached,
    size: GridSize,
    screen: &[u8],
    sequence: u64,
    events: UnboundedSender<(String, PtyEvent)>,
) -> Result<Arc<Mutex<Child>>> {
    let pty = Pty::adopt(detached, forward(id, events))?;
    let mut emulator = Emulator::new(size, CHILD_SCROLLBACK);
    emulator.feed(screen);
    Ok(child(pty, emulator, sequence))
}

/// Where a session's terminal reports, marked with the session.
fn forward(id: &str, events: UnboundedSender<(String, PtyEvent)>) -> UnboundedSender<PtyEvent> {
    let (sender, mut receiver) = unbounded_channel();
    let id = id.to_owned();
    tokio::spawn(async move {
        while let Some(event) = receiver.recv().await {
            if events.send((id.clone(), event)).is_err() {
                break;
            }
        }
    });
    sender
}

fn child(pty: Pty, emulator: Emulator, sequence: u64) -> Arc<Mutex<Child>> {
    let (output, _) = broadcast::channel(CHILD_OUTPUT_BACKLOG);
    Arc::new(Mutex::new(Child {
        pty,
        emulator,
        replay: VecDeque::new(),
        output,
        controller: None,
        next_client: 0,
        sequence,
    }))
}

async fn attach(mut stream: UnixStream, child: Arc<Mutex<Child>>, take: bool) -> Result<()> {
    let (replay, mut output, client, controls) = {
        let mut child = child.lock().expect("child lock");
        child.next_client += 1;
        let client = child.next_client;
        if child.controller.is_none() || take {
            child.controller = Some(Controller::Local(client));
        }
        (
            child.emulator.export_ansi(32),
            child.output.subscribe(),
            client,
            child.controller == Some(Controller::Local(client)),
        )
    };
    stream
        .write_all(if controls {
            b"\x1b]0;Borg session (controller)\x07"
        } else {
            b"\x1b]0;Borg session (view only)\x07"
        })
        .await?;
    stream.write_all(b"\x1b[?1049h\x1b[2J\x1b[H").await?;
    stream.write_all(&replay).await?;
    let (mut read, mut write) = stream.into_split();
    let writing = tokio::spawn(async move {
        while let Ok(bytes) = output.recv().await {
            if bytes.is_empty() {
                break;
            }
            if write.write_all(&bytes).await.is_err() {
                break;
            }
        }
    });
    let mut bytes = [0_u8; CHUNK];
    while let Ok(count) = read.read(&mut bytes).await {
        if count == 0 {
            break;
        }
        if !controls {
            continue;
        }
        let mut child = child.lock().expect("child lock");
        if child.controller == Some(Controller::Local(client)) {
            let _ = child.pty.write(&bytes[..count]);
        }
    }
    writing.abort();
    let mut child = child.lock().expect("child lock");
    if child.controller == Some(Controller::Local(client)) {
        child.controller = None;
    }
    Ok(())
}

fn mirror(call: &Call, children: &Children) -> Reply {
    let Some(id) = call.args["session"].as_str() else { return Reply::error("mirror needs a session ID") };
    let Some(child) = children.get(id) else { return Reply::error(format!("session {id} is offline")) };
    mirror_child(call, child)
}

fn mirror_child(call: &Call, child: &Arc<Mutex<Child>>) -> Reply {
    let Some(origin) = call.origin.as_deref() else { return Reply::error("mirror requires a paired computer") };
    let mut child = child.lock().expect("child lock");
    let owner = Controller::Remote(origin.to_owned());
    match call.tool.as_str() {
        "mirror_view" => Reply::ok("Viewing this session"),
        "mirror_frame" => {
            let after = call.args["after"].as_u64().unwrap_or(0);
            let (reset, bytes) = mirror_payload(&child.emulator, &child.replay, child.sequence, after);
            Reply::ok(
                json!({
                    "sequence": child.sequence,
                    "reset": reset,
                    "long_poll": true,
                    "data": base64::engine::general_purpose::STANDARD.encode(bytes),
                    "controller": child.controller == Some(owner),
                    "mode": child.emulator.mode().bits(),
                })
                .to_string(),
            )
        }
        "mirror_take" => {
            if child.controller.is_some() && child.controller != Some(owner.clone()) && call.args["force"] != true {
                return Reply::error("another viewer controls this session; request Take control explicitly");
            }
            child.controller = Some(owner);
            Reply::ok("You control this session")
        }
        "mirror_release" => {
            if child.controller == Some(owner) {
                child.controller = None;
            }
            Reply::ok("Control released")
        }
        "mirror_input" => {
            if child.controller != Some(owner) {
                return Reply::error("take control before sending input");
            }
            let encoded = call.args["data"].as_str().unwrap_or_default();
            let bytes = match base64::engine::general_purpose::STANDARD.decode(encoded) {
                Ok(bytes) if bytes.len() <= 64 * 1024 => bytes,
                _ => return Reply::error("mirror input must be base64 and at most 64 KiB"),
            };
            match child.pty.write(&bytes) {
                Ok(()) => Reply::ok("input sent"),
                Err(error) => Reply::error(format!("{error}")),
            }
        }
        "mirror_resize" => {
            if child.controller != Some(owner) {
                return Reply::error("take control before resizing");
            }
            let Some(viewer) = crate::native_mirror::viewer_size(&call.args) else {
                return Reply::error("mirror_resize needs cols and rows within the terminal sizes a session takes");
            };
            let size = GridSize { cols: viewer.width, rows: viewer.height };
            match child.pty.resize(size) {
                Ok(()) => {
                    child.emulator.resize(size);
                    Reply::ok("resized")
                }
                Err(error) => Reply::error(format!("{error:#}")),
            }
        }
        _ => Reply::error("unknown mirror request"),
    }
}

/// Wait outside the service loop and child mutex so input and PTY output stay responsive.
async fn frame_when_ready(call: Call, child: Arc<Mutex<Child>>) -> Reply {
    let wait = call.args["wait_ms"].as_u64().unwrap_or(0).min(1000);
    let after = call.args["after"].as_u64().unwrap_or(0);
    let output = {
        let child = child.lock().expect("child lock");
        (wait > 0 && after != 0 && child.sequence == after).then(|| child.output.subscribe())
    };
    if let Some(mut output) = output {
        let _ = tokio::time::timeout(std::time::Duration::from_millis(wait), output.recv()).await;
    }
    mirror_child(&call, &child)
}

async fn request_json(value: Value) -> Result<Value> {
    let mut stream = UnixStream::connect(service_socket()).await.context("Borg service is not running")?;
    stream.write_all(format!("{value}\n").as_bytes()).await?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line).await?;
    serde_json::from_str(&line).context("unreadable response from Borg service")
}

pub async fn available() -> bool {
    UnixStream::connect(service_socket()).await.is_ok()
}

/// Whether the service that runs here keeps running with no session: installed, or run by a
/// supervisor. One a Borg started on demand ends with its last session.
pub async fn kept() -> bool {
    let Ok(Ok(reply)) = tokio::time::timeout(STATUS_WAIT, request_json(json!({"mode": "status"}))).await else {
        return false;
    };
    reply["kept"] == true
}

/// Starts the service for this user when none answers, in a process group of its own so the
/// terminal's signals miss it, and waits a moment for it. Borg works on its own when it does
/// not come up.
pub async fn start_on_demand() {
    use std::os::unix::process::CommandExt;
    if available().await {
        return;
    }
    let Ok(program) = crate::program::path() else { return };
    let started = std::process::Command::new(program)
        .args([SERVICE_COMMAND, RUN, ON_DEMAND])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .process_group(0)
        .spawn();
    let Ok(mut service) = started else { return };
    std::thread::spawn(move || service.wait());
    let _ = tokio::time::timeout(START_WAIT, async {
        while !available().await {
            tokio::time::sleep(START_POLL).await;
        }
    })
    .await;
}

/// Asks the service's watcher to stop, continue or end a process, which it may be able to do
/// where the user may not. Nothing comes back when no service is running.
pub async fn watch_act(
    act: crate::watch::WatchAct,
    process: borg_core::ProcessKey,
) -> Option<borg_core::ControlOutcome> {
    let request = json!({"mode": "watch-act", "act": act, "pid": process.pid, "start": process.start});
    let reply = request_json(request).await.ok()?;
    serde_json::from_value(reply.get("outcome")?.clone()).ok()
}

/// The IDs of the sessions the service keeps running, none when no service answers.
pub async fn running_sessions() -> HashSet<String> {
    let Ok(Ok(reply)) = tokio::time::timeout(STATUS_WAIT, request_json(json!({"mode": "status"}))).await else {
        return HashSet::new();
    };
    reply["sessions"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect()
}

/// Has the service keep a session running in the background, starting it unless it runs there
/// already. A session open in a terminal stays there, and the service says so.
pub async fn keep(id: &str) -> Result<()> {
    let reply = request_json(json!({"mode": "keep", "session": id})).await?;
    if reply["ok"] != true {
        bail!("{}", reply["error"].as_str().unwrap_or("the service could not keep the session running"));
    }
    Ok(())
}

/// Attaches this terminal to the persistent workspace owned by the service, which keeps it
/// running first. Ctrl-] leaves the attached view without stopping the remote shell or Borg
/// session.
pub async fn attach_session(id: &str, take: bool) -> Result<()> {
    keep(id).await?;
    let mut stream = UnixStream::connect(service_socket()).await.context("Borg service is not running")?;
    stream.write_all(format!("{}\n", json!({"mode": "attach", "session": id, "take": take})).as_bytes()).await?;
    crossterm::terminal::enable_raw_mode()?;
    struct Raw;
    impl Drop for Raw {
        fn drop(&mut self) {
            let _ = execute!(std::io::stdout(), DisableBracketedPaste, DisableMouseCapture, LeaveAlternateScreen);
            let _ = crossterm::terminal::disable_raw_mode();
        }
    }
    let _raw = Raw;
    let (mut read, mut write) = stream.into_split();
    let mut output = tokio::spawn(async move { tokio::io::copy(&mut read, &mut tokio::io::stdout()).await });
    let mut input = keyboard();
    let mut last_size = None;
    let mut resize = tokio::time::interval(std::time::Duration::from_millis(250));
    loop {
        tokio::select! {
            _ = resize.tick() => {
                let size = crossterm::terminal::size()?;
                if last_size != Some(size)
                    && request_json(json!({"mode":"resize", "session": id, "cols":size.0, "rows":size.1}))
                        .await.is_ok_and(|reply| reply["ok"] == true) {
                        last_size = Some(size);
                }
            }
            typed = input.recv() => {
                let Some(bytes) = typed else { break };
                if bytes.contains(&DETACH) { break; }
                write.write_all(&bytes).await?;
            }
            _ = &mut output => break,
        }
    }
    output.abort();
    drop(write);
    Ok(())
}

/// The terminal's input, read on a thread of its own: a read of standard input waiting in
/// Tokio's pool would keep `borg` from exiting after the session ends, until the next key.
fn keyboard() -> tokio::sync::mpsc::UnboundedReceiver<Vec<u8>> {
    use std::io::Read;
    let (typed, input) = unbounded_channel();
    std::thread::spawn(move || {
        let mut bytes = [0_u8; CHUNK];
        while let Ok(count) = std::io::stdin().read(&mut bytes) {
            if count == 0 || typed.send(bytes[..count].to_vec()).is_err() {
                break;
            }
        }
    });
    input
}

/// The watcher's own subcommands: installing the part that runs as root, and what it found.
async fn watch_command(args: &[String]) -> Result<()> {
    match args {
        [action] if action == "install" => crate::watch::install_root(),
        [action] if action == "uninstall" => crate::watch::uninstall_root(),
        [action] if action == "status" => {
            println!("{}", crate::watch::report()?);
            Ok(())
        }
        [action] if action == "notify" => crate::watch::test_notify(),
        _ => bail!("usage: borg service watch install|uninstall|status|notify"),
    }
}

/// `borg service`: `carried` is what the previous service handed over when an update started
/// this one in its place.
pub async fn command(args: &[String], carried: Option<crate::relaunch::Carried>) -> Result<()> {
    match args {
        [action] if action == "install" => crate::service_install::install(),
        [action] if action == "uninstall" => crate::service_install::uninstall(),
        [action] if action == "enable" => crate::service_install::enable(),
        [action] if action == "disable" => crate::service_install::disable(),
        [action] if action == "start" => crate::service_install::start(),
        [action] if action == "stop" => crate::service_install::stop(),
        [action] if action == RUN => server::run(server::Lifetime::Kept, carried).await,
        [action, flag] if action == RUN && flag == ON_DEMAND => server::run(server::Lifetime::OnDemand, carried).await,
        [action] if action == "status" => {
            println!("{}", request_json(json!({"mode": "status"})).await?);
            Ok(())
        }
        [action, rest @ ..] if action == "watch" => watch_command(rest).await,
        [action] if action == "pair" => {
            let reply = request_json(json!({"mode": "pair"})).await?;
            if reply["ok"] != true {
                bail!("{}", reply["error"].as_str().unwrap_or("pairing failed"));
            }
            println!("{}", reply["link"].as_str().unwrap_or_default());
            Ok(())
        }
        [action, name] if action == "keep" => keep_named(name).await,
        [action, name] if action == "attach" => attach_session(&session_named(name)?, false).await,
        [action, name, flag] if action == "attach" && flag == "--take" => {
            attach_session(&session_named(name)?, true).await
        }
        _ => bail!(
            "usage: borg service install|uninstall|enable|disable|start|stop|run|status|pair|keep NAME|attach NAME [--take]"
        ),
    }
}

/// `borg service keep NAME`: marks the session to run in the background, and starts it there
/// when the service runs.
async fn keep_named(name: &str) -> Result<()> {
    let id = session_named(name)?;
    if available().await {
        return keep(&id).await;
    }
    let state = StateDir::locate().context("set HOME or BORG_STATE_DIR")?;
    if sessions::held(&state, &id) {
        bail!("session {name} is open in a terminal");
    }
    sessions::keep(&state, &id, true)?;
    Ok(())
}

/// The ID of the session with this name or ID.
fn session_named(name: &str) -> Result<String> {
    let state = StateDir::locate().context("set HOME or BORG_STATE_DIR")?;
    let session = sessions::list(&state)?
        .into_iter()
        .find(|session| session.id == name || session.name == name)
        .with_context(|| format!("no session named {name}"))?;
    Ok(session.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slow_viewer_receives_current_screen_instead_of_old_animation_backlog() {
        let mut screen = Emulator::new(GridSize { cols: 100, rows: 60 }, 2000);
        screen.feed(b"\x1b[55;1HLATEST SCREEN");
        let backlog = VecDeque::from(vec![b'x'; 200_000]);
        let (reset, bytes) = mirror_payload(&screen, &backlog, 200_000, 1);
        assert!(reset);
        assert!(bytes.windows(b"LATEST SCREEN".len()).any(|window| window == b"LATEST SCREEN"));
        assert!(bytes.len() < 10_000, "do not replay 200 KB of stale animation");
        assert!(mirror_payload(&screen, &backlog, 200_000, 0).0, "initial connection starts at the latest screen");
        let (reset, bytes) = mirror_payload(&screen, &backlog, 200_000, 199_997);
        assert!(!reset);
        assert_eq!(bytes, b"xxx");
        assert!(mirror_payload(&screen, &backlog, 200_000, 200_000).1.is_empty());
    }
}
