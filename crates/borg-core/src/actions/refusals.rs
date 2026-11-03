//! The actions a panel's place rules out, and what Borg says instead: files wait while the
//! activity monitor or the container list shows, remote panels take copies and little else,
//! archives take copies, moves, deletes, edits and new folders in the formats Borg writes, and
//! containers and results take only what works there.

use borg_i18n::tr;

use crate::{Action, AppState};

/// What Borg says instead of carrying out `action`, when the panels rule it out.
pub(super) fn refusal(state: &AppState, action: Action) -> Option<String> {
    let side = state.active;
    let mounted = state.panel(side).mount.is_some();
    let other_mounted = state.panel(side.other()).mount.is_some();
    let remote = state.panel(side).remote_mount.is_some() || state.panel(side.other()).remote_mount.is_some();
    let notice = if state.monitor_open() && on_files(action) {
        if state.docker.is_open() {
            tr!(docker_close_for_files)
        } else if state.network.is_open() {
            tr!(network_close_for_files)
        } else {
            tr!(activity_close_for_files)
        }
    } else if let Some(notice) = archive_refusal(state, action) {
        return Some(notice);
    } else if remote && needs_local_files(action) {
        tr!(remote_operation_unavailable)
    } else if (mounted && needs_this_machine(action)) || (other_mounted && action == Action::ExtractToOther) {
        tr!(docker_not_in_container)
    } else if (mounted || other_mounted) && action == Action::Move {
        tr!(docker_move_unsupported)
    } else if state.active_panel().is_results()
        && matches!(action, Action::Compress | Action::ExtractHere | Action::ExtractToOther | Action::Mkdir)
    {
        tr!(reveal_the_original_folder_to_create_directories)
    } else {
        return None;
    };
    Some(notice.to_owned())
}

/// What Borg says instead of `action` when an archive in the panels rules it out: one it does
/// not change, one still opening, or an action that needs a folder of this machine.
fn archive_refusal(state: &AppState, action: Action) -> Option<String> {
    let side = state.active;
    let (here, there) = (state.panel(side).archive.as_ref(), state.panel(side.other()).archive.as_ref());
    let (mount, changes) = match (here, there) {
        (Some(mount), _) if needs_local_files(action) => (mount, changes_archive(action)),
        (_, Some(mount)) if writes_to_other(action) => (mount, matches!(action, Action::Copy | Action::Move)),
        _ => return None,
    };
    if !changes {
        return Some(tr!(archive_unsupported).to_owned());
    }
    match mount.format {
        None => Some(tr!(archive_opening).to_owned()),
        Some(format) if !borg_archive::writable(format) => Some(tr!(archive_read_only, format = format.label())),
        Some(_) => None,
    }
}

/// Actions that change the archive the active panel shows.
fn changes_archive(action: Action) -> bool {
    matches!(action, Action::Move | Action::Delete | Action::Edit | Action::EditInPanel | Action::Mkdir)
}

/// Actions on the files in the panels, which the activity monitor and the container list hide.
fn on_files(action: Action) -> bool {
    matches!(
        action,
        Action::Copy
            | Action::Move
            | Action::Delete
            | Action::DeleteOriginals
            | Action::View
            | Action::Edit
            | Action::EditInPanel
            | Action::EditExternal
            | Action::Mkdir
            | Action::Compress
            | Action::ExtractHere
            | Action::ExtractToOther
            | Action::Collect
            | Action::Git(_)
            | Action::SameFolder
    )
}

/// Actions that write into the other panel's folder.
fn writes_to_other(action: Action) -> bool {
    matches!(action, Action::Copy | Action::Move | Action::ExtractToOther | Action::Collect)
}

/// Actions that work on files where Borg runs, which no remote panel holds.
fn needs_local_files(action: Action) -> bool {
    matches!(
        action,
        Action::Move
            | Action::Delete
            | Action::DeleteOriginals
            | Action::Edit
            | Action::EditInPanel
            | Action::EditExternal
            | Action::Mkdir
            | Action::Compress
            | Action::ExtractHere
            | Action::ExtractToOther
            | Action::Collect
            | Action::Git(_)
            | Action::QuickView
            | Action::Info
            | Action::Search
            | Action::AnalyzeSpace
            | Action::FolderTree
            | Action::CalculateSizes
            | Action::OpenCollection
            | Action::InsertName
            | Action::InsertPath(_)
            | Action::MeasureSizes
            | Action::RefreshSizes
            | Action::RevealOriginal
            | Action::RemoveReferences
            | Action::TimeMachine
    )
}

/// Actions that need this machine's own files, which a container's panel does not show.
fn needs_this_machine(action: Action) -> bool {
    matches!(
        action,
        Action::Edit
            | Action::EditInPanel
            | Action::EditExternal
            | Action::QuickView
            | Action::Info
            | Action::Compress
            | Action::ExtractHere
            | Action::ExtractToOther
            | Action::Search
            | Action::AnalyzeSpace
            | Action::FolderTree
            | Action::CalculateSizes
            | Action::Collect
            | Action::OpenCollection
            | Action::Git(_)
            | Action::TimeMachine
    )
}
