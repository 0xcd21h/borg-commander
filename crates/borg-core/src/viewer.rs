use borg_i18n::tr;
use std::{
    io,
    path::{Path, PathBuf},
    sync::Arc,
};

use borg_gfx::{Picture, is_picture};
use borg_view::{
    Pager, Query,
    entropy::{Profile, strip_cell},
};
use crossterm::event::{KeyCode, KeyEvent};

use crate::{
    AppState, Aside, Effect, FirstMatch, Peek, Peeked,
    pager_find::{Finding, PagerFind, PagerSlot},
};

pub const STATUS_ROWS: u16 = 1;
const WRAP_KEY: char = 'w';
/// Switches between the text and the hex rows of a file, as in the memory monitor's preview.
const HEX_KEY: char = 'h';
const QUIT_KEY: char = 'q';
const PAGE_KEY: char = ' ';
const LINE: usize = 1;
const NEXT_KEY: char = 'n';
const PREVIOUS_KEY: char = 'p';
/// Columns the entropy strip takes beside the hex rows.
pub const ENTROPY_STRIP: u16 = 1;
const NEXT_HIGH_KEY: char = ']';
const PREVIOUS_HIGH_KEY: char = '[';

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

/// The entropy along the viewer's source, read in the background once its hex rows show.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Profiling {
    #[default]
    Unread,
    Reading(u64),
    Read(Arc<Profile>),
    Failed,
}

/// What the viewer shows: a file, or bytes another screen hands it, by what they are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Shown {
    File(PathBuf),
    Bytes(String),
}

#[derive(Debug, Clone)]
pub struct Viewer {
    pub shown: Shown,
    pub content: Content,
    pub find: PagerFind,
    pub(crate) profiling: Profiling,
}

impl Viewer {
    pub fn new(path: PathBuf, content: Content) -> Self {
        Self::showing(Shown::File(path), content)
    }

    fn showing(shown: Shown, content: Content) -> Self {
        Self { shown, content, find: PagerFind::default(), profiling: Profiling::Unread }
    }

    /// The entropy along the source, once it was read.
    pub fn profile(&self) -> Option<&Profile> {
        match &self.profiling {
            Profiling::Read(profile) => Some(profile),
            _ => None,
        }
    }

    /// The file shown, when it is one.
    pub fn path(&self) -> Option<&Path> {
        match &self.shown {
            Shown::File(path) => Some(path),
            Shown::Bytes(_) => None,
        }
    }
}

/// What a file about to open shows first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Landing {
    /// A contents search's match at `offset`, with the find bar on its query.
    Match { path: PathBuf, query: Query, offset: u64 },
    /// The bytes from `offset` on, as hex rows.
    Bytes { path: PathBuf, offset: u64 },
}

