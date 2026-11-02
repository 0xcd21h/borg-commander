//! Capturing as administrator. Borg listens on a Unix socket in a folder only you can open and
//! asks the system for your password to run its own helper as root: osascript on macOS, pkexec
//! or else sudo on Linux. The helper connects, checks what it was asked, and starts tcpdump with
//! the socket as its output, so the packets stream to Borg, which saves them as you. Borg ends
//! the capture by closing its side of the socket, which it also does when it exits: the helper
//! then interrupts tcpdump as Ctrl-C would.

use super::{PROGRAM, SYSTEM_DIRS, Tap, Tcpdump, interrupt, read_all};
use anyhow::{Context, Result, bail};
use borg_core::{CaptureFailure, CaptureRequest};
use borg_i18n::tr;
use std::{
    io::ErrorKind,
    os::{fd::OwnedFd, unix::fs::MetadataExt},
    path::{Path, PathBuf},
    process::{ExitStatus, Stdio},
    time::Duration,
};
use tokio::{
    io::AsyncRead,
    net::{UnixListener, UnixStream, unix::OwnedWriteHalf},
    process::{Child, Command},
    sync::oneshot,
    task::JoinHandle,
};

/// The argument that runs Borg as the capture helper.
const HELPER: &str = "--capture-traffic";
const SOCKET: &str = "tap";
const FOLDER_PREFIX: &str = "borg-capture-";
/// How long tcpdump has to finish after the interrupt before the helper kills it.
const STOP_GRACE: Duration = Duration::from_secs(5);
const HANGUP_CHUNK: usize = 64;
const ROOT: u32 = 0;
/// The mode bits that let others than a file's owner change it.
const WRITABLE_BY_OTHERS: u32 = 0o022;
const NO: &str = "0";
const YES: &str = "1";
/// The longest interface name and filter the helper takes.
const INTERFACE_LIMIT: usize = 64;
const FILTER_LIMIT: usize = 512;
/// What an interface name holds besides letters and digits.
const INTERFACE_MARKS: [char; 3] = ['.', '_', '-'];
/// What the network screen's filters hold besides letters and digits.
const FILTER_MARKS: [char; 5] = [' ', '.', ':', '(', ')'];
/// What osascript reports when its password dialog is cancelled, and what comes before its reason.
const OSASCRIPT_CANCELLED: &str = "(-128)";
const OSASCRIPT_ERROR: &str = "execution error: ";
/// pkexec's exit code when its password dialog is dismissed.
const PKEXEC_DISMISSED: i32 = 126;
#[cfg(target_os = "macos")]
const OSASCRIPT: &str = "osascript";
#[cfg(target_os = "linux")]
const LAUNCHERS: [(&str, &str); 2] = [("pkexec", "--disable-internal-agent"), ("sudo", "-n")];

/// Whether Borg can ask the system to run its helper as root.
pub(super) fn available() -> bool {
    #[cfg(target_os = "macos")]
    let launchers = [OSASCRIPT];
    #[cfg(target_os = "linux")]
    let launchers = LAUNCHERS.map(|(name, _)| name);
    launchers.iter().any(|name| trusted(name).is_some())
}

/// A program in a system folder that only root can change.
fn trusted(name: &str) -> Option<PathBuf> {
    SYSTEM_DIRS.iter().map(|dir| Path::new(dir).join(name)).find(|path| {
        std::fs::metadata(path)
            .is_ok_and(|meta| meta.is_file() && meta.uid() == ROOT && meta.mode() & WRITABLE_BY_OTHERS == 0)
    })
}

/// The system's way to run `words` as root once you give your password.
#[cfg(target_os = "macos")]
fn launcher(words: &[String], interface: &str) -> Option<Command> {
    let prompt = tr!(capture_admin_prompt, interface = interface);
    let script = crate::admin::apple_script_prompting(words.iter().map(String::as_str), &prompt).ok()?;
    let mut command = Command::new(trusted(OSASCRIPT)?);
    command.arg("-e").arg(script);
    Some(command)
}

#[cfg(target_os = "linux")]
fn launcher(words: &[String], _interface: &str) -> Option<Command> {
    let (program, flag) = LAUNCHERS.iter().find_map(|(name, flag)| Some((trusted(name)?, *flag)))?;
    let mut command = Command::new(program);
    command.arg(flag).args(words);
    Some(command)
}

