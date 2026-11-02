//! Captures packets with the installed tcpdump into a pcap file in the folder the network
//! screen covers, or the one an agent names, as you or as administrator through Borg's helper,
//! and reads capture files for the traffic viewer, with the TLS key log beside them.

use super::Sampler;
use borg_core::{
    Capture, CaptureAccess, CaptureFailure, CaptureRequest, HEADER_BYTES, Msg, PACKET_LIMIT, PcapCounter, SeenSockets,
    Tool,
};
use borg_i18n::tr;
use rustix::process::{Pid, Signal, kill_process};
use std::{
    fs::OpenOptions,
    io::{ErrorKind, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    process::Stdio,
    sync::mpsc::{Receiver, RecvTimeoutError, channel},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::{Child, Command},
    sync::{mpsc::UnboundedSender, oneshot},
    task::JoinHandle,
    time::Instant,
};

mod elevated;

pub use elevated::helper;

const PROGRESS_EVERY: Duration = Duration::from_secs(1);
/// How often an agent's capture looks at the sockets, as the network screen samples them.
const SOCKETS_EVERY: Duration = Duration::from_secs(1);
const READ_CHUNK: usize = 64 * 1024;
/// Captures stop at this size, as they do at `PACKET_LIMIT` packets.
pub(crate) const SIZE_LIMIT: u64 = 256 * 1024 * 1024;
/// Capture files larger than this are not read whole into the viewer.
const READ_LIMIT: u64 = SIZE_LIMIT;
const FILE_MODE: u32 = 0o600;
const FILE_PREFIX: &str = "capture-";
/// The extension of the TLS key log beside a capture, named after it.
const KEY_LOG_EXTENSION: &str = "keylog";
const STAMP_FORMAT: &str = "%Y%m%d-%H%M%S";
/// What tcpdump's own messages start with.
const MESSAGE_PREFIX: &str = "tcpdump: ";
/// tcpdump's lines that report progress rather than a failure.
const PROGRESS_LINES: [&str; 3] = ["listening on", "verbose output suppressed", "data link type"];
const PERMISSION_WORDS: [&str; 3] = ["permission", "not permitted", "bpf device"];
/// macOS's interface that tags each packet with its process, before the interfaces it covers.
const PKTAP: &str = "pktap,";
/// What ends tcpdump's options, so that the filter is read as a filter whatever it starts with.
const END_OF_OPTIONS: &str = "--";

/// What capturing as administrator can do on this system.
pub(crate) fn access() -> CaptureAccess {
    let administrator = rustix::process::geteuid().is_root();
    CaptureAccess { administrator, elevate: !administrator && elevated::available(), tags: cfg!(target_os = "macos") }
}

/// The running capture, and how to ask it to stop.
pub(super) struct Recorder {
    id: u64,
    stop: Option<oneshot::Sender<()>>,
    task: JoinHandle<()>,
}

impl Recorder {
    pub(super) fn start(id: u64, request: CaptureRequest, messages: UnboundedSender<Msg>) -> Self {
        let (stop, stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            let progress = |bytes, packets| {
                let _ = messages.send(Msg::CaptureProgress { id, bytes, packets });
            };
            let result = match crate::tools::find(Tool::Tcpdump.program()) {
                Some(program) => record(&program, &request, progress, stopped).await,
                None => Err(CaptureFailure::Missing(crate::tools::installer())),
            };
            let _ = messages.send(Msg::CaptureDone { id, result });
        });
        Self { id, stop: Some(stop), task }
    }

    /// Asks tcpdump to finish; the capture so far is still saved and read.
    pub(super) fn stop(&mut self, id: u64) {
        if self.id == id
            && let Some(stop) = self.stop.take()
        {
            let _ = stop.send(());
        }
    }

    pub(super) fn abort(&self) {
        self.task.abort();
    }
}

/// Reads a capture file opened from a panel.
pub(super) fn open(path: PathBuf, messages: UnboundedSender<Msg>) {
    tokio::spawn(async move {
        let read = path.clone();
        let result = tokio::task::spawn_blocking(move || read_file(&read))
            .await
            .unwrap_or_else(|error| Err(error.to_string()))
            .map_err(|error| tr!(capture_cannot_read, path = path.display(), error = error));
        let _ = messages.send(Msg::CaptureOpened { path, result });
    });
}

/// Captures for an agent as the network screen does, for as long as `request` says, and looks
/// at the sockets every second meanwhile to tell each conversation's process.
pub(crate) async fn record_for_agent(request: &CaptureRequest) -> Result<(PathBuf, Capture, SeenSockets), String> {
    let program = crate::tools::find(Tool::Tcpdump.program()).ok_or_else(|| tr!(capture_missing).to_owned())?;
    let (done, watching) = channel::<()>();
    let watcher = tokio::task::spawn_blocking(move || watch_sockets(&watching));
    let (_stop, stopped) = oneshot::channel();
    let recorded = record(&program, request, |_, _| {}, stopped).await.map_err(|failure| refused(failure, request));
    drop(done);
    let seen = watcher.await.unwrap_or_default();
    recorded.map(|(path, capture)| (path, capture, seen))
}

