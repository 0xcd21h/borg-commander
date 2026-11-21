//! Clicks in the monitors that show beside the panels: the activity monitor, the container
//! view and the network screen.

use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent};

use super::Pointer;
use crate::{ActivityPage, ActivitySort, AppState, Effect, screens::clicked_key};

/// A screen's handler of the keys its controls press.
type RouteKey = fn(&mut AppState, &KeyEvent, Instant) -> Vec<Effect>;

/// Presses the key of a control clicked on a screen, unless the click would only type it into the
/// filter.
fn press(state: &mut AppState, code: KeyCode, route: RouteKey, now: Instant) -> Vec<Effect> {
    clicked_key(state, code).map_or_else(Vec::new, |code| route(state, &code.into(), now))
}

/// A click in the activity monitor: on its frame, a field, a control, a row or a column's title.
pub(super) fn activity(state: &mut AppState, pointer: Pointer, now: Instant) -> Vec<Effect> {
    match pointer {
        Pointer::ActivityExpand { preview, expand } => {
            state.activity.preview_focused = preview;
            state.activity.expanded = expand;
            Vec::new()
        }
        Pointer::ActivityHide => {
            state.activity.side = None;
            Vec::new()
        }
        Pointer::ActivitySearchField(index) => search_field(state, index),
        Pointer::ActivityControl { preview, code } => {
            state.activity.preview_focused = preview;
            press(state, code, crate::activity::route_key, now)
        }
        Pointer::ActivityRow { index, open, preview } => row(state, index, open, preview, now),
        Pointer::ActivitySort(sort) => sort_by(state, sort, now),
        _ => Vec::new(),
    }
}

/// A field of the monitor's search form. A click on any but the first acts on it as Space does.
fn search_field(state: &mut AppState, index: usize) -> Vec<Effect> {
    state.activity.preview_focused = true;
    if let Some(form) = state.activity.search_form.as_mut() {
        form.selected = index;
    }
    if index > 0 { crate::activity::on_key(state, &KeyCode::Char(' ').into()) } else { Vec::new() }
}

/// A row of the monitor's list or of its preview, which the cursor moves to and a double click
/// opens, also while the filter is typed. Moving to another region, search hit or finding
/// previews it after a pause.
fn row(state: &mut AppState, index: usize, open: bool, preview: bool, now: Instant) -> Vec<Effect> {
    let Some(side) = state.activity.side else { return Vec::new() };
    state.activity.preview_focused = preview;
    let page = state.activity.page;
    let context = state.activity.focused_page();
    let cursor = state.activity.list_cursor(context);
    state.activity.page = context;
    let rect = state.layout(now).panels[if preview { side.other() } else { side }.index()];
    state.activity.move_by(
        index as isize - cursor as isize,
        usize::from(rect.height.saturating_sub(crate::activity::chrome_rows(&state.activity, context, rect))),
        usize::from(rect.width.saturating_sub(2)),
    );
    state.activity.page = page;
    if !preview
        && matches!(context, ActivityPage::Regions | ActivityPage::Search | ActivityPage::Findings)
        && cursor != index
    {
        state.activity.schedule_preview(now);
    }
    if !open {
        return Vec::new();
    }
    state.activity.filtering = false;
    crate::activity::route_key(state, &KeyCode::Enter.into(), now)
}

/// A column's title sorts the processes by it, as the column's key does, also while the filter is
/// typed.
fn sort_by(state: &mut AppState, sort: ActivitySort, now: Instant) -> Vec<Effect> {
    state.activity.preview_focused = false;
    let key = match sort {
        ActivitySort::Pid => 'p',
        ActivitySort::Name => 'n',
        ActivitySort::Memory => 'm',
        ActivitySort::Cpu => 'c',
        ActivitySort::Growth => 'g',
        ActivitySort::Read => 'i',
        ActivitySort::Write => 'o',
    };
    press(state, KeyCode::Char(key), crate::activity::route_key, now)
}

