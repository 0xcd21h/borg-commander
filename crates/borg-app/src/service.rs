//! An optional machine-wide Borg process. It keeps named Borg workspaces and their PTYs alive
//! without a terminal, and lets terminal clients attach to those same workspaces.

use std::{
    collections::{HashMap, VecDeque},
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    sync::{Arc, Mutex, atomic::AtomicUsize},
};

use anyhow::{Context, Result, bail};
use base64::Engine as _;
use borg_term::{Emulator, GridSize, Pty, PtyEvent};
use crossterm::{
    event::{DisableBracketedPaste, DisableMouseCapture},
    execute,
    terminal::LeaveAlternateScreen,
};
use portable_pty::CommandBuilder;
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
    sync::{
        broadcast,
        mpsc::{UnboundedSender, unbounded_channel},
        oneshot,
    },
};

use crate::{
    agent::{Call, Reply, Session},
    config::ConfigDir,
    launch,
    session::StateDir,
    sessions,
};

const SOCKET: &str = "service.sock";
const LOCK: &str = "service.lock";
const REPLAY_LIMIT: usize = 1024 * 1024;

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

fn service_runtime() -> Result<PathBuf> {
    Ok(StateDir::locate().context("set HOME or BORG_STATE_DIR")?.path().join("runtime"))
}

pub fn runtime_dir() -> Result<PathBuf> {
    service_runtime()
}