/// Why an agent's capture saved nothing. Agents never capture as administrator, so a capture
/// the system refused says so, as it does where Borg cannot ask.
fn refused(failure: CaptureFailure, request: &CaptureRequest) -> String {
    match failure {
        CaptureFailure::Failed(error) => error,
        CaptureFailure::Missing(_) => tr!(capture_missing).to_owned(),
        CaptureFailure::Denied | CaptureFailure::Cancelled => tr!(capture_denied, interface = request.interface),
    }
}

/// The sockets seen from now until `done` closes, sampled every `SOCKETS_EVERY`.
fn watch_sockets(done: &Receiver<()>) -> SeenSockets {
    let mut sampler = Sampler::default();
    let mut seen = SeenSockets::default();
    loop {
        if let Ok((sockets, names)) = sampler.sockets() {
            seen.saw(&sockets, &names);
        }
        if done.recv_timeout(SOCKETS_EVERY) != Err(RecvTimeoutError::Timeout) {
            break;
        }
    }
    if let Ok((sockets, names)) = sampler.sockets() {
        seen.saw(&sockets, &names);
    }
    seen
}

/// Reads a capture file, saying what went wrong as the viewer does.
pub(crate) fn read_capture(path: &Path) -> Result<Capture, String> {
    read_file(path).map_err(|error| tr!(capture_cannot_read, path = path.display(), error = error))
}

fn read_file(path: &Path) -> Result<Capture, String> {
    if std::fs::metadata(path).map_err(|error| error.to_string())?.len() > READ_LIMIT {
        return Err(tr!(capture_too_large, limit = borg_ui::size_text(READ_LIMIT)));
    }
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    let mut capture = Capture::read(&bytes).map_err(|error| error.describe().to_owned())?;
    if let Some(text) = key_log_beside(path) {
        capture.add_keys(&text);
    }
    Ok(capture)
}

/// The TLS key log beside a capture and named after it, when there is one Borg can read.
fn key_log_beside(path: &Path) -> Option<String> {
    let keys = path.with_extension(KEY_LOG_EXTENSION);
    let small = std::fs::metadata(&keys).ok()?.len() <= READ_LIMIT;
    small.then(|| std::fs::read_to_string(&keys).ok()).flatten()
}

/// What tcpdump is asked to capture, the same whether Borg runs it or its helper does.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Tcpdump {
    interface: String,
    headers_only: bool,
    /// Capture on macOS's `pktap`, which tags each packet with its process.
    processes: bool,
    filter: String,
}

impl Tcpdump {
    fn of(request: &CaptureRequest) -> Self {
        Self {
            interface: request.interface.clone(),
            headers_only: request.headers_only,
            processes: request.processes,
            filter: request.filter.clone(),
        }
    }

    /// tcpdump writing raw packets to its output as each one arrives, without looking up names,
    /// and stopping at `PACKET_LIMIT` packets.
    fn args(&self) -> Vec<String> {
        let snap = if self.headers_only { HEADER_BYTES } else { 0 };
        let interface = if self.processes { format!("{PKTAP}{}", self.interface) } else { self.interface.clone() };
        let mut args: Vec<String> =
            ["-i", &interface, "-U", "-n", "-s", &snap.to_string(), "-c", &PACKET_LIMIT.to_string()]
                .into_iter()
                .chain(["-w", "-"])
                .map(str::to_owned)
                .collect();
        if !self.filter.is_empty() {
            args.extend([END_OF_OPTIONS.to_owned(), self.filter.clone()]);
        }
        args
    }
}

/// Where a capture's packets come from, and how to end it.
enum Tap {
    /// tcpdump run as you, and what it says on its error output.
    Own { child: Child, errors: JoinHandle<String> },
    /// tcpdump run as administrator by Borg's helper.
    Elevated(elevated::Session),
}

impl Tap {
    /// Asks tcpdump to stop as Ctrl-C would, so it finishes the packet it is writing.
    fn stop(&mut self) {
        match self {
            Self::Own { child, .. } => interrupt(child),
            Self::Elevated(session) => session.stop(),
        }
    }

    /// Waits for tcpdump to end, and gives what it said, or why its helper failed.
    async fn finish(self) -> Result<String, CaptureFailure> {
        match self {
            Self::Own { mut child, errors } => {
                let _ = child.wait().await;
                Ok(errors.await.unwrap_or_default())
            }
            Self::Elevated(session) => session.finish().await,
        }
    }
}

