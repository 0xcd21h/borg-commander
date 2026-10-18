use std::{io, path::PathBuf};

use borg_view::Pager;
use crossterm::event::{KeyCode, KeyEvent};

use crate::{AppState, Effect};

pub const STATUS_ROWS: u16 = 1;
const WRAP_KEY: char = 'w';
const QUIT_KEY: char = 'q';
const PAGE_KEY: char = ' ';
const LINE: usize = 1;

#[derive(Debug, Clone)]
pub struct Viewer {
    pub path: PathBuf,
    pub pager: Pager,
}

pub fn viewer_size(cols: u16, rows: u16) -> (usize, usize) {
    (usize::from(cols), usize::from(rows.saturating_sub(STATUS_ROWS)))
}

pub(crate) fn on_key(state: &mut AppState, key: &KeyEvent) -> Vec<Effect> {
    let Some(viewer) = state.viewer.as_mut() else { return Vec::new() };
    let pager = &mut viewer.pager;
    let result: io::Result<bool> = match key.code {
        KeyCode::Esc | KeyCode::F(3 | 10) | KeyCode::Char(QUIT_KEY) => Ok(false),
        KeyCode::Down => pager.scroll_down(LINE).map(|()| true),
        KeyCode::Up => pager.scroll_up(LINE).map(|()| true),
        KeyCode::PageDown | KeyCode::Char(PAGE_KEY) => pager.page_down().map(|()| true),
        KeyCode::PageUp => pager.page_up().map(|()| true),
        KeyCode::Home => pager.home().map(|()| true),
        KeyCode::End => pager.end().map(|()| true),
        KeyCode::Left => pager.scroll_left().map(|()| true),
        KeyCode::Right => pager.scroll_right().map(|()| true),
        KeyCode::F(2) | KeyCode::Char(WRAP_KEY) => pager.toggle_wrap().map(|()| true),
        _ => Ok(true),
    };

    match result {
        Ok(true) => {}
        Ok(false) => state.viewer = None,
        Err(error) => {
            state.notice = Some(format!("viewer: {error}"));
            state.viewer = None;
        }
    }
    Vec::new()
}

pub(crate) fn resize(state: &mut AppState) {
    let (width, height) = viewer_size(state.cols, state.rows);
    let failed = state.viewer.as_mut().and_then(|viewer| viewer.pager.resize(width, height).err());
    if let Some(error) = failed {
        state.notice = Some(format!("viewer: {error}"));
        state.viewer = None;
    }
}
