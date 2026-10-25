use std::{
    io::{self, Read, Write},
    thread,
};

use anyhow::{Context, Result};
use portable_pty::{Child, CommandBuilder, MasterPty, native_pty_system};
use tokio::sync::mpsc::UnboundedSender;

use crate::GridSize;

const READ_CHUNK: usize = 64 * 1024;

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

pub struct Pty {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
}

impl Pty {
    pub fn spawn(cmd: CommandBuilder, size: GridSize, events: UnboundedSender<PtyEvent>) -> Result<Self> {
        let pair = native_pty_system().openpty(size.into()).context("opening pty")?;
        let child = pair.slave.spawn_command(cmd).context("spawning shell")?;
        drop(pair.slave);

        let reader = pair.master.try_clone_reader().context("cloning pty reader")?;
        let writer = pair.master.take_writer().context("taking pty writer")?;
        thread::Builder::new().name("pty-reader".into()).spawn(move || pump(reader, &events))?;

        Ok(Self { master: pair.master, writer, child })
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
        let _ = self.child.kill();
    }
}

fn pump(mut reader: Box<dyn Read + Send>, events: &UnboundedSender<PtyEvent>) {
    let mut buf = vec![0; READ_CHUNK];
    loop {
        match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if events.send(PtyEvent::Output(buf[..n].to_vec())).is_err() {
                    return;
                }
            }
        }
    }
    let _ = events.send(PtyEvent::Closed);
}
