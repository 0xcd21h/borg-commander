//! What a delete would remove: its size, and the programs that have those files open, found
//! while the dialog asks so the answer can weigh them.

use crate::{AppState, Confirm, ConfirmPurpose, Effect, Overlay};
use std::path::PathBuf;

/// The size of what a delete would remove.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DeleteSize {
    pub bytes: u64,
    pub files: u64,
    /// False when counting ran out of time or met a folder it could not read, so there is more.
    pub complete: bool,
}

/// A program that has one of the files open, runs from it, or works in one of the folders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileHolder {
    pub pid: u32,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeleteFound {
    Size(DeleteSize),
    /// The programs using the files, or why Borg could not list them.
    Holders(Result<Vec<FileHolder>, String>),
}

/// What Borg knows so far about a delete it asks about; `None` while it is still finding out.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeleteCheck {
    pub id: u64,
    pub size: Option<DeleteSize>,
    pub holders: Option<Result<Vec<FileHolder>, String>>,
}

impl DeleteCheck {
    /// The program K offers signals for.
    pub fn first_holder(&self) -> Option<&FileHolder> {
        self.holders.as_ref()?.as_ref().ok()?.first()
    }
}

/// Starts counting and checking the files of a delete on this computer. Files in an archive,
/// a container or on another computer are left alone.
pub(crate) fn start(state: &mut AppState, sources: &[PathBuf]) -> (Option<DeleteCheck>, Vec<Effect>) {
    if state.active_panel().foreign() {
        return (None, Vec::new());
    }
    state.delete_checks += 1;
    let id = state.delete_checks;
    (Some(DeleteCheck { id, ..Default::default() }), vec![Effect::CheckDelete { id, sources: sources.to_vec() }])
}

/// Takes in what the check found while its dialog is still open.
pub(crate) fn receive(state: &mut AppState, id: u64, found: DeleteFound) {
    let Some(Overlay::Confirm(Confirm { purpose: ConfirmPurpose::Delete { check: Some(check), .. }, .. })) =
        &mut state.overlay
    else {
        return;
    };
    if check.id != id {
        return;
    }
    match found {
        DeleteFound::Size(size) => check.size = Some(size),
        DeleteFound::Holders(holders) => check.holders = Some(holders),
    }
}

/// K in the dialog: it closes, and the activity monitor opens on the first program that has the
/// files open, with its signals.
pub(crate) fn signal(state: &mut AppState, pid: u32) -> Vec<Effect> {
    state.overlay = None;
    let effects = crate::activity::open(state);
    if state.activity.side.is_some() {
        state.activity.reveal(pid, true);
    }
    effects
}