fn spawn_child(id: &str, events: UnboundedSender<(String, PtyEvent)>) -> Result<Arc<Mutex<Child>>> {
    let mut command = CommandBuilder::new(std::env::current_exe()?);
    command.args(["--session", id]);
    command.env("BORG_SERVICE_CHILD", "1");
    command.env("BORG_RUNTIME_DIR", service_runtime()?);
    command.env("TERM", "xterm-256color");
    let (sender, mut receiver) = unbounded_channel();
    let pty = Pty::spawn(command, GridSize { cols: 100, rows: 32 }, sender)?;
    let id = id.to_owned();
    tokio::spawn(async move {
        while let Some(event) = receiver.recv().await {
            if events.send((id.clone(), event)).is_err() {
                break;
            }
        }
    });
    let (output, _) = broadcast::channel(256);
    Ok(Arc::new(Mutex::new(Child {
        pty,
        emulator: Emulator::new(GridSize { cols: 100, rows: 32 }, 2000),
        replay: VecDeque::new(),
        output,
        controller: None,
        next_client: 0,
        sequence: 0,
    })))
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
    let mut bytes = [0_u8; 8192];
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
            let (Some(cols), Some(rows)) = (call.args["cols"].as_u64(), call.args["rows"].as_u64()) else {
                return Reply::error("mirror_resize needs cols and rows");
            };
            if !(20..=500).contains(&cols) || !(8..=200).contains(&rows) {
                return Reply::error("terminal size is out of range");
            }
            let size = GridSize { cols: cols as u16, rows: rows as u16 };
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

async fn run() -> Result<()> {
    let state = StateDir::locate().context("set HOME or BORG_STATE_DIR for the service")?;
    fs::create_dir_all(state.path())?;
    let runtime = launch::private_dir(&service_runtime()?)?;
    let lock_file =
        fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(state.path().join(LOCK))?;
    lock_file.try_lock().context("a Borg service is already running")?;
    let socket = service_socket();
    let _ = fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket)?;
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600))?;
    let mut config = ConfigDir::locate().map(|dir| dir.load()).transpose()?.unwrap_or_default();
    let relay = config.relay.as_deref().or(crate::link::DEFAULT_RELAY).and_then(borg_link::Relay::parse);
    let (calls_tx, mut calls_rx) = unbounded_channel::<(Call, oneshot::Sender<Reply>)>();
    let (messages, mut notices) = unbounded_channel();
    let mut cloud = crate::link::LinkServer::new(
        Some(state.path().to_path_buf()),
        runtime.clone(),
        None,
        relay,
        calls_tx,
        messages,
        Arc::new(AtomicUsize::new(0)),
    );
    let console_server = crate::remote_console::Server::new(borg_term::user_shell(), runtime.clone());
    let (events_tx, mut events_rx) = unbounded_channel::<(String, PtyEvent)>();
    cloud.set_background_service();
    let mut children: Children = HashMap::new();
    let mut removing = std::collections::HashSet::new();
    let (removed_tx, mut removed_rx) = unbounded_channel::<String>();
    let mut tick = tokio::time::interval(std::time::Duration::from_secs(2));
    loop {
        tokio::select! {
            _ = tick.tick() => {
                if let Ok(Some(current)) = ConfigDir::locate().map(|dir| dir.load()).transpose() {
                    if current.relay != config.relay {
                        let relay = current.relay.as_deref().or(crate::link::DEFAULT_RELAY).and_then(borg_link::Relay::parse);
                        cloud.set_relay(relay);
                    }
                    config = current;
                }
                cloud.sync(config.plugins.as_ref().is_none_or(|plugins| plugins.agent_cloud));
                let named: Vec<_> = sessions::ensure_default(&state)?.into_iter().filter(|session| !session.archived).collect();
                let active: std::collections::HashSet<_> = named.iter().map(|session| session.id.as_str()).collect();
                children.retain(|id, _| active.contains(id.as_str()));
                for session in named {
                    if removing.contains(&session.id) { continue; }
                    if let std::collections::hash_map::Entry::Vacant(entry) = children.entry(session.id) {
                        match spawn_child(entry.key(), events_tx.clone()) {
                            Ok(child) => { entry.insert(child); }
                            Err(error) => eprintln!("borg service: could not start {}: {error:#}", session.name),
                        }
                    }
                }
            }
            Some((id, event)) = events_rx.recv() => {
                match event {
                    PtyEvent::Output(bytes) => if let Some(child) = children.get(&id) {
                        let mut child = child.lock().expect("child lock");
                        child.emulator.feed(&bytes);
                        for answer in child.emulator.drain_replies() { let _ = child.pty.write(answer.as_bytes()); }
                        child.sequence = child.sequence.saturating_add(bytes.len() as u64);
                        child.replay.extend(&bytes);
                        while child.replay.len() > REPLAY_LIMIT { child.replay.pop_front(); }
                        let _ = child.output.send(bytes);
                    },
                    PtyEvent::Closed => {
                        if let Some(child) = children.remove(&id) {
                            let _ = child.lock().expect("child lock").output.send(Vec::new());
                        }
                    }
                }
            }
            Some((call, reply)) = calls_rx.recv() => {
                if call.tool == "session_delete" {
                    let id = call.args["id"].as_str().unwrap_or_default().to_owned();
                    if id == "default" || !sessions::list(&state).is_ok_and(|sessions| sessions.iter().any(|session| session.id == id)) {
                        let _ = reply.send(Reply::error("The Default session is protected, or the requested session no longer exists"));
                        continue;
                    }
                    if !removing.insert(id.clone()) {
                        let _ = reply.send(Reply::error("This session is already being removed"));
                        continue;
                    }
                    let child = children.remove(&id);
                    let removed = removed_tx.clone();
                    let state = state.clone();
                    let runtime = runtime.clone();
                    let consoles = console_server.clone();
                    tokio::spawn(async move {
                        let result = async {
                            if let Some(child) = child {
                                tokio::task::spawn_blocking(move || child.lock().expect("child lock").pty.stop()).await??;
                            }
                            sessions::delete_remote(&state, &runtime, &id, None).await
                        }.await;
                        let answer = match result {
                            Ok(()) => { consoles.close_session(&id); Reply::ok("Session removed") },
                            Err(error) => Reply::error(format!("{error:#}")),
                        };
                        let _ = reply.send(answer);
                        let _ = removed.send(id);
                    });
                    continue;
                }
                if call.tool.starts_with("console_") {
                    console_server.handle(call, reply);
                    continue;
                }
                if call.tool == "mirror_frame"
                    && let Some(child) = call.args["session"].as_str().and_then(|id| children.get(id)).cloned()
                {
                    tokio::spawn(async move { let _ = reply.send(frame_when_ready(call, child).await); });
                    continue;
                }
                if call.tool.starts_with("mirror_") {
                    let _ = reply.send(mirror(&call, &children));
                    continue;
                }
                let selected = state.path().to_path_buf();
                tokio::spawn(async move {
                    let registry = StateDir::at(selected);
                    let target = sessions::list(&registry).ok()
                        .and_then(|all| all.into_iter().filter(|session| !session.archived).max_by_key(|session| session.last_opened));
                    let answer = match target {
                        Some(session) => match Session::open(&service_runtime().unwrap_or_else(|_| launch::runtime_dir()), Some(&session.id)).await {
                            Some(mut open) => open.call(&call).await.unwrap_or_else(|| Reply::error("session disconnected")),
                            None => Reply::error("no running Borg session is ready"),
                        },
                        None => Reply::error("no Borg session exists"),
                    };
                    let _ = reply.send(answer);
                });
            }
            Some(id) = removed_rx.recv() => { removing.remove(&id); }
            Ok((stream, _)) = listener.accept() => {
                let mut reader = BufReader::new(stream);
                let mut line = String::new();
                if tokio::time::timeout(std::time::Duration::from_secs(2), reader.read_line(&mut line)).await.is_err() {
                    continue;
                }
                let request: Value = serde_json::from_str(&line).unwrap_or(Value::Null);
                let mut stream = reader.into_inner();
                match request["mode"].as_str() {
                    Some("attach") => {
                        let id = request["session"].as_str().unwrap_or_default();
                        if !children.contains_key(id)
                            && !removing.contains(id)
                            && sessions::list(&state).is_ok_and(|all| all.iter().any(|session| session.id == id && !session.archived))
                            && let Ok(child) = spawn_child(id, events_tx.clone())
                        {
                            children.insert(id.to_owned(), child);
                        }
                        if let Some(child) = children.get(id) {
                            tokio::spawn(attach(stream, child.clone(), request["take"] == true));
                        } else { let _ = stream.write_all(b"Session is offline\n").await; }
                    }
                    Some("pair") => {
                        let reply = cloud.pair().map(|link| json!({"ok": true, "link": link}))
                            .unwrap_or_else(|error| json!({"ok": false, "error": format!("{error:#}")}));
                        stream.write_all(format!("{reply}\n").as_bytes()).await?;
                    }
                    Some("status") => {
                        let reply = json!({"ok": true, "sessions": children.keys().collect::<Vec<_>>()});
                        stream.write_all(format!("{reply}\n").as_bytes()).await?;
                    }
                    Some("resize") => {
                        let id = request["session"].as_str().unwrap_or_default();
                        let cols = request["cols"].as_u64().unwrap_or(0);
                        let rows = request["rows"].as_u64().unwrap_or(0);
                        let ok = if let Some(child) = children.get(id).filter(|_| (20..=500).contains(&cols) && (8..=200).contains(&rows)) {
                            let mut child = child.lock().expect("child lock");
                            let size = GridSize { cols: cols as u16, rows: rows as u16 };
                            if matches!(child.controller, Some(Controller::Local(_))) && child.pty.resize(size).is_ok() {
                                child.emulator.resize(size);
                                true
                            } else { false }
                        } else { false };
                        stream.write_all(format!("{}\n", json!({"ok": ok})).as_bytes()).await?;
                    }
                    _ => { let _ = stream.write_all(b"Unknown service request\n").await; }
                }
            }
            Some(_notice) = notices.recv() => {}
        }
    }
}

