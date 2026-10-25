//! Lets an AI agent work in the Borg session the user has open, through MCP.
//!
//! `borg mcp` is the MCP server an agent starts, usually over SSH (`ssh host borg mcp`): it
//! speaks MCP on stdin and stdout and relays tool calls to the running session through a
//! Unix socket in an owner-only folder. The session answers what the user sees, reads and
//! shows paths on this machine or in containers, and runs commands and changes files only
//! after the user approves them.

mod files;
mod location;

use std::{
    collections::HashMap,
    fmt::Write as _,
    os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use anyhow::{Context, Result, bail};
use borg_core::{AgentEvent, AppState, Console, Msg, Row, Side, View};
use borg_i18n::tr;
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

pub use files::{Spot, Upload, diff, is_folder, load, parse_lines, piece, read, whole};
pub use location::{DOCKER_SCHEME, Location};

const SOCKET_PREFIX: &str = "session-";
const SOCKET_SUFFIX: &str = ".sock";
const OWNER_ONLY: u32 = 0o700;
const OTHERS: u32 = 0o077;
const LIST_LIMIT: usize = 500;
const OUTPUT_LIMIT: usize = 64 * 1024;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const PROTOCOL: &str = "2025-06-18";

/// A tool call relayed from `borg mcp` to the session, one JSON object per line.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Call {
    pub tool: String,
    #[serde(default)]
    pub args: Value,
    /// The paired computer a call came from through borg cloud; `None` for this machine.
    /// Never read from the wire, so a client cannot claim another's name.
    #[serde(default)]
    pub origin: Option<String>,
    /// The user already approved this call, as a read from another computer.
    #[serde(default)]
    pub approved: bool,
}

/// The prefix of tools and arguments only the session itself uses.
const INTERNAL: char = '_';

impl Call {
    pub fn new(tool: &str, args: Value) -> Self {
        Self { tool: tool.to_owned(), args, origin: None, approved: false }
    }

    /// A call the session makes for this one, with its origin and approval.
    pub fn follow(&self, tool: &str, args: Value) -> Self {
        Self { tool: tool.to_owned(), args, origin: self.origin.clone(), approved: self.approved }
    }