/// A click in the container view: on its frame, a row or a control, or the wheel over a
/// container's shell.
pub(super) fn containers(state: &mut AppState, pointer: Pointer, now: Instant) -> Vec<Effect> {
    match pointer {
        Pointer::DockerExpand { details, expand } => {
            state.docker.details_focused = details;
            state.docker.expanded = expand;
            Vec::new()
        }
        Pointer::DockerHide => {
            crate::docker::close(state);
            Vec::new()
        }
        Pointer::DockerRow { index, open } => crate::docker::click(state, index, open, now),
        Pointer::DockerShellScroll(lines) => {
            crate::docker::scroll_shell(state, borg_term::Scroll::Delta(-(lines as i32)))
        }
        Pointer::DockerControl { details, code } => {
            state.docker.details_focused = details;
            press(state, code, crate::docker::route_key, now)
        }
        _ => Vec::new(),
    }
}

/// A click in the startup items screen: on its frame, a row or a control.
pub(super) fn startup(state: &mut AppState, pointer: Pointer, now: Instant) -> Vec<Effect> {
    match pointer {
        Pointer::StartupExpand(expand) => {
            state.startup_items.expanded = expand;
            Vec::new()
        }
        Pointer::StartupHide => {
            crate::startup_items::close(state);
            Vec::new()
        }
        Pointer::StartupRow { index, open } => crate::startup_items::click(state, index, open),
        Pointer::StartupControl(code) => press(state, code, crate::startup_items::route_key, now),
        _ => Vec::new(),
    }
}

/// A click in the hosts screen: on its frame, a card or a row, or a control.
pub(super) fn hosts(state: &mut AppState, pointer: Pointer, now: Instant) -> Vec<Effect> {
    match pointer {
        Pointer::HostsExpand(expand) => {
            state.hosts.expanded = expand;
            Vec::new()
        }
        Pointer::HostsHide => {
            crate::hosts::close(state);
            Vec::new()
        }
        Pointer::HostsRow { index, open } => crate::hosts::click(state, index, open),
        Pointer::HostsControl(code) => press(state, code, crate::hosts::route_key, now),
        _ => Vec::new(),
    }
}

/// A click in the Alerts page: on its frame, a row or a control.
pub(super) fn alerts(state: &mut AppState, pointer: Pointer, now: Instant) -> Vec<Effect> {
    match pointer {
        Pointer::AlertsExpand(expand) => {
            state.alerts.expanded = expand;
            Vec::new()
        }
        Pointer::AlertsHide => {
            crate::watch::close(state);
            Vec::new()
        }
        Pointer::AlertsRow { index, open } => crate::watch::click(state, index, open, now),
        Pointer::AlertsControl(code) => press(state, code, crate::watch::route_key, now),
        _ => Vec::new(),
    }
}

/// A click in the network screen: on its frame, a row, a column's title or a control.
pub(super) fn network(state: &mut AppState, pointer: Pointer, now: Instant) -> Vec<Effect> {
    match pointer {
        Pointer::NetworkExpand { details, expand } => {
            state.network.details_focused = details;
            state.network.expanded = expand;
            Vec::new()
        }
        Pointer::NetworkHide => {
            crate::network::close(state);
            Vec::new()
        }
        Pointer::NetworkRow { index, open } => crate::network::click(state, index, open),
        Pointer::NetworkSort(sort) => {
            state.network.details_focused = false;
            crate::network::sort_by(state, sort);
            Vec::new()
        }
        Pointer::NetworkPage(page) => {
            crate::network::show_page(&mut state.network, page);
            Vec::new()
        }
        Pointer::TrafficPage(page) => {
            crate::network::show_traffic_page(&mut state.network, page);
            Vec::new()
        }
        Pointer::NetworkControl { details, code } => {
            state.network.details_focused = details;
            press(state, code, crate::network::route_key, now)
        }
        _ => Vec::new(),
    }
}
