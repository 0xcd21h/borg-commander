use std::{
    env,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::mpsc::{Receiver, RecvTimeoutError, channel},
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use borg_term::{Emulator, GridSize};
use borg_ui::Palette;
use portable_pty::{Child, CommandBuilder, ExitStatus, MasterPty, native_pty_system};

use crate::{Fixture, Shell, Snapshot};

const DEFAULT_COLS: u16 = 100;
const DEFAULT_ROWS: u16 = 30;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);
const ENV_TIMEOUT: &str = "BORG_TEST_TIMEOUT_SECS";
const QUIET: Duration = Duration::from_millis(250);
const POLL: Duration = Duration::from_millis(10);
const READ_CHUNK: usize = 64 * 1024;
const SCROLLBACK: usize = 0;
const SCREENSHOT_EXTENSION: &str = "png";
const TEXT_EXTENSION: &str = "txt";

const ENV_SCREENSHOTS: &str = "BORG_SCREENSHOTS";
const ENV_PATH: &str = "PATH";
const ENV_HOME: &str = "HOME";
const ENV_SHELL: &str = "SHELL";
const ENV_TERM_PROGRAM: &str = "TERM_PROGRAM";
const ENV_CONFIG_DIR: &str = "BORG_CONFIG_DIR";
const ENV_RUNTIME_DIR: &str = "XDG_RUNTIME_DIR";
const ENV_STATE_DIR: &str = "BORG_STATE_DIR";
const PASSTHROUGH_ENV: [&str; 1] = [ENV_PATH];
const FIXED_ENV: [(&str, &str); 5] = [
    ("TERM", "xterm-256color"),
    ("TZ", "UTC"),
    ("LANG", "C.UTF-8"),
    ("LC_ALL", "C.UTF-8"),
    ("BASH_SILENCE_DEPRECATION_WARNING", "1"),
];
const TRUECOLOR_ENV: (&str, &str) = ("COLORTERM", "truecolor");

enum Output {
    Bytes(Vec<u8>),
    Closed,
}

pub struct SessionBuilder<'a> {
    binary: PathBuf,
    fixture: &'a Fixture,
    shell: Shell,
    cwd: Option<PathBuf>,
    cols: u16,
    rows: u16,
    truecolor: bool,
    terminal_program: Option<String>,
    timeout: Duration,
    label: String,
}

impl<'a> SessionBuilder<'a> {
    pub fn shell(mut self, shell: Shell) -> Self {
        self.shell = shell;
        self
    }

    pub fn cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    pub fn size(mut self, cols: u16, rows: u16) -> Self {
        self.cols = cols;
        self.rows = rows;
        self
    }

    pub fn truecolor(mut self, truecolor: bool) -> Self {
        self.truecolor = truecolor;
        self
    }

    pub fn terminal_program(mut self, program: &str) -> Self {
        self.terminal_program = Some(program.to_owned());
        self
    }

    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    pub fn spawn(self) -> Result<Session> {
        let shell = self.shell.locate().with_context(|| format!("{} is not installed", self.shell.program()))?;
        let grid = GridSize { cols: self.cols, rows: self.rows };
        let pair = native_pty_system().openpty(grid.into()).context("opening pty")?;

        let mut cmd = CommandBuilder::new(&self.binary);
        cmd.env_clear();
        PASSTHROUGH_ENV.iter().filter_map(|name| Some((name, env::var_os(name)?))).for_each(|(k, v)| cmd.env(k, v));
        FIXED_ENV.iter().for_each(|(k, v)| cmd.env(k, v));
        if self.truecolor {
            cmd.env(TRUECOLOR_ENV.0, TRUECOLOR_ENV.1);
        }
        if let Some(program) = self.terminal_program {
            cmd.env(ENV_TERM_PROGRAM, program);
        }
        cmd.env(ENV_HOME, self.fixture.home());
        cmd.env(ENV_SHELL, &shell);
        cmd.env(ENV_CONFIG_DIR, self.fixture.config());
        cmd.env(ENV_RUNTIME_DIR, self.fixture.runtime());
        cmd.env(ENV_STATE_DIR, self.fixture.state());
        cmd.cwd(self.cwd.unwrap_or_else(|| self.fixture.work()));

        let child = pair.slave.spawn_command(cmd).context("spawning borg")?;
        drop(pair.slave);

        let reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;
        let (tx, rx) = channel();
        thread::spawn(move || pump(reader, &tx));

        Ok(Session {
            child,
            master: pair.master,
            writer,
            output: rx,
            emulator: Emulator::new(grid, SCROLLBACK),
            palette: Palette::default(),
            timeout: self.timeout,
            screenshots: env::var_os(ENV_SCREENSHOTS).map(|dir| PathBuf::from(dir).join(&self.label)),
            closed: false,
        })
    }
}

pub struct Session {
    child: Box<dyn Child + Send + Sync>,
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    output: Receiver<Output>,
    emulator: Emulator,
    palette: Palette,
    timeout: Duration,
    screenshots: Option<PathBuf>,
    closed: bool,
}

