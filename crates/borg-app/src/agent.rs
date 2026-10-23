//! Lets an AI agent work in the Borg session the user has open, through MCP.
//!
//! `borg mcp` is the MCP server an agent starts, usually over SSH (`ssh host borg mcp`): it
//! speaks MCP on stdin and stdout and relays tool calls to the running session through a
//! Unix socket in an owner-only folder. The session answers what the user sees, shows paths
//! in a panel, and runs commands only after the user approves them; every call is logged.

use std::{
    collections::HashMap,
    fmt::Write as _,
    io,
    os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use anyhow::{Context, Result, bail};
use borg_core::{AgentEvent, AppState, Console, Msg, Row, Side};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
    process::Command,
    sync::{
        mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel},
        oneshot,
    },
};

const AGENT_DIR: &str = "agent";
const SOCKET_PREFIX: &str = "session-";
const SOCKET_SUFFIX: &str = ".sock";
const OWNER_ONLY: u32 = 0o700;
const OTHERS: u32 = 0o077;
const LIST_LIMIT: usize = 500;
pub const READ_DEFAULT: u64 = 32 * 1024;
const READ_LIMIT: u64 = 128 * 1024;
const OUTPUT_LIMIT: usize = 64 * 1024;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const PROTOCOL: &str = "2025-06-18";

/// A tool call relayed from `borg mcp` to the session, one JSON object per line.
#[derive(Debug, Serialize, Deserialize)]
pub struct Call {
    pub tool: String,
    #[serde(default)]
    pub args: Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Reply {
    pub ok: bool,
    pub text: String,
}

impl Reply {
    fn ok(text: impl Into<String>) -> Self {
        Self { ok: true, text: text.into() }
    }

    fn error(text: impl Into<String>) -> Self {
        Self { ok: false, text: text.into() }
    }
}

/// The folder for agent logs, created owner-only.
fn agent_dir(runtime_dir: &Path) -> Result<PathBuf> {
    owner_only(&runtime_dir.join(AGENT_DIR))
}

/// Unix socket paths are short (104 bytes on macOS), so sockets live in a short folder of
/// their own. With `XDG_RUNTIME_DIR` set, as on Linux desktops and in tests, the folder is
/// named after the runtime folder, so `borg mcp` finds only its sessions; without it, as on
/// macOS, every program the user starts shares one, whatever its temporary folder.
fn socket_dir(runtime_dir: &Path) -> Result<PathBuf> {
    let uid = rustix::process::getuid().as_raw();
    let name = if std::env::var_os("XDG_RUNTIME_DIR").is_some() {
        // FNV-1a, which stays the same from one build of borg to the next.
        let hash = runtime_dir
            .as_os_str()
            .as_encoded_bytes()
            .iter()
            .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3));
        format!("{hash:016x}")
    } else {
        "default".to_owned()
    };
    owner_only(&PathBuf::from(format!("/tmp/borg-{uid}-{name}")))
}

/// Creates a folder only its owner can enter. One that someone else owns, or that others
/// can enter, is refused, so no other user can reach a session through it.
fn owner_only(dir: &Path) -> Result<PathBuf> {
    let dir = dir.to_path_buf();
    std::fs::DirBuilder::new().recursive(true).mode(OWNER_ONLY).create(&dir)?;
    let meta = std::fs::metadata(&dir)?;
    if meta.uid() != rustix::process::getuid().as_raw() || meta.mode() & OTHERS != 0 {
        bail!("{} must belong to you and be closed to others", dir.display());
    }
    Ok(dir)
}

fn hostname() -> String {
    rustix::system::uname().nodename().to_string_lossy().into_owned()
}

/// The running session's side: it accepts `borg mcp` connections and hands their calls to
/// the app, which answers them with what it knows.
pub struct Agent {
    pub incoming: UnboundedReceiver<(Call, oneshot::Sender<Reply>)>,
    socket: Option<PathBuf>,
    pub log: PathBuf,
    waiting: HashMap<u64, (oneshot::Sender<Reply>, String, PathBuf)>,
    next_id: u64,
}

