//! The file commands: viewing and editing the hovered file, and copying, moving, deleting,
//! packing and unpacking the selection.

use std::{
    path::{Path, PathBuf},
    time::Instant,
};

use borg_i18n::tr;
use borg_view::Query;

use super::{dialogs::prompt, shell::interactive, shell::run_operation};
use crate::{
    Action, AppState, Confirm, ConfirmPurpose, EditorSlot, Effect, Overlay, Panel, PromptPurpose, Side,
    ops::{self, ArchiveFormat, DEFAULT_ARCHIVE_SUFFIX},
};

/// F3: views the hovered file, from this machine, a container or a remote Borg.
pub(super) fn view(state: &mut AppState, side: Side) -> Vec<Effect> {
    let panel = state.panel(side);
    if panel.mount.is_some() {
        return hovered_file(state).map(|path| crate::docker::view(state, &path)).unwrap_or_default();
    }
    if panel.remote_mount.is_some() {
        return remote_view(state, side);
    }
    let Some(path) = hovered_file(state) else { return Vec::new() };
    match contents_match(state.panel(side), &path) {
        Some((query, offset)) => crate::viewer::open_at(state, path, query, offset),
        None => crate::viewer::open(state, path),
    }
}

/// The query a contents search found the file at `path` with, and where its first match starts.
fn contents_match(panel: &Panel, path: &Path) -> Option<(Query, u64)> {
    let results = panel.results.as_ref()?;
    Some((results.contents()?.clone(), results.line(path)?.offset))
}

pub(super) fn remote_view(state: &mut AppState, side: Side) -> Vec<Effect> {
    let panel = state.panel(side);
    let Some(mount) = &panel.remote_mount else { return Vec::new() };
    let Some(path) = panel.hovered_entry().filter(|entry| !entry.is_dir()).map(|entry| entry.path(panel.dir())) else {
        return Vec::new();
    };
    let (machine, session) = (mount.machine.clone(), mount.session.clone());
    state.opening = Some(path.clone());
    vec![Effect::RemoteView { machine, session, path }]
}

/// Opens the hovered file in Borg's editor, full screen or in a panel, on the match a contents
/// search found in it.
pub(super) fn edit_hovered(state: &mut AppState, slot: EditorSlot) -> Vec<Effect> {
    let Some(path) = hovered_file(state) else { return Vec::new() };
    match contents_match(state.active_panel(), &path) {
        Some((query, offset)) => crate::editor::open_at(state, path, slot, query, offset),
        None => crate::editor::open(state, path, slot),
    }
}

/// Opens the hovered file in the editor the shell names.
pub(super) fn edit_external(state: &mut AppState) -> Vec<Effect> {
    let Some(entry) = state.active_panel().hovered_entry().filter(|entry| !entry.is_dir()) else { return Vec::new() };
    let panel = state.active_panel();
    let command = if panel.is_results() { ops::edit_path(&entry.path(panel.dir())) } else { ops::edit(&entry.name) };
    interactive(state, &command)
}

/// F5: asks where to copy the selection, whichever machines, containers or collections the
/// panels show.
pub(super) fn copy(state: &mut AppState, side: Side) -> Vec<Effect> {
    let other = side.other();
    if state.panel(side).remote_mount.is_some() || state.panel(other).remote_mount.is_some() {
        return copy_remote(state, side);
    }
    if state.panel(side).mount.is_some() || state.panel(other).mount.is_some() {
        return transfer(state, tr!(copy), |sources| PromptPurpose::DockerCopy { sources });
    }
    if state.panel(other).is_results() {
        if shows_collection(state.panel(other)) {
            return crate::results::collect(state);
        }
        state.notice = Some(tr!(choose_a_real_folder_or_collection_as).into());
        return Vec::new();
    }
    transfer(state, tr!(copy), |sources| PromptPurpose::Copy { sources, optimize_hdd: false })
}

/// Copies to or from a remote Borg, saying in the prompt which machine and session the copy
/// goes to.
fn copy_remote(state: &mut AppState, side: Side) -> Vec<Effect> {
    let other = side.other();
    if state.panel(side).mount.is_some() || state.panel(other).mount.is_some() || state.panel(other).is_results() {
        state.notice = Some(tr!(remote_copy_destination).into());
        return Vec::new();
    }
    let from = state.panel(side).remote_mount.clone();
    let to = state.panel(other).remote_mount.clone();
    let destination = to
        .as_ref()
        .map(|mount| {
            tr!(
                remote_copy_host,
                machine = state.remote_host_name(&mount.machine),
                session = state.remote_session_name(&mount.machine, &mount.session, &mount.name)
            )
        })
        .unwrap_or_else(|| tr!(remote_copy_client).into());
    let effects = transfer(state, tr!(copy), |sources| PromptPurpose::RemoteCopy { sources, from, to });
    if let Some(Overlay::Prompt(prompt)) = &mut state.overlay {
        prompt.label = tr!(remote_copy_label, label = prompt.label, destination = destination);
    }
    effects
}