    /// A call as it may come from outside: internal tools are refused, and internal arguments
    /// (such as a container already looked up) are left out, so a client cannot pass them off.
    pub fn external(mut self) -> Result<Self, Reply> {
        self.origin = None;
        self.approved = false;
        if self.tool.starts_with(INTERNAL) {
            return Err(Reply::error(format!("unknown tool {}", self.tool)));
        }
        if let Value::Object(args) = &mut self.args {
            args.retain(|key, _| !key.starts_with(INTERNAL));
        }
        Ok(self)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Reply {
    pub ok: bool,
    pub text: String,
}

impl Reply {
    pub fn ok(text: impl Into<String>) -> Self {
        Self { ok: true, text: text.into() }
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self { ok: false, text: text.into() }
    }
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

pub fn hostname() -> String {
    rustix::system::uname().nodename().to_string_lossy().into_owned()
}

/// How Claude reaches this borg, for the Connect dialog: the program by a path that stays
/// put across upgrades, and over SSH, the address and user the user came in with.
pub fn connect_info() -> borg_core::ConnectInfo {
    let ssh = std::env::var("SSH_CONNECTION").ok().and_then(|connection| {
        // "client-address client-port server-address server-port"
        let parts: Vec<&str> = connection.split_whitespace().collect();
        let (address, port) = (*parts.get(2)?, *parts.get(3)?);
        let user = std::env::var("USER").or_else(|_| std::env::var("LOGNAME")).ok().filter(|user| !user.is_empty());
        let user = user.map_or_else(String::new, |user| format!("{user}@"));
        Some(if port == "22" {
            format!("{user}{address}")
        } else if address.contains(':') {
            format!("ssh://{user}[{address}]:{port}")
        } else {
            format!("ssh://{user}{address}:{port}")
        })
    });
    let name = match &ssh {
        Some(_) => {
            let host = hostname();
            let short: String = host
                .split('.')
                .next()
                .unwrap_or_default()
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
                .collect();
            if short.is_empty() { "borg".to_owned() } else { format!("borg-{short}") }
        }
        None => "borg".to_owned(),
    };
    borg_core::ConnectInfo { name, program: program_path(), ssh, relay: String::new() }
}

/// This program's path, preferring the one on `PATH` that leads to it, since a package
/// manager's versioned folder changes with each upgrade.
pub fn program_path() -> String {
    let exe = std::env::current_exe().ok();
    let real = exe.as_ref().and_then(|exe| std::fs::canonicalize(exe).ok());
    if let (Some(path), Some(real)) = (std::env::var_os("PATH"), &real) {
        for dir in std::env::split_paths(&path).filter(|dir| dir.is_absolute()) {
            let candidate = dir.join("borg");
            if std::fs::canonicalize(&candidate).ok().as_ref() == Some(real) {
                return candidate.display().to_string();
            }
        }
    }
    exe.map_or_else(|| "borg".to_owned(), |exe| exe.display().to_string())
}

/// The running session's side: it accepts `borg mcp` connections and hands their calls to
/// the app, which answers them with what it knows.
pub struct Agent {
    pub incoming: UnboundedReceiver<(Call, oneshot::Sender<Reply>)>,
    /// For calls that come back once something they need is known, such as a container.
    requeue: UnboundedSender<(Call, oneshot::Sender<Reply>)>,
    socket: Option<PathBuf>,
    messages: UnboundedSender<Msg>,
    /// Agents connected now: through the socket here, and from paired computers.
    clients: Arc<AtomicUsize>,
    waiting: HashMap<u64, (oneshot::Sender<Reply>, Work)>,
    next_id: u64,
}

/// What an agent asked to do, waiting for the user's approval.
#[derive(Debug, Clone)]
pub enum Work {
    Run {
        command: String,
        target: Target,
    },
    /// Replaces a file's text with `after`; `before` is what the file held when the change
    /// was shown, `None` for a new file.
    Write {
        spot: Spot,
        before: Option<String>,
        after: String,
    },
    /// A computer asks to pair through borg cloud.
    Pair {
        name: String,
    },
    /// A call from another computer, run once the user approves it.
    Forward(Call),
    /// A file from another computer, written to `spot` once the user approves it.
    Upload {
        spot: Spot,
        size: u64,
    },
    ApprovalMode {
        origin: String,
    },
}

/// Where an agent's command runs.
#[derive(Debug, Clone)]
pub enum Target {
    /// On this machine, in a folder.
    Host(PathBuf),
    /// In a container's shell in borg, by id and name, in a folder when one was named.
    Container { id: String, name: String, dir: Option<PathBuf> },
}

/// An approved ask, handed back to the app to carry out.
pub struct Approved {
    pub id: u64,
    pub reply: oneshot::Sender<Reply>,
    pub work: Work,
}

/// A command typed into a shell for an agent: `line` is what is typed, `command` what the
/// agent asked for.
pub struct Typed {
    pub id: u64,
    pub reply: oneshot::Sender<Reply>,
    pub line: String,
    pub command: String,
    pub place: String,
}

impl Typed {
    /// Answers with the command's exit status and output, notes it in the log, and tells
    /// the session it is done.
    pub fn finish(self, exit: Option<i32>, output: &str, messages: &UnboundedSender<Msg>) {
        let status = exit.map_or_else(|| "finished".to_owned(), |code| format!("exit {code}"));
        note(messages, &format!("ran in {}\n$ {}\n[{status}]", self.place, self.command));
        let _ = self.reply.send(Reply { ok: exit.is_none_or(|code| code == 0), text: format!("{status}\n{output}") });
        let notice = match exit {
            Some(code) => format!(tr!(agent_ran), command = self.command, code = code),
            None => format!(tr!(agent_stopped), command = self.command),
        };
        let _ = messages.send(Msg::Agent(AgentEvent::Finished { id: self.id, notice }));
    }

    /// Answers that the command did not run.
    pub fn fail(self, text: &str, messages: &UnboundedSender<Msg>) {
        let _ = self.reply.send(Reply::error(text));
        let notice = format!(tr!(agent_stopped), command = self.command);
        let _ = messages.send(Msg::Agent(AgentEvent::Finished { id: self.id, notice }));
    }
}

/// Adds an entry, with the time, to the log of what agents did; the session keeps it in
/// memory while the user wants a log.
pub fn note(messages: &UnboundedSender<Msg>, entry: &str) {
    let time = jiff::Zoned::now().strftime("%H:%M:%S").to_string();
    let _ = messages.send(Msg::Agent(AgentEvent::Note(format!("{time}  {entry}"))));
}

impl Agent {
    pub fn start(runtime_dir: &Path, session_id: Option<&str>, messages: UnboundedSender<Msg>) -> Self {
        let (calls, incoming) = unbounded_channel();
        let pid = std::process::id();
        let clients = Arc::new(AtomicUsize::new(0));
        let socket = socket_dir(runtime_dir).ok().and_then(|dir| {
            let socket = dir.join(match session_id {
                Some(id) => format!("{SOCKET_PREFIX}{id}-{pid}{SOCKET_SUFFIX}"),
                None => format!("{SOCKET_PREFIX}{pid}{SOCKET_SUFFIX}"),
            });
            let _ = std::fs::remove_file(&socket);
            let listener = UnixListener::bind(&socket).ok()?;
            std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).ok()?;
            let service_child = std::env::var_os("BORG_SERVICE_CHILD").is_some();
            tokio::spawn(accept(listener, calls.clone(), messages.clone(), clients.clone(), service_child));
            Some(socket)
        });
        Self { incoming, requeue: calls, socket, messages, clients, waiting: HashMap::new(), next_id: 0 }
    }

    /// Adds an entry to the log of what agents did.
    pub fn note(&self, entry: &str) {
        note(&self.messages, entry);
    }

    /// Where a call comes back once something it needs is known; also where calls from
    /// paired computers come in.
    pub fn requeue_sender(&self) -> UnboundedSender<(Call, oneshot::Sender<Reply>)> {
        self.requeue.clone()
    }

    pub fn clients(&self) -> Arc<AtomicUsize> {
        self.clients.clone()
    }

    /// Notes an ask waiting for the user's answer.
    pub fn ask(&mut self, reply: oneshot::Sender<Reply>, work: Work) -> u64 {
        self.next_id += 1;
        self.waiting.insert(self.next_id, (reply, work));
        self.next_id
    }

    /// Takes an answered ask: an approved one is returned to carry out, a declined one is
    /// answered here.
    pub fn answer(&mut self, id: u64, approved: bool) -> Option<Approved> {
        let (reply, work) = self.waiting.remove(&id)?;
        if approved {
            return Some(Approved { id, reply, work });
        }
        let what = match &work {
            Work::Run { command, target } => format!("declined in {}\n$ {command}", place(target)),
            Work::Write { spot, before, .. } => {
                format!("declined {} {}", if before.is_some() { "editing" } else { "creating" }, spot.uri())
            }
            Work::Pair { name } => format!("declined pairing with {name}"),
            Work::Forward(call) => format!("declined a {} from {}", call.tool, call.origin.as_deref().unwrap_or("?")),
            Work::Upload { spot, .. } => format!("declined an upload to {}", spot.uri()),
            Work::ApprovalMode { origin } => format!("declined Approve all for {origin}"),
        };
        self.note(&what);
        let _ = reply.send(Reply::error("The user declined this."));
        None
    }

    /// Runs a command on this machine in the background, for a console that cannot take it
    /// (a shell without Borg's integration, or a container's console), and answers with its
    /// output, which the log keeps since no terminal shows it.
    pub fn run_in_background(&self, id: u64, reply: oneshot::Sender<Reply>, command: String, dir: PathBuf) {
        let messages = self.messages.clone();
        tokio::spawn(async move {
            let (exit, output) = run(&command, &dir).await;
            let status = exit.map_or_else(|| "stopped".to_owned(), |code| format!("exit {code}"));
            note(&messages, &format!("ran in {}\n$ {command}\n{output}[{status}]", home_relative(&dir)));
            let _ = reply.send(Reply { ok: exit == Some(0), text: format!("{status}\n{output}") });
            let notice = match exit {
                Some(code) => format!(tr!(agent_ran), command = command, code = code),
                None => format!(tr!(agent_stopped), command = command),
            };
            let _ = messages.send(Msg::Agent(AgentEvent::Finished { id, notice }));
        });
    }

    /// Writes an approved change, unless the file changed while the user looked at it.
    pub fn write(&self, id: u64, reply: oneshot::Sender<Reply>, spot: Spot, before: Option<String>, after: String) {
        let messages = self.messages.clone();
        tokio::spawn(async move {
            let uri = spot.uri();
            let result = async {
                let now = files::load(&spot).await?;
                if now != before {
                    bail!("{uri} changed while the user was deciding; read it again");
                }
                files::store(&spot, &after).await
            }
            .await;
            let (reply_text, notice) = match &result {
                Ok(()) => {
                    let lines = after.lines().count();
                    let done = if before.is_some() { "Edited" } else { "Created" };
                    note(&messages, &format!("{} {uri}", done.to_lowercase()));
                    (format!("{done} {uri} ({lines} lines)"), format!(tr!(agent_wrote), path = uri))
                }
                Err(error) => (format!("{error:#}"), format!(tr!(agent_write_failed), path = uri)),
            };
            let _ = reply.send(Reply { ok: result.is_ok(), text: reply_text });
            let _ = messages.send(Msg::Agent(AgentEvent::Finished { id, notice }));
        });
    }
}

/// A folder on this machine, with the home folder as `~`.
pub fn home_relative(dir: &Path) -> String {
    let home = std::env::var_os("HOME").map(PathBuf::from).filter(|home| home.as_os_str().len() > 1);
    match home.and_then(|home| dir.strip_prefix(home).ok().map(Path::to_path_buf)) {
        Some(rest) if rest.as_os_str().is_empty() => "~".into(),
        Some(rest) => format!("~/{}", rest.display()),
        None => dir.display().to_string(),
    }
}

/// Where a command runs, for the log and the user's question.
pub fn place(target: &Target) -> String {
    match target {
        Target::Host(dir) => home_relative(dir),
        Target::Container { name, dir: None, .. } => format!(tr!(agent_container_place), name = name),
        Target::Container { name, dir: Some(dir), .. } => {
            format!(tr!(agent_container_folder_place), dir = dir.display(), name = name)
        }
    }
}

/// The output of `command` typed into a shell, from what the terminal received between the
/// command starting and the shell announcing its end (OSC 133): escape sequences and carriage
/// returns removed, the echoed command line left out, and long output cut to its end.
pub fn shell_output(bytes: &[u8], command: &str) -> String {
    const COMMAND_START: &[u8] = b"\x1b]133;C";
    const COMMAND_END: &[u8] = b"\x1b]133;D";
    let find = |needle: &[u8]| bytes.windows(needle.len()).position(|window| window == needle);
    let end = find(COMMAND_END).unwrap_or(bytes.len());
    let text = match find(COMMAND_START) {
        Some(start) if start < end => String::from_utf8_lossy(&strip_escapes(&bytes[start..end])).replace('\r', ""),
        // A shell without a command-start mark (sh, older bash): the output starts on the line
        // after the echoed command, or after the first line if the echo cannot be found.
        _ => {
            let text = String::from_utf8_lossy(&strip_escapes(&bytes[..end])).replace('\r', "");
            let echo = text.rfind(command).map_or(0, |at| at + command.len());
            text[echo..].find('\n').map_or_else(String::new, |line| text[echo + line + 1..].to_owned())
        }
    };
    let text = if text.len() > OUTPUT_LIMIT {
        let cut = text.len() - OUTPUT_LIMIT;
        let from = (cut..text.len()).find(|&i| text.is_char_boundary(i)).unwrap_or(text.len());
        format!("[earlier output left out]\n{}", &text[from..])
    } else {
        text
    };
    if text.is_empty() || text.ends_with('\n') { text } else { format!("{text}\n") }
}

/// Removes terminal escape sequences: CSI (`ESC [ … final`), OSC (`ESC ] … BEL` or `ESC \\`)
/// and two-byte escapes.
fn strip_escapes(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != 0x1b {
            out.push(bytes[i]);
            i += 1;
            continue;
        }
        match bytes.get(i + 1) {
            Some(b'[') => {
                i += 2;
                while i < bytes.len() && !(0x40..=0x7e).contains(&bytes[i]) {
                    i += 1;
                }
                i += 1;
            }
            Some(b']') => {
                i += 2;
                while i < bytes.len() && bytes[i] != 0x07 && !(bytes[i] == 0x1b && bytes.get(i + 1) == Some(&b'\\')) {
                    i += 1;
                }
                i += if bytes.get(i) == Some(&0x07) { 1 } else { 2 };
            }
            _ => i += 2,
        }
    }
    out
}

/// The containers, for an agent to choose one.
pub async fn containers() -> Reply {
    match borg_docker::list().await {
        Ok(containers) if containers.is_empty() => Reply::ok("No containers."),
        Ok(containers) => Reply::ok(
            containers
                .iter()
                .map(|c| {
                    let project = c.project.as_deref().map_or_else(String::new, |p| format!("  project {p}"));
                    format!("{}  {:?}  {}  {}{project}", c.name, c.state, c.status, c.image)
                })
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        Err(error) => Reply::error(format!("{error:#}")),
    }
}

/// Finds a container by name, Compose service, or id prefix.
pub fn find_container<'a>(
    containers: &'a [borg_docker::Container],
    wanted: &str,
) -> Option<&'a borg_docker::Container> {
    containers.iter().find(|c| c.name == wanted).or_else(|| {
        let matches: Vec<_> =
            containers.iter().filter(|c| c.service.as_deref() == Some(wanted) || c.id.starts_with(wanted)).collect();
        (matches.len() == 1).then(|| matches[0])
    })
}

