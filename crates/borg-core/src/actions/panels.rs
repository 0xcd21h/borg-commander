//! Moving in and between the panels, and how they are split, listed and sorted.

use std::time::Instant;

use super::{LINE_DOWN, LINE_UP, files::remote_view, shell::interactive};
use crate::{AppState, Effect, ListingMode, Panel, Row, Side, Sort, SortKey, command::CommandLine, ops};

pub(super) fn moved(state: &mut AppState, motion: impl FnOnce(&mut Panel)) -> Vec<Effect> {
    motion(state.panel_mut(state.active));
    Vec::new()
}

pub(super) fn mark_and_move(state: &mut AppState, step: isize) -> Vec<Effect> {
    moved(state, |panel| {
        panel.toggle_mark();
        panel.move_by(step);
    })
}

/// Tab: to the other panel, or between the panel and the preview beside it.
pub(super) fn switch(state: &mut AppState) -> Vec<Effect> {
    if state.aside.is_some() {
        state.preview_focused = !state.preview_focused;
    } else {
        state.active = state.active.other();
    }
    Vec::new()
}

/// Enter on the hovered row: opens a group or folder, runs a program, or views a file.
pub(super) fn open(state: &mut AppState, side: Side) -> Vec<Effect> {
    let panel = state.panel(side);
    match panel.hovered() {
        Some(Row::Group { .. }) => {
            state.panel_mut(side).toggle_group();
            Vec::new()
        }
        Some(Row::Parent) => leave(state, side).into_iter().collect(),
        Some(Row::Entry(entry)) if entry.is_dir() => descend(state, side).into_iter().collect(),
        Some(Row::Entry(_)) if panel.remote_mount.is_some() => remote_view(state, side),
        Some(Row::Entry(entry)) if panel.mount.is_some() => {
            let path = entry.path(panel.dir());
            crate::docker::view(state, &path)
        }
        Some(Row::Entry(entry)) if entry.executable => {
            let command = if panel.is_results() {
                CommandLine::default().path(&entry.path(panel.dir()))
            } else {
                ops::run(&entry.name)
            };
            interactive(state, &command)
        }
        Some(Row::Entry(entry)) => crate::viewer::open(state, entry.path(panel.dir())),
        None => Vec::new(),
    }
}

/// Right: opens a tree's branch, moves a column on in a brief listing, or enters a folder.
pub(super) fn step_in(state: &mut AppState, side: Side) -> Vec<Effect> {
    if state.panel(side).is_tree() {
        state.panel_mut(side).tree_step(true);
        return Vec::new();
    }
    if state.panel(side).mode() == ListingMode::Brief {
        return moved(state, |panel| panel.move_column(LINE_DOWN));
    }
    descend(state, side).into_iter().collect()
}

/// Left: closes a tree's branch, moves a column back in a brief listing, or leaves the folder.
pub(super) fn step_out(state: &mut AppState, side: Side) -> Vec<Effect> {
    if state.panel(side).is_tree() {
        state.panel_mut(side).tree_step(false);
        return Vec::new();
    }
    if state.panel(side).mode() == ListingMode::Brief {
        return moved(state, |panel| panel.move_column(LINE_UP));
    }
    leave(state, side).into_iter().collect()
}

fn descend(state: &mut AppState, side: Side) -> Option<Effect> {
    if matches!(state.panel(side).hovered(), Some(Row::Group { .. })) {
        state.panel_mut(side).toggle_group();
        return None;
    }
    let panel = state.panel(side);
    let entry = panel.hovered_entry().filter(|entry| entry.is_dir())?;
    let dir = entry.path(panel.dir());
    Some(state.navigate(side, dir, None))
}

fn leave(state: &mut AppState, side: Side) -> Option<Effect> {
    if state.panel(side).remote_mount.is_some() && state.panel(side).dir().parent().is_none() {
        return crate::remote::unmount(state, side);
    }
    if state.panel(side).mount.is_some() && state.panel(side).dir().parent().is_none() {
        return crate::docker::unmount(state, side);
    }
    if state.panel(side).is_results() {
        return crate::results::back(state).into_iter().next();
    }
    let current = state.panel(side).dir();
    let parent = current.parent()?.to_path_buf();
    let child = current.file_name().map(|name| name.to_string_lossy().into_owned());
    Some(state.navigate(side, parent, child))
}

/// Swaps the panels with all they hold: listings, results, their history and their editors.
pub(super) fn swap(state: &mut AppState) -> Vec<Effect> {
    state.panels.swap(0, 1);
    state.result_history.swap(0, 1);
    state.last_results.swap(0, 1);
    state.panels.iter_mut().for_each(|panel| panel.git = crate::GitPanel::default());
    state.docked.swap(0, 1);
    for (side, editor) in Side::ALL.into_iter().zip(&mut state.docked) {
        if let Some(editor) = editor {
            editor.slot = crate::EditorSlot::Panel(side);
        }
    }
    crate::editor::resize(state);
    if state.panels.iter().any(Panel::is_results) { state.reload_all() } else { Vec::new() }
}

/// Moves the split between the panels to `split`, the left panel's share of the width.
pub(super) fn set_split(state: &mut AppState, split: f32, now: Instant) -> Vec<Effect> {
    state.set_split(split, now);
    crate::editor::resize(state);
    Vec::new()
}

pub(super) fn nudge_split(state: &mut AppState, columns: i16, now: Instant) -> Vec<Effect> {
    let step = f32::from(columns) / f32::from(state.cols.max(1));
    set_split(state, state.split_target() + step, now)
}

pub(super) fn set_listing(state: &mut AppState, side: Side, mode: ListingMode) -> Vec<Effect> {
    state.panel_mut(side).set_mode(mode);
    Vec::new()
}

pub(super) fn set_sort(state: &mut AppState, side: Side, key: SortKey) -> Vec<Effect> {
    let reverse = state.panel(side).sort().reverse;
    state.panel_mut(side).set_sort(Sort { key, reverse });
    Vec::new()
}

pub(super) fn toggle_reverse(state: &mut AppState, side: Side) -> Vec<Effect> {
    let sort = state.panel(side).sort();
    state.panel_mut(side).set_sort(Sort { reverse: !sort.reverse, ..sort });
    Vec::new()
}