/// tcpdump run as you, and its packets.
fn own(program: &Path, request: &CaptureRequest) -> Result<(Box<dyn AsyncRead + Unpin + Send>, Tap), CaptureFailure> {
    let mut child = command(program, &Tcpdump::of(request)).spawn().map_err(|error| match error.kind() {
        ErrorKind::NotFound => CaptureFailure::Failed(tr!(capture_missing).to_owned()),
        _ => CaptureFailure::Failed(tr!(capture_failed, error = error)),
    })?;
    let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
        return Err(CaptureFailure::Failed(tr!(capture_failed, error = ErrorKind::BrokenPipe)));
    };
    Ok((Box::new(stdout), Tap::Own { child, errors: tokio::spawn(read_all(stderr)) }))
}

/// Runs tcpdump as `request` asks and saves what it writes, telling `progress` the bytes and
/// packets so far every second, until the deadline, `stopped`, or a limit.
async fn record(
    program: &Path,
    request: &CaptureRequest,
    progress: impl Fn(u64, usize),
    mut stopped: oneshot::Receiver<()>,
) -> Result<(PathBuf, Capture), CaptureFailure> {
    let (mut packets, mut tap) =
        if request.administrator { elevated::start(request, &mut stopped).await? } else { own(program, request)? };
    let path = request.folder.join(file_name(request));
    let mut file = match OpenOptions::new().write(true).create_new(true).mode(FILE_MODE).open(&path) {
        Ok(file) => file,
        Err(error) => {
            tap.stop();
            let _ = tap.finish().await;
            return Err(CaptureFailure::Failed(tr!(capture_cannot_save, error = error)));
        }
    };
    let deadline = request.seconds.map(|seconds| Instant::now() + Duration::from_secs(seconds));
    let mut counter = PcapCounter::default();
    let mut written: u64 = 0;
    let mut chunk = vec![0; READ_CHUNK];
    let mut ticks = tokio::time::interval(PROGRESS_EVERY);
    let mut stopping = false;
    let mut failure = None;
    loop {
        tokio::select! {
            read = packets.read(&mut chunk) => match read {
                Ok(0) => break,
                Ok(count) => {
                    if let Err(error) = file.write_all(&chunk[..count]) {
                        failure = Some(tr!(capture_cannot_save, error = error));
                        tap.stop();
                        break;
                    }
                    counter.feed(&chunk[..count]);
                    written += count as u64;
                    if written >= SIZE_LIMIT && !stopping {
                        stopping = true;
                        tap.stop();
                    }
                }
                Err(error) => {
                    failure = Some(tr!(capture_failed, error = error));
                    break;
                }
            },
            _ = ticks.tick() => progress(written, counter.packets),
            _ = sleep_until(deadline), if !stopping => {
                stopping = true;
                tap.stop();
            }
            _ = &mut stopped, if !stopping => {
                stopping = true;
                tap.stop();
            }
        }
    }
    drop(packets);
    let said = tap.finish().await;
    let saved = file.sync_all();
    drop(file);
    if let Some(failure) = failure {
        let _ = std::fs::remove_file(&path);
        return Err(CaptureFailure::Failed(failure));
    }
    if written == 0 {
        let _ = std::fs::remove_file(&path);
        return Err(explain(&said?));
    }
    saved.map_err(|error| CaptureFailure::Failed(tr!(capture_cannot_save, error = error)))?;
    let read = path.clone();
    let capture = tokio::task::spawn_blocking(move || read_file(&read))
        .await
        .unwrap_or_else(|error| Err(error.to_string()))
        .map_err(|error| CaptureFailure::Failed(tr!(capture_cannot_read, path = path.display(), error = error)))?;
    Ok((path, capture))
}

fn command(program: &Path, tcpdump: &Tcpdump) -> Command {
    let mut command = Command::new(program);
    command.args(tcpdump.args()).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    command
}

fn file_name(request: &CaptureRequest) -> String {
    let stamp = jiff::Zoned::now().strftime(STAMP_FORMAT).to_string();
    format!("{FILE_PREFIX}{stamp}.{}", request.extension())
}

fn interrupt(child: &Child) {
    if let Some(pid) = child.id().and_then(|id| i32::try_from(id).ok()).and_then(Pid::from_raw) {
        let _ = kill_process(pid, Signal::INT);
    }
}

async fn sleep_until(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

async fn read_all(mut stream: impl AsyncRead + Unpin) -> String {
    let mut text = String::new();
    let _ = stream.read_to_string(&mut text).await;
    text
}

/// Why tcpdump wrote nothing, from what it said.
fn explain(errors: &str) -> CaptureFailure {
    let lowered = errors.to_lowercase();
    if PERMISSION_WORDS.iter().any(|word| lowered.contains(word)) {
        return CaptureFailure::Denied;
    }
    let reason = errors
        .lines()
        .map(|line| line.strip_prefix(MESSAGE_PREFIX).unwrap_or(line).trim())
        .find(|line| !line.is_empty() && !PROGRESS_LINES.iter().any(|progress| line.starts_with(progress)))
        .unwrap_or_default();
    CaptureFailure::Failed(tr!(capture_failed, error = reason))
}

#[cfg(test)]
mod tests;
