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

const PAGE: usize = 10;

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
        KeyCode::PageUp => list.step(-(PAGE as isize)),
        KeyCode::PageDown => list.step(PAGE as isize),
        KeyCode::Home => list.cursor = 0,
        KeyCode::End => list.step(isize::MAX),
        KeyCode::Char(' ') | KeyCode::Insert => {
            if let Some(path) = list.hovered().map(|item| item.path.clone())
                && !list.marks.remove(&path)
            {
                list.marks.insert(path);
            }
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
mod tests {
    use super::*;
    use crate::{Msg, Settings, Startup, update};
    use std::time::Instant;

    fn state() -> AppState {
        let now = Instant::now();
        let themes = vec![crate::ThemeChoice { id: "midnight".into(), name: "midnight".into() }];
        let appearance = crate::Appearance::new(themes, 0);
        let (mut state, _) = AppState::new(Startup::at("/work".into()), 100, 30, Settings::default(), appearance, now);
        state.backup.available = true;
        state
    }

    fn key(state: &mut AppState, code: KeyCode) -> Vec<Effect> {
        update(state, Msg::Key(KeyEvent::from(code)), Instant::now())
    }

    /// The window's requests, without the panels' own.
    fn requests(effects: &[Effect]) -> Vec<&BackupRequest> {
        effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::Backup(BackupRequest::Listing { .. }) => None,
                Effect::Backup(request) => Some(request),
                _ => None,
            })
            .collect()
    }

    fn item(path: &str, exclusion: Exclusion) -> BackupItem {
        BackupItem { path: path.into(), dir: true, exclusion, ..BackupItem::default() }
    }

    fn opened_on_caches(state: &mut AppState) -> u64 {
        let effects = crate::actions::perform(state, crate::Action::TimeMachine, Instant::now());
        match requests(&effects)[..] {
            [BackupRequest::Caches { id, root }] => {
                assert_eq!(root, Path::new("/work"));
                *id
            }
            ref other => panic!("expected a cache search, got {other:?}"),
        }
    }

    #[test]
    fn panels_ask_for_marks_once_per_listing_and_only_where_time_machine_is() {
        let mut state = state();
        let listings = |effects: &[Effect]| {
            effects.iter().filter(|effect| matches!(effect, Effect::Backup(BackupRequest::Listing { .. }))).count()
        };
        assert_eq!(listings(&sync(&mut state)), 2);
        assert!(sync(&mut state).is_empty(), "the same listing is not asked about again");
        let mut elsewhere = self::state();
        elsewhere.backup.available = false;
        assert!(sync(&mut elsewhere).is_empty());
    }

    #[test]
    fn marks_from_an_older_listing_are_dropped() {
        let mut state = state();
        sync(&mut state);
        let listing =
            BackupListing { folder: Exclusion::Included, entries: vec![("target".into(), Exclusion::Sticky)] };
        let event = |generation| {
            Msg::Backup(BackupEvent::Listing {
                side: Side::Left,
                generation,
                dir: "/work".into(),
                result: Ok(listing.clone()),
            })
        };
        let generation = state.panel(Side::Left).generation();
        update(&mut state, event(generation + 7), Instant::now());
        assert!(!state.panel(Side::Left).backup.marks());
        update(&mut state, event(generation), Instant::now());
        assert_eq!(state.panel(Side::Left).backup.entry("target"), Some(&Exclusion::Sticky));
    }

    #[test]
    fn off_macos_the_window_explains_itself_instead_of_opening() {
        let mut state = state();
        state.backup.available = false;
        let effects = crate::actions::perform(&mut state, crate::Action::TimeMachine, Instant::now());
        assert!(effects.is_empty());
        assert!(state.overlay.is_none());
        assert_eq!(state.notice.as_deref(), Some(tr!(backup_macos_only)));
    }

    #[test]
    fn tabs_search_the_first_time_they_show_and_esc_stops_the_searches() {
        let mut state = state();
        opened_on_caches(&mut state);
        assert_eq!(state.backup.window.tab, BackupTab::Caches);
        let effects = key(&mut state, KeyCode::Tab);
        assert!(matches!(requests(&effects)[..], [BackupRequest::All { .. }]));
        assert!(requests(&key(&mut state, KeyCode::Char('2'))).is_empty(), "caches were already searched");
        let effects = key(&mut state, KeyCode::Esc);
        assert!(requests(&effects).contains(&&BackupRequest::Cancel));
        assert!(state.overlay.is_none());
    }

    #[test]
    fn caches_sort_by_size_once_found_and_keep_the_cursor_on_its_folder() {
        let mut state = state();
        let id = opened_on_caches(&mut state);
        let found = vec![item("/work/a/target", Exclusion::Included), item("/work/node_modules", Exclusion::Sticky)];
        receive(&mut state, BackupEvent::Found { id, items: found });
        receive(&mut state, BackupEvent::Sized { id, path: "/work/a/target".into(), bytes: 10 });
        receive(&mut state, BackupEvent::Sized { id, path: "/work/node_modules".into(), bytes: 900 });
        assert_eq!(state.backup.window.list().hovered().unwrap().path, Path::new("/work/a/target"));
        receive(&mut state, BackupEvent::Finished { id, error: None });
        let list = state.backup.window.list();
        assert_eq!(list.items[0].path, Path::new("/work/node_modules"));
        assert_eq!(list.hovered().unwrap().path, Path::new("/work/a/target"));
        assert!(!list.loading);
    }

    #[test]
    fn exclude_sends_only_what_is_still_backed_up_and_reports_the_outcome() {
        let mut state = state();
        let id = opened_on_caches(&mut state);
        let found = vec![item("/work/target", Exclusion::Sticky), item("/work/node_modules", Exclusion::Included)];
        receive(&mut state, BackupEvent::Found { id, items: found });
        key(&mut state, KeyCode::Char('*'));
        let effects = key(&mut state, KeyCode::Char('e'));
        let [BackupRequest::Change { id, change: BackupChange::Exclude, items }] = &requests(&effects)[..] else {
            panic!("expected one change");
        };
        assert_eq!(items.iter().map(|item| item.path.as_path()).collect::<Vec<_>>(), [Path::new("/work/node_modules")]);
        assert_eq!(state.backup.window.working, Some((*id, false)));
        assert!(requests(&key(&mut state, KeyCode::Char('e'))).is_empty(), "one change at a time");

        let changed = vec![item("/work/node_modules", Exclusion::Sticky)];
        receive(&mut state, BackupEvent::Changed { id: *id, items: changed, error: None });
        let window = &state.backup.window;
        assert!(window.working.is_none());
        assert!(window.list().marks.is_empty());
        assert!(window.list().items.iter().all(|item| item.exclusion == Exclusion::Sticky));
        assert_eq!(window.outcome.as_deref(), Some(tr!(backup_now_excluded, items = "1 item").as_str()));
        assert!(state.panels.iter().all(|panel| panel.backup.key.is_none()), "the panels' marks are asked again");
    }

    #[test]
    fn excluding_a_path_or_including_one_waits_for_the_password() {
        let mut state = state();
        let id = opened_on_caches(&mut state);
        receive(&mut state, BackupEvent::Found { id, items: vec![item("/work/node_modules", Exclusion::Included)] });
        key(&mut state, KeyCode::Char('p'));
        assert!(state.backup.window.working.is_some_and(|(_, password)| password));

        let mut state = self::state();
        let id = opened_on_caches(&mut state);
        let found = vec![item("/work/a", Exclusion::Sticky), item("/work/b", Exclusion::Path)];
        receive(&mut state, BackupEvent::Found { id, items: found });
        key(&mut state, KeyCode::Char('*'));
        let effects = key(&mut state, KeyCode::Char('i'));
        let [BackupRequest::Change { change: BackupChange::Include, items, .. }] = &requests(&effects)[..] else {
            panic!("expected one change");
        };
        assert_eq!(items.len(), 2);
        assert!(state.backup.window.working.is_some_and(|(_, password)| password));
    }

    #[test]
    fn including_what_is_not_its_own_says_what_to_do_instead() {
        let mut state = state();
        let id = opened_on_caches(&mut state);
        let found = vec![item("/work/target/debug", Exclusion::Inside("/work/target".into()))];
        receive(&mut state, BackupEvent::Found { id, items: found });
        assert!(requests(&key(&mut state, KeyCode::Char('i'))).is_empty());
        assert_eq!(state.backup.window.outcome, Some(tr!(backup_include_folder, folder = "target")));

        receive(&mut state, BackupEvent::Found { id, items: vec![item("/System", Exclusion::System)] });
        key(&mut state, KeyCode::Down);
        assert!(requests(&key(&mut state, KeyCode::Char('i'))).is_empty());
        assert_eq!(state.backup.window.outcome.as_deref(), Some(tr!(backup_system_fixed)));
    }

    #[test]
    fn all_exclusions_group_by_kind_and_keep_the_group_after_a_change() {
        let mut state = state();
        opened_on_caches(&mut state);
        key(&mut state, KeyCode::Char('3'));
        let id = state.backup.window.list().id;
        let found = vec![item("/z", Exclusion::System), item("/b", Exclusion::Path), item("/c", Exclusion::Sticky)];
        receive(&mut state, BackupEvent::Found { id, items: found });
        let paths = |state: &AppState| -> Vec<PathBuf> {
            state.backup.window.list().items.iter().map(|item| item.path.clone()).collect()
        };
        assert_eq!(paths(&state), [PathBuf::from("/c"), "/b".into(), "/z".into()]);
        let effects = key(&mut state, KeyCode::Char('i'));
        let [BackupRequest::Change { id, .. }] = &requests(&effects)[..] else { panic!("expected one change") };
        receive(
            &mut state,
            BackupEvent::Changed { id: *id, items: vec![item("/c", Exclusion::Included)], error: None },
        );
        let first = &state.backup.window.list().items[0];
        assert_eq!((first.exclusion.clone(), first.group.clone()), (Exclusion::Included, Exclusion::Sticky));
    }

    #[test]
    fn a_path_typed_into_add_becomes_an_exclusion_of_the_chosen_kind() {
        let mut state = state();
        opened_on_caches(&mut state);
        key(&mut state, KeyCode::Char('3'));
        key(&mut state, KeyCode::Char('a'));
        assert!(matches!(
            state.overlay,
            Some(Overlay::Prompt(Prompt { purpose: PromptPurpose::BackupAdd { .. }, .. }))
        ));
        let effects = add_path(&mut state, "relative/path", false);
        assert!(effects.is_empty());
        assert_eq!(state.backup.window.outcome.as_deref(), Some(tr!(backup_add_absolute)));
        let effects = add_path(&mut state, "/work/logs/", true);
        let [BackupRequest::Change { change: BackupChange::ExcludePath, items, .. }] = &requests(&effects)[..] else {
            panic!("expected one change");
        };
        assert_eq!(items[0].path, Path::new("/work/logs"));
        assert!(matches!(state.overlay, Some(Overlay::TimeMachine)));
        assert_eq!(state.backup.window.list().hovered().unwrap().group, Exclusion::Path);
    }
}
