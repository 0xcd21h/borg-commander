use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use borg_term::{Emulator, Foreground, GridSize, Pty, PtyEvent, Screen, Scroll};
use serde_json::{Value, json};
use tokio::{
    sync::{broadcast, mpsc},
    task::JoinHandle,
};

use crate::{
    agent::{Call, Reply},
    link,
    session::StateDir,
};

const BUFFER_LIMIT: usize = 1024 * 1024;
const FRAME_LIMIT: usize = 64 * 1024;
const SHELL_LIMIT: usize = 64;
const OWNER_LIMIT: usize = 16;
const IDLE_TIMEOUT: Duration = Duration::from_secs(120);
const CALL_TIMEOUT: Duration = Duration::from_secs(20);
const KEEPALIVE: Duration = Duration::from_secs(20);

type Key = (String, String);
type Hosts = Arc<Mutex<HashMap<String, Arc<Mutex<Host>>>>>;

struct Host {
    owner: String,
    session: Option<String>,
    pty: Pty,
    bytes: VecDeque<u8>,
    output: broadcast::Sender<()>,
    closed: bool,
    overflow: bool,
    touched: Instant,
    _runtime: tempfile::TempDir,
}

#[derive(Clone)]
pub struct Server {
    hosts: Hosts,
    retired: Arc<Mutex<HashSet<String>>>,
    shell: PathBuf,
    runtime: PathBuf,
}

impl Server {
    pub fn close_session(&self, session: &str) {
        self.retired.lock().expect("retired remote consoles").insert(session.to_owned());
        self.hosts
            .lock()
            .expect("remote consoles")
            .retain(|_, host| host.lock().expect("remote console").session.as_deref() != Some(session));
    }