impl Tcpdump {
    /// What the helper is told to capture: the interface, the two choices, and the filter.
    fn helper_args(&self) -> Vec<String> {
        let flag = |on: bool| if on { YES } else { NO }.to_owned();
        let mut args = vec![self.interface.clone(), flag(self.headers_only), flag(self.processes)];
        if !self.filter.is_empty() {
            args.push(self.filter.clone());
        }
        args
    }

    /// What the helper was told, when it is what Borg asks for and nothing else.
    fn parse(args: &[String]) -> Option<Self> {
        let flag = |text: &str| match text {
            NO => Some(false),
            YES => Some(true),
            _ => None,
        };
        let (interface, headers, processes, filter) = match args {
            [interface, headers, processes] => (interface, headers, processes, ""),
            [interface, headers, processes, filter] => (interface, headers, processes, filter.as_str()),
            _ => return None,
        };
        let named = interface.len() <= INTERFACE_LIMIT
            && interface.starts_with(|first: char| first.is_ascii_alphanumeric())
            && interface.chars().all(|c| c.is_ascii_alphanumeric() || INTERFACE_MARKS.contains(&c));
        let filtered = filter.len() <= FILTER_LIMIT
            && filter.chars().all(|c| c.is_ascii_alphanumeric() || FILTER_MARKS.contains(&c));
        let (headers_only, processes) = (flag(headers)?, flag(processes)?);
        let tagged = !processes || cfg!(target_os = "macos");
        (named && filtered && tagged).then(|| Self {
            interface: interface.clone(),
            headers_only,
            processes,
            filter: filter.to_owned(),
        })
    }
}

/// The helper's side of a capture while it runs.
pub(super) struct Session {
    launcher: Child,
    /// What the helper prints when it ends, and what the launcher says.
    output: JoinHandle<String>,
    said: JoinHandle<String>,
    writer: Option<OwnedWriteHalf>,
    _folder: tempfile::TempDir,
}

impl Session {
    /// Closes Borg's side of the socket, which the helper takes as Ctrl-C.
    pub(super) fn stop(&mut self) {
        self.writer.take();
    }

    pub(super) async fn finish(mut self) -> Result<String, CaptureFailure> {
        self.writer.take();
        let status = self.launcher.wait().await.ok();
        let output = self.output.await.unwrap_or_default();
        let said = self.said.await.unwrap_or_default();
        outcome(&output, status, &said)
    }
}

/// What tcpdump said, from the helper's report, or why the launcher or the helper failed.
fn outcome(output: &str, status: Option<ExitStatus>, said: &str) -> Result<String, CaptureFailure> {
    match serde_json::from_str::<Result<String, String>>(output.trim()) {
        Ok(Ok(errors)) => Ok(errors),
        Ok(Err(error)) => Err(CaptureFailure::Failed(error)),
        Err(_) => Err(refused(status, said)),
    }
}

/// Why the launcher ended without running the helper: the dialog was cancelled, or the system
/// gave a reason.
fn refused(status: Option<ExitStatus>, said: &str) -> CaptureFailure {
    let dismissed = status.and_then(|status| status.code()) == Some(PKEXEC_DISMISSED);
    if dismissed || said.contains(OSASCRIPT_CANCELLED) {
        return CaptureFailure::Cancelled;
    }
    let reason = said.split_once(OSASCRIPT_ERROR).map_or(said, |(_, reason)| reason).trim();
    CaptureFailure::Failed(tr!(capture_admin_unavailable, error = reason))
}

/// What the wait for the helper ended with.
enum Waited {
    Connected(std::io::Result<UnixStream>),
    Ended,
    Stopped,
}

