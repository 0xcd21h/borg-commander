use std::path::PathBuf;

use borg_term::ShellSignal;
use borg_vfs::Entry;
use crossterm::event::KeyEvent;

use borg_jobs::{Event, Spec};

use crate::{Aside, JobCommand, Peeked, ShellView, Side, Viewer};

#[derive(Debug)]
pub enum Msg {
    Key(KeyEvent),
    Paste(String),
    Resize { cols: u16, rows: u16 },
    Batch { side: Side, generation: u64, entries: Vec<Entry> },
    Listed { side: Side, generation: u64, error: Option<String> },
    Observe(ShellView),
    ShellOutput(ShellView),
    Signal(ShellSignal),
    Tick,
    Opened(Viewer),
    ShellExited,
    Notice(String),
    Job { id: u64, event: Event },
    Peeked(Peeked),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    List { side: Side, generation: u64, dir: PathBuf },
    Write(Vec<u8>),
    View(PathBuf),
    SaveConfig,
    StartJob { id: u64, spec: Spec },
    JobControl { id: u64, command: JobCommand },
    Peek { aside: Aside, path: PathBuf, size: (usize, usize) },
    Quit,
}