impl Drop for Agent {
    fn drop(&mut self) {
        if let Some(socket) = &self.socket {
            let _ = std::fs::remove_file(socket);
        }
    }
}

async fn accept(
    listener: UnixListener,
    calls: UnboundedSender<(Call, oneshot::Sender<Reply>)>,
    messages: UnboundedSender<Msg>,
    clients: Arc<AtomicUsize>,
    service_child: bool,
) {
    while let Ok((stream, _)) = listener.accept().await {
        let count = clients.fetch_add(1, Ordering::SeqCst) + 1;
        let _ = messages.send(Msg::Agent(AgentEvent::Connected { clients: count }));
        note(&messages, "an agent connected");
        let (calls, messages, clients) = (calls.clone(), messages.clone(), clients.clone());
        tokio::spawn(async move {
            let _ = serve_connection(stream, &calls, service_child).await;
            let count = clients.fetch_sub(1, Ordering::SeqCst) - 1;
            let _ = messages.send(Msg::Agent(AgentEvent::Connected { clients: count }));
            note(&messages, "an agent disconnected");
        });
    }
}

async fn serve_connection(
    stream: UnixStream,
    calls: &UnboundedSender<(Call, oneshot::Sender<Reply>)>,
    service_child: bool,
) -> Result<()> {
    let (read, mut write) = stream.into_split();
    let mut lines = BufReader::new(read).lines();
    while let Some(line) = lines.next_line().await? {
        let decoded =
            serde_json::from_str::<Call>(&line).map(|call| if service_child { Ok(call) } else { call.external() });
        let reply = match decoded {
            Ok(Ok(call)) => {
                let (sender, receiver) = oneshot::channel();
                calls.send((call, sender)).context("the session closed")?;
                receiver.await.unwrap_or_else(|_| Reply::error("Borg closed before answering."))
            }
            Ok(Err(refused)) => refused,
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

/// The console's terminal: the text it shows and what is typed at its prompt.
pub struct ConsoleText {
    pub lines: Vec<String>,
    pub typed: String,
}

/// How many of the console's last lines `screen` gives while the panels hide it.
const CONSOLE_LINES: usize = 20;

/// What the user sees: where they are and what the panels hold, in words an agent can use
/// (panels as locations, which every tool takes as they are), then the screen itself as text.
pub fn screen(state: &AppState, frame: &[String], console: &ConsoleText) -> String {
    let console_shown = state.effective_view() == View::Shell;
    let looking = if state.viewer.is_some() {
        "a file in Borg's viewer"
    } else if console_shown {
        "Borg's console, which hides the panels (Ctrl-O switches)"
    } else if state.docker.is_open() {
        "the Docker view"
    } else if state.activity.side.is_some() {
        "the activity monitor"
    } else {
        "the panels"
    };
    let mut text = format!("Borg on {}. The user is looking at {looking}.\n", hostname());
    if matches!(state.overlay, Some(borg_core::Overlay::Approval(_))) {
        text.push_str("A dialog asks the user to approve a request.\n");
    }
    let _ = writeln!(text, "Active panel: {}", side_name(state.active));
    for side in Side::ALL {
        let panel = state.panel(side);
        let _ = writeln!(text, "{} panel: {}", side_name(side), panel_location(state, side));
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
    let shell = match &state.console {
        Console::Remote { machine, session } => format!("remote shell on {machine}:{session}"),
        Console::Host => format!("host shell in {}", state.shell.cwd.display()),
        Console::Container(_) => format!("the {} container's shell", state.console_container().unwrap_or_default()),
    };
    let doing = if state.shell.captured() {
        "running a full-screen program"
    } else if state.shell.busy() {
        "running a command"
    } else {
        "at its prompt"
    };
    let typed =
        if console.typed.is_empty() { String::new() } else { format!(", with {:?} typed there", console.typed) };
    let _ = writeln!(text, "Command line: {shell}, {doing}{typed}");
    let shells: Vec<&str> = state.docker.shells.iter().map(|shell| shell.name.as_str()).collect();
    if !shells.is_empty() {
        let _ = writeln!(text, "Container shells open in Borg: {}", shells.join(", "));
    }
    if state.docker.is_open() {
        let selected = state.docker.container().map_or_else(String::new, |c| format!(", on {} ({})", c.name, c.status));
        let _ = writeln!(text, "Docker view open{selected}");
    }
    if let Some(process) = state.activity.process().filter(|_| state.activity.side.is_some()) {
        let _ = writeln!(text, "Activity monitor open, on {} (PID {})", process.name, process.key.pid);
    }
    // Long runs of empty rows read as one, to keep the text short.
    let shown = |lines: &[String]| {
        let end = lines.iter().rposition(|line| !line.is_empty()).map_or(0, |last| last + 1);
        let mut out: Vec<String> = Vec::new();
        let mut empty = 0;
        for line in &lines[..end] {
            if line.is_empty() {
                empty += 1;
                continue;
            }
            match empty {
                0 => {}
                1 | 2 => out.extend(std::iter::repeat_n(String::new(), empty)),
                _ => out.push(format!("⋯ {empty} empty lines")),
            }
            empty = 0;
            out.push(line.clone());
        }
        out.join("\n")
    };
    if !frame.is_empty() {
        let _ = write!(text, "\nThe screen as the user sees it, as text:\n{}\n", shown(frame));
    }
    if !console_shown {
        let lines = shown(&console.lines);
        let last: Vec<&str> = lines.lines().collect();
        let last = &last[last.len().saturating_sub(CONSOLE_LINES)..];
        if !last.is_empty() {
            let _ = write!(text, "\nThe console behind the panels, its last lines:\n{}\n", last.join("\n"));
        }
    }
    text
}

fn side_name(side: Side) -> &'static str {
    match side {
        Side::Left => "left",
        Side::Right => "right",
    }
}

/// Where a panel is: a folder on this machine, or in a container's filesystem.
pub fn panel_location(state: &AppState, side: Side) -> Location {
    let panel = state.panel(side);
    match &panel.mount {
        Some(mount) => Location::Container { container: mount.name.clone(), path: panel.dir().to_path_buf() },
        None => Location::Host(panel.dir().to_path_buf()),
    }
}

/// A location an agent wrote, relative to the active panel unless it says otherwise.
pub fn locate(state: &AppState, text: &str) -> Location {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    Location::parse(text, &panel_location(state, state.active), home.as_deref())
}

/// Where an agent's commands run on this machine without a folder of their own: the active
/// panel's folder, or the other panel's while the active one shows a container.
pub fn host_dir(state: &AppState) -> PathBuf {
    [state.active, state.active.other()]
        .into_iter()
        .map(|side| state.panel(side))
        .find(|panel| panel.mount.is_none())
        .map_or_else(|| state.shell.cwd.clone(), |panel| panel.dir().to_path_buf())
}

/// The tools `borg mcp` offers, as MCP describes them: few and general. The agent looks with
/// `screen` and `read` without asking, puts things in front of the user with `show`, and
/// changes anything only through `write`, `edit` and `run`, which wait for the user's approval.
fn tools() -> Value {
    const LOCATIONS: &str = "A location is an absolute path on the machine Borg runs on, ~/… for its home folder, docker://CONTAINER/path inside a container (by name, Compose service or id), or a path relative to the active panel's folder, wherever that panel is. `screen` names the panels this way.";
    let read_only = json!({ "readOnlyHint": true });
    json!([
        {
            "name": "screen",
            "description": "What the user sees in Borg right now: whether they look at the panels or the console, both panels' locations, the highlighted and marked files, where the command line runs and what is typed there, then the whole screen as text, as the user sees it (and the console's last lines while the panels hide it).",
            "inputSchema": { "type": "object", "properties": {} },
            "annotations": read_only,
        },
        {
            "name": "read",
            "description": format!("Reads without asking the user. With `path` (a location; the active panel's folder when left out): a folder's entries, or a text file's lines numbered like `cat -n`: the first 2000, or `lines` such as \"120-180\", \"120-\" or \"-50\" (the last 50), or only those containing the text `find`. Quote lines without their numbers when you edit. With `source` \"processes\": the `limit` (default 40) busiest processes on the machine Borg runs on, by `sort` \"memory\" (default) or \"cpu\". With `source` \"containers\": the Docker containers, their state and Compose project. {LOCATIONS}"),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "lines": { "type": "string" },
                    "find": { "type": "string" },
                    "source": { "type": "string", "enum": ["processes", "containers"] },
                    "limit": { "type": "integer", "minimum": 1 },
                    "sort": { "type": "string", "enum": ["memory", "cpu"] }
                }
            },
            "annotations": read_only,
        },
        {
            "name": "show",
            "description": format!("Shows a location to the user in Borg's other panel: a folder opens, a file is selected in its folder. The user's active panel stays where it is. Answers once the panel shows it. {LOCATIONS}"),
            "inputSchema": { "type": "object", "properties": { "path": { "type": "string" } }, "required": ["path"] },
        },
        {
            "name": "write",
            "description": format!("Creates a text file, or replaces all of one, with `content`, after the user approves it in Borg: they see the new file, or the change as a diff. The folder must exist. For a small change to an existing file, use edit. {LOCATIONS}"),
            "inputSchema": {
                "type": "object",
                "properties": { "path": { "type": "string" }, "content": { "type": "string" } },
                "required": ["path", "content"]
            },
            "annotations": { "destructiveHint": true },
        },
        {
            "name": "edit",
            "description": format!("Replaces the text `old` in a file with `new`, after the user approves the diff in Borg. `old` must match the file exactly, spaces and line breaks included, and only once, unless `all` replaces every match; read the file first and include enough surrounding lines to make it unique. {LOCATIONS}"),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "old": { "type": "string" },
                    "new": { "type": "string" },
                    "all": { "type": "boolean" }
                },
                "required": ["path", "old", "new"]
            },
            "annotations": { "destructiveHint": true },
        },
        {
            "name": "run",
            "description": "Runs a shell command after the user approves it in Borg, and returns its exit status and output. It is typed into a shell the user watches: Borg's own command line on the machine Borg runs on, or with `at` set to docker://CONTAINER (or docker://CONTAINER/folder) that container's shell in Borg's Docker view. The shell keeps its folder and variables between commands; the user can answer prompts in it. `at` may also be a folder on the machine to run in. Explain what you run and why: the user sees each command before it runs. While the shell is busy (the user is typing, or a program runs), the command is refused; try again later.",
            "inputSchema": {
                "type": "object",
                "properties": { "command": { "type": "string" }, "at": { "type": "string" } },
                "required": ["command"]
            },
            "annotations": { "destructiveHint": true },
        },
        {
            "name": "type",
            "description": "Types `text` at the cursor of whatever runs in Borg's console (what Ctrl-O shows): the shell's command line, or an editor such as vim or nano. Nothing runs: Enter is never pressed, text with several lines goes in as a paste, and control keys are left out, so the user reviews the text, corrects it if needed, and runs or saves it. Use it to suggest a command the user runs, or to put a script into the editor the user has open. It is refused where the program there cannot take pasted text safely.",
            "inputSchema": { "type": "object", "properties": { "text": { "type": "string" } }, "required": ["text"] },
        },
        {
            "name": "approval_mode",
            "description": "Requests Approve all for this paired computer in the selected Borg session, or turns it off. Enabling requires the user's approval in Borg. While enabled, this computer's commands, edits, and uploads run without separate prompts. It ends when this connection closes.",
            "inputSchema": { "type": "object", "properties": { "enabled": { "type": "boolean" } }, "required": ["enabled"] },
        }
    ])
}

