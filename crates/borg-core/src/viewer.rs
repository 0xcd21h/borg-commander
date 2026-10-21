use borg_i18n::tr;
use std::{io, path::PathBuf};

use borg_gfx::{Picture, is_picture};
use borg_view::Pager;
use crossterm::event::{KeyCode, KeyEvent};

use crate::{AppState, Aside, Effect, Peek, Peeked};

pub const STATUS_ROWS: u16 = 1;
const WRAP_KEY: char = 'w';
const QUIT_KEY: char = 'q';
const PAGE_KEY: char = ' ';
const LINE: usize = 1;
const NEXT_KEY: char = 'n';
const PREVIOUS_KEY: char = 'p';

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
    if let Some(Peeked { aside: Aside::QuickView, path: shown, peek: Peek::Text(pager) }) = &state.peeked
        && *shown == path
    {
        state.viewer = Some(Viewer { path, content: Content::Text(pager.clone()) });
        resize(state);
        return Vec::new();
    }
    vec![Effect::View(path)]
}

fn close(state: &mut AppState) {
    if let Some(Viewer { path, content: Content::Text(pager) }) = state.viewer.take()
        && state.aside == Some(Aside::QuickView)
        && crate::aside::hovered_path(state).as_ref() == Some(&path)
    {
        state.peeked = Some(Peeked { aside: Aside::QuickView, path, peek: Peek::Text(pager) });
        state.peeking = None;
    }
}

pub(crate) fn on_key(state: &mut AppState, key: &KeyEvent) -> Vec<Effect> {
    let Some(viewer) = state.viewer.as_mut() else { return Vec::new() };
    let pager = match &mut viewer.content {
        Content::Text(pager) => pager,
        Content::Image(_) => return on_image_key(state, key.code),
    };
    let result = if is_close(key.code) { Ok(false) } else { scroll(pager, key.code).map(|()| true) };

    match result {
        Ok(true) => {}
        Ok(false) => close(state),
        Err(error) => {
            state.notice = Some(format!(tr!(viewer_error), error = error));
            state.viewer = None;
        }
    }
    Vec::new()
}

fn on_image_key(state: &mut AppState, code: KeyCode) -> Vec<Effect> {
    let step = match code {
        code if is_close(code) => {
            state.viewer = None;
            return Vec::new();
        }
        KeyCode::Right | KeyCode::Down | KeyCode::PageDown | KeyCode::Char(PAGE_KEY | NEXT_KEY) => Step::Next,
        KeyCode::Left | KeyCode::Up | KeyCode::PageUp | KeyCode::Backspace | KeyCode::Char(PREVIOUS_KEY) => {
            Step::Previous
        }
        _ => return Vec::new(),
    };
    sibling(state, step).into_iter().map(Effect::View).collect()
}

fn sibling(state: &mut AppState, step: Step) -> Option<PathBuf> {
    let current = state.viewer.as_ref()?.path.clone();
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
        state.notice = Some(format!(tr!(viewer_error), error = error));
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
