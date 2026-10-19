use std::{
    path::PathBuf,
    time::{Instant, SystemTime},
};

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
    Text(borg_view::Pager),
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
    state.preview_focused = false;
    state.peeked = None;
    state.peeking = None;
    Vec::new()
}

pub(crate) fn hovered_path(state: &AppState) -> Option<PathBuf> {
    let panel = state.active_panel();
    match panel.hovered()? {
        Row::Entry(entry) => Some(panel.dir().join(&entry.name)),
        Row::Parent => panel.dir().parent().map(PathBuf::from),
    }
}

pub(crate) fn sync(state: &mut AppState) -> Option<Effect> {
    let aside = state.aside?;
    let Some(path) = hovered_path(state) else {
        state.peeked = None;
        state.peeking = None;
        return None;
    };
    let wanted = Some((aside, path.clone()));
    let shown = state.peeked.as_ref().map(|peeked| (peeked.aside, peeked.path.clone()));
    let layout = Layout::new(state.cols, state.rows, state.chrome(), state.split_target());
    let area = layout.panels[state.active.other().index()];
    let size = (
        usize::from(area.width.saturating_sub(BORDER_CELLS + PADDING_CELLS)),
        usize::from(area.height.saturating_sub(BORDER_CELLS)),
    );
    if shown == wanted {
        if let Some(Peeked { peek: Peek::Text(pager), .. }) = state.peeked.as_mut()
            && let Err(error) = pager.resize(size.0, size.1)
        {
            state.notice = Some(format!("preview: {error}"));
        }
        return None;
    }
    state.peeked = None;
    if state.peeking == wanted {
        return None;
    }
    state.peeking = wanted;
    Some(Effect::Peek { aside, path, size })
}

pub(crate) fn receive(state: &mut AppState, peeked: Peeked) {
    if state.peeking.as_ref().is_some_and(|(aside, path)| *aside == peeked.aside && *path == peeked.path) {
        state.peeking = None;
        state.peeked = Some(peeked);
    }
}

pub(crate) fn on_key(state: &mut AppState, key: &crossterm::event::KeyEvent, now: Instant) -> Vec<Effect> {
    use crossterm::event::{KeyCode, KeyModifiers};
    match key.code {
        KeyCode::Tab | KeyCode::BackTab | KeyCode::Esc => state.preview_focused = false,
        KeyCode::Char('o') if key.modifiers == KeyModifiers::CONTROL => {
            state.preview_focused = false;
            return crate::actions::perform(state, crate::Action::ToggleShell, now);
        }
        KeyCode::Char('l') if key.modifiers == KeyModifiers::CONTROL => return toggle(state, Aside::Info),
        KeyCode::F(9) => return crate::actions::perform(state, crate::Action::OpenMenuBar, now),
        KeyCode::Char('q') if key.modifiers == KeyModifiers::CONTROL => return toggle(state, Aside::QuickView),
        KeyCode::F(3) => {
            return state
                .peeked
                .as_ref()
                .map(|peeked| peeked.path.clone())
                .map(|path| crate::viewer::open(state, path))
                .unwrap_or_default();
        }
        _ => {
            if let Some(Peeked { peek: Peek::Text(pager), .. }) = state.peeked.as_mut()
                && let Err(error) = crate::viewer::scroll(pager, key.code)
            {
                state.notice = Some(format!("preview: {error}"));
            }
        }
    }
    Vec::new()
}
