//! Folders to jump to: the hotlist you keep, the folders the panels visited, the picker that
//! lists them above the command line while you type `cd `, and the dialog Ctrl-\ opens.

use std::path::{Path, PathBuf};

use borg_i18n::tr;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{AppState, Console, Effect, LineInput, Overlay, Side, View};

/// Recent folders remembered, newest first.
pub const RECENT_LIMIT: usize = 64;
/// Folders on the hotlist.
pub const HOTLIST_LIMIT: usize = 256;
/// Rows the picker above the command line shows at most.
pub const PICKER_ROWS: usize = 8;
/// What the picker types to clear the command line: the end of the line, then everything
/// before the cursor. Bash, zsh and fish all take these keys.
pub const CLEAR_LINE: &[u8] = b"\x05\x15";
/// The command a typed line must start with for the picker to show.
pub const CD: &str = "cd";

const HOME_PREFIX: &str = "~";
/// Characters that make a command line more than a change of folder.
const COMPOUND: [char; 9] = [';', '&', '|', '<', '>', '`', '(', ')', '\n'];
const SUBSTITUTION: &str = "$(";
const QUOTES: [char; 2] = ['\'', '"'];
const DIALOG_PAGE: usize = 10;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Folders {
    hotlist: Vec<PathBuf>,
    recent: Vec<PathBuf>,
    home: Option<PathBuf>,
    /// The folder each panel last arrived in, so a panel listed again is no new visit.
    arrived: [Option<PathBuf>; 2],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderChoice {
    pub path: PathBuf,
    /// On the hotlist, which comes first.
    pub pinned: bool,
}

impl Folders {
    pub fn new(hotlist: Vec<PathBuf>, recent: Vec<PathBuf>, home: Option<PathBuf>) -> Self {
        let mut folders = Self { home, ..Self::default() };
        for dir in hotlist {
            folders.pin(&dir);
        }
        for dir in recent.into_iter().rev() {
            folders.visit(&dir);
        }
        folders
    }

    pub fn hotlist(&self) -> &[PathBuf] {
        &self.hotlist
    }

    pub fn recent(&self) -> &[PathBuf] {
        &self.recent
    }

    /// `dir` as the lists show it, with the home folder as `~`.
    pub fn display(&self, dir: &Path) -> String {
        match self.home.as_deref().and_then(|home| dir.strip_prefix(home).ok()) {
            Some(rest) if rest.as_os_str().is_empty() => HOME_PREFIX.to_owned(),
            Some(rest) => format!("{HOME_PREFIX}/{}", rest.display()),
            None => dir.display().to_string(),
        }
    }

    /// Puts `dir` first among the recent folders; false when it already was.
    pub(crate) fn visit(&mut self, dir: &Path) -> bool {
        if self.recent.first().is_some_and(|first| first == dir) {
            return false;
        }
        self.recent.retain(|recent| recent != dir);
        self.recent.insert(0, dir.to_path_buf());
        self.recent.truncate(RECENT_LIMIT);
        true
    }

    /// A panel arrived in `dir`: a visit, unless the panel was already there.
    pub(crate) fn arrive(&mut self, side: Side, dir: &Path) -> bool {
        let arrived = &mut self.arrived[side.index()];
        if arrived.as_deref() == Some(dir) {
            return false;
        }
        *arrived = Some(dir.to_path_buf());
        self.visit(dir)
    }

    /// Adds `dir` to the end of the hotlist; false when it is on it or the hotlist is full.
    pub(crate) fn pin(&mut self, dir: &Path) -> bool {
        if self.hotlist.iter().any(|pinned| pinned == dir) || self.hotlist.len() >= HOTLIST_LIMIT {
            return false;
        }
        self.hotlist.push(dir.to_path_buf());
        true
    }

    /// Takes `choice` off the hotlist, or out of the recent folders.
    pub(crate) fn remove(&mut self, choice: &FolderChoice) {
        let list = if choice.pinned { &mut self.hotlist } else { &mut self.recent };
        list.retain(|dir| *dir != choice.path);
    }

    /// The hotlist, then the recent folders not on it, that hold every word of `query`, with
    /// `skip` left out. Folders named like the last word come first: the same name, then a name
    /// that starts with it, then one that holds it.
    pub fn choices(&self, query: &str, skip: Option<&Path>) -> Vec<FolderChoice> {
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        let matches = |dir: &Path| {
            let shown = self.display(dir).to_lowercase();
            let full = dir.to_string_lossy().to_lowercase();
            words.iter().all(|word| shown.contains(word) || full.contains(word))
        };
        let pinned = self.hotlist.iter().map(|path| (path, true));
        let recent = self.recent.iter().filter(|dir| !self.hotlist.contains(dir)).map(|path| (path, false));
        let mut choices: Vec<FolderChoice> = pinned
            .chain(recent)
            .filter(|(dir, _)| Some(dir.as_path()) != skip && matches(dir))
            .map(|(path, pinned)| FolderChoice { path: path.clone(), pinned })
            .collect();
        if let Some(last) = words.last() {
            choices.sort_by_key(|choice| name_rank(&choice.path, last));
        }
        choices
    }
}

fn name_rank(dir: &Path, word: &str) -> u8 {
    let name = dir.file_name().map(|name| name.to_string_lossy().to_lowercase()).unwrap_or_default();
    let word = word.rsplit('/').next().unwrap_or(word);
    if name == word {
        0
    } else if name.starts_with(word) {
        1
    } else if name.contains(word) {
        2
    } else {
        3
    }
}

/// What follows `cd ` on a command line that only changes folder, without quotes, or `None`
/// for any other line.
pub fn cd_argument(line: &str) -> Option<&str> {
    let rest = line.strip_prefix(CD)?.strip_prefix(char::is_whitespace)?;
    plain(line).then(|| rest.trim_start().trim_start_matches(QUOTES).trim_end_matches(QUOTES))
}

/// Whether `line` only changes folder, so it can run while the panels stay in view.
pub fn plain_cd(line: &str) -> bool {
    let line = line.trim();
    plain(line) && (line == CD || cd_argument(line).is_some())
}

fn plain(line: &str) -> bool {
    !line.contains(COMPOUND) && !line.contains(SUBSTITUTION)
}

/// The picker above the command line: which folder is chosen, and whether Esc hid it for the
/// `cd ` being typed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CdPicker {
    /// Counted from the row nearest the command line.
    pub selected: Option<usize>,
    pub hidden: bool,
}

