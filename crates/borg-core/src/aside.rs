use borg_i18n::tr;
use std::{
    io,
    path::{Path, PathBuf},
    time::{Instant, SystemTime},
};

use borg_vfs::Entry;
use borg_view::Pager;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{
    AppState, Direction, Effect, FirstMatch, Image, Row,
    pager_find::{Finding, FoundInPager, PagerFind, PagerSlot},
};

const BORDER_CELLS: u16 = 2;
const PADDING_CELLS: u16 = 2;
pub const FIND_ROWS: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Aside {
    QuickView,
    Info,
    /// A regular file the inspected process has open, previewed under its details in the
    /// activity monitor.
    OpenFile,
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
    /// How Time Machine treats it, on macOS.
    pub backup: Option<crate::Exclusion>,
    /// What Borg knows of the drive it lies on.
    pub drive: Option<borg_drives::Facts>,
}

#[derive(Debug, Clone)]
pub enum Peek {
    Text(borg_view::Pager),
    Picture(Image),
    Folder { folders: usize, files: usize, bytes: u64, entries: Vec<Entry> },
    Info(Facts),
    Unavailable(String),
}

#[derive(Debug, Clone)]
pub struct Peeked {
    pub aside: Aside,
    pub path: PathBuf,
    /// The match the quick view opens a contents search's result on.
    pub at: Option<FirstMatch>,
    pub peek: Peek,
}

/// Scrolls the preview of a contents search's result to the match found at `at`, marking the
/// query's matches as the viewer's find bar does, in the source the search read.
pub fn land_on_match(pager: &mut Pager, at: &FirstMatch, go_on: &dyn Fn(u64) -> bool) -> io::Result<()> {
    let search = at.query.compile().map_err(io::Error::other)?;
    if pager.formatted() {
        pager.toggle_source()?;
    }
    pager.show_matches(search)?;
    let found = crate::find_in(&*pager.source(), &at.query, at.offset, Direction::Forward, false, go_on)?;
    found.range.map_or(Ok(()), |range| pager.reveal(range))
}

pub(crate) fn toggle(state: &mut AppState, aside: Aside) -> Vec<Effect> {
    if state.docked[state.active.other().index()].is_some() {
        state.notice = Some(tr!(aside_beside_editor).into());
        return Vec::new();
    }
    state.aside = (state.aside != Some(aside)).then_some(aside);
    state.preview_focused = false;
    state.peeking = None;
    forget(state)
}

pub(crate) fn forget(state: &mut AppState) -> Vec<Effect> {
    state.peeked = None;
    let effects = state.preview_find.stop(PagerSlot::QuickView);
    state.preview_find = PagerFind::default();
    effects
}

pub(crate) fn landing_find(at: Option<&FirstMatch>) -> PagerFind {
    at.map_or_else(PagerFind::default, |at| PagerFind::on(&at.query))
}

pub(crate) fn hovered_path(state: &AppState) -> Option<PathBuf> {
    let panel = state.active_panel();
    match panel.hovered()? {
        Row::Entry(entry) => Some(entry.path(panel.dir())),
        Row::Group { .. } => None,
        Row::Parent => panel.dir().parent().map(PathBuf::from),
    }
}

/// The match the quick view opens the file at `path` on, among a contents search's results.
pub(crate) fn opens_on(state: &AppState, aside: Aside, path: &Path) -> Option<FirstMatch> {
    let results = state.active_panel().results.as_ref().filter(|_| aside == Aside::QuickView)?;
    results.first_match(path)
}