/// Asks for your password and waits for the helper to connect; stopping first cancels.
pub(super) async fn start(
    request: &CaptureRequest,
    stopped: &mut oneshot::Receiver<()>,
) -> Result<(Box<dyn AsyncRead + Unpin + Send>, Tap), CaptureFailure> {
    let failed = |error: std::io::Error| CaptureFailure::Failed(tr!(capture_failed, error = error));
    let unavailable = || CaptureFailure::Failed(tr!(capture_admin_unavailable, error = ErrorKind::Unsupported));
    let folder = tempfile::Builder::new().prefix(FOLDER_PREFIX).tempdir().map_err(failed)?;
    let socket = folder.path().join(SOCKET);
    let listener = UnixListener::bind(&socket).map_err(failed)?;
    let exe = std::env::current_exe().map_err(failed)?;
    let mut words: Vec<String> = [exe.to_str(), Some(HELPER), socket.to_str()]
        .into_iter()
        .map(|word| word.map(str::to_owned))
        .collect::<Option<_>>()
        .ok_or_else(unavailable)?;
    words.extend(Tcpdump::of(request).helper_args());
    let mut launcher = launcher(&words, &request.interface).ok_or_else(unavailable)?;
    let mut launcher = launcher
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(failed)?;
    let (Some(stdout), Some(stderr)) = (launcher.stdout.take(), launcher.stderr.take()) else {
        return Err(failed(ErrorKind::BrokenPipe.into()));
    };
    let (output, said) = (tokio::spawn(read_all(stdout)), tokio::spawn(read_all(stderr)));
    let waited = tokio::select! {
        accepted = listener.accept() => Waited::Connected(accepted.map(|(stream, _)| stream)),
        _ = launcher.wait() => Waited::Ended,
        _ = &mut *stopped => Waited::Stopped,
    };
    let stream = match waited {
        Waited::Connected(stream) => stream.map_err(failed)?,
        Waited::Ended => {
            let status = launcher.wait().await.ok();
            let output = output.await.unwrap_or_default();
            let said = said.await.unwrap_or_default();
            return Err(outcome(&output, status, &said).err().unwrap_or_else(|| refused(status, "")));
        }
        Waited::Stopped => {
            let _ = launcher.start_kill();
            return Err(CaptureFailure::Cancelled);
        }
    };
    if stream.peer_cred().map_err(failed)?.uid() != ROOT {
        return Err(CaptureFailure::Failed(tr!(capture_helper_not_root).to_owned()));
    }
    let (reader, writer) = stream.into_split();
    let session = Session { launcher, output, said, writer: Some(writer), _folder: folder };
    Ok((Box::new(reader), Tap::Elevated(session)))
}

/// Borg as the capture helper, run as root: connects to the socket, runs tcpdump into it until
/// Borg closes its side, and prints what tcpdump said, or why it could not run it.
pub async fn helper(args: &[String]) -> Result<()> {
    let result = serve(args).await.map_err(|error| error.to_string());
    println!("{}", serde_json::to_string(&result)?);
    Ok(())
}

async fn serve(args: &[String]) -> Result<String> {
    let (socket, rest) = args.split_first().context(tr!(capture_request_invalid))?;
    let tcpdump = Tcpdump::parse(rest).context(tr!(capture_request_invalid))?;
    let socket = Path::new(socket);
    if !socket.is_absolute() {
        bail!(tr!(capture_request_invalid));
    }
    let program = trusted(PROGRAM).context(tr!(capture_missing))?;
    run(socket, &tcpdump, &program).await
}

/// What the helper's wait for tcpdump ended with.
enum Ended {
    HungUp,
    Exited,
}

async fn run(socket: &Path, tcpdump: &Tcpdump, program: &Path) -> Result<String> {
    let stream = std::os::unix::net::UnixStream::connect(socket)?;
    let output = OwnedFd::from(stream.try_clone()?);
    stream.set_nonblocking(true)?;
    let watch = UnixStream::from_std(stream)?;
    let mut child = Command::new(program)
        .args(tcpdump.args())
        .stdin(Stdio::null())
        .stdout(output)
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let errors = tokio::spawn(read_all(child.stderr.take().context(tr!(capture_request_invalid))?));
    let ended = tokio::select! {
        _ = hangup(&watch) => Ended::HungUp,
        _ = child.wait() => Ended::Exited,
    };
    if let Ended::HungUp = ended {
        interrupt(&child);
        if tokio::time::timeout(STOP_GRACE, child.wait()).await.is_err() {
            let _ = child.kill().await;
        }
    }
    drop(watch);
    Ok(errors.await.unwrap_or_default())
}

/// Waits until Borg closes its side of the socket, or is gone.
async fn hangup(stream: &UnixStream) {
    let mut chunk = [0; HANGUP_CHUNK];
    loop {
        if stream.readable().await.is_err() {
            return;
        }
        match stream.try_read(&mut chunk) {
            Ok(0) => return,
            Ok(_) => {}
            Err(error) if error.kind() == ErrorKind::WouldBlock => {}
            Err(_) => return,
        }
    }
}

#[cfg(test)]
mod tests;