impl Landing {
    pub fn path(&self) -> &Path {
        match self {
            Self::Match { path, .. } | Self::Bytes { path, .. } => path,
        }
    }
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

pub(crate) fn is_hex_key(code: KeyCode) -> bool {
    matches!(code, KeyCode::Char(c) if c.eq_ignore_ascii_case(&HEX_KEY))
}

pub(crate) fn open(state: &mut AppState, path: PathBuf) -> Vec<Effect> {
    state.landing = None;
    show(state, path)
}

/// Opens `path` on the match a contents search found at `offset`, the find bar ready with
/// `query` to step on through the others.
pub(crate) fn open_at(state: &mut AppState, path: PathBuf, query: Query, offset: u64) -> Vec<Effect> {
    state.landing = Some(Landing::Match { path: path.clone(), query, offset });
    show(state, path)
}

/// Opens `path` as hex rows from the block at `offset` on.
pub(crate) fn open_on_bytes(state: &mut AppState, path: PathBuf, offset: u64) -> Vec<Effect> {
    state.landing = Some(Landing::Bytes { path: path.clone(), offset });
    show(state, path)
}

/// Shows `path` at once when the quick view has it, or else asks to load it.
fn show(state: &mut AppState, path: PathBuf) -> Vec<Effect> {
    if let Some(Peeked { aside: Aside::QuickView, path: shown, peek: Peek::Text(pager), .. }) = &state.peeked
        && *shown == path
    {
        let mut viewer = Viewer::new(path, Content::Text(pager.clone()));
        let mut effects = state.preview_find.stop(PagerSlot::QuickView);
        viewer.find = std::mem::take(&mut state.preview_find);
        effects.extend(present(state, viewer));
        return effects;
    }
    vec![request(state, path)]
}

/// Shows `bytes` another screen hands the viewer, named `name`, as hex rows.
pub(crate) fn open_bytes(state: &mut AppState, name: String, bytes: Vec<u8>) -> Vec<Effect> {
    let (width, height) = viewer_size(state.cols, state.rows);
    state.opening = None;
    state.landing = None;
    let hex = Pager::new(Arc::new(bytes), width, height).and_then(|mut pager| pager.toggle_hex().map(|()| pager));
    match hex {
        Ok(pager) => {
            state.viewer = Some(Viewer::showing(Shown::Bytes(name), Content::Text(pager)));
            resize(state);
            return profile(state);
        }
        Err(error) => state.notice = Some(tr!(viewer_error, error = error)),
    }
    Vec::new()
}

/// Asks to load `path` for the viewer, on the match it lands on, so that the load colours the
/// page around it; of the loads asked for, only the latest is shown.
pub(crate) fn request(state: &mut AppState, path: PathBuf) -> Effect {
    state.landing = state.landing.take().filter(|landing| landing.path() == path);
    state.opening = Some(path.clone());
    let at = match &state.landing {
        Some(Landing::Match { query, offset, .. }) => Some(FirstMatch { query: query.clone(), offset: *offset }),
        _ => None,
    };
    Effect::View { path, at }
}

/// Shows a loaded file, if the viewer still waits for it, a binary file as hex rows.
pub(crate) fn opened(state: &mut AppState, mut viewer: Viewer) -> Vec<Effect> {
    if state.opening.as_deref() != viewer.path() {
        return Vec::new();
    }
    if let Content::Text(pager) = &mut viewer.content
        && let Err(error) = binary_as_hex(pager)
    {
        state.opening = None;
        state.landing = None;
        state.notice = Some(tr!(viewer_error, error = error));
        return Vec::new();
    }
    present(state, viewer)
}

/// Shows a freshly read file as hex rows when it looks binary, with its text a key away.
pub(crate) fn binary_as_hex(pager: &mut Pager) -> io::Result<()> {
    if pager.binary() && !pager.hex() { pager.toggle_hex() } else { Ok(()) }
}

/// Puts `viewer` in front, on the match it lands on.
fn present(state: &mut AppState, viewer: Viewer) -> Vec<Effect> {
    state.opening = None;
    state.viewer = Some(viewer);
    resize(state);
    let mut effects = land(state);
    effects.extend(profile(state));
    effects
}

pub(crate) fn failed(state: &mut AppState, path: PathBuf, notice: String) {
    if state.opening.as_ref() == Some(&path) {
        state.opening = None;
        state.landing = None;
        state.notice = Some(notice);
    }
}

/// Opens the find bar on a match landing's query and looks for the match from where it was
/// found, in the file's source, where the offset was counted. A bytes landing shows the file as
/// hex rows from its offset.
fn land(state: &mut AppState) -> Vec<Effect> {
    let Some(landing) = state.landing.take() else { return Vec::new() };
    let Some(viewer) = state.viewer.as_mut().filter(|viewer| viewer.path() == Some(landing.path())) else {
        return Vec::new();
    };
    let Content::Text(pager) = &mut viewer.content else { return Vec::new() };
    match landing {
        Landing::Match { query, offset, .. } => {
            let finding =
                Finding { slot: PagerSlot::Viewer, next_id: &mut state.next_find, recent: &mut state.find_recent };
            let landed = viewer.find.land(pager, &query, offset, finding);
            finished(state, landed)
        }
        Landing::Bytes { offset, .. } => {
            let hex = if pager.hex() { Ok(()) } else { pager.toggle_hex() };
            let shown = hex.and_then(|()| pager.show_byte(offset)).map(|()| true);
            finish(state, shown);
            Vec::new()
        }
    }
}

fn close(state: &mut AppState) {
    state.opening = None;
    if let Some(Viewer { shown: Shown::File(path), content: Content::Text(mut pager), .. }) = state.viewer.take()
        && state.aside == Some(Aside::QuickView)
        && crate::aside::hovered_path(state).as_ref() == Some(&path)
    {
        // The quick view marks a contents search's matches, and not the viewer's.
        let at = crate::aside::opens_on(state, Aside::QuickView, &path);
        let marks = at.as_ref().and_then(|at| at.query.compile().ok().flatten());
        let _ = pager.show_matches(marks).and_then(|()| pager.forget_match());
        state.preview_find = crate::aside::landing_find(at.as_ref());
        state.peeked = Some(Peeked { aside: Aside::QuickView, path, at, peek: Peek::Text(pager) });
        state.peeking = None;
    }
}

pub(crate) fn on_key(state: &mut AppState, key: &KeyEvent) -> Vec<Effect> {
    let Some(viewer) = state.viewer.as_mut() else { return Vec::new() };
    let Content::Text(pager) = &mut viewer.content else {
        return on_image_key(state, crate::keyboard::latin(key).code);
    };
    let finding = Finding { slot: PagerSlot::Viewer, next_id: &mut state.next_find, recent: &mut state.find_recent };
    if let Some(result) = viewer.find.on_key(pager, key, finding) {
        return finished(state, result);
    }
    let key = &crate::keyboard::latin(key);
    let mut effects = viewer.find.page_key(pager, key.code, PagerSlot::Viewer);
    if is_close(key.code) {
        effects.extend(viewer.find.stop(PagerSlot::Viewer));
        effects.extend(stop_profile(viewer));
    }
    let Content::Text(pager) = &mut viewer.content else { return effects };
    let result = match key.code {
        code if is_close(code) => Ok(false),
        code if is_hex_key(code) => pager.toggle_hex().map(|()| true),
        KeyCode::Char(c @ (NEXT_HIGH_KEY | PREVIOUS_HIGH_KEY)) if pager.hex() => {
            let profile = match &viewer.profiling {
                Profiling::Read(profile) => Some(profile.as_ref()),
                _ => None,
            };
            jump_to_high(pager, profile, c == NEXT_HIGH_KEY).map(|()| true)
        }
        code => scroll(pager, code).map(|()| true),
    };
    finish(state, result);
    resize(state);
    effects.extend(profile(state));
    effects
}

/// Brings the next or the previous run of high-entropy blocks to the top of the hex rows, once
/// the source's entropy was read.
fn jump_to_high(pager: &mut Pager, profile: Option<&Profile>, forward: bool) -> io::Result<()> {
    let at = pager.offset();
    let target = profile.and_then(|profile| if forward { profile.next_high(at) } else { profile.previous_high(at) });
    target.map_or(Ok(()), |offset| pager.show_byte(offset))
}

/// Scrolls the hex rows to where the `cell`th of the entropy strip's `cells` starts.
pub(crate) fn show_strip_cell(state: &mut AppState, cell: usize, cells: usize) {
    let Some(Viewer { content: Content::Text(pager), .. }) = state.viewer.as_mut() else { return };
    let shown = pager.show_byte(strip_cell(pager.len(), cells, cell).start).map(|()| true);
    finish(state, shown);
}

/// Asks for the entropy along the source once the page shows it as hex rows, unless it was read
/// or is being read.
fn profile(state: &mut AppState) -> Vec<Effect> {
    let Some(viewer) = state.viewer.as_mut() else { return Vec::new() };
    let Content::Text(pager) = &viewer.content else { return Vec::new() };
    if !pager.hex() || pager.is_empty() || viewer.profiling != Profiling::Unread {
        return Vec::new();
    }
    state.next_profile += 1;
    viewer.profiling = Profiling::Reading(state.next_profile);
    vec![Effect::ProfileViewer { id: state.next_profile }]
}

fn stop_profile(viewer: &mut Viewer) -> Vec<Effect> {
    if !matches!(viewer.profiling, Profiling::Reading(_)) {
        return Vec::new();
    }
    viewer.profiling = Profiling::Unread;
    vec![Effect::StopProfileViewer]
}

/// Keeps the entropy read `id` found, when the viewer still waits for it.
pub(crate) fn profiled(state: &mut AppState, id: u64, profile: Result<Profile, String>) {
    let Some(viewer) = state.viewer.as_mut().filter(|viewer| viewer.profiling == Profiling::Reading(id)) else {
        return;
    };
    match profile {
        Ok(profile) => viewer.profiling = Profiling::Read(Arc::new(profile)),
        Err(error) => {
            viewer.profiling = Profiling::Failed;
            state.notice = Some(tr!(viewer_error, error = error));
        }
    }
}

fn finished(state: &mut AppState, result: io::Result<Vec<Effect>>) -> Vec<Effect> {
    result.unwrap_or_else(|error| {
        finish(state, Err(error));
        Vec::new()
    })
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

pub(crate) fn found(state: &mut AppState, id: u64, result: Result<crate::FoundInPager, String>) {
    let Some(Viewer { content: Content::Text(pager), find, .. }) = state.viewer.as_mut() else { return };
    let revealed = find.found(pager, id, result).map(|()| true);
    finish(state, revealed);
}

pub(crate) fn searching(state: &mut AppState, id: u64, percent: u8) {
    if let Some(viewer) = state.viewer.as_mut() {
        viewer.find.progress(id, percent);
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
    let current = state.opening.clone().or_else(|| state.viewer.as_ref()?.path().map(Path::to_path_buf))?;
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

/// Lays the page out for the screen: as wide as it, less the entropy strip beside hex rows.
pub(crate) fn resize(state: &mut AppState) {
    let (width, height) = viewer_size(state.cols, state.rows);
    let failed = state.viewer.as_mut().and_then(|viewer| match &mut viewer.content {
        Content::Text(pager) => {
            let strip = if pager.hex() { usize::from(ENTROPY_STRIP) } else { 0 };
            pager.resize(width.saturating_sub(strip), height).err()
        }
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