/// Text an agent types, with every control character left out but line breaks and tabs, so
/// nothing in it can end a paste or act as a key.
pub fn typable(text: &str) -> String {
    text.replace("\r\n", "\n").chars().filter(|&c| c == '\n' || c == '\t' || !c.is_control()).collect()
}

/// The `shown` busiest processes, by memory or CPU, with the machine's totals.
pub fn processes(sample: &borg_core::SystemSample, by_cpu: bool, shown: usize) -> Reply {
    use borg_ui::size_text;
    let cores = sample.cpu.len().max(1);
    let cpu = sample.cpu.iter().map(|&c| u64::from(c)).sum::<u64>() as f64 / cores as f64 / 100.0;
    let mut processes: Vec<_> = sample.processes.iter().collect();
    if by_cpu {
        processes.sort_by_key(|p| std::cmp::Reverse(p.cpu.unwrap_or(0)));
    } else {
        processes.sort_by_key(|p| std::cmp::Reverse(p.resident.unwrap_or(0)));
    }
    let mut text = format!(
        "CPU {cpu:.1}% of {cores} cores · RAM {} of {} · {} processes\nPID      CPU%    RAM  NAME\n",
        size_text(sample.used),
        size_text(sample.total),
        sample.processes.len()
    );
    for p in processes.into_iter().take(shown) {
        let cpu = p.cpu.map_or_else(|| "—".into(), |c| format!("{:.1}", f64::from(c) / 100.0));
        let ram = p.resident.map_or_else(|| "—".into(), size_text);
        let path = p.executable.as_ref().map_or_else(String::new, |e| format!("  {}", e.display()));
        let _ = writeln!(text, "{:<8} {cpu:>5} {ram:>6}  {}{path}", p.key.pid, p.name);
    }
    Reply::ok(text)
}

