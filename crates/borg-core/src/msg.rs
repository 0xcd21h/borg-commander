use std::path::PathBuf;

use borg_term::ShellSignal;
use borg_vfs::Entry;
use crossterm::event::KeyEvent;

use crate::{ShellView, Side};

#[derive(Debug)]
pub enum Msg {
    Key(KeyEvent),
    Paste(String),
    Resize { cols: u16, rows: u16 },
    Batch { side: Side, generation: u64, entries: Vec<Entry> },
    Listed { side: Side, generation: u64, error: Option<String> },
    Observe(ShellView),
    Signal(ShellSignal),
    ShellExited,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    List { side: Side, generation: u64, dir: PathBuf },
    Write(Vec<u8>),
    Quit,
}
