//! The startup items screen's keys and clicks: moving, the filter, copying, and opening an
//! item's file.

use std::time::Instant;

use borg_i18n::tr;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{FILTER_LIMIT, PAGE, StartupItems, close, reread};
use crate::{Action, AppState, EditorSlot, Effect};

pub(crate) fn route_key(state: &mut AppState, key: &KeyEvent, now: Instant) -> Vec<Effect> {
    let (code, modifiers) = (key.code, key.modifiers);
    if modifiers == KeyModifiers::ALT.union(KeyModifiers::SHIFT) && matches!(code, KeyCode::Left | KeyCode::Right) {
        let step = if code == KeyCode::Right { crate::keymap::SPLIT_STEP } else { -crate::keymap::SPLIT_STEP };
        return crate::actions::perform(state, Action::NudgeSplit(step), now);
    }
    if modifiers == KeyModifiers::ALT {
        return match code {
            KeyCode::Char('w') => {
                state.startup_items.expanded = !state.startup_items.expanded;
                Vec::new()
            }
            KeyCode::Char('u') => {
                close(state);
                Vec::new()
            }
            KeyCode::Char('m') => crate::actions::perform(state, Action::Activity, now),
            KeyCode::Char('n') => crate::actions::perform(state, Action::Network, now),
            KeyCode::Char('c') => crate::actions::perform(state, Action::Docker, now),
            KeyCode::Char('l') => crate::actions::perform(state, Action::Alerts, now),
            _ => Vec::new(),
        };
    }
    if let Some(action) = crate::activity::panels_action(key) {
        return crate::actions::perform(state, action, now);
    }
    if state.startup_items.filtering && filter_key(&mut state.startup_items, key) {
        return Vec::new();
    }
    let key = &crate::keyboard::latin(key);
    let code = key.code;
    if code == KeyCode::Char('c') && modifiers == KeyModifiers::CONTROL {
        return copy(state);
    }
    if !(modifiers.is_empty() || modifiers == KeyModifiers::SHIFT) {
        return Vec::new();
    }
    let startup = &mut state.startup_items;
    match code {
        KeyCode::Esc => return escape(state),
        KeyCode::Enter => return show_in_panel(state),
        KeyCode::F(3) => return view(state),
        KeyCode::F(4) => return edit(state),
        KeyCode::Char('/') => startup.filtering = true,
        KeyCode::Char('r' | 'R') => return reread(state),
        code => move_cursor(startup, code),
    }
    Vec::new()
}

/// Pastes into the filter while it is typed.
pub(crate) fn paste(state: &mut AppState, text: &str) {
    let startup = &mut state.startup_items;
    if startup.filtering {
        let room = FILTER_LIMIT.saturating_sub(startup.filter.chars().count());
        startup.filter.extend(text.chars().filter(|c| !c.is_control()).take(room));
        startup.rebuild();
    }
}

/// A row clicked in the list: the cursor moves to it, and a double click acts as Enter does.
pub(crate) fn click(state: &mut AppState, row: usize, open: bool) -> Vec<Effect> {
    state.startup_items.select(row);
    if open { show_in_panel(state) } else { Vec::new() }
}

/// The filter's keys while it is typed. Moving keys still move the list.
fn filter_key(startup: &mut StartupItems, key: &KeyEvent) -> bool {
    match key.code {
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            if startup.filter.chars().count() < FILTER_LIMIT {
                startup.filter.push(c);
            }
        }
        KeyCode::Backspace => _ = startup.filter.pop(),
        KeyCode::Enter => startup.filtering = false,
        KeyCode::Esc => {
            startup.filtering = false;
            startup.filter.clear();
        }
        _ => return false,
    }
    startup.rebuild();
    true
}

/// Esc closes the screen, which keeps its filter for the next time it opens.
fn escape(state: &mut AppState) -> Vec<Effect> {
    close(state);
    Vec::new()
}

/// Enter shows the selected item's file in a panel beside the list, as Alt-O does.
fn show_in_panel(state: &mut AppState) -> Vec<Effect> {
    let Some(list) = state.startup_items.side else { return Vec::new() };
    crate::screens::show_row(state, crate::screens::Screen::Startup, list)
}

fn view(state: &mut AppState) -> Vec<Effect> {
    let Some(file) = state.startup_items.selected().map(|item| item.file.clone()) else { return Vec::new() };
    crate::viewer::open(state, file)
}

fn edit(state: &mut AppState) -> Vec<Effect> {
    let Some(file) = state.startup_items.selected().map(|item| item.file.clone()) else { return Vec::new() };
    crate::editor::open(state, file, EditorSlot::Screen)
}

fn move_cursor(startup: &mut StartupItems, code: KeyCode) {
    match code {
        KeyCode::Up => startup.move_by(-1),
        KeyCode::Down => startup.move_by(1),
        KeyCode::PageUp => startup.move_by(-(PAGE as isize)),
        KeyCode::PageDown => startup.move_by(PAGE as isize),
        KeyCode::Home => startup.select(0),
        KeyCode::End => startup.select(startup.len().saturating_sub(1)),
        _ => {}
    }
}

/// Ctrl-C copies what the selected item runs.
fn copy(state: &mut AppState) -> Vec<Effect> {
    let Some(text) = state.startup_items.selected().map(|item| item.found.runs.clone()).filter(|runs| !runs.is_empty())
    else {
        return Vec::new();
    };
    state.notice = Some(tr!(startup_copied).into());
    crate::clipboard::copy(state, text)
}
