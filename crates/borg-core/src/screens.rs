//! What Borg's full-width screens share: the activity monitor, the container list, the network
//! screen, the startup items and the alerts. One of them shows at a time.
//!
//! A screen lends the pane beside its list to a file panel: Alt-O opens the folder of the
//! selected row's file there, with the cursor on the file, while the list keeps its filter,
//! order and selection. Tab moves the keys between the list and the files, and Esc in the list
//! or the screen's own key gives the pane back to the details.

use std::path::{Path, PathBuf};

use borg_i18n::tr;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{Action, AppState, Effect, Side, View, activity::ActivityPage, keymap};

/// Rows PgUp and PgDn move a list.
pub(crate) const PAGE: usize = 10;
/// Characters a screen's filter holds at most.
pub(crate) const FILTER_LIMIT: usize = 128;
/// Rows a list pane's frame, summary, filter, column titles and footer take.
pub(crate) const CHROME_ROWS: u16 = 6;
/// With Alt, shows the file of the list's row in the pane beside it.
const SHOW_FILE: KeyCode = KeyCode::Char('o');
/// What Linux adds to the path of a program deleted while it runs.
const DELETED: &str = " (deleted)";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Screen {
    Activity,
    Docker,
    Network,
    Startup,
    Alerts,
}

const SCREENS: [Screen; 5] = [Screen::Activity, Screen::Docker, Screen::Network, Screen::Startup, Screen::Alerts];

impl Screen {
    /// The panels' action that opens the screen, whose key closes it again.
    fn action(self) -> Action {
        match self {
            Self::Activity => Action::Activity,
            Self::Docker => Action::Docker,
            Self::Network => Action::Network,
            Self::Startup => Action::StartupItems,
            Self::Alerts => Action::Alerts,
        }
    }
}

/// Where a row's file is, for the panel beside the list.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Place {
    /// A file, which the cursor goes to in its folder.
    File(PathBuf),
    /// A folder, which opens.
    Folder(PathBuf),
    /// A program deleted while it runs: its folder opens.
    Deleted(PathBuf),
}

fn close(state: &mut AppState, screen: Screen) {
    match screen {
        Screen::Activity => state.activity.side = None,
        Screen::Docker => crate::docker::close(state),
        Screen::Network => crate::network::close(state),
        Screen::Startup => crate::startup_items::close(state),
        Screen::Alerts => crate::watch::close(state),
    }
}

/// Closes every screen, as the panels' own views need.
pub(crate) fn close_all(state: &mut AppState) {
    SCREENS.into_iter().for_each(|screen| close(state, screen));
}

/// Closes every screen but the one about to open.
pub(crate) fn close_others(state: &mut AppState, opening: Screen) {
    SCREENS.into_iter().filter(|screen| *screen != opening).for_each(|screen| close(state, screen));
}

/// The screen that shows, and the side of its list.
pub(crate) fn open_screen(state: &AppState) -> Option<(Screen, Side)> {
    [
        (Screen::Docker, state.docker.side),
        (Screen::Activity, state.activity.side),
        (Screen::Network, state.network.side),
        (Screen::Alerts, state.alerts.side),
        (Screen::Startup, state.startup_items.side),
    ]
    .into_iter()
    .find_map(|(screen, side)| side.map(|side| (screen, side)))
}

impl AppState {
    /// Whether the file panel a screen lends beside its list has the keys.
    pub fn files_beside_focused(&self) -> bool {
        self.lent == Some(self.active) && open_screen(self).is_some()
    }

    /// Whether Esc in a screen's list gives the pane it lends back to the details.
    pub fn esc_gives_back(&self) -> bool {
        self.lent.is_some()
            && !self.files_beside_focused()
            && open_screen(self).is_some_and(|(screen, _)| !typing(self, screen))
    }
}

fn expanded(state: &AppState, screen: Screen) -> bool {
    match screen {
        Screen::Activity => state.activity.expanded,
        Screen::Docker => state.docker.expanded,
        Screen::Network => state.network.expanded,
        Screen::Startup => state.startup_items.expanded,
        Screen::Alerts => state.alerts.expanded,
    }
}