impl Agent {
    pub fn start(runtime_dir: &Path, messages: UnboundedSender<Msg>) -> Self {
        let (calls, incoming) = unbounded_channel();
        let pid = std::process::id();
        let log = agent_dir(runtime_dir).map_or_else(|_| PathBuf::new(), |dir| dir.join(format!("log-{pid}.txt")));
        let socket = socket_dir(runtime_dir).ok().and_then(|dir| {
            let socket = dir.join(format!("{SOCKET_PREFIX}{pid}{SOCKET_SUFFIX}"));
            let _ = std::fs::remove_file(&socket);
            let listener = UnixListener::bind(&socket).ok()?;
            std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).ok()?;
            tokio::spawn(accept(listener, calls, messages, log.clone()));
            Some(socket)
        });
        Self { incoming, socket, log, waiting: HashMap::new(), next_id: 0 }
    }

    /// Notes a command waiting for the user's answer.
    pub fn ask(&mut self, reply: oneshot::Sender<Reply>, command: String, dir: PathBuf) -> u64 {
        self.next_id += 1;
        self.waiting.insert(self.next_id, (reply, command, dir));
        self.next_id
    }

    /// Runs an approved command, or tells the agent the user declined it.
    pub fn answer(&mut self, id: u64, approved: bool, messages: UnboundedSender<Msg>) {
        let Some((reply, command, dir)) = self.waiting.remove(&id) else { return };
        let log = self.log.clone();
        if !approved {
            append(&log, &format!("declined in {}\n$ {command}\n\n", dir.display()));
            let _ = reply.send(Reply::error("The user declined to run this command."));
            return;
        }
        tokio::spawn(async move {
            let (exit, output) = run(&command, &dir).await;
            let status = exit.map_or_else(|| "stopped".to_owned(), |code| format!("exit {code}"));
            append(&log, &format!("ran in {}\n$ {command}\n{output}[{status}]\n\n", dir.display()));
            let _ = reply.send(Reply { ok: exit == Some(0), text: format!("{status}\n{output}") });
            let _ = messages.send(Msg::Agent(AgentEvent::Finished { id, command, exit }));
        });
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        if let Some(socket) = &self.socket {
            let _ = std::fs::remove_file(socket);
        }
    }
}

fn append(log: &Path, entry: &str) {
    use std::io::Write as _;
    let time = jiff::Zoned::now().strftime("%H:%M:%S").to_string();
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(log) {
        let _ = write!(file, "── {time} {entry}");
    }
}

async fn accept(
    listener: UnixListener,
    calls: UnboundedSender<(Call, oneshot::Sender<Reply>)>,
    messages: UnboundedSender<Msg>,
    log: PathBuf,
) {
    let clients = Arc::new(AtomicUsize::new(0));
    while let Ok((stream, _)) = listener.accept().await {
        let count = clients.fetch_add(1, Ordering::SeqCst) + 1;
        let _ = messages.send(Msg::Agent(AgentEvent::Connected { clients: count, log: log.clone() }));
        append(&log, "an agent connected\n\n");
        let (calls, messages, clients, log) = (calls.clone(), messages.clone(), clients.clone(), log.clone());
        tokio::spawn(async move {
            let _ = serve_connection(stream, &calls).await;
            let count = clients.fetch_sub(1, Ordering::SeqCst) - 1;
            let _ = messages.send(Msg::Agent(AgentEvent::Connected { clients: count, log: log.clone() }));
            append(&log, "an agent disconnected\n\n");
        });
    }
}

async fn serve_connection(stream: UnixStream, calls: &UnboundedSender<(Call, oneshot::Sender<Reply>)>) -> Result<()> {
    let (read, mut write) = stream.into_split();
    let mut lines = BufReader::new(read).lines();
    while let Some(line) = lines.next_line().await? {
        let reply = match serde_json::from_str::<Call>(&line) {
            Ok(call) => {
                let (sender, receiver) = oneshot::channel();
                calls.send((call, sender)).context("the session closed")?;
                receiver.await.unwrap_or_else(|_| Reply::error("Borg closed before answering."))
            }
            Err(error) => Reply::error(format!("unreadable call: {error}")),
        };
        write.write_all(format!("{}\n", serde_json::to_string(&reply)?).as_bytes()).await?;
    }
    Ok(())
}