/// Whether the active panel shows a host folder in the panels, at the host's command line,
/// while nothing else has the keys.
fn at_host_command_line(state: &AppState) -> bool {
    let panel = state.active_panel();
    state.view == View::Panels
        && state.overlay.is_none()
        && state.activity.side.is_none()
        && !state.docker.is_open()
        && state.viewer.is_none()
        && state.editor.is_none()
        && !state.preview_focused
        && crate::editor::focused(state).is_none()
        && state.console == Console::Host
        && !state.shell.busy()
        && panel.mount.is_none()
        && panel.remote_mount.is_none()
        && !panel.is_results()
        && !panel.is_size_view()
}

/// The folders the picker shows for the `cd ` being typed, nearest the command line first.
pub fn cd_choices(state: &AppState) -> Option<Vec<FolderChoice>> {
    if !at_host_command_line(state) || state.cd_picker.hidden {
        return None;
    }
    let query = cd_argument(&state.shell.observed.line)?;
    let mut choices = state.folders.choices(query, Some(state.active_panel().dir()));
    choices.truncate(PICKER_ROWS);
    (!choices.is_empty()).then_some(choices)
}

/// A new command line lets go of the picker's choice, and once it is no `cd ` the picker shows
/// again for the next one.
pub(crate) fn line_changed(state: &mut AppState) {
    state.cd_picker.selected = None;
    if cd_argument(&state.shell.observed.line).is_none() {
        state.cd_picker.hidden = false;
    }
}

/// Up and Down move through the picker, Enter opens the chosen folder, and Esc hides the
/// picker for this `cd `. Other keys go to the command line.
pub(crate) fn picker_key(state: &mut AppState, key: &KeyEvent) -> Option<Vec<Effect>> {
    if !key.modifiers.difference(KeyModifiers::SHIFT).is_empty() {
        return None;
    }
    let choices = cd_choices(state)?;
    let last = choices.len() - 1;
    let picker = &mut state.cd_picker;
    match (key.code, picker.selected) {
        (KeyCode::Up, None) => picker.selected = Some(0),
        (KeyCode::Up, Some(row)) => picker.selected = Some((row + 1).min(last)),
        (KeyCode::Down, Some(0)) => picker.selected = None,
        (KeyCode::Down, Some(row)) => picker.selected = Some(row - 1),
        (KeyCode::Esc, _) => *picker = CdPicker { selected: None, hidden: true },
        (KeyCode::Enter, Some(row)) => return Some(take(state, choices.get(row)?.path.clone())),
        _ => return None,
    }
    Some(Vec::new())
}

/// A click on a row of the picker opens its folder, as choosing it and Enter do.
pub(crate) fn pick(state: &mut AppState, row: usize) -> Vec<Effect> {
    match cd_choices(state).and_then(|choices| choices.into_iter().nth(row)) {
        Some(choice) => take(state, choice.path),
        None => Vec::new(),
    }
}

/// Clears the typed `cd ` and opens `dir` in the active panel; the shell follows the panel as
/// it does after any move.
fn take(state: &mut AppState, dir: PathBuf) -> Vec<Effect> {
    state.cd_picker = CdPicker::default();
    state.shell.clear_line();
    state.shell.observed.line.clear();
    let mut effects = vec![Effect::Write(CLEAR_LINE.to_vec())];
    effects.extend(open(state, dir));
    effects
}

