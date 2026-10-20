use std::path::PathBuf;

use borg_term::{Scroll, ShellSignal};
use borg_vfs::Entry;
use crossterm::event::KeyEvent;

use borg_jobs::{Event, Spec};

use crate::{Aside, JobCommand, Peeked, ShellView, Side, Viewer};

#[derive(Debug)]
pub enum Msg {
    SearchProgress { side: Side, generation: u64, progress: crate::SearchProgress },
    Key(KeyEvent),
    Pointer(crate::Pointer),
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
    GitStatus { side: Side, generation: u64, dir: PathBuf, result: Result<Option<borg_git::Repository>, String> },
    GitFinished { id: u64, result: Result<Option<Viewer>, String> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    Search { side: Side, generation: u64, request: crate::SearchRequest },
    RefreshCollection { side: Side, generation: u64, entries: Vec<Entry> },
    CancelSearch(Side),
    SaveCollections,
    List { side: Side, generation: u64, dir: PathBuf },
    Write(Vec<u8>),
    ScrollConsole(Scroll),
    View(PathBuf),
    SaveConfig,
    StartJob { id: u64, spec: Spec },
    JobControl { id: u64, command: JobCommand },
    Peek { aside: Aside, path: PathBuf, size: (usize, usize) },
    GitScan { side: Side, generation: u64, dir: PathBuf },
    GitTool { id: u64, tool: borg_git::Tool, dir: PathBuf, paths: Vec<PathBuf> },
    Quit,
}
