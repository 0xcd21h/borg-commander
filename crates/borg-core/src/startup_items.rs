//! The startup items screen: every place a program is set to start from on its own, newest
//! change first, in one panel, and the selected item's details in the other. Core reads what
//! the files say; the app reads the files, their owners and times, and runs the rules.

use borg_i18n::tr;
use borg_rules::{Level, Problem, RuleMatch};
use std::{
    path::{Path, PathBuf},
    time::{Instant, SystemTime},
};

use crate::{AppState, Effect, Side, View};

mod keys;
mod parse;
mod places;
mod plist;

pub(crate) use keys::{click, paste, route_key};
pub use parse::{COMMAND_SEPARATOR, StartupEntry, parse, program_of, shell_words};
pub use places::{StartupFormat, StartupPlace, StartupSpot, StartupSystem, UNIT_LINK_FOLDERS, spots};

const FILTER_LIMIT: usize = 128;
const PAGE: usize = 10;
/// Rows the list pane's frame, summary, filter, column titles and footer take.
pub const STARTUP_CHROME_ROWS: u16 = 6;
const EXCLUDE: char = '!';

/// Whether an item's program is on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgramPresence {
    Present,
    Missing,
    /// The item names no program, or one only a variable or a relative path gives.
    Unknown,
}

impl ProgramPresence {
    pub fn label(self) -> &'static str {
        match self {
            Self::Present => tr!(startup_program_found),
            Self::Missing => tr!(startup_program_missing),
            Self::Unknown => "",
        }
    }
}

/// A rule an item matched, in the item's own file or in the program it runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleHit {
    pub rule: RuleMatch,
    pub in_program: bool,
}

/// Something set to start on its own, with what the app found out about its file and program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupItem {
    pub place: StartupPlace,
    pub file: PathBuf,
    pub found: StartupEntry,
    /// Who owns the file.
    pub owner: Option<String>,
    pub modified: Option<SystemTime>,
    pub presence: ProgramPresence,
    /// Where a program named without a folder was found.
    pub resolved: Option<PathBuf>,
    pub hits: Vec<RuleHit>,
    pub read_only: bool,
}

impl StartupItem {
    pub fn new(spot: &StartupSpot, file: PathBuf, found: StartupEntry) -> Self {
        Self {
            place: spot.place_of(&file),
            file,
            found,
            owner: None,
            modified: None,
            presence: ProgramPresence::Unknown,
            resolved: None,
            hits: Vec::new(),
            read_only: spot.read_only,
        }
    }

    /// The worst level of the rules it matched.
    pub fn worst(&self) -> Option<Level> {
        self.hits.iter().map(|hit| hit.rule.level).min()
    }

    /// The names of the rules it matched, each once, worst first.
    pub fn rule_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = Vec::new();
        for hit in &self.hits {
            if !names.contains(&hit.rule.rule.as_str()) {
                names.push(&hit.rule.rule);
            }
        }
        names
    }

    fn key(&self) -> ItemKey {
        ItemKey { file: self.file.clone(), line: self.found.line, name: self.found.name.clone() }
    }

    /// Everything the filter looks through, in lower case.
    fn haystack(&self) -> String {
        let rules = self.hits.iter().map(|hit| hit.rule.rule.as_str());
        let words = [
            self.place.label(),
            self.place.code(),
            &self.found.name,
            &self.found.runs,
            &self.file.to_string_lossy(),
            self.owner.as_deref().unwrap_or_default(),
            self.found.user.as_deref().unwrap_or_default(),
            self.found.when.as_deref().unwrap_or_default(),
            self.presence.label(),
        ];
        words.into_iter().chain(rules).collect::<Vec<_>>().join("\n").to_lowercase()
    }
}

/// Newest change first; items whose time is unknown last; then by place, file and line.
pub fn newest_first(items: &mut [StartupItem]) {
    items.sort_by(|a, b| {
        b.modified
            .cmp(&a.modified)
            .then(a.place.cmp(&b.place))
            .then_with(|| a.file.cmp(&b.file))
            .then(a.found.line.cmp(&b.found.line))
    });
}

/// The items `filter` keeps: each word must appear, and none of those after `!`.
pub fn filtered(items: &[StartupItem], filter: &str) -> Vec<usize> {
    let words: Vec<String> = filter.split_whitespace().map(str::to_lowercase).collect();
    items
        .iter()
        .enumerate()
        .filter(|(_, item)| {
            let haystack = item.haystack();
            words.iter().all(|word| match word.strip_prefix(EXCLUDE) {
                Some(excluded) => excluded.is_empty() || !haystack.contains(excluded),
                None => haystack.contains(word.as_str()),
            })
        })
        .map(|(index, _)| index)
        .collect()
}

/// What the screen lists: the items, newest change first, places that are there but could not
/// be read, and rule files left out.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StartupReport {
    pub items: Vec<StartupItem>,
    pub unread: Vec<(PathBuf, String)>,
    pub problems: Vec<Problem>,
}

