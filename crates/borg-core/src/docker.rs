//! The Docker view: Compose projects with their containers in one panel, and the selected
//! project or container with its live resource use in the other. Ctrl-O stays the host
//! console.

use borg_i18n::tr;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    time::{Duration, Instant},
};

use borg_docker::{Action, Container, Details, State, Stats};
use borg_term::{GridSize, Scroll};

use crate::{AppState, Effect, HISTORY, ShellStatus, Side, View};

mod files;
mod keys;
mod shells;

pub(crate) use files::{browse, copy, delete, mkdir, unmount, view};
pub(crate) use keys::{act, click, paste, route_key, start, stats, tab_key};
use keys::{pause_toggle, targets};
use shells::shell_key;
pub use shells::{container_shell_busy, shell_rect};
pub(crate) use shells::{
    open_shell, parked_signal, scroll_shell, shell_closed, shell_output, show_shell, sync_console,
};

const REFRESH: Duration = Duration::from_secs(2);
/// After an error, such as Docker not running, ask again less often.
const RETRY: Duration = Duration::from_secs(5);
/// Moving through the list inspects the container the cursor rests on, not every one it passes.
const INSPECT_DELAY: Duration = Duration::from_millis(150);
const PAGE: usize = 10;
const FILTER_LIMIT: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DockerRequest {
    List,
    Inspect(String),
    Act { action: Action, ids: Vec<String> },
    Files { container: String, op: FileOp },
}

impl DockerRequest {
    /// Whether a newer request makes this one pointless, so it can be dropped.
    pub fn is_query(&self) -> bool {
        matches!(self, Self::List | Self::Inspect(_))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DockerData {
    Listed {
        context: Option<String>,
        containers: Vec<Container>,
    },
    Inspected {
        id: String,
        details: Box<Details>,
    },
    /// A container file copied out for viewing.
    Fetched(PathBuf),
    Done,
}

/// Work on a container's files. Paths inside the container are absolute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileOp {
    Delete { paths: Vec<PathBuf> },
    Mkdir { path: PathBuf },
    Fetch { path: PathBuf },
}

/// A panel showing a container's filesystem in place of a host folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mount {
    pub id: String,
    pub name: String,
    /// The host folder the panel showed before; `..` at the container's root returns to it.
    pub host: PathBuf,
}

/// A shell running inside a container. It stays open while hidden, until it exits, the
/// container stops, or Borg quits.
#[derive(Debug, Clone)]
pub struct ContainerShell {
    pub container: String,
    pub name: String,
    /// Its shell's state; while it is the console, `AppState::shell` holds the live one.
    pub status: ShellStatus,
    opened: Instant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellRequest {
    /// `prompt` names the container in the shell's prompt; `workdir` is where it starts.
    Open {
        container: String,
        prompt: String,
        size: GridSize,
        workdir: Option<PathBuf>,
    },
    Input {
        container: String,
        bytes: Vec<u8>,
    },
    Scroll {
        container: String,
        scroll: Scroll,
    },
}

/// File work in progress, shown in the footer of the panel it changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileWork {
    pub id: u64,
    pub side: Side,
    pub op: FileOp,
}

/// What the cursor is on, kept across refreshes while rows come and go.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Key {
    Group(Option<String>),
    Container(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockerRow {
    Group(usize),
    Container(usize),
}

/// A Compose project, or the containers that belong to none (`project` is `None`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockerGroup {
    pub project: Option<String>,
    pub containers: Vec<usize>,
    pub running: usize,
}