/// Runs a command with `sh`, keeping the end of its output when there is a lot of it.
async fn run(command: &str, dir: &Path) -> (Option<i32>, String) {
    let child = Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn();
    let Ok(child) = child else { return (None, "could not start sh\n".into()) };
    match tokio::time::timeout(COMMAND_TIMEOUT, child.wait_with_output()).await {
        Ok(Ok(output)) => {
            let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
            text.push_str(&String::from_utf8_lossy(&output.stderr));
            if text.len() > OUTPUT_LIMIT {
                let cut = text.len() - OUTPUT_LIMIT;
                let start = (cut..text.len()).find(|&i| text.is_char_boundary(i)).unwrap_or(text.len());
                text = format!("[earlier output left out]\n{}", &text[start..]);
            }
            if !text.is_empty() && !text.ends_with('\n') {
                text.push('\n');
            }
            (output.status.code(), text)
        }
        Ok(Err(error)) => (None, format!("{error}\n")),
        Err(_) => (None, "stopped after 10 minutes\n".into()),
    }
}

/// What the user sees, in words an agent can use.
pub fn screen(state: &AppState) -> String {
    let mut text = format!("Borg on {}\nActive panel: {}\n", hostname(), side_name(state.active));
    for side in Side::ALL {
        let panel = state.panel(side);
        let place = match &panel.mount {
            Some(mount) => format!("container {} filesystem, {}", mount.name, panel.dir().display()),
            None => panel.dir().display().to_string(),
        };
        let _ = writeln!(text, "{} panel: {place}", side_name(side));
        match panel.hovered() {
            Some(Row::Entry(entry)) => drop(writeln!(text, "  highlighted: {}", entry.name)),
            Some(Row::Parent) => drop(writeln!(text, "  highlighted: .. (the parent folder)")),
            Some(Row::Group { label, .. }) => drop(writeln!(text, "  highlighted: group {label}")),
            None => {}
        }
        let marked: Vec<&str> = panel.marked().map(|entry| entry.name.as_str()).take(LIST_LIMIT).collect();
        if !marked.is_empty() {
            let _ = writeln!(text, "  marked: {}", marked.join(", "));
        }
    }
    let console = match &state.console {
        Console::Host => format!("host shell in {}", state.shell.cwd.display()),
        Console::Container(_) => format!("container {}", state.console_container().unwrap_or_default()),
    };
    let _ = writeln!(text, "Command line: {console}");
    if state.docker.is_open() {
        let selected = state.docker.container().map_or_else(String::new, |c| format!(", on {} ({})", c.name, c.status));
        let _ = writeln!(text, "Docker view open{selected}");
    }
    if let Some(process) = state.activity.process().filter(|_| state.activity.side.is_some()) {
        let _ = writeln!(text, "Activity monitor open, on {} (PID {})", process.name, process.key.pid);
    }
    text
}

fn side_name(side: Side) -> &'static str {
    match side {
        Side::Left => "left",
        Side::Right => "right",
    }
}

/// A path from an agent, relative to the active panel's folder unless absolute.
pub fn resolve(state: &AppState, path: &str) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() { path.to_path_buf() } else { host_dir(state).join(path) }
}

/// Where an agent's commands run: the active panel's folder, or the other panel's while the
/// active one shows a container, since commands run on this machine.
pub fn host_dir(state: &AppState) -> PathBuf {
    [state.active, state.active.other()]
        .into_iter()
        .map(|side| state.panel(side))
        .find(|panel| panel.mount.is_none())
        .map_or_else(|| state.shell.cwd.clone(), |panel| panel.dir().to_path_buf())
}

pub fn list_folder(dir: &Path) -> Reply {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => return Reply::error(format!("{}: {error}", dir.display())),
    };
    let mut lines: Vec<String> = entries
        .filter_map(Result::ok)
        .map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            match entry.metadata() {
                Ok(meta) if meta.is_dir() => format!("{name}/"),
                Ok(meta) => format!("{name}  {} bytes", meta.len()),
                Err(_) => name,
            }
        })
        .collect();
    lines.sort();
    let total = lines.len();
    lines.truncate(LIST_LIMIT);
    let more = if total > LIST_LIMIT { format!("\n… and {} more", total - LIST_LIMIT) } else { String::new() };
    Reply::ok(format!("{} ({total} entries)\n{}{more}", dir.display(), lines.join("\n")))
}