impl StartupReport {
    pub fn matched(&self) -> usize {
        self.items.iter().filter(|item| !item.hits.is_empty()).count()
    }
}

/// What the cursor is on, kept while the list is read again.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ItemKey {
    file: PathBuf,
    line: Option<usize>,
    name: String,
}

#[derive(Debug, Clone, Default)]
pub struct StartupItems {
    pub side: Option<Side>,
    pub expanded: bool,
    pub report: StartupReport,
    pub loaded: bool,
    /// The items the filter keeps, by their place in the report.
    pub rows: Vec<usize>,
    cursor: usize,
    top: usize,
    chosen: Option<ItemKey>,
    pub filter: String,
    pub filtering: bool,
    pub error: Option<String>,
    pub pending: Option<u64>,
    next_id: u64,
}

impl StartupItems {
    pub fn is_open(&self) -> bool {
        self.side.is_some()
    }

    pub fn typing(&self) -> bool {
        self.filtering
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn item(&self, row: usize) -> Option<&StartupItem> {
        self.rows.get(row).and_then(|index| self.report.items.get(*index))
    }

    pub fn selected(&self) -> Option<&StartupItem> {
        self.item(self.cursor)
    }

    pub fn list_start(&self, height: usize) -> usize {
        self.top.min(self.cursor).max(self.cursor.saturating_sub(height.max(1) - 1))
    }

    fn keep_visible(&mut self, height: usize) {
        self.top = self.list_start(height);
    }

    pub(crate) fn select(&mut self, row: usize) {
        let row = row.min(self.len().saturating_sub(1));
        self.cursor = row;
        self.chosen = self.item(row).map(StartupItem::key);
    }

    fn move_by(&mut self, delta: isize) {
        let last = self.len().saturating_sub(1) as isize;
        self.select((self.cursor as isize + delta).clamp(0, last.max(0)) as usize);
    }

    /// Filters the items again, keeping the cursor on the item it was on.
    fn rebuild(&mut self) {
        self.rows = filtered(&self.report.items, &self.filter);
        let chosen = self.chosen.clone();
        let found = chosen
            .and_then(|key| (0..self.len()).find(|&row| self.item(row).map(StartupItem::key) == Some(key.clone())));
        self.cursor = found.unwrap_or(self.cursor.min(self.len().saturating_sub(1)));
        self.chosen = self.item(self.cursor).map(StartupItem::key);
    }

    fn request(&mut self) -> Vec<Effect> {
        self.next_id += 1;
        self.pending = Some(self.next_id);
        vec![Effect::StartupItems { id: self.next_id }]
    }
}

pub(crate) fn open(state: &mut AppState) -> Vec<Effect> {
    if !state.settings.preferences.system_plugin {
        state.notice = Some(tr!(activity_disabled).into());
        return Vec::new();
    }
    state.overlay = None;
    state.activity.side = None;
    crate::docker::close(state);
    crate::network::close(state);
    crate::watch::close(state);
    let startup = &mut state.startup_items;
    startup.side = Some(state.active);
    startup.error = None;
    startup.request()
}

pub(crate) fn close(state: &mut AppState) {
    state.startup_items.side = None;
    state.startup_items.filtering = false;
}

/// Alt-U opens the screen, and closes it while it is open.
pub(crate) fn toggle(state: &mut AppState) -> Vec<Effect> {
    if state.startup_items.is_open() {
        close(state);
        return Vec::new();
    }
    open(state)
}

/// Reads the places again.
fn reread(state: &mut AppState) -> Vec<Effect> {
    state.startup_items.error = None;
    state.startup_items.request()
}

/// Keeps the cursor in sight of the list pane.
pub(crate) fn sync(state: &mut AppState, now: Instant) {
    if !state.startup_items.is_open() || state.view != View::Panels || state.overlay.is_some() {
        return;
    }
    let side = state.startup_items.side.unwrap_or(state.active);
    let list = state.layout(now).panels[side.index()];
    if !list.is_empty() {
        state.startup_items.keep_visible(usize::from(list.height.saturating_sub(STARTUP_CHROME_ROWS)));
    }
}

pub(crate) fn receive(state: &mut AppState, id: u64, result: Result<StartupReport, String>) {
    let startup = &mut state.startup_items;
    if startup.pending != Some(id) {
        return;
    }
    startup.pending = None;
    match result {
        Ok(report) => {
            startup.report = report;
            startup.loaded = true;
            startup.error = None;
            startup.rebuild();
        }
        Err(error) => startup.error = Some(error),
    }
}

/// The folder and name of an item's file, to show it in a panel.
fn place_in_folder(file: &Path) -> Option<(PathBuf, String)> {
    Some((file.parent()?.to_path_buf(), file.file_name()?.to_string_lossy().into_owned()))
}

#[cfg(test)]
mod tests;
