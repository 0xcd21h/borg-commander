//! Borg's own files: where it keeps them, what each place holds and how long it lasts, which
//! of them no running Borg uses any more, and the window that lists them. The app measures
//! the places and removes leftovers; core decides what is left over and what the window does.

mod leftovers;

use borg_i18n::{Count, count, tr};
use std::{path::PathBuf, time::SystemTime};

use crossterm::event::{KeyCode, KeyEvent};

use crate::{AppState, Confirm, ConfirmPurpose, Effect, Overlay};

pub use leftovers::{
    CONSOLE_PREFIX, CacheLists, DOCKER_SCRATCH, Found, FoundKind, INDEX_DIR, INDEX_SUFFIX, Judge, Leftover,
    REMOTE_PREVIEWS, SCAN_PREFIX, SCRATCH_AGE, SETTLE, SIZES_INDEX, SIZES_PREFIX, SOCKET_SUFFIX, SQLITE_SUFFIX,
    STAGING_SUFFIX, STALE, TRUST_EXTENSION, TRUST_PREFIX, TTL_FILE, abandoned_leftover, cache_leftovers,
    runtime_leftovers, socket_leftovers,
};

/// Captures Borg remembers having saved, newest first.
pub const CAPTURE_RECORDS: usize = 1_000;
const PAGE: usize = 10;
/// Folders the cleanup names one by one; what the others hold is summed up.
const NAMED_FOLDERS: usize = 5;

/// A kind of file Borg keeps, in the order the window lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Place {
    Temporary,
    Sockets,
    Caches,
    State,
    Settings,
    Captures,
}

impl Place {
    pub const ALL: [Self; 6] =
        [Self::Temporary, Self::Sockets, Self::Caches, Self::State, Self::Settings, Self::Captures];

    pub fn label(self) -> &'static str {
        match self {
            Self::Temporary => tr!(storage_temporary),
            Self::Sockets => tr!(storage_sockets),
            Self::Caches => tr!(storage_caches),
            Self::State => tr!(storage_state),
            Self::Settings => tr!(storage_settings),
            Self::Captures => tr!(storage_captures),
        }
    }

    /// What the place holds and how long it lasts.
    pub fn about(self) -> &'static str {
        match self {
            Self::Temporary => tr!(storage_temporary_about),
            Self::Sockets => tr!(storage_sockets_about),
            Self::Caches => tr!(storage_caches_about),
            Self::State => tr!(storage_state_about),
            Self::Settings => tr!(storage_settings_about),
            Self::Captures => tr!(storage_captures_about),
        }
    }
}

/// One place, measured.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaceUsage {
    pub place: Place,
    /// The folder Borg uses for it now. Captures have none: they stay where they were saved.
    pub path: Option<PathBuf>,
    pub bytes: u64,
    /// Files and folders in it; for captures, how many there are.
    pub items: usize,
}

/// A capture Borg saved that is still there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedCapture {
    pub path: PathBuf,
    /// Its bytes, with the key log beside it.
    pub bytes: u64,
    pub saved: SystemTime,
    /// The TLS key log saved with it.
    pub keys: Option<PathBuf>,
}

/// What a cleanup removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cleanup {
    pub at: SystemTime,
    pub removed: usize,
    pub bytes: u64,
    /// Leftovers the system refused to remove.
    pub failed: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StorageReport {
    pub places: Vec<PlaceUsage>,
    pub captures: Vec<SavedCapture>,
    pub leftovers: Vec<Leftover>,
    /// The last cleanup this Borg did, on its own or when asked.
    pub cleaned: Option<Cleanup>,
}

impl StorageReport {
    pub fn leftover_bytes(&self) -> u64 {
        self.leftovers.iter().map(|leftover| leftover.bytes).sum()
    }
}

/// Where the leftovers are: how many each folder holds, most first, then how many the folders
/// past the first few hold together.
pub fn leftover_lines(leftovers: &[Leftover], display: impl Fn(&std::path::Path) -> String) -> Vec<String> {
    let mut folders: std::collections::HashMap<&std::path::Path, usize> = std::collections::HashMap::new();
    for leftover in leftovers {
        *folders.entry(leftover.path.parent().unwrap_or(&leftover.path)).or_default() += 1;
    }
    let mut folders: Vec<(&std::path::Path, usize)> = folders.into_iter().collect();
    folders.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    let rest: usize = folders.iter().skip(NAMED_FOLDERS).map(|(_, items)| items).sum();
    folders
        .iter()
        .take(NAMED_FOLDERS)
        .map(|(folder, items)| tr!(storage_clean_folder, items = count(*items, Count::Item), folder = display(folder)))
        .chain((rest > 0).then(|| tr!(storage_clean_elsewhere, items = count(rest, Count::Item))))
        .collect()
}