pub fn read_file(path: &Path, offset: u64, limit: u64) -> Reply {
    use std::io::{Read, Seek, SeekFrom};
    let limit = limit.clamp(1, READ_LIMIT);
    let read = || -> io::Result<(Vec<u8>, u64)> {
        let mut file = std::fs::File::open(path)?;
        let size = file.metadata()?.len();
        file.seek(SeekFrom::Start(offset))?;
        let mut bytes = Vec::new();
        file.take(limit).read_to_end(&mut bytes)?;
        Ok((bytes, size))
    };
    match read() {
        Ok((bytes, size)) if bytes.contains(&0) => Reply::ok(format!("{} is binary ({size} bytes)", path.display())),
        Ok((bytes, size)) => {
            let end = offset + bytes.len() as u64;
            let range =
                if offset > 0 || end < size { format!(" (bytes {offset}–{end} of {size})") } else { String::new() };
            Reply::ok(format!("{}{range}\n{}", path.display(), String::from_utf8_lossy(&bytes)))
        }
        Err(error) => Reply::error(format!("{}: {error}", path.display())),
    }
}

/// The tools `borg mcp` offers, as MCP describes them.
fn tools() -> Value {
    let read_only = json!({ "readOnlyHint": true });
    json!([
        {
            "name": "borg_screen",
            "description": "What the user sees in Borg right now: both panels' folders (or a container's filesystem), the highlighted and marked files, which panel is active, where the command line runs, and an open Docker view or activity monitor.",
            "inputSchema": { "type": "object", "properties": {} },
            "annotations": read_only,
        },
        {
            "name": "list_folder",
            "description": "Lists a folder on the machine Borg runs on. Relative paths start at the active panel's folder.",
            "inputSchema": { "type": "object", "properties": { "path": { "type": "string" } }, "required": ["path"] },
            "annotations": read_only,
        },
        {
            "name": "read_file",
            "description": "Reads a text file on the machine Borg runs on, up to 128 KiB at a time from `offset`. Relative paths start at the active panel's folder.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "offset": { "type": "integer", "minimum": 0 },
                    "limit": { "type": "integer", "minimum": 1 }
                },
                "required": ["path"]
            },
            "annotations": read_only,
        },
        {
            "name": "show_in_panel",
            "description": "Shows a file or folder to the user in Borg's other panel: a folder opens, a file is selected in its folder. The user's active panel stays where it is.",
            "inputSchema": { "type": "object", "properties": { "path": { "type": "string" } }, "required": ["path"] },
        },
        {
            "name": "run_command",
            "description": "Runs a shell command on the machine Borg runs on, after the user approves it in Borg, and returns its exit status and output. It runs with sh in `cwd`, or in the active panel's folder. Stops after 10 minutes. Keep commands focused and explain why you need them; the user sees each one before it runs.",
            "inputSchema": {
                "type": "object",
                "properties": { "command": { "type": "string" }, "cwd": { "type": "string" } },
                "required": ["command"]
            },
            "annotations": { "destructiveHint": true },
        }
    ])
}

/// The newest session that answers, among the sockets in the agent folder.
async fn connect(runtime_dir: &Path) -> Result<UnixStream> {
    let dir = socket_dir(runtime_dir)?;
    let mut sockets: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(&dir)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            name.starts_with(SOCKET_PREFIX) && name.ends_with(SOCKET_SUFFIX)
        })
        .filter_map(|path| Some((std::fs::metadata(&path).ok()?.modified().ok()?, path)))
        .collect();
    sockets.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    for (_, socket) in sockets {
        if let Ok(stream) = UnixStream::connect(&socket).await {
            return Ok(stream);
        }
    }
    bail!("no session")
}

struct Session {
    lines: tokio::io::Lines<BufReader<tokio::net::unix::OwnedReadHalf>>,
    write: tokio::net::unix::OwnedWriteHalf,
}

impl Session {
    async fn open(runtime_dir: &Path) -> Option<Self> {
        let (read, write) = connect(runtime_dir).await.ok()?.into_split();
        Some(Self { lines: BufReader::new(read).lines(), write })
    }

    async fn call(&mut self, call: &Call) -> Option<Reply> {
        self.write.write_all(format!("{}\n", serde_json::to_string(call).ok()?).as_bytes()).await.ok()?;
        serde_json::from_str(&self.lines.next_line().await.ok()??).ok()
    }
}

