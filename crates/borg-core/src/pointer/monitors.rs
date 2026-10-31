//! Clicks in the monitors that show beside the panels: the activity monitor and the container
//! view.

use std::time::Instant;

use crossterm::event::KeyCode;

use super::Pointer;
use crate::{ActivityPage, ActivitySort, AppState, Effect};

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
            crate::activity::route_key(state, &code.into(), now)
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
/// opens. Moving to another region, search hit or finding previews it after a pause.
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
        usize::from(rect.height.saturating_sub(crate::activity::chrome_rows(context))),
        usize::from(rect.width.saturating_sub(2)),
    );
    state.activity.page = page;
    if !preview
        && matches!(context, ActivityPage::Regions | ActivityPage::Search | ActivityPage::Integrity)
        && cursor != index
    {
        state.activity.schedule_preview(now);
    }
    if open { crate::activity::route_key(state, &KeyCode::Enter.into(), now) } else { Vec::new() }
}

/// A column's title sorts the processes by it, as the column's key does.
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
    crate::activity::route_key(state, &KeyCode::Char(key).into(), now)
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
            crate::docker::route_key(state, &code.into(), now)
        }
        _ => Vec::new(),
    }
}