/// The newest session that answers, among the sockets in the agent folder.
async fn connect(runtime_dir: &Path, session_id: Option<&str>) -> Result<UnixStream> {
    let dir = socket_dir(runtime_dir)?;
    let mut sockets: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(&dir)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if let Some(id) = session_id {
                name.starts_with(&format!("{SOCKET_PREFIX}{id}-")) && name.ends_with(SOCKET_SUFFIX)
            } else {
                name.starts_with(SOCKET_PREFIX) && name.ends_with(SOCKET_SUFFIX)
            }
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

pub(crate) struct Session {
    lines: tokio::io::Lines<BufReader<tokio::net::unix::OwnedReadHalf>>,
    write: tokio::net::unix::OwnedWriteHalf,
}

impl Session {
    pub(crate) async fn open(runtime_dir: &Path, session_id: Option<&str>) -> Option<Self> {
        let (read, write) = connect(runtime_dir, session_id).await.ok()?.into_split();
        Some(Self { lines: BufReader::new(read).lines(), write })
    }

    pub(crate) async fn call(&mut self, call: &Call) -> Option<Reply> {
        self.write.write_all(format!("{}\n", serde_json::to_string(call).ok()?).as_bytes()).await.ok()?;
        serde_json::from_str(&self.lines.next_line().await.ok()??).ok()
    }
}

/// Where `borg mcp` sends the calls it gets: the borg open on this machine, or a paired
/// borg reached through borg cloud.
enum Backend {
    Local { runtime_dir: PathBuf, session_id: String, session: Option<Session> },
    Remote { state: PathBuf, server: crate::link::Server, remote: Option<Box<crate::link::Remote>> },
    Routing(Router),
}

/// One MCP connection can address this computer or any server paired with it. Selection is
/// private to this MCP process, so two agents cannot silently redirect each other's calls.
struct Router {
    runtime_dir: PathBuf,
    state: Option<PathBuf>,
    selected: String,
    locals: HashMap<String, Session>,
    remotes: HashMap<String, Box<crate::link::Remote>>,
}

impl Router {
    fn servers(&self) -> Vec<crate::link::Server> {
        self.state.as_deref().map(crate::link::servers).unwrap_or_default()
    }

    fn local_sessions(&self) -> Vec<crate::sessions::NamedSession> {
        self.state
            .as_ref()
            .map(|state| crate::sessions::list(&crate::session::StateDir::at(state.clone())).unwrap_or_default())
            .filter(|sessions| !sessions.is_empty())
            .unwrap_or_else(|| {
                vec![crate::sessions::NamedSession {
                    id: "default".into(),
                    name: "Default".into(),
                    archived: false,
                    last_opened: 0,
                }]
            })
    }

    fn resolve(&self, text: &str) -> Result<String, String> {
        if let Some((machine, session)) = text.strip_prefix("remote:").and_then(|rest| rest.split_once('/'))
            && !session.is_empty()
            && self.servers().iter().any(|server| server.name == machine)
        {
            return Ok(format!("remote:{machine}/{session}"));
        }
        let local_matches: Vec<_> = self
            .local_sessions()
            .into_iter()
            .filter(|session| text == format!("local:{}", session.id) || text == session.id || text == session.name)
            .collect();
        if text == "local" {
            return Ok(format!(
                "local:{}",
                self.local_sessions()
                    .into_iter()
                    .filter(|session| !session.archived)
                    .max_by_key(|session| session.last_opened)
                    .map_or_else(|| "default".into(), |session| session.id)
            ));
        }
        let servers = self.servers();
        let remote_matches: Vec<_> =
            servers.iter().filter(|server| text == format!("remote:{}", server.name) || text == server.name).collect();
        match (local_matches.as_slice(), remote_matches.as_slice()) {
            ([session], []) => Ok(format!("local:{}", session.id)),
            ([], [server]) => Ok(format!("remote:{}", server.name)),
            ([], []) => Err(format!("No session named {text}. Call list_sessions to see available targets.")),
            _ => Err(format!("{text} is ambiguous. Use the full session ID from list_sessions.")),
        }
    }

    async fn call(&mut self, call: &Call) -> Reply {
        match call.tool.as_str() {
            "list_sessions" => {
                let mut lines: Vec<_> = self
                    .local_sessions()
                    .into_iter()
                    .map(|session| {
                        format!(
                            "local:{}  {}  {}{}",
                            session.id,
                            hostname(),
                            session.name,
                            if session.archived { " (archived)" } else { "" }
                        )
                    })
                    .collect();
                lines.extend(
                    self.servers().into_iter().map(|server| format!("remote:{}  {}", server.name, server.relay)),
                );
                return Reply::ok(lines.join("\n"));
            }
            "use_session" => {
                let Some(name) = call.args["session"].as_str() else {
                    return Reply::error("use_session needs a session ID or name from list_sessions");
                };
                return match self.resolve(name) {
                    Ok(id) => {
                        self.selected = id.clone();
                        Reply::ok(format!("Selected {id} for this MCP connection."))
                    }
                    Err(error) => Reply::error(error),
                };
            }
            "list_remote_sessions" => {
                let Some(machine) = call.args["server"].as_str() else {
                    return Reply::error("list_remote_sessions needs a paired server name");
                };
                let Some(server) = self.servers().into_iter().find(|server| server.name == machine) else {
                    return Reply::error(format!("no paired server named {machine}"));
                };
                let Some(state) = self.state.as_ref() else {
                    return Reply::error("Borg has no state folder for paired servers");
                };
                let key = format!("remote:{machine}");
                if !self.remotes.contains_key(&key) {
                    match crate::link::Remote::open(state, &server).await {
                        Ok(open) => {
                            self.remotes.insert(key.clone(), Box::new(open));
                        }
                        Err(error) => return Reply::error(format!("{error:#}")),
                    }
                }
                let reply = self
                    .remotes
                    .get_mut(&key)
                    .expect("opened remote")
                    .call(&Call::new("list_sessions", json!({})))
                    .await;
                return match reply {
                    Ok(reply) if reply.ok => Reply::ok(
                        reply
                            .text
                            .lines()
                            .map(|line| format!("remote:{machine}/{line}"))
                            .collect::<Vec<_>>()
                            .join("\n"),
                    ),
                    Ok(reply) => reply,
                    Err(error) => {
                        self.remotes.remove(&key);
                        Reply::error(format!("{error:#}"))
                    }
                };
            }
            _ => {}
        }
        let requested = call.args["session"].as_str().unwrap_or(&self.selected);
        let target = match self.resolve(requested) {
            Ok(id) => id,
            Err(error) => return Reply::error(error),
        };
        let mut routed = call.clone();
        if let Value::Object(args) = &mut routed.args {
            args.remove("session");
        }
        if let Some(id) = target.strip_prefix("local:") {
            let mut backend = Backend::Local {
                runtime_dir: self.runtime_dir.clone(),
                session_id: id.into(),
                session: self.locals.remove(id),
            };
            let reply = Box::pin(backend.call(&routed)).await;
            if let Backend::Local { session: Some(session), .. } = backend {
                self.locals.insert(id.into(), session);
            }
            return reply;
        }
        let Some(state) = self.state.as_ref() else {
            return Reply::error("Borg has no state folder for paired servers");
        };
        let remote_target = target.trim_start_matches("remote:");
        let (name, remote_session) =
            remote_target.split_once('/').map_or((remote_target, None), |(name, id)| (name, Some(id)));
        let Some(server) = self.servers().into_iter().find(|server| server.name == name) else {
            return Reply::error(format!("{target} is no longer paired"));
        };
        if let Some(id) = remote_session
            && let Value::Object(args) = &mut routed.args
        {
            args.insert("session".into(), json!(id));
        }
        let cache_key = format!("remote:{name}");
        let mut backend = Backend::Remote { state: state.clone(), server, remote: self.remotes.remove(&cache_key) };
        let reply = Box::pin(backend.call(&routed)).await;
        if let Backend::Remote { remote: Some(remote), .. } = backend {
            self.remotes.insert(cache_key, remote);
        }
        reply
    }
}

impl Backend {
    fn instructions(&self) -> String {
        match self {
            Self::Local { .. } => format!(
                "Borg Commander is a terminal file manager the user has open on {}. These tools see what the user sees there and work on that machine. Every command and change waits for the user's approval in Borg, so explain what you want to do and why.",
                hostname()
            ),
            Self::Remote { server, .. } => format!(
                "Borg Commander is a terminal file manager the user has open on {}, which these tools reach through borg cloud, encrypted end to end. They see what the user sees there and work on that machine; `download` and `upload` copy files between it and this computer, where you run. Every command and change waits for the user's approval in Borg, so explain what you want to do and why.",
                server.name
            ),
            Self::Routing(_) => format!(
                "Borg Commander can work in the local session on {} or a paired remote. Call list_sessions, then use_session to choose the target for this MCP connection. Every tool also accepts an optional session ID to override the target for one call. Commands and changes wait for approval in the target Borg.",
                hostname()
            ),
        }
    }

    fn tools(&self) -> Value {
        let mut tools = tools();
        if let (Self::Remote { .. }, Value::Array(list)) = (self, &mut tools) {
            list.extend(transfer_tools());
        }
        if let (Self::Routing(_), Value::Array(list)) = (self, &mut tools) {
            list.extend(transfer_tools());
            list.extend(routing_tools());
            for tool in list.iter_mut() {
                tool["inputSchema"]["properties"]["session"] = json!({
                    "type": "string",
                    "description": "Target ID from list_sessions. Overrides the session selected with use_session for this call."
                });
            }
        }
        tools
    }

    async fn call(&mut self, call: &Call) -> Reply {
        match self {
            Self::Local { runtime_dir, session_id, session } => {
                if session.is_none() {
                    *session = Session::open(runtime_dir, Some(session_id)).await;
                }
                let mut reply = match session.as_mut() {
                    Some(open) => open.call(call).await,
                    None => None,
                };
                if reply.is_none() {
                    // The session may have restarted: try the newest one once more.
                    *session = Session::open(runtime_dir, Some(session_id)).await;
                    if let Some(open) = session.as_mut() {
                        reply = open.call(call).await;
                    }
                }
                reply.unwrap_or_else(|| {
                    Reply::error(format!(
                        "Borg is not open on {}. Ask the user to start borg in their session there.",
                        hostname()
                    ))
                })
            }
            Self::Remote { state, server, remote } => {
                // A session the relay dropped, after idling or a restart, is opened again once.
                for attempt in 0..2 {
                    if remote.is_none() {
                        match crate::link::Remote::open(state, server).await {
                            Ok(opened) => *remote = Some(Box::new(opened)),
                            Err(error) => return Reply::error(format!("{error:#}")),
                        }
                    }
                    let Some(open) = remote.as_mut() else { break };
                    let result = match call.tool.as_str() {
                        "download" => transfer_download(open, call).await,
                        "upload" => transfer_upload(open, call).await,
                        _ => open.call(call).await,
                    };
                    match result {
                        Ok(reply) => return reply,
                        Err(error) if attempt == 1 => return Reply::error(format!("{error:#}")),
                        Err(_) => *remote = None,
                    }
                }
                Reply::error(format!("borg on {} is not reachable", server.name))
            }
            Self::Routing(router) => router.call(call).await,
        }
    }
}

fn routing_tools() -> Vec<Value> {
    vec![
        json!({
            "name": "list_sessions",
            "description": "Lists the local Borg and all paired remote targets with the IDs to use for subsequent calls.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "use_session",
            "description": "Selects the target for subsequent calls on this MCP connection. Other MCP connections are unaffected.",
            "inputSchema": {
                "type": "object",
                "properties": { "session": { "type": "string" } },
                "required": ["session"]
            }
        }),
        json!({
            "name": "list_remote_sessions",
            "description": "Lists named Borg sessions on one paired server and returns IDs for use_session.",
            "inputSchema": {
                "type": "object",
                "properties": { "server": { "type": "string" } },
                "required": ["server"]
            }
        }),
    ]
}

/// The tools only `borg mcp remote` offers: it runs on the user's computer, so it can copy
/// files between that and the server.
fn transfer_tools() -> Vec<Value> {
    vec![
        json!({
            "name": "download",
            "description": "Copies a file from the machine Borg runs on to this computer, where you run. `path` is a location there; `to` is a path here: a folder keeps the file's name, and ~/Downloads is used when it is left out. The relay carries about 2 MB a second.",
            "inputSchema": {
                "type": "object",
                "properties": { "path": { "type": "string" }, "to": { "type": "string" } },
                "required": ["path"]
            },
        }),
        json!({
            "name": "upload",
            "description": "Copies a file from this computer, where you run, to `to`, a location on the machine Borg runs on, after the user approves it in Borg there. Up to 1 GiB; the relay carries about 2 MB a second.",
            "inputSchema": {
                "type": "object",
                "properties": { "from": { "type": "string" }, "to": { "type": "string" } },
                "required": ["from", "to"]
            },
            "annotations": { "destructiveHint": true },
        }),
    ]
}

/// A path on this computer: absolute, under ~, or from the folder `borg mcp` runs in.
fn local_path(text: &str) -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    match (text.strip_prefix('~'), home) {
        (Some(rest), Some(home)) if rest.is_empty() || rest.starts_with('/') => home.join(rest.trim_start_matches('/')),
        _ => PathBuf::from(text),
    }
}