/// The folders dialog: what is typed to narrow it and which folder is chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoldersDialog {
    pub filter: LineInput,
    pub selected: usize,
}

/// The hotlist and the recent folders that match the dialog's filter. The active panel's own
/// folder is listed too, so Insert can pin it.
pub fn dialog_choices(state: &AppState, filter: &str) -> Vec<FolderChoice> {
    let here = state.active_panel().dir();
    if !host_panel(state, state.active) || state.folders.recent.iter().any(|dir| dir == here) {
        return state.folders.choices(filter, None);
    }
    let mut folders = state.folders.clone();
    folders.recent.insert(0, here.to_path_buf());
    folders.choices(filter, None)
}

fn host_panel(state: &AppState, side: Side) -> bool {
    let panel = state.panel(side);
    panel.mount.is_none() && panel.remote_mount.is_none() && !panel.is_results()
}

/// Ctrl-\ opens the dialog on the active panel's folder.
pub(crate) fn open_dialog(state: &mut AppState) -> Vec<Effect> {
    let here = state.active_panel().dir().to_path_buf();
    let selected = dialog_choices(state, "").iter().position(|choice| choice.path == here).unwrap_or_default();
    state.overlay = Some(Overlay::Folders(FoldersDialog { filter: LineInput::default(), selected }));
    Vec::new()
}

/// Enter opens the chosen folder, Insert pins it, Delete takes it off the list, and typing
/// narrows the list. Esc clears what was typed, then closes.
pub(crate) fn dialog_key(state: &mut AppState, mut dialog: FoldersDialog, key: &KeyEvent) -> Vec<Effect> {
    let choices = dialog_choices(state, dialog.filter.text());
    let last = choices.len().saturating_sub(1);
    let chosen = choices.get(dialog.selected).cloned();
    let mut effects = Vec::new();
    match key.code {
        KeyCode::Esc if !dialog.filter.text().is_empty() => {
            dialog = FoldersDialog { filter: LineInput::default(), selected: 0 }
        }
        KeyCode::Esc | KeyCode::F(10) => {
            state.overlay = None;
            return Vec::new();
        }
        KeyCode::Up => dialog.selected = dialog.selected.saturating_sub(1),
        KeyCode::Down => dialog.selected = (dialog.selected + 1).min(last),
        KeyCode::PageUp => dialog.selected = dialog.selected.saturating_sub(DIALOG_PAGE),
        KeyCode::PageDown => dialog.selected = (dialog.selected + DIALOG_PAGE).min(last),
        KeyCode::Home if dialog.filter.text().is_empty() => dialog.selected = 0,
        KeyCode::End if dialog.filter.text().is_empty() => dialog.selected = last,
        KeyCode::Enter => {
            let Some(choice) = chosen else { return keep(state, dialog) };
            state.overlay = None;
            return open(state, choice.path);
        }
        KeyCode::Insert => match chosen.filter(|choice| !choice.pinned) {
            Some(choice) if state.folders.pin(&choice.path) => {
                effects.push(Effect::SetHotlist(state.folders.hotlist().to_vec()));
                dialog.selected = state.folders.hotlist().len() - 1;
                dialog.filter = LineInput::default();
            }
            Some(_) => state.notice = Some(tr!(folders_hotlist_full, count = HOTLIST_LIMIT)),
            None => {}
        },
        KeyCode::Delete | KeyCode::F(8) => {
            if let Some(choice) = chosen {
                state.folders.remove(&choice);
                effects.push(if choice.pinned {
                    Effect::SetHotlist(state.folders.hotlist().to_vec())
                } else {
                    Effect::SaveFolders
                });
                let left = dialog_choices(state, dialog.filter.text()).len();
                dialog.selected = dialog.selected.min(left.saturating_sub(1));
            }
        }
        _ => {
            let before = dialog.filter.text().to_owned();
            dialog.filter.apply(key);
            if dialog.filter.text() != before {
                dialog.selected = 0;
            }
        }
    }
    effects.extend(keep(state, dialog));
    effects
}

fn keep(state: &mut AppState, dialog: FoldersDialog) -> Vec<Effect> {
    state.overlay = Some(Overlay::Folders(dialog));
    Vec::new()
}

/// Opens `dir` in the active panel, leaving a container's or a remote machine's files first.
fn open(state: &mut AppState, dir: PathBuf) -> Vec<Effect> {
    let side = state.active;
    crate::remote::unmount(state, side);
    state.panel_mut(side).mount = None;
    vec![state.navigate(side, dir, None)]
}

/// A panel's folder listed in full: a visit worth remembering, while folder history is on.
pub(crate) fn listed(state: &mut AppState, side: Side) -> Vec<Effect> {
    if !state.settings.folder_history || !host_panel(state, side) {
        return Vec::new();
    }
    let dir = state.panel(side).dir().to_path_buf();
    if state.folders.arrive(side, &dir) { vec![Effect::SaveFolders] } else { Vec::new() }
}

#[cfg(test)]
mod tests;
