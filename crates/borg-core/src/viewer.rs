use borg_i18n::tr;
use std::{io, ops::Range, path::PathBuf};

use borg_gfx::{Picture, is_picture};
use borg_view::{Count, Pager, Query, Source};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{
    AppState, Aside, Effect, Peek, Peeked,
    find_bar::{BarKey, Direction, FindBar, FindNote},
};

pub const STATUS_ROWS: u16 = 1;
const WRAP_KEY: char = 'w';
const QUIT_KEY: char = 'q';
const PAGE_KEY: char = ' ';
const LINE: usize = 1;
const NEXT_KEY: char = 'n';
const PREVIOUS_KEY: char = 'p';
const FIND_KEY: char = '/';
const NEXT_MATCH_KEY: char = 'n';
const PREVIOUS_MATCH_KEY: char = 'N';
/// Files up to this size have their matches counted, which reads all of a file on every step.
const COUNT_LIMIT: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct Image {
    pub picture: Picture,
    pub original: (u32, u32),
    pub format: &'static str,
    pub bytes: u64,
}

#[derive(Debug, Clone)]
pub enum Content {
    Text(Pager),
    Image(Image),
}

#[derive(Debug, Clone)]
pub struct Viewer {
    pub path: PathBuf,
    pub content: Content,
    pub find: FindBar,
    /// The search running in the background, by the id its answer comes with.
    pub(crate) searching: Option<u64>,
    /// Where the page started when the find bar opened, which a changed query searches on from.
    origin: u64,
}

impl Viewer {
    pub fn new(path: PathBuf, content: Content) -> Self {
        Self { path, content, find: FindBar::default(), searching: None, origin: 0 }
    }
}

/// Where a file about to open shows a contents search's match, with the find bar on its query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Landing {
    pub path: PathBuf,
    pub query: Query,
    pub offset: u64,
}

/// What a search through the viewer's file found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundInViewer {
    pub range: Option<Range<u64>>,
    /// Found only after going on from the other end of the file.
    pub wrapped: bool,
    pub count: Option<Count>,
}

/// Looks for `query` in `source` from `from` on, or back from before it, going on from the
/// other end when there is no match that way, and counts where the match stands when asked.
pub fn find_in(
    source: &dyn Source,
    query: &Query,
    from: u64,
    direction: Direction,
    count: bool,
    go_on: &dyn Fn(u64) -> bool,
) -> io::Result<FoundInViewer> {
    let search = query.compile().map_err(io::Error::other)?.ok_or_else(|| io::Error::other("nothing to look for"))?;
    let look = |at: u64| match direction {
        Direction::Forward => search.next(source, at, go_on),
        Direction::Backward => search.previous(source, at, go_on),
    };
    let (range, wrapped) = match look(from)? {
        Some(range) => (Some(range), false),
        None => (look(if direction == Direction::Forward { 0 } else { source.len() })?, true),
    };
    let count = match &range {
        Some(range) if count => Some(search.count(source, range.start, go_on)?),
        _ => None,
    };
    let wrapped = wrapped && range.is_some();
    Ok(FoundInViewer { range, wrapped, count })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Next,
    Previous,
}

pub fn viewer_size(cols: u16, rows: u16) -> (usize, usize) {
    (usize::from(cols), usize::from(rows.saturating_sub(STATUS_ROWS)))
}

fn is_close(code: KeyCode) -> bool {
    matches!(code, KeyCode::Esc | KeyCode::F(3 | 10) | KeyCode::Char(QUIT_KEY))
}

pub(crate) fn open(state: &mut AppState, path: PathBuf) -> Vec<Effect> {
    state.landing = None;
    show(state, path)
}

/// Opens `path` on the match a contents search found at `offset`, the find bar ready with
/// `query` to step on through the others.
pub(crate) fn open_at(state: &mut AppState, path: PathBuf, query: Query, offset: u64) -> Vec<Effect> {
    state.landing = Some(Landing { path: path.clone(), query, offset });
    show(state, path)
}

/// Shows `path` at once when the quick view has it, or else asks to load it.
fn show(state: &mut AppState, path: PathBuf) -> Vec<Effect> {
    if let Some(Peeked { aside: Aside::QuickView, path: shown, peek: Peek::Text(pager), .. }) = &state.peeked
        && *shown == path
    {
        state.viewer = Some(Viewer::new(path, Content::Text(pager.clone())));
        state.opening = None;
        resize(state);
        return land(state);
    }
    vec![request(state, path)]
}