    pub fn new(shell: PathBuf, runtime: PathBuf) -> Self {
        let hosts: Hosts = Default::default();
        let weak = Arc::downgrade(&hosts);
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(30));
            loop {
                tick.tick().await;
                let Some(hosts) = weak.upgrade() else { break };
                hosts
                    .lock()
                    .expect("remote consoles")
                    .retain(|_, host| host.lock().expect("remote console").touched.elapsed() < IDLE_TIMEOUT);
            }
        });
        Self { hosts, retired: Default::default(), shell, runtime }
    }

    fn open(&self, owner: String, args: &Value) -> Result<Value> {
        let retired = self.retired.lock().expect("retired remote consoles");
        if args["session"].as_str().is_some_and(|session| retired.contains(session)) {
            bail!("This session was removed");
        }
        let dir = args["path"]
            .as_str()
            .map(Path::new)
            .filter(|dir| dir.is_absolute())
            .context("console_open needs an absolute path")?;
        let size = size(args)?;
        let mut hosts = self.hosts.lock().expect("remote consoles");
        if hosts.len() >= SHELL_LIMIT
            || hosts.values().filter(|host| host.lock().expect("remote console").owner == owner).count() >= OWNER_LIMIT
        {
            bail!("Too many remote consoles are open");
        }
        let mut random = [0; 16];
        getrandom::fill(&mut random).context("creating console ID")?;
        let id = borg_link::hex(&random);
        let runtime = tempfile::Builder::new().prefix("console-").tempdir_in(&self.runtime)?;
        let command = borg_term::shell_command(&self.shell, dir, runtime.path())?;
        let (tx, mut rx) = mpsc::unbounded_channel();
        let pty = Pty::spawn(command, size, tx)?;
        let (output, _) = broadcast::channel(1);
        let host = Arc::new(Mutex::new(Host {
            owner,
            session: args["session"].as_str().map(str::to_owned),
            pty,
            bytes: VecDeque::new(),
            output,
            closed: false,
            overflow: false,
            touched: Instant::now(),
            _runtime: runtime,
        }));
        let weak = Arc::downgrade(&host);
        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                let Some(host) = weak.upgrade() else { break };
                let mut host = host.lock().expect("remote console");
                match event {
                    PtyEvent::Output(bytes) => {
                        if host.bytes.len() + bytes.len() > BUFFER_LIMIT {
                            host.overflow = true;
                        } else {
                            host.bytes.extend(bytes);
                        }
                    }
                    PtyEvent::Closed => host.closed = true,
                }
                let _ = host.output.send(());
            }
        });
        hosts.insert(id.clone(), host);
        drop(retired);
        Ok(json!({"id":id}))
    }

    pub fn handle(&self, call: Call, reply: tokio::sync::oneshot::Sender<Reply>) {
        if call.tool == "console_open" {
            let result = call
                .origin
                .clone()
                .context("Remote consoles require a paired computer")
                .and_then(|owner| self.open(owner, &call.args));
            let _ = reply.send(match result {
                Ok(value) => Reply::ok(value.to_string()),
                Err(error) => Reply::error(format!("{error:#}")),
            });
            return;
        }
        let result = self.prepare(&call);
        match result {
            Ok(Some(host)) if call.tool == "console_frame" => {
                tokio::spawn(async move {
                    let mut output = {
                        let mut host = host.lock().expect("remote console");
                        host.touched = Instant::now();
                        (host.bytes.is_empty() && !host.closed && !host.overflow).then(|| host.output.subscribe())
                    };
                    if let Some(output) = output.as_mut() {
                        let _ = tokio::time::timeout(Duration::from_secs(1), output.recv()).await;
                    }
                    let mut host = host.lock().expect("remote console");
                    let answer = if host.overflow {
                        Reply::error("Remote console output exceeded its buffer; reconnect the panel")
                    } else {
                        let count = host.bytes.len().min(FRAME_LIMIT);
                        let bytes: Vec<_> = host.bytes.drain(..count).collect();
                        Reply::ok(
                            json!({"data":STANDARD.encode(bytes),"closed":host.closed && host.bytes.is_empty(),
                                "foreground":match host.pty.foreground() { Foreground::Shell => "shell", Foreground::Program => "program", Foreground::Unknown => "unknown" },
                                "raw_input":host.pty.raw_input()})
                                .to_string(),
                        )
                    };
                    let _ = reply.send(answer);
                });
            }
            result => {
                let answer = match result {
                    Ok(_) => Reply::ok("{}"),
                    Err(answer) => answer,
                };
                let _ = reply.send(answer);
            }
        }
    }

    fn prepare(&self, call: &Call) -> Result<Option<Arc<Mutex<Host>>>, Reply> {
        let owner = call.origin.as_ref().ok_or_else(|| Reply::error("Remote consoles require a paired computer"))?;
        let id = call.args["id"].as_str().unwrap_or_default();
        let mut hosts = self.hosts.lock().expect("remote consoles");
        let host = hosts
            .get(id)
            .filter(|host| {
                let host = host.lock().expect("remote console");
                host.owner == *owner && host.session.as_deref() == call.args["session"].as_str()
            })
            .cloned()
            .ok_or_else(|| Reply::error("Remote console is closed or belongs to another computer"))?;
        if call.tool == "console_close" {
            hosts.remove(id);
            return Ok(None);
        }
        drop(hosts);
        {
            let mut shell = host.lock().expect("remote console");
            shell.touched = Instant::now();
            match call.tool.as_str() {
                "console_input" => {
                    let text = call.args["data"]
                        .as_str()
                        .filter(|text| text.len() <= FRAME_LIMIT * 2)
                        .ok_or_else(|| Reply::error("console_input needs bounded base64 data"))?;
                    let bytes = STANDARD.decode(text).map_err(|error| Reply::error(error.to_string()))?;
                    shell.pty.write(&bytes).map_err(|error| Reply::error(error.to_string()))?;
                }
                "console_resize" => shell
                    .pty
                    .resize(size(&call.args).map_err(|error| Reply::error(error.to_string()))?)
                    .map_err(|error| Reply::error(error.to_string()))?,
                "console_frame" => {}
                _ => return Err(Reply::error("Unknown console operation")),
            }
        }
        Ok(Some(host))
    }
}

fn size(args: &Value) -> Result<GridSize> {
    let cols = args["cols"].as_u64().filter(|n| (2..=500).contains(n)).context("cols must be 2..500")?;
    let rows = args["rows"].as_u64().filter(|n| (2..=300).contains(n)).context("rows must be 2..300")?;
    Ok(GridSize { cols: cols as u16, rows: rows as u16 })
}

pub enum Event {
    Output { bytes: Vec<u8>, foreground: Foreground, raw_input: bool },
    Closed(String),
}

enum Input {
    Write(Vec<u8>),
    Resize(GridSize),
}

struct Client {
    emulator: Emulator,
    size: GridSize,
    foreground: Foreground,
    raw_input: bool,
    input: mpsc::Sender<Input>,
    _task: Task<()>,
}

struct Task<T>(JoinHandle<T>);
impl<T> Drop for Task<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub struct Clients {
    clients: HashMap<Key, Client>,
    events: mpsc::UnboundedSender<(Key, Event)>,
    pub incoming: mpsc::UnboundedReceiver<(Key, Event)>,
}

impl Clients {
    pub fn new() -> Self {
        let (events, incoming) = mpsc::unbounded_channel();
        Self { clients: HashMap::new(), events, incoming }
    }

