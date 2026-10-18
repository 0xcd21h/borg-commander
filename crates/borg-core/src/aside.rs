use std::{path::PathBuf, time::SystemTime};

use borg_vfs::Entry;

use crate::{AppState, Effect, Image, Layout, Row};

const BORDER_CELLS: u16 = 2;
const PADDING_CELLS: u16 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Aside {
    QuickView,
    Info,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Nature {
    File,
    Folder,
    Link(PathBuf),
    Special,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    pub nature: Nature,
    pub size: u64,
    pub permissions: u32,
    pub owner: String,
    pub group: String,
    pub modified: Option<SystemTime>,
    pub accessed: Option<SystemTime>,
    pub created: Option<SystemTime>,
    pub inode: u64,
    pub links: u64,
    pub items: Option<usize>,
    pub disk: Option<(u64, u64)>,
}

#[derive(Debug, Clone)]
pub enum Peek {
    Text(Vec<String>),
    Picture(Image),
    Folder { folders: usize, files: usize, bytes: u64, entries: Vec<Entry> },
    Info(Facts),
    Binary,
    Unavailable(String),
}

#[derive(Debug, Clone)]
pub struct Peeked {
    pub aside: Aside,
    pub path: PathBuf,
    pub peek: Peek,
}

pub(crate) fn toggle(state: &mut AppState, aside: Aside) -> Vec<Effect> {
    state.aside = (state.aside != Some(aside)).then_some(aside);
    state.peeked = None;
    state.peeking = None;
    Vec::new()
}

fn hovered_path(state: &AppState) -> Option<PathBuf> {
    let panel = state.active_panel();
    match panel.hovered()? {
        Row::Entry(entry) => Some(panel.dir().join(&entry.name)),
        Row::Parent => panel.dir().parent().map(PathBuf::from),
    }
}

pub(crate) fn sync(state: &mut AppState) -> Option<Effect> {
    let aside = state.aside?;
    let path = hovered_path(state)?;
    let wanted = Some((aside, path.clone()));
    let shown = state.peeked.as_ref().map(|peeked| (peeked.aside, peeked.path.clone()));
    if shown == wanted || state.peeking == wanted {
        return None;
    }
    let layout = Layout::new(state.cols, state.rows, state.chrome(), state.split_target());
    let area = layout.panels[state.active.other().index()];
    let size = (
        usize::from(area.width.saturating_sub(BORDER_CELLS + PADDING_CELLS)),
        usize::from(area.height.saturating_sub(BORDER_CELLS)),
    );
    state.peeking = wanted;
    Some(Effect::Peek { aside, path, size })
}

pub(crate) fn receive(state: &mut AppState, peeked: Peeked) {
    if state.peeking.as_ref().is_some_and(|(aside, path)| *aside == peeked.aside && *path == peeked.path) {
        state.peeking = None;
        state.peeked = Some(peeked);
    }
}