/// Asks to load `path` for the viewer; of the loads asked for, only the latest is shown.
pub(crate) fn request(state: &mut AppState, path: PathBuf) -> Effect {
    state.landing = state.landing.take().filter(|landing| landing.path == path);
    state.opening = Some(path.clone());
    Effect::View(path)
}

/// Shows a loaded file, if the viewer still waits for it.
pub(crate) fn opened(state: &mut AppState, viewer: Viewer) -> Vec<Effect> {
    if state.opening.as_ref() != Some(&viewer.path) {
        return Vec::new();
    }
    state.opening = None;
    state.viewer = Some(viewer);
    resize(state);
    land(state)
}

pub(crate) fn failed(state: &mut AppState, path: PathBuf, notice: String) {
    if state.opening.as_ref() == Some(&path) {
        state.opening = None;
        state.landing = None;
        state.notice = Some(notice);
    }
}

/// Opens the find bar on the landing's query and looks for the match from where it was found,
/// in the file's source, where the offset was counted.
fn land(state: &mut AppState) -> Vec<Effect> {
    let Some(landing) = state.landing.take() else { return Vec::new() };
    let Some(viewer) = state.viewer.as_mut().filter(|viewer| viewer.path == landing.path) else {
        return Vec::new();
    };
    let Content::Text(pager) = &mut viewer.content else { return Vec::new() };
    viewer.find.load(&landing.query);
    viewer.find.open(None);
    viewer.origin = landing.offset;
    let source = if pager.formatted() { pager.toggle_source() } else { Ok(()) };
    let shown = source.and_then(|()| pager.show_matches(viewer.find.search().cloned())).map(|()| true);
    finish(state, shown);
    search(state, Direction::Forward, Some(landing.offset))
}

fn close(state: &mut AppState) {
    state.opening = None;
    if let Some(Viewer { path, content: Content::Text(mut pager), .. }) = state.viewer.take()
        && state.aside == Some(Aside::QuickView)
        && crate::aside::hovered_path(state).as_ref() == Some(&path)
    {
        // The quick view marks a contents search's matches, and not the viewer's.
        let at = crate::aside::opens_on(state, Aside::QuickView, &path);
        let marks = at.as_ref().and_then(|at| at.query.compile().ok().flatten());
        let _ = pager.show_matches(marks).and_then(|()| pager.forget_match());
        state.peeked = Some(Peeked { aside: Aside::QuickView, path, at, peek: Peek::Text(pager) });
        state.peeking = None;
    }
}

pub(crate) fn on_key(state: &mut AppState, key: &KeyEvent) -> Vec<Effect> {
    let Some(viewer) = state.viewer.as_mut() else { return Vec::new() };
    if matches!(viewer.content, Content::Image(_)) {
        return on_image_key(state, key.code);
    }
    if viewer.find.open {
        return on_bar(state, key);
    }
    if let Some(effects) = on_find_key(state, key) {
        return effects;
    }
    let Some(viewer) = state.viewer.as_mut() else { return Vec::new() };
    // A search runs in the background: Esc stops it before it closes the viewer.
    if key.code == KeyCode::Esc && viewer.searching.is_some() {
        return stop(viewer);
    }
    viewer.find.note = None;
    viewer.find.count = None;
    let mut effects = Vec::new();
    if matches!(key.code, KeyCode::Char('s') | KeyCode::F(4)) && viewer.searching.is_some() {
        effects = stop(viewer);
    }
    if is_close(key.code) {
        effects.extend(stop(viewer));
    }
    let Content::Text(pager) = &mut viewer.content else { return effects };
    let result = if is_close(key.code) { Ok(false) } else { scroll(pager, key.code).map(|()| true) };
    finish(state, result);
    effects
}

/// Closes the viewer when a key asked to, or with the error a key ran into.
fn finish(state: &mut AppState, result: io::Result<bool>) {
    match result {
        Ok(true) => {}
        Ok(false) => close(state),
        Err(error) => {
            state.notice = Some(tr!(viewer_error, error = error));
            state.viewer = None;
        }
    }
}