/// Whether the list takes typed text: a filter, the memory search form or a container's shell.
fn typing(state: &AppState, screen: Screen) -> bool {
    match screen {
        Screen::Activity => state.activity.filtering || state.activity.search_form.is_some(),
        Screen::Docker => state.docker.typing() || state.docker.filtering || state.docker.disk.filtering,
        Screen::Network => state.network.typing(),
        Screen::Startup => state.startup_items.typing(),
        Screen::Alerts => state.alerts.typing(),
    }
}

/// Stops typing in a filter, which keeps what was typed.
fn stop_filtering(state: &mut AppState) {
    state.activity.filtering = false;
    state.docker.filtering = false;
    state.docker.disk.filtering = false;
    state.network.filtering = false;
    state.startup_items.filtering = false;
    state.alerts.filtering = false;
}

/// The keys for the panel a screen lends beside its list. Alt-O shows the row's file there,
/// Tab moves the keys between the list and the files, and Esc in the list or the screen's own
/// key gives the pane back to the details.
pub(crate) fn lent_key(state: &mut AppState, key: &KeyEvent) -> Option<Vec<Effect>> {
    if state.view != View::Panels {
        return None;
    }
    let (screen, list) = open_screen(state)?;
    let files = state.files_beside_focused();
    let lent = state.lent.is_some();
    let plain = key.modifiers.is_empty();
    if lent && key.code == KeyCode::Tab && plain && state.activity.search_form.is_none() {
        stop_filtering(state);
        state.active = if files { list } else { list.other() };
        return Some(Vec::new());
    }
    if !files && typing(state, screen) {
        return None;
    }
    if key.code == SHOW_FILE && key.modifiers == KeyModifiers::ALT {
        return Some(show_row(state, screen, list));
    }
    let own = || keymap::lookup(key, keymap::Context { view: View::Panels, line_empty: false, at_prompt: false });
    if (key.code == KeyCode::Esc && plain && state.esc_gives_back()) || (lent && own() == Some(screen.action())) {
        give_back(state, list);
        return Some(Vec::new());
    }
    None
}

/// The details take the pane back, and the list the keys.
fn give_back(state: &mut AppState, list: Side) {
    state.lent = None;
    state.awaited = None;
    state.active = list;
}

/// A click on a screen gives its list the keys.
pub(crate) fn focus_list(state: &mut AppState) {
    if let Some((_, list)) = open_screen(state).filter(|_| state.lent.is_some()) {
        state.active = list;
    }
}

/// Shows the file of the list's selected row in the panel beside it, or says why there is none.
pub(crate) fn show_row(state: &mut AppState, screen: Screen, list: Side) -> Vec<Effect> {
    match place(state, screen) {
        Ok(place) => show(state, screen, list, place),
        Err(notice) => {
            state.notice = Some(notice);
            Vec::new()
        }
    }
}

fn place(state: &AppState, screen: Screen) -> Result<Place, String> {
    let missing = || tr!(lent_no_file).to_owned();
    match screen {
        Screen::Activity => activity_place(state).ok_or_else(missing),
        Screen::Startup => {
            state.startup_items.selected().map(|item| Place::File(item.file.clone())).ok_or_else(missing)
        }
        Screen::Alerts => {
            let process = state.alerts.selected().map(|alert| &alert.process).ok_or_else(missing)?;
            let program = process.program.clone().filter(|path| path.is_absolute()).ok_or_else(missing)?;
            Ok(if process.program_deleted { Place::Deleted(program) } else { Place::File(program) })
        }
        Screen::Docker => compose_place(state).ok_or_else(|| tr!(lent_no_compose_folder).to_owned()),
        Screen::Network => Err(tr!(lent_network_hint).to_owned()),
    }
}

/// The selected process's program, or on the pages that list a process's files, the selected
/// open file or the file a memory region maps.
fn activity_place(state: &AppState) -> Option<Place> {
    let activity = &state.activity;
    let path = match activity.list {
        ActivityPage::Files => activity.files.get(activity.file_cursor).map(|file| file.name.clone()),
        ActivityPage::Regions => activity.selected_region().and_then(|region| region.path.clone()),
        _ => activity.process().and_then(|process| process.executable.as_ref()).map(|path| path.display().to_string()),
    }?;
    let place = match path.strip_suffix(DELETED) {
        Some(deleted) => Place::Deleted(deleted.into()),
        None => Place::File(path.as_str().into()),
    };
    Path::new(path.strip_suffix(DELETED).unwrap_or(&path)).is_absolute().then_some(place)
}

