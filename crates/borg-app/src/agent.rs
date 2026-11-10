//! Lets an AI agent work in the Borg session the user has open, through MCP.
//!
//! `borg mcp` is the MCP server an agent starts, usually over SSH (`ssh host borg mcp`): it
//! speaks MCP on stdin and stdout and relays tool calls to the running session through a
//! Unix socket in an owner-only folder. The session answers what the user sees, reads and
//! shows paths on this machine or in containers, and runs commands and changes files only
//! after the user approves them.

mod answers;
mod capture;
mod files;
mod location;
mod scan;
mod secrets;
mod server;
mod socket;
mod tools;

use std::{
    collections::HashMap,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use anyhow::{Context, Result, bail};
use borg_core::{AgentEvent, Msg};
use borg_i18n::tr;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
    process::Command,
    sync::{
        mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel},
        oneshot,
    },
};

pub use answers::{
    ConsoleText, connections, containers, find_container, home_relative, host_dir, interfaces, locate, memory_report,
    place, ports, processes, screen, shell_output, shown_as_typed, startup_items, typable,
};
pub use capture::{capture_call, packet_number, summary_rows};
pub use files::{Spot, Upload, diff, is_folder, load, parse_lines, piece, read, whole};
pub use location::{DOCKER_SCHEME, Location};
pub use scan::{ScanCall, ScanTarget, files_answer, process_answer, rule_text_problem, scan_call};
pub(crate) use server::Session;
pub use server::{serve, serve_remote, setup};
use socket::SOCKET_SUFFIX;
pub(crate) use socket::{SOCKET_PREFIX, same_user, socket_dir, socket_dirs, socket_path};
pub use socket::{connect_info, hostname, program_path};
use tools::{routing_tools, tools, transfer_tools};

pub(crate) const LIST_LIMIT: usize = 500;
const OUTPUT_LIMIT: usize = 64 * 1024;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const PROTOCOL: &str = "2025-06-18";
const NOTE_TIME: &str = "%Y-%m-%d %H:%M:%S";
const NOTE_GAP: &str = "  ";

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
    /// The call came on a paired computer's viewer connection, which borg opens for a panel
    /// that shows this session, and never on an agent's.
    #[serde(default)]
    pub viewer: bool,
}

/// The prefix of tools and arguments only the session itself uses.
const INTERNAL: char = '_';

impl Call {
    pub fn new(tool: &str, args: Value) -> Self {
        Self { tool: tool.to_owned(), args, origin: None, approved: false, viewer: false }
    }

    /// A call the session makes for this one, with its origin and approval.
    pub fn follow(&self, tool: &str, args: Value) -> Self {
        Self { tool: tool.to_owned(), args, origin: self.origin.clone(), approved: self.approved, viewer: self.viewer }
    }

    /// A call as it may come from outside: internal tools are refused, and internal arguments
    /// (such as a container already looked up) are left out, so a client cannot pass them off.
    pub fn external(mut self) -> Result<Self, Reply> {
        self.origin = None;
        self.approved = false;
        self.viewer = false;
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
    pub waiting: Waiting,
    next_id: u64,
}

#[derive(Default)]
pub struct Waiting(HashMap<u64, (oneshot::Sender<Reply>, Work)>);

impl Waiting {
    pub async fn withdrawn(&mut self) -> u64 {
        let asks = &mut self.0;
        let id = std::future::poll_fn(|context| {
            asks.iter_mut()
                .find_map(|(id, (reply, _))| reply.poll_closed(context).is_ready().then_some(*id))
                .map_or(std::task::Poll::Pending, std::task::Poll::Ready)
        })
        .await;
        asks.remove(&id);
        id
    }
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
    /// Captures traffic; `filter` narrows the summary to the scope, which lists at most `limit`
    /// rows of each kind.
    Capture {
        request: borg_core::CaptureRequest,
        filter: String,
        limit: usize,
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
            Some(code) => tr!(agent_ran, command = self.command, code = code),
            None => tr!(agent_stopped, command = self.command),
        };
        let _ = messages.send(Msg::Agent(AgentEvent::Finished { id: self.id, notice }));
    }

    /// Answers that the command did not run.
    pub fn fail(self, text: &str, messages: &UnboundedSender<Msg>) {
        let _ = self.reply.send(Reply::error(text));
        let notice = tr!(agent_stopped, command = self.command);
        let _ = messages.send(Msg::Agent(AgentEvent::Finished { id: self.id, notice }));
    }
}

/// Adds an entry, with the date and time and with the secrets it recognizes hidden, to the log
/// of what agents did; the session keeps it, on disk too, while the user wants a log.
pub fn note(messages: &UnboundedSender<Msg>, entry: &str) {
    let entry = secrets::hidden(entry);
    let _ = messages.send(Msg::Agent(AgentEvent::Note(stamped(&jiff::Zoned::now(), &entry))));
}