/// The Borg's files window: what the app found, `None` while it looks, and the chosen row.
/// The places come first, then the captures.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StorageWindow {
    pub report: Option<StorageReport>,
    pub selected: usize,
}

/// What a row of the window shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageRow<'a> {
    Place(&'a PlaceUsage),
    Capture(&'a SavedCapture),
}

impl StorageWindow {
    pub fn rows(&self) -> Vec<StorageRow<'_>> {
        self.report.as_ref().map_or_else(Vec::new, |report| {
            report.places.iter().map(StorageRow::Place).chain(report.captures.iter().map(StorageRow::Capture)).collect()
        })
    }

    pub fn chosen(&self) -> Option<StorageRow<'_>> {
        self.rows().get(self.selected).copied()
    }
}

/// F9 → Options → Borg's files: opens the window and asks the app to measure.
pub(crate) fn open(state: &mut AppState) -> Vec<Effect> {
    state.overlay = Some(Overlay::Storage(StorageWindow::default()));
    vec![Effect::ScanStorage]
}

/// The app measured: the open window shows it.
pub(crate) fn receive(state: &mut AppState, report: StorageReport) -> Vec<Effect> {
    if let Some(Overlay::Storage(window)) = &mut state.overlay {
        let rows = report.places.len() + report.captures.len();
        window.selected = window.selected.min(rows.saturating_sub(1));
        window.report = Some(report);
    }
    Vec::new()
}

/// Enter opens the chosen place's folder, or the folder of the chosen capture with the
/// capture under the cursor. F8 asks to remove the leftovers. Esc closes.
pub(crate) fn on_key(state: &mut AppState, mut window: StorageWindow, key: &KeyEvent) -> Vec<Effect> {
    let last = window.rows().len().saturating_sub(1);
    match key.code {
        KeyCode::Esc | KeyCode::F(10) => {
            state.overlay = None;
            return Vec::new();
        }
        KeyCode::Up => window.selected = window.selected.saturating_sub(1),
        KeyCode::Down => window.selected = (window.selected + 1).min(last),
        KeyCode::PageUp => window.selected = window.selected.saturating_sub(PAGE),
        KeyCode::PageDown => window.selected = (window.selected + PAGE).min(last),
        KeyCode::Home => window.selected = 0,
        KeyCode::End => window.selected = last,
        KeyCode::Enter => return enter(state, window),
        KeyCode::F(8) => return ask_to_clean(state, window),
        _ => {}
    }
    state.overlay = Some(Overlay::Storage(window));
    Vec::new()
}

fn enter(state: &mut AppState, mut window: StorageWindow) -> Vec<Effect> {
    let target = match window.chosen() {
        Some(StorageRow::Place(PlaceUsage { path: Some(path), .. })) => Some((path.clone(), None)),
        Some(StorageRow::Capture(capture)) => capture
            .path
            .parent()
            .map(|dir| (dir.to_path_buf(), capture.path.file_name().map(|name| name.to_string_lossy().into_owned()))),
        Some(StorageRow::Place(PlaceUsage { place: Place::Captures, items, .. })) if *items > 0 => {
            window.selected += 1;
            None
        }
        Some(StorageRow::Place(_)) | None => None,
    };
    match target {
        Some((dir, focus)) => {
            state.overlay = None;
            crate::folders::open(state, state.active, dir, focus)
        }
        None => {
            state.overlay = Some(Overlay::Storage(window));
            Vec::new()
        }
    }
}

fn ask_to_clean(state: &mut AppState, window: StorageWindow) -> Vec<Effect> {
    let leftovers = window.report.as_ref().map_or(&[][..], |report| &report.leftovers[..]);
    let paths: Vec<PathBuf> = leftovers.iter().map(|leftover| leftover.path.clone()).collect();
    let lines = leftover_lines(leftovers, |folder| state.folders.display(folder));
    if paths.is_empty() {
        state.notice = Some(tr!(storage_no_leftovers).into());
        state.overlay = Some(Overlay::Storage(window));
        return Vec::new();
    }
    state.overlay = Some(Overlay::Confirm(Confirm {
        title: tr!(storage_clean_title),
        message: format!(
            "{}\n\n{}",
            tr!(storage_clean_question, items = count(paths.len(), Count::Item)),
            lines.join("\n")
        ),
        purpose: ConfirmPurpose::CleanStorage { paths, window: Box::new(window) },
        accept: true,
    }));
    Vec::new()
}

/// The answer to the cleanup question: Yes removes the leftovers and measures again.
pub(crate) fn answer(state: &mut AppState, yes: bool, paths: Vec<PathBuf>, window: StorageWindow) -> Vec<Effect> {
    if !yes {
        state.overlay = Some(Overlay::Storage(window));
        return Vec::new();
    }
    state.overlay = Some(Overlay::Storage(StorageWindow { report: None, ..window }));
    vec![Effect::CleanStorage(paths)]
}

#[cfg(test)]
mod tests;