/// F6: asks where to move the selection, which only a real folder can take.
pub(super) fn move_selection(state: &mut AppState, side: Side) -> Vec<Effect> {
    if state.panel(side.other()).is_results() {
        state.notice = Some(tr!(move_needs_a_real_destination_folder).into());
        return Vec::new();
    }
    transfer(state, tr!(move), |sources| PromptPurpose::Move { sources })
}

/// F8: asks before deleting the selection; in a collection it only takes the references out.
pub(super) fn delete(state: &mut AppState) -> Vec<Effect> {
    if shows_collection(state.active_panel()) {
        crate::results::perform(state, Action::RemoveReferences)
    } else {
        confirm_delete(state)
    }
}

fn shows_collection(panel: &Panel) -> bool {
    matches!(panel.results.as_ref().map(|r| &r.source), Some(crate::ResultSource::Collection(_)))
}

fn transfer(
    state: &mut AppState,
    title: &'static str,
    purpose: impl FnOnce(Vec<PathBuf>) -> PromptPurpose,
) -> Vec<Effect> {
    let sources = crate::selected_paths(state.active_panel());
    if sources.is_empty() {
        state.notice = Some(tr!(nothing_selected).to_owned());
        return Vec::new();
    }
    let label = tr!(transfer_to, title = title, selection = describe_selection(state.active_panel(), &sources));
    let dest = state.panel(state.active.other()).dir().display().to_string();
    prompt(state, title, &label, &dest, purpose(sources))
}

pub(super) fn confirm_delete(state: &mut AppState) -> Vec<Effect> {
    let sources = crate::selected_paths(state.active_panel());
    if sources.is_empty() {
        state.notice = Some(tr!(nothing_selected).to_owned());
        return Vec::new();
    }
    let message = tr!(delete_permanently, describe_selection(state.active_panel(), &sources));
    state.overlay = Some(Overlay::Confirm(Confirm {
        title: tr!(delete),
        message,
        purpose: ConfirmPurpose::Delete { sources },
        accept: !state.active_panel().is_results(),
    }));
    Vec::new()
}

pub(super) fn compress(state: &mut AppState) -> Vec<Effect> {
    let sources = selected_names(state.active_panel());
    let stem = match sources.as_slice() {
        [] => {
            state.notice = Some(tr!(nothing_selected).to_owned());
            return Vec::new();
        }
        [single] => single.clone(),
        _ => state.active_panel().dir().file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
    };
    let default = format!("{stem}{DEFAULT_ARCHIVE_SUFFIX}");
    prompt(state, tr!(compress), &tr!(archive_as, describe(&sources)), &default, PromptPurpose::Compress { sources })
}

/// Unpacks the hovered archive into the folder of the panel on `to`.
pub(super) fn extract(state: &mut AppState, to: Side, now: Instant) -> Vec<Effect> {
    let dest = state.panel(to).dir().to_path_buf();
    let Some(entry) = state.active_panel().hovered_entry() else { return Vec::new() };
    let Some(format) = ArchiveFormat::detect(&entry.name) else { return Vec::new() };
    let command = ops::extract(&entry.name, format, &dest);
    run_operation(state, tr!(extract), &command, None, now)
}

fn hovered_file(state: &AppState) -> Option<PathBuf> {
    let panel = state.active_panel();
    panel.hovered_entry().filter(|entry| !entry.is_dir()).map(|entry| entry.path(panel.dir()))
}

fn selected_names(panel: &Panel) -> Vec<String> {
    panel.selection().iter().map(|entry| entry.name.clone()).collect()
}

/// The selection in a prompt's words: its names, or for results their paths and how many of
/// those the filter hides.
fn describe_selection(panel: &Panel, paths: &[PathBuf]) -> String {
    if !panel.is_results() {
        return describe(&selected_names(panel));
    }
    let description = describe(&paths.iter().map(|path| path.display().to_string()).collect::<Vec<_>>());
    let hidden =
        panel.results.as_ref().map_or(0, |results| panel.marked().filter(|entry| !results.matches(entry)).count());
    if hidden > 0 { tr!(hidden_by_filter, description = description, hidden = hidden) } else { description }
}

fn describe(names: &[String]) -> String {
    match names {
        [single] => format!("\"{single}\""),
        many => tr!(items, many.len()),
    }
}