/// A container's recent CPU and memory, one sample a second.
#[derive(Debug, Clone, Default)]
pub struct Usage {
    pub cpu: Vec<u64>,
    pub memory: Vec<u64>,
    pub latest: Stats,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Working {
    pub id: u64,
    pub action: Action,
    pub ids: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Docker {
    pub side: Option<Side>,
    pub expanded: bool,
    pub details_focused: bool,
    pub context: Option<String>,
    pub containers: Vec<Container>,
    pub groups: Vec<DockerGroup>,
    pub rows: Vec<DockerRow>,
    pub cursor: usize,
    pub top: usize,
    pub loaded: bool,
    pub usage: HashMap<String, Usage>,
    pub details: Option<(String, Details)>,
    pub filter: String,
    pub filtering: bool,
    pub error: Option<String>,
    pub working: Vec<Working>,
    pub files: Vec<FileWork>,
    pub shells: Vec<ContainerShell>,
    /// A container whose console shell failed to start, so it is not opened again until
    /// another panel is active.
    failed_console: Option<String>,
    /// Whether the details panel shows the selected container's shell, when it has one.
    pub shell_tab: bool,
    pub pending: Option<(u64, DockerRequest)>,
    collapsed: HashSet<Option<String>>,
    selected: Option<Key>,
    watching: bool,
    next_id: u64,
    next: Option<Instant>,
    inspect_due: Option<Instant>,
}

fn matches(container: &Container, filter: &str) -> bool {
    [Some(&container.name), Some(&container.image), container.project.as_ref(), container.service.as_ref()]
        .into_iter()
        .flatten()
        .any(|value| value.to_lowercase().contains(filter))
}

impl Docker {
    pub fn is_open(&self) -> bool {
        self.side.is_some()
    }

    pub fn row(&self) -> Option<DockerRow> {
        self.rows.get(self.cursor).copied()
    }

    pub fn container(&self) -> Option<&Container> {
        match self.row()? {
            DockerRow::Container(index) => self.containers.get(index),
            DockerRow::Group(_) => None,
        }
    }

    pub fn group(&self) -> Option<&DockerGroup> {
        match self.row()? {
            DockerRow::Group(index) => self.groups.get(index),
            DockerRow::Container(index) => self.groups.iter().find(|g| g.containers.contains(&index)),
        }
    }

    pub fn collapsed(&self, group: &DockerGroup) -> bool {
        self.collapsed.contains(&group.project)
    }

    /// The details of the selected container, once inspected.
    pub fn selected_details(&self) -> Option<&Details> {
        let container = self.container()?;
        self.details.as_ref().filter(|(id, _)| *id == container.id).map(|(_, details)| details)
    }

    pub fn shell(&self, container: &str) -> Option<&ContainerShell> {
        self.shells.iter().find(|shell| shell.container == container)
    }

    /// The shell in the details panel: the selected container's, on the Shell tab.
    pub fn visible_shell(&self) -> Option<&ContainerShell> {
        let container = self.container().filter(|_| self.shell_tab)?;
        self.shell(&container.id)
    }

    /// Whether keys go to a container's shell.
    pub fn typing(&self) -> bool {
        self.is_open() && self.details_focused && self.visible_shell().is_some()
    }

    /// The action running on a container, so its row can say "Stopping…".
    pub fn working_on(&self, id: &str) -> Option<Action> {
        self.working.iter().find(|w| w.ids.iter().any(|i| i == id)).map(|w| w.action)
    }

    /// Total CPU (hundredths of a percent) and memory of a group's running containers.
    pub fn group_usage(&self, group: &DockerGroup) -> (u64, u64) {
        group
            .containers
            .iter()
            .filter_map(|&i| self.usage.get(&self.containers[i].id))
            .fold((0, 0), |(cpu, memory), usage| (cpu + u64::from(usage.latest.cpu), memory + usage.latest.memory))
    }

    pub fn list_start(&self, height: usize) -> usize {
        self.top.min(self.cursor).max(self.cursor.saturating_sub(height.max(1) - 1))
    }

    pub(crate) fn keep_visible(&mut self, height: usize) {
        self.top = self.list_start(height);
    }

    fn key_at(&self, row: DockerRow) -> Key {
        match row {
            DockerRow::Group(index) => Key::Group(self.groups[index].project.clone()),
            DockerRow::Container(index) => Key::Container(self.containers[index].id.clone()),
        }
    }

    fn select(&mut self, cursor: usize, now: Instant) {
        let cursor = cursor.min(self.rows.len().saturating_sub(1));
        let before = self.container().map(|c| c.id.clone());
        self.cursor = cursor;
        self.selected = self.row().map(|row| self.key_at(row));
        let after = self.container().map(|c| c.id.clone());
        if after.is_some() && after != before {
            self.inspect_due = Some(now + INSPECT_DELAY);
        }
    }

    fn move_by(&mut self, delta: isize, now: Instant) {
        let last = self.rows.len().saturating_sub(1) as isize;
        self.select((self.cursor as isize + delta).clamp(0, last.max(0)) as usize, now);
    }

    /// Groups containers into projects, sorted by name, with containers outside Compose
    /// last, and lists the rows the filter and folding leave visible.
    pub(crate) fn rebuild(&mut self) {
        let mut projects: Vec<Option<String>> = self.containers.iter().map(|c| c.project.clone()).collect();
        projects.sort_by_key(|p| (p.is_none(), p.as_ref().map(|name| name.to_lowercase())));
        projects.dedup();
        let filter = self.filter.to_lowercase();
        self.groups = projects
            .into_iter()
            .filter_map(|project| {
                let named = project.as_ref().is_some_and(|name| name.to_lowercase().contains(&filter));
                let mut containers: Vec<usize> = (0..self.containers.len())
                    .filter(|&i| self.containers[i].project == project)
                    .filter(|&i| named || matches(&self.containers[i], &filter))
                    .collect();
                containers
                    .sort_by_key(|&i| (self.containers[i].label().to_lowercase(), self.containers[i].name.clone()));
                let running = containers.iter().filter(|&&i| self.containers[i].state == State::Running).count();
                (!containers.is_empty()).then_some(DockerGroup { project, containers, running })
            })
            .collect();
        self.rows = self
            .groups
            .iter()
            .enumerate()
            .flat_map(|(index, group)| {
                let open = !self.collapsed.contains(&group.project) || !filter.is_empty();
                std::iter::once(DockerRow::Group(index))
                    .chain(group.containers.iter().copied().filter(move |_| open).map(DockerRow::Container))
            })
            .collect();
        let found = self.selected.clone().and_then(|key| self.rows.iter().position(|&row| self.key_at(row) == key));
        self.cursor = found.unwrap_or(self.cursor.min(self.rows.len().saturating_sub(1)));
        // Before the first list, a wanted selection waits for it.
        if found.is_none() && !self.rows.is_empty() {
            self.selected = self.row().map(|row| self.key_at(row));
        }
    }

    fn request(&mut self, request: DockerRequest) -> Vec<Effect> {
        self.next_id += 1;
        let id = self.next_id;
        match &request {
            DockerRequest::Act { action, ids } => {
                self.working.push(Working { id, action: *action, ids: ids.clone() });
            }
            DockerRequest::Files { .. } => {}
            _ => {
                self.pending = Some((id, request.clone()));
                self.next = None;
            }
        }
        vec![Effect::Docker { id, request }]
    }

    fn sample(&mut self, frame: Vec<(String, Stats)>) {
        for (id, stats) in frame {
            let usage = self.usage.entry(id).or_default();
            for (values, value) in [(&mut usage.cpu, u64::from(stats.cpu)), (&mut usage.memory, stats.memory)] {
                values.push(value);
                if values.len() > HISTORY {
                    values.remove(0);
                }
            }
            usage.latest = stats;
        }
    }
}

pub(crate) fn open(state: &mut AppState, now: Instant) -> Vec<Effect> {
    if !state.settings.preferences.docker_plugin {
        state.notice = Some(tr!(docker_disabled).into());
        return Vec::new();
    }
    state.overlay = None;
    state.activity.side = None;
    let docker = &mut state.docker;
    docker.side = Some(state.active);
    docker.details_focused = false;
    docker.error = None;
    docker.inspect_due = docker.container().map(|_| now);
    docker.request(DockerRequest::List)
}

pub(crate) fn close(state: &mut AppState) {
    state.docker.side = None;
    state.docker.filtering = false;
}

pub(crate) fn toggle(state: &mut AppState, now: Instant) -> Vec<Effect> {
    if state.docker.is_open() {
        close(state);
        Vec::new()
    } else {
        open(state, now)
    }
}

pub(crate) fn filter(state: &mut AppState) {
    if state.docker.is_open() {
        state.docker.filtering = true;
        state.docker.details_focused = false;
    }
}

/// Whether an action would change anything in the selection, for the Docker menu.
pub(crate) fn can(state: &AppState, action: Action) -> bool {
    state.docker.is_open() && !targets(&state.docker, action).is_empty()
}

/// Whether the selection is a container with a shell open, or a running one to open one in.
pub(crate) fn can_open_shell(state: &AppState) -> bool {
    state.docker.is_open()
        && state.docker.container().is_some_and(|c| c.state == State::Running || state.docker.shell(&c.id).is_some())
}

/// Whether the selection is a running container, whose files can be browsed.
pub(crate) fn can_browse(state: &AppState) -> bool {
    state.docker.is_open() && state.docker.container().is_some_and(|c| c.state == State::Running)
}

/// Pause, or Resume when the selection is paused.
pub(crate) fn pause_action(state: &AppState) -> Action {
    pause_toggle(&state.docker)
}

pub(crate) fn wake_at(state: &AppState) -> Option<Instant> {
    let docker = &state.docker;
    if !docker.is_open() || state.view != View::Panels || docker.pending.is_some() {
        return None;
    }
    docker.inspect_due.into_iter().chain(docker.next).min()
}

/// Starts and stops the stats stream with the view, refreshes the list, and inspects the
/// selected container once the cursor rests on it.
pub(crate) fn sync(state: &mut AppState, now: Instant) -> Vec<Effect> {
    let enabled = state.settings.preferences.docker_plugin;
    let docker = &mut state.docker;
    if !enabled {
        docker.side = None;
    }
    let live = docker.is_open() && state.view == View::Panels;
    let mut effects = Vec::new();
    if live != docker.watching {
        docker.watching = live;
        effects.push(Effect::DockerWatch(live));
    }
    if !live {
        docker.pending = None;
        docker.next = None;
        if !docker.is_open() {
            docker.usage.clear();
        }
        return effects;
    }
    let rows = state.layout(now).panels[docker_side(state).index()].height.saturating_sub(6);
    let docker = &mut state.docker;
    docker.keep_visible(usize::from(rows));
    if docker.pending.is_some() {
        return effects;
    }
    if docker.inspect_due.is_some_and(|due| now >= due) {
        docker.inspect_due = None;
        if let Some(id) = docker.container().map(|c| c.id.clone()) {
            effects.extend(docker.request(DockerRequest::Inspect(id)));
            return effects;
        }
    }
    if docker.next.is_none_or(|next| now >= next) && docker.loaded {
        effects.extend(docker.request(DockerRequest::List));
    }
    effects
}

fn docker_side(state: &AppState) -> Side {
    state.docker.side.unwrap_or(state.active)
}

pub(crate) fn receive(state: &mut AppState, id: u64, result: Result<DockerData, String>, now: Instant) -> Vec<Effect> {
    let docker = &mut state.docker;
    if let Some(index) = docker.files.iter().position(|w| w.id == id) {
        let work = docker.files.remove(index);
        return finish_files(state, work, result);
    }
    if let Some(index) = docker.working.iter().position(|w| w.id == id) {
        let work = docker.working.remove(index);
        if let Err(error) = result {
            state.notice = Some(tr!(docker_action_failed, error = error));
        } else if work.action == Action::Stop || work.action == Action::Restart || work.action == Action::Start {
            docker.inspect_due = Some(now);
        }
        docker.next = Some(now);
        return Vec::new();
    }
    if docker.pending.as_ref().is_none_or(|(pending, _)| *pending != id) {
        return Vec::new();
    }
    docker.pending = None;
    match result {
        Ok(DockerData::Listed { context, containers }) => {
            let selected = docker.container().map(|c| (c.id.clone(), c.status.clone()));
            if context.is_some() {
                docker.context = context;
            }
            let ids: HashSet<&str> = containers.iter().map(|c| c.id.as_str()).collect();
            docker.usage.retain(|id, _| ids.contains(id.as_str()));
            docker.containers = containers;
            docker.loaded = true;
            docker.error = None;
            docker.rebuild();
            let current = docker.container().map(|c| (c.id.clone(), c.status.clone()));
            // A container that changed state has new details: a start time, an exit code.
            if current.is_some() && (current != selected || docker.selected_details().is_none()) {
                docker.inspect_due.get_or_insert(now);
            }
            docker.next = Some(now + REFRESH);
        }
        Ok(DockerData::Inspected { id, details }) => {
            docker.details = Some((id, *details));
            docker.next.get_or_insert(now + REFRESH);
        }
        Ok(DockerData::Done | DockerData::Fetched(_)) => {}
        Err(error) => {
            docker.loaded = true;
            docker.error = Some(error);
            docker.next = Some(now + RETRY);
        }
    }
    Vec::new()
}

fn finish_files(state: &mut AppState, work: FileWork, result: Result<DockerData, String>) -> Vec<Effect> {
    match (result, work.op) {
        (Ok(DockerData::Fetched(path)), _) => crate::viewer::open(state, path),
        (Err(error), FileOp::Fetch { .. }) => {
            state.notice = Some(tr!(docker_action_failed, error = error));
            Vec::new()
        }
        (result, op) => {
            let count = |paths: &[PathBuf]| borg_i18n::count(paths.len(), borg_i18n::Count::Item);
            state.notice = Some(match (result, op) {
                (Err(error), _) => tr!(docker_action_failed, error = error),
                (Ok(_), FileOp::Delete { paths }) => tr!(docker_deleted, items = count(&paths)),
                (Ok(_), FileOp::Mkdir { .. } | FileOp::Fetch { .. }) => String::new(),
            })
            .filter(|notice| !notice.is_empty());
            // Partial copies and deletes change the folders too.
            state.reload_all()
        }
    }
}

#[cfg(test)]
mod tests;