pub(crate) fn sync(state: &mut AppState) -> Vec<Effect> {
    // A preview has no room while a panel holds an editor.
    let docked = state.docked.iter().any(Option::is_some);
    if docked || state.active_panel().foreign() {
        state.aside = None;
    }
    let Some(aside) = state.aside else { return forget(state) };
    // The activity monitor covers the panels and previews with the same loader; the quick view
    // asks again once the panels show.
    if state.activity.side.is_some() {
        state.peeking = None;
        return Vec::new();
    }
    let Some(path) = hovered_path(state) else {
        state.peeking = None;
        return forget(state);
    };
    let at = opens_on(state, aside, &path);
    let wanted = Some((aside, path.clone(), at.clone()));
    let shown = state.peeked.as_ref().map(|peeked| (peeked.aside, peeked.path.clone(), peeked.at.clone()));
    let layout = state.layout(state.epoch);
    let area = layout.panels[state.active.other().index()];
    let size = (
        usize::from(area.width.saturating_sub(BORDER_CELLS + PADDING_CELLS)),
        usize::from(area.height.saturating_sub(BORDER_CELLS)),
    );
    if shown == wanted {
        let bar = if state.preview_find.bar.open { FIND_ROWS } else { 0 };
        if let Some(Peeked { peek: Peek::Text(pager), .. }) = state.peeked.as_mut()
            && let Err(error) = pager.resize(size.0, size.1.saturating_sub(usize::from(bar)))
        {
            state.notice = Some(tr!(preview_error, error = error));
        }
        return Vec::new();
    }
    let mut effects = forget(state);
    if state.peeking == wanted {
        return effects;
    }
    state.peeking = wanted;
    effects.push(Effect::Peek { aside, path, at, size });
    effects
}

pub(crate) fn receive(state: &mut AppState, mut peeked: Peeked) {
    if peeked.aside == Aside::OpenFile {
        return crate::activity::receive_file_preview(&mut state.activity, peeked);
    }
    let asked = |(aside, path, at): &(Aside, PathBuf, Option<FirstMatch>)| {
        *aside == peeked.aside && *path == peeked.path && *at == peeked.at
    };
    if state.peeking.as_ref().is_some_and(asked) {
        if let Peek::Text(pager) = &mut peeked.peek
            && let Err(error) = crate::viewer::binary_as_hex(pager)
        {
            state.notice = Some(tr!(preview_error, error = error));
        }
        state.peeking = None;
        state.preview_find = landing_find(peeked.at.as_ref());
        state.peeked = Some(peeked);
    }
}

pub(crate) fn on_key(state: &mut AppState, key: &KeyEvent, now: Instant) -> Vec<Effect> {
    if matches!(key.code, KeyCode::Tab | KeyCode::BackTab) {
        state.preview_focused = false;
        return Vec::new();
    }
    if let Some(effects) = find_key(state, key) {
        return effects;
    }
    match key.code {
        KeyCode::Esc => state.preview_focused = false,
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
            if let Some(Peeked { peek: Peek::Text(pager), .. }) = state.peeked.as_mut() {
                let code = crate::keyboard::latin(key).code;
                let effects = state.preview_find.page_key(pager, code, PagerSlot::QuickView);
                if let Err(error) = preview_key(pager, code) {
                    state.notice = Some(tr!(preview_error, error = error));
                }
                return effects;
            }
        }
    }
    Vec::new()
}

fn find_key(state: &mut AppState, key: &KeyEvent) -> Option<Vec<Effect>> {
    let Some(Peeked { aside: Aside::QuickView, peek: Peek::Text(pager), .. }) = state.peeked.as_mut() else {
        return None;
    };
    let finding = Finding { slot: PagerSlot::QuickView, next_id: &mut state.next_find, recent: &mut state.find_recent };
    let result = state.preview_find.on_key(pager, key, finding)?;
    Some(result.unwrap_or_else(|error| {
        state.notice = Some(tr!(preview_error, error = error));
        Vec::new()
    }))
}

pub(crate) fn found(state: &mut AppState, id: u64, result: Result<FoundInPager, String>) {
    let Some(Peeked { aside: Aside::QuickView, peek: Peek::Text(pager), .. }) = state.peeked.as_mut() else {
        return;
    };
    if let Err(error) = state.preview_find.found(pager, id, result) {
        state.notice = Some(tr!(preview_error, error = error));
    }
}

pub(crate) fn searching(state: &mut AppState, id: u64, percent: u8) {
    state.preview_find.progress(id, percent);
}

/// What a key does to the text the quick view shows: `h` turns hex rows on or off, and the other
/// keys scroll it as in the viewer.
fn preview_key(pager: &mut Pager, code: KeyCode) -> io::Result<()> {
    if crate::viewer::is_hex_key(code) { pager.toggle_hex() } else { crate::viewer::scroll(pager, code) }
}
