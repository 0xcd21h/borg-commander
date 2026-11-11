use std::{
    ffi::{OsStr, OsString},
    fs::File,
    io::{self, PipeReader, PipeWriter, Read, Write},
    os::{fd::OwnedFd, unix::process::CommandExt},
    path::Path,
    process::{Command, Stdio},
    thread::{self, JoinHandle},
    time::Duration,
};

use anyhow::{Context, Result};
use rustix::{
    event::{PollFd, PollFlags, poll},
    fs::{Mode, OFlags},
    io::{Errno, FdFlags, fcntl_setfd},
    process::{Pid, Signal, WaitOptions, kill_process, waitpid},
    pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt},
    termios::{LocalModes, Winsize, tcgetattr, tcgetpgrp, tcsetwinsize},
};
use tokio::sync::mpsc::UnboundedSender;

use crate::GridSize;

const READ_CHUNK: usize = 64 * 1024;
/// The argument that has a Borg make the terminal its controlling one and then run the command
/// that follows it.
pub const LEAD: &str = "--lead-terminal";
/// How often a program that was hung up on is checked before it is killed.
const HANGUP_CHECKS: u32 = 5;
const HANGUP_CHECK_EVERY: Duration = Duration::from_millis(50);

#[derive(Debug)]
pub enum PtyEvent {
    Output(Vec<u8>),
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Foreground {
    Shell,
    Program,
    Unknown,
}

/// A terminal's program, still running, and the terminal's own end, for another Borg to take
/// over with [`Pty::adopt`].
#[derive(Debug)]
pub struct Detached {
    pub master: OwnedFd,
    pub process: u32,
}

pub struct Pty {
    master: File,
    process: Pid,
    reader: Option<Reader>,
    stopped: bool,
}

/// The thread that reads the terminal, and the pipe whose closing ends it.
struct Reader {
    stop: PipeWriter,
    thread: JoinHandle<()>,
}

impl Pty {
    /// Runs `command` in a new terminal. `launcher` is a Borg, which makes the terminal the
    /// command's controlling one before it runs the command in its place.
    pub fn spawn(
        launcher: &Path,
        command: &Command,
        size: GridSize,
        events: UnboundedSender<PtyEvent>,
    ) -> Result<Self> {
        let master = open_master().context("opening pty")?;
        grantpt(&master).context("opening pty")?;
        unlockpt(&master).context("opening pty")?;
        let name = ptsname(&master, Vec::new()).context("naming pty")?;
        let terminal =
            rustix::fs::open(name.as_c_str(), OFlags::RDWR | OFlags::NOCTTY | OFlags::CLOEXEC, Mode::empty())
                .context("opening pty")?;
        tcsetwinsize(&terminal, winsize(size)).context("sizing pty")?;
        let mut leader = led(launcher, command);
        leader
            .stdin(Stdio::from(terminal.try_clone()?))
            .stdout(Stdio::from(terminal.try_clone()?))
            .stderr(Stdio::from(terminal));
        let child = leader.spawn().context("spawning shell")?;
        let process = pid(child.id()).context("spawning shell")?;
        Self::start(master, process, events)
    }

    /// Takes over a terminal another Borg left running.
    pub fn adopt(detached: Detached, events: UnboundedSender<PtyEvent>) -> Result<Self> {
        let process = pid(detached.process).context("adopting the terminal")?;
        fcntl_setfd(&detached.master, FdFlags::CLOEXEC)?;
        Self::start(detached.master, process, events)
    }

    fn start(master: OwnedFd, process: Pid, events: UnboundedSender<PtyEvent>) -> Result<Self> {
        let reading = File::from(master.try_clone()?);
        let (wake, stop) = io::pipe()?;
        let thread = thread::Builder::new().name("pty-reader".into()).spawn(move || pump(reading, &wake, &events))?;
        Ok(Self { master: File::from(master), process, reader: Some(Reader { stop, thread }), stopped: false })
    }

    /// Stops reading the terminal and lets go of its program, which keeps running with the
    /// output not read yet still in the terminal. This one then neither reads nor stops it.
    pub fn detach(&mut self) -> Result<Detached> {
        self.stop_reading();
        self.stopped = true;
        let master = OwnedFd::from(self.master.try_clone()?);
        Ok(Detached { master, process: self.process.as_raw_nonzero().get().unsigned_abs() })
    }

    fn stop_reading(&mut self) {
        if let Some(Reader { stop, thread }) = self.reader.take() {
            drop(stop);
            let _ = thread.join();
        }
    }

    pub fn stop(&mut self) -> Result<()> {
        if self.stopped {
            return Ok(());
        }
        if !self.exited().context("checking the terminal process")? {
            self.hang_up().context("stopping the terminal process")?;
        }
        match waitpid(Some(self.process), WaitOptions::empty()) {
            Ok(_) | Err(Errno::CHILD) => {}
            Err(error) => return Err(error).context("waiting for the terminal process to stop"),
        }
        self.stopped = true;
        Ok(())
    }