    pub fn open(&mut self, key: Key, dir: PathBuf, size: GridSize, scrollback: usize) {
        if self.clients.contains_key(&key) {
            return;
        }
        let (input, receive) = mpsc::channel(256);
        let events = self.events.clone();
        let target = key.clone();
        let task = tokio::spawn(async move {
            let result = connect(target.clone(), dir, size, receive, events.clone()).await;
            let error = match result {
                Ok(()) => "Shell exited".into(),
                Err(error) => format!("{error:#}"),
            };
            let _ = events.send((target, Event::Closed(error)));
        });
        self.clients.insert(
            key,
            Client {
                emulator: Emulator::new(size, scrollback),
                size,
                foreground: Foreground::Unknown,
                raw_input: false,
                input,
                _task: Task(task),
            },
        );
    }

    pub fn input_mode(&self, key: &Key) -> (Foreground, bool) {
        self.clients
            .get(key)
            .map(|client| (client.foreground, client.raw_input))
            .unwrap_or((Foreground::Unknown, false))
    }

    pub fn set_input_mode(&mut self, key: &Key, foreground: Foreground, raw_input: bool) {
        if let Some(client) = self.clients.get_mut(key) {
            client.foreground = foreground;
            client.raw_input = raw_input;
        }
    }

    pub fn emulator(&mut self, key: &Key) -> Option<&mut Emulator> {
        self.clients.get_mut(key).map(|client| &mut client.emulator)
    }

    pub fn screen(&self, key: &Key) -> Option<Screen<'_>> {
        self.clients.get(key).map(|client| client.emulator.screen())
    }

    pub fn write(&mut self, key: &Key, bytes: Vec<u8>) -> Result<()> {
        let client = self.clients.get_mut(key).context("Remote console is not connected")?;
        client.emulator.scroll(Scroll::Bottom);
        for bytes in bytes.chunks(FRAME_LIMIT) {
            client
                .input
                .try_send(Input::Write(bytes.into()))
                .context("Remote console input queue is full or disconnected")?;
        }
        Ok(())
    }

    pub fn fit(&mut self, size: GridSize) {
        for client in self.clients.values_mut().filter(|client| client.size != size) {
            if client.input.try_send(Input::Resize(size)).is_ok() {
                client.size = size;
                client.emulator.resize(size);
            }
        }
    }

    pub fn close(&mut self, key: &Key) {
        self.clients.remove(key);
    }
}

async fn rpc(remote: &mut link::Remote, session: &str, tool: &str, mut args: Value) -> Result<Value> {
    args["session"] = json!(session);
    let reply = tokio::time::timeout(CALL_TIMEOUT, remote.call(&Call::new(tool, args))).await??;
    if !reply.ok {
        if tool == "console_open" && reply.text.to_ascii_lowercase().contains("unknown tool") {
            bail!(borg_i18n::tr!(remote_console_host_upgrade));
        }
        bail!("{}", reply.text);
    }
    Ok(serde_json::from_str(&reply.text)?)
}