/// The keys that find with the bar closed: F7, Ctrl-F or `/` open it, as in less; `n` and
/// Shift-F7 step on to the next match, `N` and Alt-F7 back to the previous one.
fn on_find_key(state: &mut AppState, key: &KeyEvent) -> Option<Vec<Effect>> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let direction = match key.code {
        KeyCode::F(7) if alt => Direction::Backward,
        KeyCode::F(7) if shift => Direction::Forward,
        KeyCode::F(7) | KeyCode::Char(FIND_KEY) => return Some(open_bar(state)),
        KeyCode::Char('f') if ctrl => return Some(open_bar(state)),
        KeyCode::Char(NEXT_MATCH_KEY) if !ctrl && !alt => Direction::Forward,
        KeyCode::Char(PREVIOUS_MATCH_KEY) if !ctrl && !alt => Direction::Backward,
        _ => return None,
    };
    let viewer = state.viewer.as_mut()?;
    if viewer.find.search().is_none() {
        return Some(open_bar(state));
    }
    Some(search(state, direction, None))
}

/// Opens the find bar on the query it had, marking its matches on the page.
fn open_bar(state: &mut AppState) -> Vec<Effect> {
    let Some(viewer) = state.viewer.as_mut() else { return Vec::new() };
    let Content::Text(pager) = &mut viewer.content else { return Vec::new() };
    viewer.origin = pager.offset();
    viewer.find.open(None);
    let shown = pager.show_matches(viewer.find.search().cloned()).map(|()| true);
    finish(state, shown);
    Vec::new()
}

/// A key in the find bar. Page Up and Page Down, and F2 and F4, scroll and switch the page.
fn on_bar(state: &mut AppState, key: &KeyEvent) -> Vec<Effect> {
    let Some(viewer) = state.viewer.as_mut() else { return Vec::new() };
    let action = viewer.find.on_key(key, &state.find_recent);
    if matches!(action, BarKey::Step(_) | BarKey::Closed) {
        viewer.find.remember(&mut state.find_recent);
    }
    let Content::Text(pager) = &mut viewer.content else { return Vec::new() };
    match action {
        BarKey::Step(direction) => search(state, direction, None),
        BarKey::Changed => {
            let origin = viewer.origin;
            let shown = pager.forget_match().and_then(|()| pager.show_matches(viewer.find.search().cloned()));
            let shown = shown.map(|()| true);
            finish(state, shown);
            search(state, Direction::Forward, Some(origin))
        }
        BarKey::Closed => {
            // The match stepped to stays marked, for the next step to go on from.
            let shown = pager.show_matches(None).map(|()| true);
            let effects = stop(viewer);
            finish(state, shown);
            effects
        }
        BarKey::Kept => Vec::new(),
        BarKey::Unhandled => {
            if matches!(key.code, KeyCode::PageUp | KeyCode::PageDown | KeyCode::F(2 | 4)) {
                let scrolled = scroll(pager, key.code).map(|()| true);
                finish(state, scrolled);
            }
            Vec::new()
        }
    }
}

/// Starts looking for the bar's query in the background: from `from`, or else on from the
/// match stepped to, or from the top of the page.
fn search(state: &mut AppState, direction: Direction, from: Option<u64>) -> Vec<Effect> {
    state.next_find += 1;
    let id = state.next_find;
    let Some(viewer) = state.viewer.as_mut() else { return Vec::new() };
    let Content::Text(pager) = &mut viewer.content else { return Vec::new() };
    let Some(search) = viewer.find.search().cloned() else {
        return stop(viewer);
    };
    let from = from.unwrap_or_else(|| match (direction, pager.current_match()) {
        (Direction::Forward, Some(found)) => found.end,
        (Direction::Backward, Some(found)) => found.start,
        _ => pager.offset(),
    });
    // The matches went off the page when the bar closed; a step brings them back.
    let shown = if pager.shows_matches() { Ok(()) } else { pager.show_matches(Some(search)) };
    viewer.searching = Some(id);
    let count = pager.len() <= COUNT_LIMIT;
    let effect = Effect::FindInViewer { id, query: viewer.find.query(), from, direction, count };
    finish(state, shown.map(|()| true));
    vec![effect]
}