/// `borg mcp`: an MCP server on stdin and stdout for the Borg session open on this machine.
pub async fn serve(runtime_dir: PathBuf) -> Result<()> {
    let mut input = BufReader::new(tokio::io::stdin()).lines();
    let mut output = tokio::io::stdout();
    let mut session = Session::open(&runtime_dir).await;
    while let Some(line) = input.next_line().await? {
        let Ok(message) = serde_json::from_str::<Value>(&line) else { continue };
        let Some(id) = message.get("id").cloned() else { continue };
        let params = &message["params"];
        let result = match message["method"].as_str().unwrap_or_default() {
            "initialize" => json!({
                "protocolVersion": params["protocolVersion"].as_str().unwrap_or(PROTOCOL),
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "borg", "version": crate::update::VERSION },
                "instructions": format!(
                    "Borg Commander is a terminal file manager the user has open on {}. These tools see what the user sees there and work on that machine. Every command waits for the user's approval in Borg, so explain what you want to run and why.",
                    hostname()
                ),
            }),
            "ping" => json!({}),
            "tools/list" => json!({ "tools": tools() }),
            "tools/call" => {
                let call = Call {
                    tool: params["name"].as_str().unwrap_or_default().to_owned(),
                    args: params["arguments"].clone(),
                };
                if session.is_none() {
                    session = Session::open(&runtime_dir).await;
                }
                let mut reply = match session.as_mut() {
                    Some(open) => open.call(&call).await,
                    None => None,
                };
                if reply.is_none() {
                    // The session may have restarted: try the newest one once more.
                    session = Session::open(&runtime_dir).await;
                    if let Some(open) = session.as_mut() {
                        reply = open.call(&call).await;
                    }
                }
                let reply = reply.unwrap_or_else(|| {
                    Reply::error(format!(
                        "Borg is not open on {}. Ask the user to start borg in their session there.",
                        hostname()
                    ))
                });
                json!({ "content": [{ "type": "text", "text": reply.text }], "isError": !reply.ok })
            }
            _ => {
                let error =
                    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": "Method not found" } });
                output.write_all(format!("{error}\n").as_bytes()).await?;
                output.flush().await?;
                continue;
            }
        };
        let response = json!({ "jsonrpc": "2.0", "id": id, "result": result });
        output.write_all(format!("{response}\n").as_bytes()).await?;
        output.flush().await?;
    }
    Ok(())
}

/// `borg mcp setup [host]`: how to connect Claude Code and Claude Desktop to the borg on
/// `host` over SSH, or to this machine's without one.
pub fn setup(host: Option<&str>) -> String {
    // SSH runs commands without the login shell's PATH, so the installer's folder is added;
    // BatchMode makes a missing key fail at once instead of waiting for a password.
    let remote = r#"PATH="$HOME/.local/bin:$PATH" borg mcp"#;
    let (name, command, args) = match host {
        Some(host) => (
            format!("borg-{host}"),
            "ssh".to_owned(),
            vec!["-o".into(), "BatchMode=yes".into(), host.to_owned(), remote.to_owned()],
        ),
        None => (
            "borg".to_owned(),
            std::env::current_exe().map_or_else(|_| "borg".into(), |exe| exe.display().to_string()),
            vec!["mcp".to_owned()],
        ),
    };
    let quoted: Vec<String> =
        args.iter().map(|arg| if arg.contains(' ') { format!("'{arg}'") } else { arg.clone() }).collect();
    let desktop = json!({ "mcpServers": { &name: { "command": command, "args": args } } });
    format!(
        "Claude Code:\n\n  claude mcp add {name} -- {command} {}\n\nClaude Desktop (Settings → Developer → Edit Config), merge into the file:\n\n{}\n\nThen open borg{}; Claude works in that session and asks before running anything.",
        quoted.join(" "),
        serde_json::to_string_pretty(&desktop).unwrap_or_default(),
        host.map_or_else(String::new, |host| format!(" on {host}"))
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_connects_over_ssh_with_the_installers_folder_on_the_path() {
        let text = setup(Some("prod"));
        assert!(
            text.contains(
                r#"claude mcp add borg-prod -- ssh -o BatchMode=yes prod 'PATH="$HOME/.local/bin:$PATH" borg mcp'"#
            ),
            "{text}"
        );
        assert!(text.contains(r#""command": "ssh""#) && text.contains(r#""borg-prod""#), "{text}");
    }

    #[test]
    fn socket_folders_are_short_and_owner_only() {
        let one = socket_dir(Path::new("/tmp/borg-test-a")).unwrap();
        assert!(one.join("session-4294967295.sock").as_os_str().len() < 100);
        assert_eq!(std::fs::metadata(&one).unwrap().mode() & 0o777, 0o700);
    }
}