async fn transfer_download(remote: &mut crate::link::Remote, call: &Call) -> Result<Reply> {
    let Some(path) = call.args["path"].as_str() else { return Ok(Reply::error("download needs a path")) };
    let name = Path::new(path).file_name().map_or_else(|| "download".into(), |name| name.to_os_string());
    let mut to = match call.args["to"].as_str() {
        Some(to) => local_path(to),
        None => {
            let downloads = local_path("~/Downloads");
            if downloads.is_dir() { downloads } else { PathBuf::from(".") }
        }
    };
    if to.is_dir() {
        to = to.join(name);
    }
    match crate::link::download_in_session(remote, call.args["session"].as_str(), path, &to, |_, _| {}).await {
        Ok(size) => Ok(Reply::ok(format!("Downloaded {path} ({size} bytes) to {}", to.display()))),
        Err(error) if error.downcast_ref::<std::io::Error>().is_some() => Ok(Reply::error(format!("{error:#}"))),
        Err(error) => Err(error),
    }
}

async fn transfer_upload(remote: &mut crate::link::Remote, call: &Call) -> Result<Reply> {
    let (Some(from), Some(to)) = (call.args["from"].as_str(), call.args["to"].as_str()) else {
        return Ok(Reply::error("upload needs from and to"));
    };
    let from = local_path(from);
    if !from.is_file() {
        return Ok(Reply::error(format!("{} is not a file on this computer", from.display())));
    }
    match crate::link::upload_in_session(remote, call.args["session"].as_str(), &from, to, |_, _| {}).await {
        Ok(size) => Ok(Reply::ok(format!("Uploaded {} ({size} bytes) to {to}", from.display()))),
        Err(error) => Ok(Reply::error(format!("{error:#}"))),
    }
}