async fn connect(
    key: Key,
    dir: PathBuf,
    size: GridSize,
    mut input: mpsc::Receiver<Input>,
    events: mpsc::UnboundedSender<(Key, Event)>,
) -> Result<()> {
    let state = StateDir::locate().context("Borg has no state folder")?;
    let server = link::server(state.path(), &key.0)?;
    let mut writer = tokio::time::timeout(CALL_TIMEOUT, link::Remote::open(state.path(), &server)).await??;
    let opened =
        rpc(&mut writer, &key.1, "console_open", json!({"path":dir,"cols":size.cols,"rows":size.rows})).await?;
    let id = opened["id"].as_str().context("Host did not return a console ID")?.to_owned();
    let mut reader = tokio::time::timeout(CALL_TIMEOUT, link::Remote::open(state.path(), &server)).await??;
    let read_key = key.clone();
    let read_id = id.clone();
    let mut read = Task(tokio::spawn(async move {
        let mut previous = None;
        loop {
            let frame = rpc(&mut reader, &read_key.1, "console_frame", json!({"id":read_id})).await?;
            let data = STANDARD.decode(frame["data"].as_str().context("Missing console output")?)?;
            let foreground = match frame["foreground"].as_str() {
                Some("program") => Foreground::Program,
                Some("shell") => Foreground::Shell,
                _ => Foreground::Unknown,
            };
            let raw_input = frame["raw_input"] == true;
            if !data.is_empty() || previous != Some((foreground, raw_input)) {
                previous = Some((foreground, raw_input));
                events.send((read_key.clone(), Event::Output { bytes: data, foreground, raw_input }))?;
            }
            if frame["closed"] == true {
                break;
            }
        }
        Ok::<_, anyhow::Error>(())
    }));
    let mut pending = None;
    let mut keepalive = tokio::time::interval_at(tokio::time::Instant::now() + KEEPALIVE, KEEPALIVE);
    keepalive.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let result = loop {
        tokio::select! {
            result = &mut read.0 => break result.context("remote console reader")?.context("Remote console output connection failed"),
            _ = keepalive.tick() => {
                if let Err(error) = rpc(&mut writer, &key.1, "console_input", json!({"id":id,"data":""})).await {
                    break Err(error.context("Remote console input connection failed"));
                }
            }
            command = async { match pending.take() { Some(command) => Some(command), None => input.recv().await } } => {
                let result = match command {
                    Some(Input::Write(mut data)) => {
                        tokio::time::sleep(Duration::from_millis(2)).await;
                        while data.len() < FRAME_LIMIT {
                            match input.try_recv() {
                                Ok(Input::Write(next)) if data.len() + next.len() <= FRAME_LIMIT => data.extend(next),
                                Ok(next) => { pending = Some(next); break; }
                                Err(_) => break,
                            }
                        }
                        rpc(&mut writer, &key.1, "console_input", json!({"id":id,"data":STANDARD.encode(data)})).await
                    },
                    Some(Input::Resize(size)) => rpc(&mut writer, &key.1, "console_resize", json!({"id":id,"cols":size.cols,"rows":size.rows})).await,
                    None => break Ok(()),
                };
                if let Err(error) = result { break Err(error.context("Remote console input connection failed")); }
            }
        }
    };
    read.0.abort();
    let _ = rpc(&mut writer, &key.1, "console_close", json!({"id":id})).await;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn call(server: &Server, owner: Option<&str>, tool: &str, args: Value) -> Reply {
        let mut call = Call::new(tool, args);
        call.origin = owner.map(str::to_owned);
        let (tx, rx) = tokio::sync::oneshot::channel();
        server.handle(call, tx);
        rx.await.unwrap()
    }

    #[tokio::test]
    async fn removed_sessions_close_their_consoles_and_cannot_reopen_them() -> Result<()> {
        let runtime = tempfile::tempdir()?;
        let server = Server::new(PathBuf::from("/bin/bash"), runtime.path().into());
        let open = json!({"path":runtime.path(),"cols":100,"rows":24,"session":"removed"});
        let opened = call(&server, Some("owner"), "console_open", open.clone()).await;
        assert!(opened.ok, "{}", opened.text);
        let id: Value = serde_json::from_str(&opened.text)?;
        server.close_session("removed");
        assert!(!call(&server, Some("owner"), "console_frame", json!({"id":id["id"],"session":"removed"})).await.ok);
        assert!(!call(&server, Some("owner"), "console_open", open).await.ok);
        let kept = call(
            &server,
            Some("owner"),
            "console_open",
            json!({"path":runtime.path(),"cols":100,"rows":24,"session":"kept"}),
        )
        .await;
        assert!(kept.ok, "{}", kept.text);
        server.close_session("kept");
        Ok(())
    }

    #[tokio::test]
    async fn console_uses_its_own_pty_and_only_its_paired_owner_can_access_it() -> Result<()> {
        let runtime = tempfile::tempdir()?;
        let server = Server::new(PathBuf::from("/bin/bash"), runtime.path().into());
        let open = json!({"path":runtime.path(),"cols":100,"rows":24});
        assert!(!call(&server, None, "console_open", open.clone()).await.ok);
        let opened = call(&server, Some("owner"), "console_open", open).await;
        assert!(opened.ok, "{}", opened.text);
        let id: Value = serde_json::from_str(&opened.text)?;
        let id = id["id"].clone();
        assert!(!call(&server, Some("owner"), "console_frame", json!({"id":id,"session":"other"})).await.ok);
        let payload = json!({"id":id,"data":STANDARD.encode(b"printf 'remote terminal' > proof\r")});
        for tool in ["console_input", "console_frame", "console_close", "console_resize"] {
            assert!(!call(&server, Some("stranger"), tool, payload.clone()).await.ok);
        }
        assert!(call(&server, Some("owner"), "console_input", payload).await.ok);
        let mut emulator = Emulator::new(GridSize { cols: 100, rows: 24 }, 100);
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let frame = call(&server, Some("owner"), "console_frame", json!({"id":id})).await;
            assert!(frame.ok, "{}", frame.text);
            let value: Value = serde_json::from_str(&frame.text)?;
            emulator.feed(&STANDARD.decode(value["data"].as_str().unwrap())?);
            if runtime.path().join("proof").exists() {
                break;
            }
            assert!(Instant::now() < deadline);
        }
        assert_eq!(std::fs::read_to_string(runtime.path().join("proof"))?, "remote terminal");
        assert!(call(&server, Some("owner"), "console_resize", json!({"id":id,"cols":80,"rows":30})).await.ok);
        assert!(call(&server, Some("owner"), "console_close", json!({"id":id})).await.ok);
        assert!(!call(&server, Some("owner"), "console_input", json!({"id":id,"data":"YQ=="})).await.ok);
        Ok(())
    }
}