fn stop(viewer: &mut Viewer) -> Vec<Effect> {
    if viewer.searching.take().is_none() {
        return Vec::new();
    }
    if matches!(viewer.find.note, Some(FindNote::Searching(_))) {
        viewer.find.note = None;
    }
    vec![Effect::StopFindInViewer]
}

/// Shows what a search found, when it is the one the viewer waits for.
pub(crate) fn found(state: &mut AppState, id: u64, result: Result<FoundInViewer, String>) {
    let Some(viewer) = state.viewer.as_mut().filter(|viewer| viewer.searching == Some(id)) else { return };
    viewer.searching = None;
    let Content::Text(pager) = &mut viewer.content else { return };
    let revealed = match result {
        Ok(FoundInViewer { range: Some(range), wrapped, count }) => {
            viewer.find.note = wrapped.then_some(FindNote::Wrapped);
            viewer.find.count = count;
            pager.reveal(range).map(|()| true)
        }
        Ok(FoundInViewer { range: None, .. }) => {
            viewer.find.note = Some(FindNote::NoMatch);
            viewer.find.count = None;
            Ok(true)
        }
        Err(error) => Err(io::Error::other(error)),
    };
    finish(state, revealed);
}

/// Shows how far the search the viewer waits for got.
pub(crate) fn searching(state: &mut AppState, id: u64, percent: u8) {
    if let Some(viewer) = state.viewer.as_mut().filter(|viewer| viewer.searching == Some(id)) {
        viewer.find.note = Some(FindNote::Searching(percent));
    }
}

fn on_image_key(state: &mut AppState, code: KeyCode) -> Vec<Effect> {
    let step = match code {
        code if is_close(code) => {
            close(state);
            return Vec::new();
        }
        KeyCode::Right | KeyCode::Down | KeyCode::PageDown | KeyCode::Char(PAGE_KEY | NEXT_KEY) => Step::Next,
        KeyCode::Left | KeyCode::Up | KeyCode::PageUp | KeyCode::Backspace | KeyCode::Char(PREVIOUS_KEY) => {
            Step::Previous
        }
        _ => return Vec::new(),
    };
    sibling(state, step).map(|path| request(state, path)).into_iter().collect()
}

fn sibling(state: &mut AppState, step: Step) -> Option<PathBuf> {
    // Stepping again before a picture loads goes on from the one loading.
    let current = state.opening.clone().or_else(|| state.viewer.as_ref().map(|viewer| viewer.path.clone()))?;
    let panel = state.panels.get_mut(state.active.index())?;
    let pictures: Vec<PathBuf> = panel
        .visible_entries()
        .filter(|entry| !entry.is_dir() && is_picture(std::path::Path::new(&entry.name)))
        .map(|entry| entry.path(panel.dir()))
        .collect();
    let here = pictures.iter().position(|path| *path == current)?;
    let target = match step {
        Step::Next => pictures.get(here + 1),
        Step::Previous => here.checked_sub(1).and_then(|index| pictures.get(index)),
    }?
    .clone();
    panel.point_at_path(target.clone());
    Some(target)
}

pub(crate) fn resize(state: &mut AppState) {
    let (width, height) = viewer_size(state.cols, state.rows);
    let failed = state.viewer.as_mut().and_then(|viewer| match &mut viewer.content {
        Content::Text(pager) => pager.resize(width, height).err(),
        Content::Image(_) => None,
    });
    if let Some(error) = failed {
        state.notice = Some(tr!(viewer_error, error = error));
        state.viewer = None;
    }
}

pub(crate) fn scroll(pager: &mut Pager, code: KeyCode) -> io::Result<()> {
    match code {
        KeyCode::Down => pager.scroll_down(LINE),
        KeyCode::Up => pager.scroll_up(LINE),
        KeyCode::PageDown | KeyCode::Char(PAGE_KEY) => pager.page_down(),
        KeyCode::PageUp => pager.page_up(),
        KeyCode::Home => pager.home(),
        KeyCode::End => pager.end(),
        KeyCode::Left => pager.scroll_left(),
        KeyCode::Right => pager.scroll_right(),
        KeyCode::Char('s') | KeyCode::F(4) => pager.toggle_source(),
        KeyCode::F(2) | KeyCode::Char(WRAP_KEY) => pager.toggle_wrap(),
        _ => Ok(()),
    }
}