/// `borg mcp`: one MCP server for this machine and every server paired with it.
pub async fn serve(runtime_dir: PathBuf) -> Result<()> {
    let state = crate::session::StateDir::locate().map(|dir| dir.path().to_path_buf());
    let mut selected = state
        .as_ref()
        .and_then(|root| crate::sessions::list(&crate::session::StateDir::at(root.clone())).ok())
        .and_then(|sessions| {
            sessions.into_iter().filter(|session| !session.archived).max_by_key(|session| session.last_opened)
        })
        .map_or_else(|| "local:default".to_owned(), |session| format!("local:{}", session.id));
    // Connects at once, so the selected borg shows that Claude connected before its first call.
    let mut locals = HashMap::new();
    let mut open = Session::open(&runtime_dir, selected.strip_prefix("local:")).await;
    if open.is_none() {
        // The MCP process may have a different state-folder environment from the terminal
        // that launched Borg. The runtime socket is still the authority for the live session.
        selected = "local:default".into();
        open = Session::open(&runtime_dir, Some("default")).await;
        if open.is_none() {
            open = Session::open(&runtime_dir, None).await;
        }
    }
    if let Some(session) = open {
        locals.insert(selected.trim_start_matches("local:").to_owned(), session);
    }
    serve_with(Backend::Routing(Router { runtime_dir, state, selected, locals, remotes: HashMap::new() })).await
}