/// The Compose file a container or project was started from, or else its project's folder.
fn compose_place(state: &AppState) -> Option<Place> {
    let docker = &state.docker;
    if docker.disk.open {
        return None;
    }
    let container = docker.container().or_else(|| {
        let first = *docker.group()?.containers.first()?;
        docker.containers.get(first)
    })?;
    let folder = container.project_dir.as_deref().map(PathBuf::from);
    let file = container.config_files.first().map(PathBuf::from).and_then(|file| match &folder {
        _ if file.is_absolute() => Some(file),
        Some(folder) => Some(folder.join(file)),
        None => None,
    });
    file.map(Place::File).or_else(|| folder.filter(|folder| folder.is_absolute()).map(Place::Folder))
}

fn split(path: &Path) -> (PathBuf, Option<String>) {
    match (path.parent(), path.file_name()) {
        (Some(folder), Some(name)) => (folder.to_path_buf(), Some(name.to_string_lossy().into_owned())),
        _ => (path.to_path_buf(), None),
    }
}

/// Opens the place in the panel beside the list, which gets the keys. A full-width list makes
/// room for it.
fn show(state: &mut AppState, screen: Screen, list: Side, place: Place) -> Vec<Effect> {
    let side = list.other();
    narrow(state, screen);
    leave_details(state);
    state.lent = Some(side);
    state.active = side;
    state.aside = None;
    state.preview_focused = false;
    state.awaited = None;
    let (folder, focus) = match &place {
        Place::File(file) => split(file),
        Place::Deleted(file) => {
            state.notice = Some(tr!(lent_program_deleted, file = file.display()));
            (split(file).0, None)
        }
        Place::Folder(folder) => (folder.clone(), None),
    };
    state.refit();
    let effects = crate::folders::open(state, side, folder, focus);
    if let (Place::File(file), Some(Effect::List { generation, .. })) = (place, effects.last()) {
        state.awaited = Some((*generation, file));
    }
    effects
}

fn narrow(state: &mut AppState, screen: Screen) {
    match screen {
        Screen::Activity => state.activity.expanded = false,
        Screen::Docker => state.docker.expanded = false,
        Screen::Network => state.network.expanded = false,
        Screen::Startup => state.startup_items.expanded = false,
        Screen::Alerts => state.alerts.expanded = false,
    }
}

/// The details take the keys: the memory pane with its search form, or a container's or a
/// connection's details with a container's shell.
fn details_focused(state: &AppState) -> bool {
    state.activity.preview_focused
        || state.activity.search_form.is_some()
        || state.docker.details_focused
        || state.network.details_focused
}

fn leave_details(state: &mut AppState) {
    state.activity.preview_focused = false;
    state.docker.details_focused = false;
    state.network.details_focused = false;
}

/// Takes the lent panel back once its screen closes, moves or fills the width, or its details
/// take the keys, and keeps a preview beside the files from covering the list.
pub(crate) fn sync_lent(state: &mut AppState) {
    let Some(side) = state.lent else { return };
    let keeps = open_screen(state)
        .is_some_and(|(screen, list)| list.other() == side && !expanded(state, screen) && !details_focused(state));
    if !keeps {
        state.lent = None;
        state.awaited = None;
        return;
    }
    state.aside = None;
    state.preview_focused = false;
}

/// The lent panel's folder listed: a notice says when the file it was opened for is not there,
/// as for a program deleted since, or one that runs in a container and is only there.
pub(crate) fn listed(state: &mut AppState, side: Side, generation: u64) {
    if state.lent != Some(side) || state.awaited.as_ref().is_none_or(|(awaited, _)| *awaited != generation) {
        return;
    }
    let Some((_, file)) = state.awaited.take() else { return };
    let (folder, name) = split(&file);
    let panel = state.panel(side);
    let found =
        panel.dir() == folder && name.is_some_and(|name| panel.entries().iter().any(|entry| entry.name == name));
    if !found {
        state.notice = Some(tr!(lent_file_missing, file = file.display()));
    }
}

#[cfg(test)]
mod tests;
