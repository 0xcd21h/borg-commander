//! The Alerts page's keys and clicks: moving, the filter, and what the user does about a
//! frozen process.

use std::time::Instant;

use borg_i18n::tr;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{Alerts, AlertsRequest, FILTER_LIMIT, PAGE, close, reread};
use crate::{Action, AppState, Confirm, ConfirmPurpose, Effect, Overlay};

pub(crate) fn route_key(state: &mut AppState, key: &KeyEvent, now: Instant) -> Vec<Effect> {
    let (code, modifiers) = (key.code, key.modifiers);
    if modifiers == KeyModifiers::ALT.union(KeyModifiers::SHIFT) && matches!(code, KeyCode::Left | KeyCode::Right) {
        let step = if code == KeyCode::Right { crate::keymap::SPLIT_STEP } else { -crate::keymap::SPLIT_STEP };
        return crate::actions::perform(state, Action::NudgeSplit(step), now);
    }
    if modifiers == KeyModifiers::ALT {
        return match code {
            KeyCode::Char('w') => {
                state.alerts.expanded = !state.alerts.expanded;
                Vec::new()
            }
            KeyCode::Char('l') => {
                close(state);
                Vec::new()
            }
            KeyCode::Char('m') => crate::actions::perform(state, Action::Activity, now),
            KeyCode::Char('n') => crate::actions::perform(state, Action::Network, now),
            KeyCode::Char('u') => crate::actions::perform(state, Action::StartupItems, now),
            KeyCode::Char('c') => crate::actions::perform(state, Action::Docker, now),
            _ => Vec::new(),
        };
    }
    if let Some(action) = crate::activity::panels_action(key) {
        return crate::actions::perform(state, action, now);
    }
    if state.alerts.filtering && filter_key(&mut state.alerts, key) {
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
    match code {
        KeyCode::Esc => return escape(state),
        KeyCode::Enter => return in_activity(state, now),
        KeyCode::Char('/') => state.alerts.filtering = true,
        KeyCode::Char('r' | 'R') => return reread(state),
        KeyCode::Char('c' | 'C') => return resume(state),
        KeyCode::Char('k' | 'K') => return ask_to_kill(state),
        KeyCode::Char('a' | 'A') => return allow(state),
        code => move_cursor(&mut state.alerts, code),
    }
    Vec::new()
}

/// Pastes into the filter while it is typed.
pub(crate) fn paste(state: &mut AppState, text: &str) {
    let alerts = &mut state.alerts;
    if alerts.filtering {
        let room = FILTER_LIMIT.saturating_sub(alerts.filter.chars().count());
        alerts.filter.extend(text.chars().filter(|c| !c.is_control()).take(room));
        alerts.rebuild();
    }
}

/// A row clicked in the list: the cursor moves to it, and a double click acts as Enter does.
pub(crate) fn click(state: &mut AppState, row: usize, open: bool, now: Instant) -> Vec<Effect> {
    state.alerts.select(row);
    if open { in_activity(state, now) } else { Vec::new() }
}

/// The filter's keys while it is typed. Moving keys still move the list.
fn filter_key(alerts: &mut Alerts, key: &KeyEvent) -> bool {
    match key.code {
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            if alerts.filter.chars().count() < FILTER_LIMIT {
                alerts.filter.push(c);
            }
        }
        KeyCode::Backspace => _ = alerts.filter.pop(),
        KeyCode::Enter => alerts.filtering = false,
        KeyCode::Esc => {
            alerts.filtering = false;
            alerts.filter.clear();
        }
        _ => return false,
    }
    alerts.rebuild();
    true
}

/// Esc closes the page, which keeps its filter for the next time it opens.
fn escape(state: &mut AppState) -> Vec<Effect> {
    close(state);
    Vec::new()
}

/// Enter shows the alert's process in the activity monitor, where the user sees what it does
/// now and can look through its memory.
fn in_activity(state: &mut AppState, now: Instant) -> Vec<Effect> {
    let Some(pid) = state.alerts.selected().map(|alert| alert.process.key.pid) else { return Vec::new() };
    let effects = crate::actions::perform(state, Action::Activity, now);
    state.activity.reveal(pid, false);
    effects
}

fn resume(state: &mut AppState) -> Vec<Effect> {
    let Some(alert) = state.alerts.selected().filter(|alert| alert.resumable()) else {
        state.notice = Some(tr!(watch_nothing_to_resume).into());
        return Vec::new();
    };
    let request = AlertsRequest::Resume { alert: alert.id, process: alert.process.key };
    state.alerts.request(request)
}

/// K asks first: killing a process cannot be taken back, and a frozen one is not going
/// anywhere meanwhile.
fn ask_to_kill(state: &mut AppState) -> Vec<Effect> {
    let Some(alert) = state.alerts.selected() else { return Vec::new() };
    let (id, process, name) = (alert.id, alert.process.key, alert.program().to_owned());
    state.overlay = Some(Overlay::Confirm(Confirm {
        title: tr!(watch_kill_title),
        message: tr!(watch_confirm_kill, name = name, pid = process.pid),
        purpose: ConfirmPurpose::AlertKill { alert: id, process },
        accept: false,
    }));
    Vec::new()
}

pub(crate) fn kill(state: &mut AppState, alert: u64, process: crate::ProcessKey) -> Vec<Effect> {
    state.alerts.request(AlertsRequest::Kill { alert, process })
}

/// A allows starts like the selected alert's from now on, and writes it to the configuration.
fn allow(state: &mut AppState) -> Vec<Effect> {
    let Some(alert) = state.alerts.selected() else { return Vec::new() };
    let request = AlertsRequest::Allow { alert: alert.id, allowance: alert.allowance() };
    state.alerts.request(request)
}

fn move_cursor(alerts: &mut Alerts, code: KeyCode) {
    match code {
        KeyCode::Up => alerts.move_by(-1),
        KeyCode::Down => alerts.move_by(1),
        KeyCode::PageUp => alerts.move_by(-(PAGE as isize)),
        KeyCode::PageDown => alerts.move_by(PAGE as isize),
        KeyCode::Home => alerts.select(0),
        KeyCode::End => alerts.select(alerts.len().saturating_sub(1)),
        _ => {}
    }
}

/// Ctrl-C copies the command the alert's process ran.
fn copy(state: &mut AppState) -> Vec<Effect> {
    let Some(text) = state.alerts.selected().map(|alert| alert.process.command_line()).filter(|line| !line.is_empty())
    else {
        return Vec::new();
    };
    state.notice = Some(tr!(watch_copied).into());
    crate::clipboard::copy(state, text)
}