impl Session {
    pub fn builder(binary: impl AsRef<Path>, fixture: &Fixture) -> SessionBuilder<'_> {
        SessionBuilder {
            binary: binary.as_ref().to_path_buf(),
            fixture,
            shell: Shell::Bash,
            cwd: None,
            cols: DEFAULT_COLS,
            rows: DEFAULT_ROWS,
            truecolor: true,
            terminal_program: None,
            timeout: configured_timeout(),
            label: String::from("session"),
        }
    }

    pub fn process_id(&self) -> Option<u32> {
        self.child.process_id()
    }

    pub fn send(&mut self, bytes: impl AsRef<[u8]>) -> Result<&mut Self> {
        self.writer.write_all(bytes.as_ref())?;
        self.writer.flush()?;
        Ok(self)
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot::capture(&self.emulator.screen(), &self.palette)
    }

    pub fn wait_until(&mut self, description: &str, check: impl Fn(&Snapshot) -> bool) -> Result<Snapshot> {
        let deadline = Instant::now() + self.timeout;
        loop {
            let snapshot = self.snapshot();
            if check(&snapshot) {
                return Ok(snapshot);
            }
            if self.closed || Instant::now() >= deadline {
                let state = if self.closed { "borg exited" } else { "timed out" };
                bail!("{state} waiting for {description}\n{}", frame(&snapshot));
            }
            self.receive(POLL)?;
        }
    }

    pub fn wait_text(&mut self, text: &str) -> Result<Snapshot> {
        self.wait_until(&format!("{text:?} on screen"), |s| s.contains(text))
    }

    pub fn wait_gone(&mut self, text: &str) -> Result<Snapshot> {
        self.wait_until(&format!("{text:?} to disappear"), |s| !s.contains(text))
    }

    pub fn pump(&mut self, duration: Duration) -> Result<()> {
        let deadline = Instant::now() + duration;
        while let Some(left) = deadline.checked_duration_since(Instant::now()).filter(|left| !left.is_zero()) {
            self.receive(left.min(POLL))?;
        }
        Ok(())
    }

    pub fn settle(&mut self) -> Result<Snapshot> {
        let deadline = Instant::now() + self.timeout;
        let mut quiet_since = Instant::now();
        while Instant::now() < deadline && quiet_since.elapsed() < QUIET {
            if self.receive(POLL)? {
                quiet_since = Instant::now();
            }
        }
        Ok(self.snapshot())
    }

    pub fn checkpoint(&mut self, name: &str) -> Result<Snapshot> {
        let snapshot = self.settle()?;
        if let Some(dir) = &self.screenshots {
            snapshot.save_png(&dir.join(name).with_extension(SCREENSHOT_EXTENSION))?;
            std::fs::write(dir.join(name).with_extension(TEXT_EXTENSION), snapshot.text())?;
        }
        Ok(snapshot)
    }

    pub fn resize(&mut self, cols: u16, rows: u16) -> Result<()> {
        let grid = GridSize { cols, rows };
        self.emulator.resize(grid);
        self.master.resize(grid.into())
    }

    pub fn wait_exit(&mut self) -> Result<ExitStatus> {
        let deadline = Instant::now() + self.timeout;
        loop {
            if let Some(status) = self.child.try_wait()? {
                return Ok(status);
            }
            if Instant::now() >= deadline {
                bail!("timed out waiting for borg to exit\n{}", frame(&self.snapshot()));
            }
            self.receive(POLL)?;
        }
    }

    fn receive(&mut self, wait: Duration) -> Result<bool> {
        let first = match self.output.recv_timeout(wait) {
            Ok(output) => output,
            Err(RecvTimeoutError::Timeout) => return Ok(false),
            Err(RecvTimeoutError::Disconnected) => Output::Closed,
        };
        let pending: Vec<_> = std::iter::once(first).chain(self.output.try_iter()).collect();
        for output in pending {
            match output {
                Output::Bytes(bytes) => {
                    self.emulator.feed(&bytes);
                    for reply in self.emulator.drain_replies() {
                        self.writer.write_all(reply.as_bytes())?;
                    }
                }
                Output::Closed => self.closed = true,
            }
        }
        self.writer.flush()?;
        Ok(true)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

fn pump(mut reader: Box<dyn Read + Send>, tx: &std::sync::mpsc::Sender<Output>) {
    let mut buf = vec![0; READ_CHUNK];
    while let Ok(n @ 1..) = reader.read(&mut buf) {
        if tx.send(Output::Bytes(buf[..n].to_vec())).is_err() {
            return;
        }
    }
    let _ = tx.send(Output::Closed);
}

fn configured_timeout() -> Duration {
    env::var(ENV_TIMEOUT).ok().and_then(|secs| secs.parse().ok()).map_or(DEFAULT_TIMEOUT, Duration::from_secs)
}

fn frame(snapshot: &Snapshot) -> String {
    let rule = "─".repeat(usize::from(snapshot.cols()));
    format!("┌{rule}┐\n{}\n└{rule}┘", snapshot.lines().iter().map(|l| format!("│{l}")).collect::<Vec<_>>().join("\n"))
}
