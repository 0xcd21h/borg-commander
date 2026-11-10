use std::{
    io::{self, Read, Write},
    ops::Deref,
    sync::{Arc, Condvar, Mutex},
    thread,
};

use anyhow::{Context, Result};
use portable_pty::{Child, CommandBuilder, MasterPty, native_pty_system};
use tokio::sync::mpsc::UnboundedSender;

use crate::GridSize;

const READ_CHUNK: usize = 64 * 1024;

/// Output Borg holds that nobody has taken in yet, at most. A program that writes faster than
/// Borg shows it waits at this point, as it would in any terminal, and Borg's memory stays put.
const UNREAD_LIMIT: usize = 8 * 1024 * 1024;

/// How much output has been read from a terminal and not yet taken in.
#[derive(Debug, Default)]
struct Unread {
    bytes: Mutex<usize>,
    taken: Condvar,
}

impl Unread {
    fn wait_for_room(&self) {
        let mut bytes = self.bytes.lock().expect("unread output");
        while *bytes >= UNREAD_LIMIT {
            bytes = self.taken.wait(bytes).expect("unread output");
        }
    }

    fn chunk(self: &Arc<Self>, bytes: Vec<u8>) -> Chunk {
        *self.bytes.lock().expect("unread output") += bytes.len();
        Chunk { bytes, unread: Arc::clone(self) }
    }
}

/// A piece of a terminal's output, which counts as unread until it is dropped.
#[derive(Debug)]
pub struct Chunk {
    bytes: Vec<u8>,
    unread: Arc<Unread>,
}

impl Deref for Chunk {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        &self.bytes
    }
}

impl Drop for Chunk {
    fn drop(&mut self) {
        let mut unread = self.unread.bytes.lock().expect("unread output");
        *unread = unread.saturating_sub(self.bytes.len());
        self.unread.taken.notify_all();
    }
}

#[derive(Debug)]
pub enum PtyEvent {
    Output(Chunk),
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Foreground {
    Shell,
    Program,
    Unknown,
}

pub struct Pty {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
    stopped: bool,
}

impl Pty {
    pub fn spawn(cmd: CommandBuilder, size: GridSize, events: UnboundedSender<PtyEvent>) -> Result<Self> {
        let pair = native_pty_system().openpty(size.into()).context("opening pty")?;
        let child = pair.slave.spawn_command(cmd).context("spawning shell")?;
        drop(pair.slave);

        let reader = pair.master.try_clone_reader().context("cloning pty reader")?;
        let writer = pair.master.take_writer().context("taking pty writer")?;
        thread::Builder::new().name("pty-reader".into()).spawn(move || pump(reader, &events))?;

        Ok(Self { master: pair.master, writer, child, stopped: false })
    }

    pub fn stop(&mut self) -> Result<()> {
        if self.stopped {
            return Ok(());
        }
        if self.child.try_wait().context("checking the terminal process")?.is_none() {
            self.child.kill().context("stopping the terminal process")?;
        }
        self.child.wait().context("waiting for the terminal process to stop")?;
        self.stopped = true;
        Ok(())
    }

    pub fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.writer.write_all(bytes)?;
        self.writer.flush()
    }

    pub fn resize(&self, size: GridSize) -> Result<()> {
        self.master.resize(size.into())
    }

    pub fn raw_input(&self) -> bool {
        self.foreground() == Foreground::Program
            && self
                .master
                .get_termios()
                .is_some_and(|termios| termios.local_flags.bits() & rustix::termios::LocalModes::ICANON.bits() == 0)
    }

    pub fn foreground(&self) -> Foreground {
        let shell = self.child.process_id().and_then(|pid| i32::try_from(pid).ok());
        match (self.master.process_group_leader(), shell) {
            (Some(leader), Some(shell)) if leader == shell => Foreground::Shell,
            (Some(_), Some(_)) => Foreground::Program,
            _ => Foreground::Unknown,
        }
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        if !self.stopped {
            let _ = self.child.kill();
        }
    }
}

fn pump(mut reader: Box<dyn Read + Send>, events: &UnboundedSender<PtyEvent>) {
    let unread = Arc::new(Unread::default());
    let mut buf = vec![0; READ_CHUNK];
    loop {
        unread.wait_for_room();
        match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if events.send(PtyEvent::Output(unread.chunk(buf[..n].to_vec()))).is_err() {
                    return;
                }
            }
        }
    }
    let _ = events.send(PtyEvent::Closed);
}

#[cfg(test)]
mod unread_tests {
    use super::*;
    use std::time::Duration;
    use tokio::sync::mpsc::unbounded_channel;

    const SETTLE: Duration = Duration::from_millis(200);

    fn waiting(events: &mut tokio::sync::mpsc::UnboundedReceiver<PtyEvent>) -> Vec<PtyEvent> {
        std::iter::from_fn(|| events.try_recv().ok()).collect()
    }

    fn bytes_of(events: &[PtyEvent]) -> usize {
        events.iter().map(|event| if let PtyEvent::Output(chunk) = event { chunk.len() } else { 0 }).sum()
    }

    /// A program that never stops writing is read only as far as the limit while nobody takes
    /// its output in, and read further once somebody does.
    #[test]
    fn output_nobody_takes_in_is_read_no_further_than_the_limit() {
        let (sender, mut events) = unbounded_channel();
        thread::spawn(move || pump(Box::new(io::repeat(b'y')), &sender));
        thread::sleep(SETTLE);
        let held = waiting(&mut events);
        let read = bytes_of(&held);
        assert!((UNREAD_LIMIT..UNREAD_LIMIT + READ_CHUNK).contains(&read), "{read} bytes were read");
        thread::sleep(SETTLE);
        assert!(waiting(&mut events).is_empty(), "nothing more is read while that output is held");
        drop(held);
        thread::sleep(SETTLE);
        assert!(bytes_of(&waiting(&mut events)) > 0, "reading goes on once the output is taken in");
    }
}