    /// Whether the program has ended, which collects it.
    fn exited(&self) -> io::Result<bool> {
        match waitpid(Some(self.process), WaitOptions::NOHANG) {
            Ok(status) => Ok(status.is_some()),
            Err(Errno::CHILD) => Ok(true),
            Err(error) => Err(error.into()),
        }
    }

    /// Hangs up on the program, as closing its terminal would, and kills it when it does not
    /// end within a moment.
    fn hang_up(&self) -> io::Result<()> {
        match kill_process(self.process, Signal::HUP) {
            Ok(()) => {}
            Err(Errno::SRCH) => return Ok(()),
            Err(error) => return Err(error.into()),
        }
        for _ in 0..HANGUP_CHECKS {
            thread::sleep(HANGUP_CHECK_EVERY);
            if self.exited()? {
                return Ok(());
            }
        }
        match kill_process(self.process, Signal::KILL) {
            Ok(()) | Err(Errno::SRCH) => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    pub fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.master.write_all(bytes)?;
        self.master.flush()
    }

    pub fn resize(&self, size: GridSize) -> Result<()> {
        tcsetwinsize(&self.master, winsize(size)).context("resizing pty")
    }

    pub fn raw_input(&self) -> bool {
        self.foreground() == Foreground::Program
            && tcgetattr(&self.master).is_ok_and(|termios| !termios.local_modes.contains(LocalModes::ICANON))
    }

    pub fn foreground(&self) -> Foreground {
        match tcgetpgrp(&self.master) {
            Ok(leader) if leader == self.process => Foreground::Shell,
            Ok(_) => Foreground::Program,
            Err(_) => Foreground::Unknown,
        }
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        self.stop_reading();
        if !self.stopped {
            let _ = self.hang_up();
            let process = self.process;
            let _ = thread::Builder::new()
                .name("pty-reaper".into())
                .spawn(move || waitpid(Some(process), WaitOptions::empty()));
        }
    }
}

/// Makes the terminal on standard input the controlling terminal of a new session, then runs
/// `program` in this process's place. It returns only when that fails.
pub fn lead(program: &OsStr, args: &[OsString]) -> io::Error {
    if let Err(error) = rustix::process::setsid() {
        return error.into();
    }
    if let Err(error) = rustix::process::ioctl_tiocsctty(io::stdin()) {
        return error.into();
    }
    Command::new(program).args(args).exec()
}

/// The command that has `launcher` lead the terminal and run `command`, with its folder and
/// environment.
fn led(launcher: &Path, command: &Command) -> Command {
    let mut leader = Command::new(launcher);
    leader.arg(LEAD).arg(command.get_program()).args(command.get_args());
    for (key, value) in command.get_envs() {
        match value {
            Some(value) => leader.env(key, value),
            None => leader.env_remove(key),
        };
    }
    if let Some(dir) = command.get_current_dir() {
        leader.current_dir(dir);
    }
    leader
}

#[cfg(target_os = "linux")]
fn open_master() -> io::Result<OwnedFd> {
    Ok(openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY | OpenptFlags::CLOEXEC)?)
}

#[cfg(not(target_os = "linux"))]
fn open_master() -> io::Result<OwnedFd> {
    let master = openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY)?;
    fcntl_setfd(&master, FdFlags::CLOEXEC)?;
    Ok(master)
}

fn pid(raw: u32) -> io::Result<Pid> {
    i32::try_from(raw).ok().and_then(Pid::from_raw).ok_or_else(|| Errno::SRCH.into())
}

fn winsize(size: GridSize) -> Winsize {
    Winsize { ws_row: size.rows, ws_col: size.cols, ws_xpixel: 0, ws_ypixel: 0 }
}

/// Reads the terminal until its program closes it, or until the pipe from [`Pty`] closes.
fn pump(mut reader: File, wake: &PipeReader, events: &UnboundedSender<PtyEvent>) {
    let mut buf = vec![0; READ_CHUNK];
    loop {
        let mut watched = [PollFd::new(&reader, PollFlags::IN), PollFd::new(wake, PollFlags::IN)];
        match poll(&mut watched, None) {
            Ok(_) => {}
            Err(Errno::INTR) => continue,
            Err(_) => break,
        }
        if !watched[1].revents().is_empty() {
            return;
        }
        match reader.read(&mut buf) {
            Ok(0) => break,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => break,
            Ok(n) => {
                if events.send(PtyEvent::Output(buf[..n].to_vec())).is_err() {
                    return;
                }
            }
        }
    }
    let _ = events.send(PtyEvent::Closed);
}

#[cfg(test)]
mod tests;