/// An entry of the log as it reads: the date and time it happened, then what happened.
fn stamped(time: &jiff::Zoned, entry: &str) -> String {
    format!("{}{NOTE_GAP}{entry}", time.strftime(NOTE_TIME))
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
            tokio::spawn(accept(listener, calls.clone(), messages.clone(), clients.clone()));
            Some(socket)
        });
        Self { incoming, requeue: calls, socket, messages, clients, waiting: Waiting::default(), next_id: 0 }
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
        self.waiting.0.insert(self.next_id, (reply, work));
        self.next_id
    }

    /// Takes an answered ask: an approved one is returned to carry out, a declined one is
    /// answered here.
    pub fn answer(&mut self, id: u64, approved: bool) -> Option<Approved> {
        let (reply, work) = self.waiting.0.remove(&id)?;
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
            Work::Capture { request, .. } => format!("declined capturing on {}", request.interface),
        };
        self.note(&what);
        let _ = reply.send(Reply::error("The user declined this."));
        None
    }

    /// Runs a command on this machine in the background, for a console that cannot take it
    /// (a shell without Borg's integration, or a container's console), and answers with its
    /// output, which the log leaves out since output can hold secrets.
    pub fn run_in_background(&self, id: u64, reply: oneshot::Sender<Reply>, command: String, dir: PathBuf) {
        let messages = self.messages.clone();
        tokio::spawn(async move {
            let (exit, output) = run(&command, &dir).await;
            let status = exit.map_or_else(|| "stopped".to_owned(), |code| format!("exit {code}"));
            note(&messages, &format!("ran in {}\n$ {command}\n[{status}]", home_relative(&dir)));
            let _ = reply.send(Reply { ok: exit == Some(0), text: format!("{status}\n{output}") });
            let notice = match exit {
                Some(code) => tr!(agent_ran, command = command, code = code),
                None => tr!(agent_stopped, command = command),
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
                    (format!("{done} {uri} ({lines} lines)"), tr!(agent_wrote, path = uri))
                }
                Err(error) => (format!("{error:#}"), tr!(agent_write_failed, path = uri)),
            };
            let _ = reply.send(Reply { ok: result.is_ok(), text: reply_text });
            let _ = messages.send(Msg::Agent(AgentEvent::Finished { id, notice }));
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

/// Takes `borg mcp` connections from this user's programs, and checks each call as an agent's.
async fn accept(
    listener: UnixListener,
    calls: UnboundedSender<(Call, oneshot::Sender<Reply>)>,
    messages: UnboundedSender<Msg>,
    clients: Arc<AtomicUsize>,
) {
    while let Ok((stream, _)) = listener.accept().await {
        if !same_user(&stream) {
            continue;
        }
        let count = clients.fetch_add(1, Ordering::SeqCst) + 1;
        let _ = messages.send(Msg::Agent(AgentEvent::Connected { clients: count }));
        note(&messages, "an agent connected");
        let (calls, messages, clients) = (calls.clone(), messages.clone(), clients.clone());
        tokio::spawn(async move {
            let _ = serve_connection(stream, &calls).await;
            let count = clients.fetch_sub(1, Ordering::SeqCst) - 1;
            let _ = messages.send(Msg::Agent(AgentEvent::Connected { clients: count }));
            note(&messages, "an agent disconnected");
        });
    }
}

async fn serve_connection(stream: UnixStream, calls: &UnboundedSender<(Call, oneshot::Sender<Reply>)>) -> Result<()> {
    let (read, mut write) = stream.into_split();
    let mut lines = BufReader::new(read).lines();
    while let Some(line) = lines.next_line().await? {
        let decoded = serde_json::from_str::<Call>(&line).map(Call::external);
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

/// Output of a running command Borg holds at most twice over; what a reply carries is cut from
/// its end, so a command that prints without end never fills Borg's memory.
const OUTPUT_HELD: usize = 4 * OUTPUT_LIMIT;
const OUTPUT_PIECE: usize = 64 * 1024;

/// Adds `bytes` to the output kept of a command, letting go of its start once there is far
/// more than a reply carries.
pub(crate) fn keep_end(kept: &mut Vec<u8>, bytes: &[u8]) {
    kept.extend_from_slice(bytes);
    if kept.len() > 2 * OUTPUT_HELD {
        kept.drain(..kept.len() - OUTPUT_HELD);
    }
}

/// Reads a command's output to its end, keeping the end of it.
async fn end_of(mut output: impl tokio::io::AsyncRead + Unpin) -> Vec<u8> {
    use tokio::io::AsyncReadExt as _;
    let mut kept = Vec::new();
    let mut piece = vec![0; OUTPUT_PIECE];
    while let Ok(read) = output.read(&mut piece).await {
        if read == 0 {
            break;
        }
        keep_end(&mut kept, &piece[..read]);
    }
    kept
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
    let Ok(mut child) = child else { return (None, "could not start sh\n".into()) };
    let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
        return (None, "could not start sh\n".into());
    };
    let finished = async { tokio::join!(end_of(stdout), end_of(stderr), child.wait()) };
    match tokio::time::timeout(COMMAND_TIMEOUT, finished).await {
        Ok((stdout, stderr, Ok(status))) => {
            let mut text = String::from_utf8_lossy(&stdout).into_owned();
            text.push_str(&String::from_utf8_lossy(&stderr));
            if text.len() > OUTPUT_LIMIT {
                let cut = text.len() - OUTPUT_LIMIT;
                let start = (cut..text.len()).find(|&i| text.is_char_boundary(i)).unwrap_or(text.len());
                text = format!("[earlier output left out]\n{}", &text[start..]);
            }
            if !text.is_empty() && !text.ends_with('\n') {
                text.push('\n');
            }
            (status.code(), text)
        }
        Ok((_, _, Err(error))) => (None, format!("{error}\n")),
        Err(_) => (None, "stopped after 10 minutes\n".into()),
    }
}

#[cfg(test)]
mod tests;