async fn request(value: Value) -> Result<Value> {
    let mut stream = UnixStream::connect(service_socket()).await.context("Borg service is not running")?;
    stream.write_all(format!("{value}\n").as_bytes()).await?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line).await?;
    serde_json::from_str(&line).context("unreadable response from Borg service")
}

pub async fn available() -> bool {
    UnixStream::connect(service_socket()).await.is_ok()
}

/// Attaches this terminal to the persistent workspace owned by the service. Ctrl-] leaves
/// the attached view without stopping the remote shell or Borg session.
pub async fn attach_session(id: &str, take: bool) -> Result<()> {
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
    let mut input = tokio::io::stdin();
    let mut bytes = [0_u8; 8192];
    let mut last_size = None;
    let mut resize = tokio::time::interval(std::time::Duration::from_millis(250));
    loop {
        tokio::select! {
            _ = resize.tick() => {
                let size = crossterm::terminal::size()?;
                if last_size != Some(size)
                    && request(json!({"mode":"resize", "session": id, "cols":size.0, "rows":size.1}))
                        .await.is_ok_and(|reply| reply["ok"] == true) {
                        last_size = Some(size);
                }
            }
            result = input.read(&mut bytes) => {
                let count = result?;
                if count == 0 { break; }
                if bytes[..count].contains(&0x1d) { break; }
                write.write_all(&bytes[..count]).await?;
            }
            _ = &mut output => break,
        }
    }
    output.abort();
    drop(write);
    Ok(())
}

pub async fn command(args: &[String]) -> Result<()> {
    match args {
        [action] if action == "install" => crate::service_install::install(),
        [action] if action == "uninstall" => crate::service_install::uninstall(),
        [action] if action == "enable" => crate::service_install::enable(),
        [action] if action == "disable" => crate::service_install::disable(),
        [action] if action == "start" => crate::service_install::start(),
        [action] if action == "stop" => crate::service_install::stop(),
        [action] if action == "run" => run().await,
        [action] if action == "status" => {
            println!("{}", request(json!({"mode": "status"})).await?);
            Ok(())
        }
        [action] if action == "pair" => {
            let reply = request(json!({"mode": "pair"})).await?;
            if reply["ok"] != true {
                bail!("{}", reply["error"].as_str().unwrap_or("pairing failed"));
            }
            println!("{}", reply["link"].as_str().unwrap_or_default());
            Ok(())
        }
        [action, name] if action == "attach" => {
            let state = StateDir::locate().context("set HOME or BORG_STATE_DIR")?;
            let session = sessions::list(&state)?
                .into_iter()
                .find(|session| session.id == *name || session.name == *name)
                .with_context(|| format!("no session named {name}"))?;
            attach_session(&session.id, false).await
        }
        [action, name, flag] if action == "attach" && flag == "--take" => {
            let state = StateDir::locate().context("set HOME or BORG_STATE_DIR")?;
            let session = sessions::list(&state)?
                .into_iter()
                .find(|session| session.id == *name || session.name == *name)
                .with_context(|| format!("no session named {name}"))?;
            attach_session(&session.id, true).await
        }
        _ => bail!(
            "usage: borg service install|uninstall|enable|disable|start|stop|run|status|pair|attach NAME [--take]"
        ),
    }
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
