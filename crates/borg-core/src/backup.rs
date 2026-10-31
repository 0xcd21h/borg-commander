//! Time Machine exclusions on macOS: which entries of each panel's folder backups leave out,
//! and the Time Machine window that lists and changes them. The app asks `tmutil` and the
//! system; core keeps what it reported and decides what to ask for next.

use borg_i18n::{Count, count, tr};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{AppState, Effect, LineInput, Overlay, Prompt, PromptPurpose, Side};

const MAX_HEIGHT: u16 = 30;
const MIN_HEIGHT: u16 = 12;
/// Rows of the window around its list: the borders, the tabs, the summary and a blank line
/// above it, and a blank line, the explanation and the outcome below it.
const CHROME_ROWS: u16 = 2 + 3 + 4;

/// Whether Time Machine backs an item up, and if not, why.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Exclusion {
    #[default]
    Included,
    /// Marked on the item itself: the mark moves and copies with it.
    Sticky,
    /// The path is in Time Machine's list of excluded paths, whatever is there.
    Path,
    /// A whole volume Time Machine leaves out.
    Volume,
    /// Inside this excluded folder.
    Inside(PathBuf),
    /// macOS leaves it out by itself.
    System,
    /// The system would not say, as for folders that need Full Disk Access.
    Unknown,
}

impl Exclusion {
    pub fn excluded(&self) -> bool {
        !matches!(self, Self::Included | Self::Unknown)
    }

    /// Whether the exclusion is the item's own, so including the item again removes it.
    pub fn own(&self) -> bool {
        matches!(self, Self::Sticky | Self::Path | Self::Volume)
    }

    pub fn needs_password(&self) -> bool {
        matches!(self, Self::Path | Self::Volume)
    }
}

/// What a panel's folder holds that backups leave out.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BackupListing {
    /// Why the folder itself is left out, if it is.
    pub folder: Exclusion,
    /// Entries excluded on their own, by name.
    pub entries: Vec<(String, Exclusion)>,
}

#[derive(Debug, Clone, Default)]
pub struct BackupPanel {
    pub folder: Exclusion,
    pub entries: HashMap<String, Exclusion>,
    key: Option<(u64, PathBuf)>,
}

impl BackupPanel {
    pub fn entry(&self, name: &str) -> Option<&Exclusion> {
        self.entries.get(name)
    }