/// `borg mcp remote NAME`: the same for a borg paired through borg cloud.
pub async fn serve_remote(state: PathBuf, name: &str) -> Result<()> {
    let server = crate::link::server(&state, name)?;
    serve_with(Backend::Remote { state, server, remote: None }).await
}

async fn serve_with(mut backend: Backend) -> Result<()> {
    let mut input = BufReader::new(tokio::io::stdin()).lines();
    let mut output = tokio::io::stdout();
    while let Some(line) = input.next_line().await? {
        let Ok(message) = serde_json::from_str::<Value>(&line) else { continue };
        let Some(id) = message.get("id").cloned() else { continue };
        let params = &message["params"];
        let result = match message["method"].as_str().unwrap_or_default() {
            "initialize" => json!({
                "protocolVersion": params["protocolVersion"].as_str().unwrap_or(PROTOCOL),
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "borg", "version": crate::update::VERSION },
                "instructions": backend.instructions(),
            }),
            "ping" => json!({}),
            "tools/list" => json!({ "tools": backend.tools() }),
            "tools/call" => {
                let call = Call::new(params["name"].as_str().unwrap_or_default(), params["arguments"].clone());
                let reply = backend.call(&call).await;
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
    fn external_calls_cannot_claim_an_origin_or_prior_approval() {
        let forged: Call = serde_json::from_value(json!({
            "tool": "run", "args": { "command": "true" }, "origin": "trusted", "approved": true
        }))
        .unwrap();
        let checked = forged.external().unwrap();
        assert!(checked.origin.is_none());
        assert!(!checked.approved);
    }

    #[test]
    fn router_resolves_named_local_sessions_without_redirecting_other_connections() {
        let temp = tempfile::tempdir().unwrap();
        let state = crate::session::StateDir::at(temp.path().join("state"));
        let _ = crate::sessions::ensure_default(&state).unwrap();
        let prod = crate::sessions::create(&state, "prod").unwrap();
        let router = Router {
            runtime_dir: temp.path().into(),
            state: Some(state.path().into()),
            selected: "local:default".into(),
            locals: HashMap::new(),
            remotes: HashMap::new(),
        };
        assert_eq!(router.resolve("prod"), Ok(format!("local:{}", prod.id)));
        assert_eq!(router.resolve(&format!("local:{}", prod.id)), Ok(format!("local:{}", prod.id)));
        assert!(router.resolve("missing").is_err());
        assert_eq!(router.selected, "local:default");
    }

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
    fn shell_output_runs_from_the_command_start_to_its_end() {
        let bash = b"\x15ls -l\r\n\x1b]133;C\x07total 0\r\n\x1b[1;34mbin\x1b[0m\r\n\x1b]133;D;0\x07\x1b]7;file://box/\x07\x1b]133;A\x07$ ";
        assert_eq!(shell_output(bash, "ls -l"), "total 0\nbin\n");
        let sh = b"\x1b[7m web \x1b[0m $ \x1b]133;B\x07\r\n\x1b[7m web \x1b[0m $ echo hi\r\nhi\r\n\x1b]133;D;0\x07\x1b]133;A\x07/ # ";
        assert_eq!(shell_output(sh, "echo hi"), "hi\n", "without a command-start mark, the echoed command is left out");
        assert_eq!(shell_output(b"true\r\n\x1b]133;D;0\x07", "true"), "");
    }

    #[test]
    fn containers_are_found_by_name_service_or_id() {
        let container = |id: &str, name: &str, service: Option<&str>| borg_docker::Container {
            id: id.into(),
            name: name.into(),
            image: String::new(),
            command: String::new(),
            created: String::new(),
            state: borg_docker::State::Running,
            status: String::new(),
            ports: String::new(),
            networks: String::new(),
            project: None,
            service: service.map(str::to_owned),
            project_dir: None,
            config_files: Vec::new(),
        };
        let all = [container("a1b2", "shop-web-1", Some("web")), container("c3d4", "blog-web-1", Some("web"))];
        assert_eq!(find_container(&all, "shop-web-1").map(|c| c.id.as_str()), Some("a1b2"));
        assert_eq!(find_container(&all, "c3"), Some(&all[1]));
        assert!(find_container(&all, "web").is_none(), "two services named web are ambiguous");
    }

    #[test]
    fn socket_folders_are_short_and_owner_only() {
        let one = socket_dir(Path::new("/tmp/borg-test-a")).unwrap();
        assert!(one.join("session-4294967295.sock").as_os_str().len() < 100);
        assert_eq!(std::fs::metadata(&one).unwrap().mode() & 0o777, 0o700);
    }
}