    /// Whether rows need room for a mark: some entry is excluded on its own.
    pub fn marks(&self) -> bool {
        !self.entries.is_empty()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BackupTab {
    #[default]
    Selection,
    Caches,
    All,
}

impl BackupTab {
    pub const ALL: [Self; 3] = [Self::Selection, Self::Caches, Self::All];

    pub fn index(self) -> usize {
        match self {
            Self::Selection => 0,
            Self::Caches => 1,
            Self::All => 2,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BackupItem {
    pub path: PathBuf,
    pub dir: bool,
    /// The root of a volume, which only a volume exclusion covers.
    pub volume: bool,
    /// Nothing is at the path: a fixed path waiting for something to appear there.
    pub missing: bool,
    pub size: Option<u64>,
    pub exclusion: Exclusion,
    /// The kind All exclusions lists it under, kept when it is included again.
    pub group: Exclusion,
}

#[derive(Debug, Clone, Default)]
pub struct BackupList {
    pub items: Vec<BackupItem>,
    pub cursor: usize,
    pub marks: HashSet<PathBuf>,
    pub loading: bool,
    pub started: bool,
    /// Folders looked through so far.
    pub folders: u64,
    pub error: Option<String>,
    /// The first row shown, headings included.
    pub top: usize,
    id: u64,
}

impl BackupList {
    pub fn hovered(&self) -> Option<&BackupItem> {
        self.items.get(self.cursor)
    }

    /// The marked items, or the one under the cursor when none is marked.
    pub fn targets(&self) -> Vec<&BackupItem> {
        if self.marks.is_empty() {
            self.hovered().into_iter().collect()
        } else {
            self.items.iter().filter(|item| self.marks.contains(&item.path)).collect()
        }
    }

    fn step(&mut self, delta: isize) {
        let last = self.items.len().saturating_sub(1);
        self.cursor = self.cursor.saturating_add_signed(delta).min(last);
    }

    fn settle(&mut self) {
        self.cursor = self.cursor.min(self.items.len().saturating_sub(1));
    }

    /// Marks the item at `index`, or unmarks it.
    fn toggle(&mut self, index: usize) {
        if let Some(path) = self.items.get(index).map(|item| item.path.clone())
            && !self.marks.remove(&path)
        {
            self.marks.insert(path);
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct BackupWindow {
    pub tab: BackupTab,
    /// The active panel's folder when the window opened, where caches and exclusions are looked for.
    pub root: PathBuf,
    pub lists: [BackupList; 3],
    /// A change on its way, and whether it waits for the user's password.
    pub working: Option<(u64, bool)>,
    /// What the last change did, or why it did nothing.
    pub outcome: Option<String>,
}

impl BackupWindow {
    pub fn list(&self) -> &BackupList {
        &self.lists[self.tab.index()]
    }

    fn list_mut(&mut self) -> &mut BackupList {
        &mut self.lists[self.tab.index()]
    }

    pub fn busy(&self) -> bool {
        self.working.is_some() || self.lists.iter().any(|list| list.loading)
    }

    /// The kind All exclusions names above the item at `index`, where a new kind starts.
    pub fn heading(&self, index: usize) -> Option<&Exclusion> {
        let items = &self.list().items;
        let group = &items.get(index)?.group;
        let starts = index == 0 || items[index - 1].group != *group;
        let named = matches!(group, Exclusion::Sticky | Exclusion::Path | Exclusion::Volume | Exclusion::System);
        (self.tab == BackupTab::All && starts && named).then_some(group)
    }

    /// The row of the item at `index`, counting the headings above it.
    fn row_of(&self, index: usize) -> usize {
        index + (0..=index).filter(|&above| self.heading(above).is_some()).count()
    }

    /// The first row to show in `height` rows: where the list was, moved only as far as it takes
    /// to show the cursor, with the heading of its kind when it is the first.
    pub fn list_start(&self, height: usize) -> usize {
        let list = self.list();
        let at = self.row_of(list.cursor);
        let first = at - usize::from(self.heading(list.cursor).is_some());
        list.top.min(first).max(at.saturating_sub(height.max(1) - 1))
    }

    /// Puts the cursor on the item at `index`, and marks or unmarks it when `mark`, as a click
    /// and a right-click do.
    pub(crate) fn point(&mut self, index: usize, mark: bool) {
        let list = self.list_mut();
        list.cursor = index;
        list.settle();
        if mark {
            list.toggle(list.cursor);
        }
    }

    fn keep_visible(&mut self, height: usize) {
        let top = self.list_start(height);
        self.list_mut().top = top;
    }
}

/// The height of the Time Machine window on a screen this tall.
pub fn window_height(screen: u16) -> u16 {
    screen.saturating_sub(2).clamp(MIN_HEIGHT, MAX_HEIGHT).min(screen)
}

/// The rows the window's list has on a screen this tall.
pub fn list_rows(screen: u16) -> usize {
    usize::from(window_height(screen).saturating_sub(CHROME_ROWS))
}

#[derive(Debug, Clone, Default)]
pub struct Backup {
    /// Whether Time Machine is there to ask, which only macOS has.
    pub available: bool,
    pub window: BackupWindow,
    next: u64,
}

impl Backup {
    fn next_id(&mut self) -> u64 {
        self.next += 1;
        self.next
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupChange {
    /// Marks the items themselves, so the exclusion moves with them.
    Exclude,
    /// Adds the items' paths to Time Machine's list, which asks for the user's password.
    ExcludePath,
    /// Removes whatever exclusion each item has of its own.
    Include,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackupRequest {
    /// Which entries of a panel's folder backups leave out.
    Listing {
        side: Side,
        generation: u64,
        dir: PathBuf,
    },
    /// How backups treat each of these paths.
    Inspect {
        id: u64,
        paths: Vec<PathBuf>,
    },
    /// Cache and build folders under `root`, with their sizes.
    Caches {
        id: u64,
        root: PathBuf,
    },
    /// Every exclusion there is to find: marked items Spotlight knows of and those under `root`,
    /// excluded paths and volumes, and places macOS leaves out by itself.
    All {
        id: u64,
        root: PathBuf,
    },
    Change {
        id: u64,
        change: BackupChange,
        items: Vec<BackupItem>,
    },
    /// Stops the window's searches.
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackupEvent {
    Listing {
        side: Side,
        generation: u64,
        dir: PathBuf,
        result: Result<BackupListing, String>,
    },
    Found {
        id: u64,
        items: Vec<BackupItem>,
    },
    Sized {
        id: u64,
        path: PathBuf,
        bytes: u64,
    },
    Progress {
        id: u64,
        folders: u64,
    },
    Finished {
        id: u64,
        error: Option<String>,
    },
    /// How backups treat the changed items now, and what went wrong, if anything.
    Changed {
        id: u64,
        items: Vec<BackupItem>,
        error: Option<String>,
    },
}

fn shows_marks(panel: &crate::Panel) -> bool {
    !panel.is_results() && panel.mount.is_none() && panel.remote_mount.is_none()
}

pub(crate) fn sync(state: &mut AppState) -> Vec<Effect> {
    if matches!(state.overlay, Some(Overlay::TimeMachine)) {
        let height = list_rows(state.rows);
        state.backup.window.keep_visible(height);
    }
    let available = state.backup.available;
    let mut effects = Vec::new();
    for side in Side::ALL {
        let panel = state.panel_mut(side);
        if !available || !shows_marks(panel) {
            panel.backup = BackupPanel::default();
            continue;
        }
        let key = (panel.generation(), panel.destination().to_path_buf());
        if panel.backup.key.as_ref() == Some(&key) {
            continue;
        }
        if panel.backup.key.as_ref().is_none_or(|(_, dir)| *dir != key.1) {
            panel.backup = BackupPanel::default();
        }
        panel.backup.key = Some(key.clone());
        effects.push(Effect::Backup(BackupRequest::Listing { side, generation: key.0, dir: key.1 }));
    }
    effects
}

/// Asks again for both panels' marks, after a change.
fn refresh_marks(state: &mut AppState) {
    state.panels.iter_mut().for_each(|panel| panel.backup.key = None);
}

pub(crate) fn receive(state: &mut AppState, event: BackupEvent) -> Vec<Effect> {
    match event {
        BackupEvent::Listing { side, generation, dir, result } => {
            let panel = state.panel_mut(side);
            if panel.backup.key.as_ref() != Some(&(generation, dir)) {
                return Vec::new();
            }
            let listing = result.unwrap_or_default();
            panel.backup.folder = listing.folder;
            panel.backup.entries = listing.entries.into_iter().collect();
        }
        BackupEvent::Found { id, items } => {
            let all = state.backup.window.lists[BackupTab::All.index()].id == id;
            if let Some(list) = list_of(state, id) {
                let hovered = list.hovered().map(|item| item.path.clone());
                list.items.extend(items.into_iter().map(|item| {
                    let group = if all { item.exclusion.clone() } else { Exclusion::Included };
                    BackupItem { group, ..item }
                }));
                if all {
                    list.items.sort_by(|a, b| (rank(&a.group), &a.path).cmp(&(rank(&b.group), &b.path)));
                    follow(list, hovered);
                }
            }
        }
        BackupEvent::Sized { id, path, bytes } => {
            if let Some(item) = list_of(state, id).and_then(|list| list.items.iter_mut().find(|item| item.path == path))
            {
                item.size = Some(bytes);
            }
        }
        BackupEvent::Progress { id, folders } => {
            if let Some(list) = list_of(state, id) {
                list.folders = folders;
            }
        }
        BackupEvent::Finished { id, error } => {
            let caches = state.backup.window.lists[BackupTab::Caches.index()].id == id;
            if let Some(list) = list_of(state, id) {
                list.loading = false;
                list.error = error;
                if caches {
                    let hovered = list.hovered().map(|item| item.path.clone());
                    list.items.sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.path.cmp(&b.path)));
                    follow(list, hovered);
                }
            }
        }
        BackupEvent::Changed { id, items, error } => {
            let window = &mut state.backup.window;
            if window.working.is_none_or(|(working, _)| working != id) {
                return Vec::new();
            }
            window.working = None;
            for list in &mut window.lists {
                for changed in &items {
                    if let Some(item) = list.items.iter_mut().find(|item| item.path == changed.path) {
                        item.exclusion = changed.exclusion.clone();
                        item.missing = changed.missing;
                    }
                }
                list.marks.clear();
            }
            let excluded = items.iter().filter(|item| item.exclusion.excluded()).count();
            let included = items.len() - excluded;
            window.outcome = Some(match error {
                Some(error) => tr!(backup_change_failed, error = error),
                None if included == 0 => tr!(backup_now_excluded, items = count(excluded, Count::Item)),
                None if excluded == 0 => tr!(backup_now_included, items = count(included, Count::Item)),
                None => tr!(
                    backup_now_mixed,
                    excluded = count(excluded, Count::Item),
                    included = count(included, Count::Item)
                ),
            });
            refresh_marks(state);
        }
    }
    Vec::new()
}

fn list_of(state: &mut AppState, id: u64) -> Option<&mut BackupList> {
    state.backup.window.lists.iter_mut().find(|list| list.started && list.id == id)
}

fn follow(list: &mut BackupList, path: Option<PathBuf>) {
    if let Some(index) = path.and_then(|path| list.items.iter().position(|item| item.path == path)) {
        list.cursor = index;
    }
    list.settle();
}

/// The order of All exclusions' groups.
pub fn rank(exclusion: &Exclusion) -> u8 {
    match exclusion {
        Exclusion::Sticky => 0,
        Exclusion::Path => 1,
        Exclusion::Volume => 2,
        Exclusion::System => 3,
        Exclusion::Inside(_) | Exclusion::Included | Exclusion::Unknown => 4,
    }
}

pub(crate) fn open(state: &mut AppState) -> Vec<Effect> {
    if !state.backup.available {
        state.notice = Some(tr!(backup_macos_only).into());
        return Vec::new();
    }
    let panel = state.active_panel();
    let paths: Vec<PathBuf> = panel.selection().into_iter().map(|entry| entry.path(panel.dir())).collect();
    let tab = if paths.is_empty() { BackupTab::Caches } else { BackupTab::Selection };
    state.backup.window = BackupWindow { tab, root: panel.dir().to_path_buf(), ..BackupWindow::default() };
    state.overlay = Some(Overlay::TimeMachine);
    let mut effects = Vec::new();
    if !paths.is_empty() {
        effects.extend(start(state, BackupTab::Selection, paths));
    }
    effects.extend(show(state, tab));
    effects
}

/// Switches to a tab, looking for its items the first time it shows.
fn show(state: &mut AppState, tab: BackupTab) -> Vec<Effect> {
    state.backup.window.tab = tab;
    if state.backup.window.lists[tab.index()].started || tab == BackupTab::Selection {
        return Vec::new();
    }
    start(state, tab, Vec::new())
}

fn start(state: &mut AppState, tab: BackupTab, paths: Vec<PathBuf>) -> Vec<Effect> {
    let id = state.backup.next_id();
    let window = &mut state.backup.window;
    let root = window.root.clone();
    let selection = window.lists[BackupTab::Selection.index()].items.iter().map(|item| item.path.clone()).collect();
    window.lists[tab.index()] = BackupList { started: true, loading: true, id, ..BackupList::default() };
    let request = match tab {
        BackupTab::Selection => BackupRequest::Inspect { id, paths: if paths.is_empty() { selection } else { paths } },
        BackupTab::Caches => BackupRequest::Caches { id, root },
        BackupTab::All => BackupRequest::All { id, root },
    };
    vec![Effect::Backup(request)]
}

pub(crate) fn on_key(state: &mut AppState, key: &KeyEvent, now: std::time::Instant) -> Vec<Effect> {
    let tab = state.backup.window.tab;
    let page = isize::try_from(list_rows(state.rows).saturating_sub(1).max(1)).unwrap_or(isize::MAX);
    let turn = |delta: isize| {
        let index = (tab.index() as isize + delta).rem_euclid(BackupTab::ALL.len() as isize);
        BackupTab::ALL[index as usize]
    };
    let list = state.backup.window.list_mut();
    match key.code {
        KeyCode::Esc => {
            state.overlay = None;
            return vec![Effect::Backup(BackupRequest::Cancel)];
        }
        KeyCode::Tab => return show(state, turn(1)),
        KeyCode::BackTab => return show(state, turn(-1)),
        KeyCode::Char(digit @ '1'..='3') => return show(state, BackupTab::ALL[digit as usize - '1' as usize]),
        KeyCode::Up => list.step(-1),
        KeyCode::Down => list.step(1),
        KeyCode::PageUp => list.step(-page),
        KeyCode::PageDown => list.step(page),
        KeyCode::Home => list.cursor = 0,
        KeyCode::End => list.step(isize::MAX),
        KeyCode::Char(' ') | KeyCode::Insert => {
            list.toggle(list.cursor);
            list.step(1);
        }
        KeyCode::Char('*') => {
            let all: HashSet<PathBuf> = list.items.iter().map(|item| item.path.clone()).collect();
            list.marks = all.difference(&list.marks).cloned().collect();
        }
        KeyCode::Char('e' | 'E') => return change(state, BackupChange::Exclude),
        KeyCode::Char('p' | 'P') => return change(state, BackupChange::ExcludePath),
        KeyCode::Char('i' | 'I') | KeyCode::Delete | KeyCode::F(8) => return change(state, BackupChange::Include),
        KeyCode::Char('r' | 'R') if state.backup.window.working.is_none() => {
            state.backup.window.outcome = None;
            return start(state, tab, Vec::new());
        }
        KeyCode::Char('a' | 'A') if tab == BackupTab::All => return add(state),
        KeyCode::Enter => return reveal(state),
        KeyCode::Char('o') if key.modifiers == KeyModifiers::CONTROL => {
            state.overlay = None;
            return crate::actions::perform(state, crate::Action::ToggleShell, now);
        }
        _ => {}
    }
    Vec::new()
}

/// Shows the item under the cursor in the active panel.
fn reveal(state: &mut AppState) -> Vec<Effect> {
    let Some(path) = state.backup.window.list().hovered().map(|item| item.path.clone()) else { return Vec::new() };
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else { return Vec::new() };
    state.overlay = None;
    let name = name.to_string_lossy().into_owned();
    let side = state.active;
    vec![Effect::Backup(BackupRequest::Cancel), state.navigate(side, parent.to_path_buf(), Some(name))]
}

fn add(state: &mut AppState) -> Vec<Effect> {
    let root = state.backup.window.root.display().to_string();
    let value = if root.ends_with('/') { root } else { format!("{root}/") };
    state.overlay = Some(Overlay::Prompt(Prompt {
        title: tr!(backup_add_title),
        label: tr!(backup_add_label).to_owned(),
        input: LineInput::new(value),
        purpose: PromptPurpose::BackupAdd { fixed: false },
        option_focused: false,
    }));
    Vec::new()
}

/// Excludes a path typed into the Add prompt, then returns to the window.
pub(crate) fn add_path(state: &mut AppState, text: &str, fixed: bool) -> Vec<Effect> {
    state.overlay = Some(Overlay::TimeMachine);
    let path = PathBuf::from(text.trim_end_matches('/'));
    if text.is_empty() || !path.is_absolute() {
        state.backup.window.outcome = Some(tr!(backup_add_absolute).into());
        return Vec::new();
    }
    let id = state.backup.next_id();
    let window = &mut state.backup.window;
    window.working = Some((id, fixed));
    window.outcome = None;
    let change = if fixed { BackupChange::ExcludePath } else { BackupChange::Exclude };
    let item = BackupItem { path: path.clone(), ..BackupItem::default() };
    let list = &mut window.lists[BackupTab::All.index()];
    if !list.items.iter().any(|item| item.path == path) {
        let group = if fixed { Exclusion::Path } else { Exclusion::Sticky };
        list.items.push(BackupItem { group, ..item.clone() });
        list.items.sort_by(|a, b| (rank(&a.group), &a.path).cmp(&(rank(&b.group), &b.path)));
    }
    follow(list, Some(path));
    vec![Effect::Backup(BackupRequest::Change { id, change, items: vec![item] })]
}

fn change(state: &mut AppState, change: BackupChange) -> Vec<Effect> {
    let window = &state.backup.window;
    if window.working.is_some() {
        return Vec::new();
    }
    let targets = window.list().targets();
    if targets.is_empty() {
        return Vec::new();
    }
    let wanted = |item: &&&BackupItem| match change {
        BackupChange::Exclude => !item.exclusion.excluded() && !item.missing,
        BackupChange::ExcludePath => {
            matches!(item.exclusion, Exclusion::Included | Exclusion::Unknown | Exclusion::Sticky)
        }
        BackupChange::Include => item.exclusion.own(),
    };
    let items: Vec<BackupItem> = targets.iter().filter(wanted).map(|item| (*item).clone()).collect();
    if items.is_empty() {
        let inside = targets.iter().find_map(|item| match &item.exclusion {
            Exclusion::Inside(folder) => Some(folder.clone()),
            _ => None,
        });
        let system = targets.iter().any(|item| item.exclusion == Exclusion::System);
        state.backup.window.outcome = Some(match (change, inside) {
            (BackupChange::Include, Some(folder)) => tr!(backup_include_folder, folder = name_of(&folder)),
            (BackupChange::Include, None) if system => tr!(backup_system_fixed).into(),
            (BackupChange::Include, None) => tr!(backup_already_included).into(),
            _ => tr!(backup_already_excluded).into(),
        });
        return Vec::new();
    }
    let password = match change {
        BackupChange::ExcludePath => true,
        BackupChange::Exclude => items.iter().any(|item| item.volume),
        BackupChange::Include => items.iter().any(|item| item.exclusion.needs_password()),
    };
    let id = state.backup.next_id();
    let window = &mut state.backup.window;
    window.working = Some((id, password));
    window.outcome = None;
    vec![Effect::Backup(BackupRequest::Change { id, change, items })]
}

pub fn name_of(path: &Path) -> String {
    path.file_name().map_or_else(|| path.display().to_string(), |name| name.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests;
