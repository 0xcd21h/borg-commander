//! The Docker view: Compose projects with their containers in one panel, and the selected
//! project or container with its live resource use in the other. Ctrl-O stays the host
//! console.

use borg_i18n::tr;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use borg_docker::{Action, Container, Details, State, Stats};
use borg_term::{GridSize, Scroll, TermMode, encode_key, encode_paste};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;

use crate::{AppState, Confirm, ConfirmPurpose, Console, Effect, HISTORY, Overlay, ShellStatus, Side, View};

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

/// The rows of the details panel below its status and tabs, where a shell runs.
pub fn shell_rect(panel: Rect) -> Rect {
    const HEADER: u16 = 2;
    let inner = Rect {
        x: panel.x + 1,
        y: panel.y + 1,
        width: panel.width.saturating_sub(2),
        height: panel.height.saturating_sub(2),
    };
    Rect { y: inner.y + HEADER.min(inner.height), height: inner.height.saturating_sub(HEADER), ..inner }
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

/// Opens a shell in the selected container, or shows the one already open, and sends keys to it.
pub(crate) fn open_shell(state: &mut AppState, now: Instant) -> Vec<Effect> {
    let Some(container) = state.docker.container().cloned() else {
        state.notice = Some(tr!(docker_choose_container).into());
        return Vec::new();
    };
    if state.docker.shell(&container.id).is_none() && container.state != State::Running {
        state.notice = Some(tr!(docker_start_for_shell).into());
        return Vec::new();
    }
    let docker = &mut state.docker;
    docker.shell_tab = true;
    docker.details_focused = true;
    docker.filtering = false;
    if docker.shell(&container.id).is_some() {
        return Vec::new();
    }
    let side = docker.side.unwrap_or(state.active).other();
    let area = shell_rect(state.layout(now).panels[side.index()]);
    let size = GridSize { cols: area.width.max(MIN_SHELL.0), rows: area.height.max(MIN_SHELL.1) };
    start_shell(state, &container, size, None, now)
}

fn start_shell(
    state: &mut AppState,
    container: &Container,
    size: GridSize,
    workdir: Option<PathBuf>,
    now: Instant,
) -> Vec<Effect> {
    let status = ShellStatus::new(workdir.clone().unwrap_or_default());
    let name = container.label().to_owned();
    state.docker.shells.push(ContainerShell { container: container.id.clone(), name, status, opened: now });
    let prompt = container.name.clone();
    vec![Effect::DockerShell(ShellRequest::Open { container: container.id.clone(), prompt, size, workdir })]
}

/// A shell that closes this soon after opening failed to start, such as in an image with no
/// shell, and is not reopened by itself.
const FAILED_START: Duration = Duration::from_secs(3);

/// Whose shell the console should be: a container's while its filesystem panel is active and
/// the Docker command line is on, otherwise the host's. The Docker view keeps the host's.
fn wanted_console(state: &AppState) -> Console {
    if let Some(mount) = &state.active_panel().remote_mount
        && state.settings.preferences.docker_console
        && !state.docker.is_open()
        && state.failed_remote_console.as_ref() != Some(&(mount.machine.clone(), mount.session.clone()))
    {
        return Console::Remote { machine: mount.machine.clone(), session: mount.session.clone() };
    }
    let follow = state.settings.preferences.docker_plugin && state.settings.preferences.docker_console;
    match state.active_panel().mount.as_ref() {
        Some(mount) if follow && !state.docker.is_open() && state.docker.failed_console.as_ref() != Some(&mount.id) => {
            Console::Container(mount.id.clone())
        }
        _ => Console::Host,
    }
}

/// Hands the command line and Ctrl-O console to the shell that should have them, opening a
/// container's shell in the panel's folder when it has none.
pub(crate) fn sync_console(state: &mut AppState, now: Instant) -> Vec<Effect> {
    let mount = state.active_panel().mount.as_ref().map(|mount| mount.id.clone());
    if state.docker.failed_console.is_some() && state.docker.failed_console != mount {
        state.docker.failed_console = None;
    }
    let remote = state.active_panel().remote_mount.as_ref().map(|m| (m.machine.clone(), m.session.clone()));
    if state.failed_remote_console.is_some() && state.failed_remote_console != remote {
        state.failed_remote_console = None;
        state.remote_console_failure = None;
    }
    let wanted = wanted_console(state);
    // A console in use keeps its shell until the panels come back.
    if wanted == state.console || state.view == View::Shell {
        return Vec::new();
    }
    let mut effects = Vec::new();
    if let Console::Container(id) = &wanted
        && state.docker.shell(id).is_none()
    {
        let mount = state.active_panel().mount.clone();
        let container = state.docker.containers.iter().find(|c| c.id == *id).cloned().unwrap_or_else(|| Container {
            id: id.clone(),
            name: mount.as_ref().map(|m| m.name.clone()).unwrap_or_default(),
            ..placeholder()
        });
        let size = GridSize::console(state.cols, state.rows);
        // Where the panel is going: a folder still loading has not replaced the host's yet.
        let dir = state.active_panel().destination().to_path_buf();
        effects.extend(start_shell(state, &container, size, Some(dir), now));
    }
    let incoming = match &wanted {
        Console::Remote { machine, session } => state
            .remote_consoles
            .remove(&(machine.clone(), session.clone()))
            .unwrap_or_else(|| ShellStatus::new(state.active_panel().destination().into())),
        Console::Host => {
            state.parked_host.take().unwrap_or_else(|| ShellStatus::new(state.active_panel().dir().into()))
        }
        Console::Container(id) => {
            let shell = state.docker.shells.iter_mut().find(|shell| shell.container == *id);
            shell
                .map(|shell| std::mem::replace(&mut shell.status, ShellStatus::new(PathBuf::new())))
                .unwrap_or_else(|| ShellStatus::new(PathBuf::new()))
        }
    };
    let outgoing = std::mem::replace(&mut state.shell, incoming);
    match std::mem::replace(&mut state.console, wanted.clone()) {
        Console::Remote { machine, session } => {
            state.remote_consoles.insert((machine, session), outgoing);
        }
        Console::Host => state.parked_host = Some(outgoing),
        Console::Container(id) => {
            if let Some(shell) = state.docker.shells.iter_mut().find(|shell| shell.container == id) {
                shell.status = outgoing;
            }
        }
    }
    effects.push(Effect::UseConsole(wanted));
    effects
}

/// A stand-in for a container the list has not loaded, such as one browsed before the
/// Docker view refreshed.
fn placeholder() -> Container {
    Container {
        id: String::new(),
        name: String::new(),
        image: String::new(),
        command: String::new(),
        created: String::new(),
        state: State::Running,
        status: String::new(),
        ports: String::new(),
        networks: String::new(),
        project: None,
        service: None,
        project_dir: None,
        config_files: Vec::new(),
    }
}

/// A signal from a shell that is not the console updates its saved state.
pub(crate) fn parked_signal(state: &mut AppState, container: Option<&str>, signal: &borg_term::ShellSignal) {
    let status = match container {
        None => state.parked_host.as_mut(),
        Some(id) => state.docker.shells.iter_mut().find(|shell| shell.container == id).map(|shell| &mut shell.status),
    };
    if let Some(status) = status {
        status.note(signal);
    }
}

/// The smallest terminal a shell starts with, should the details panel be hidden.
const MIN_SHELL: (u16, u16) = (20, 5);

pub(crate) fn shell_output(state: &mut AppState, container: &str, mode: TermMode) {
    if let Some(shell) = state.docker.shells.iter_mut().find(|shell| shell.container == container) {
        shell.status.observed.mode = mode;
    }
}

pub(crate) fn shell_closed(state: &mut AppState, container: &str, now: Instant) {
    let docker = &mut state.docker;
    let Some(index) = docker.shells.iter().position(|shell| shell.container == container) else { return };
    let shell = docker.shells.remove(index);
    if docker.container().is_some_and(|c| c.id == container) {
        docker.details_focused = false;
        docker.shell_tab = false;
    }
    if state.console == Console::Container(container.to_owned()) {
        // The console returns to the host; a shell that never got going is not reopened.
        state.shell = state.parked_host.take().unwrap_or_else(|| ShellStatus::new(state.active_panel().dir().into()));
        state.console = Console::Host;
        state.view = View::Panels;
        if now.saturating_duration_since(shell.opened) < FAILED_START {
            state.docker.failed_console = Some(container.to_owned());
        }
    }
    state.notice = Some(tr!(docker_shell_closed, name = shell.name));
}

/// Opens the Docker view on a container's Shell tab, opening its shell if it has none, so the
/// user watches what an agent runs there. The container is selected once the list has it.
pub(crate) fn show_shell(state: &mut AppState, id: &str, name: &str) -> Vec<Effect> {
    let now = state.epoch;
    let mut effects = if state.docker.is_open() { Vec::new() } else { open(state, now) };
    let docker = &mut state.docker;
    docker.selected = Some(Key::Container(id.to_owned()));
    docker.rebuild();
    docker.shell_tab = true;
    docker.details_focused = false;
    if docker.shell(id).is_none() {
        let container = docker.containers.iter().find(|c| c.id == id).cloned().unwrap_or_else(|| Container {
            id: id.to_owned(),
            name: name.to_owned(),
            ..placeholder()
        });
        let side = docker.side.unwrap_or(state.active).other();
        let area = shell_rect(state.layout(now).panels[side.index()]);
        let size = GridSize { cols: area.width.max(MIN_SHELL.0), rows: area.height.max(MIN_SHELL.1) };
        effects.extend(start_shell(state, &container, size, None, now));
    }
    effects
}

/// Whether a container's shell is running a command, so nothing should be typed into it.
pub fn container_shell_busy(state: &AppState, id: &str) -> bool {
    if state.console == Console::Container(id.to_owned()) {
        return state.shell.busy();
    }
    state.docker.shell(id).is_some_and(|shell| shell.status.running)
}

/// Keys while typing in a shell: Alt-C returns to the list, Alt-W maximizes, Ctrl-O shows
/// the host console, and everything else goes to the shell.
fn shell_key(state: &mut AppState, key: &KeyEvent, now: Instant) -> Vec<Effect> {
    match (key.modifiers, key.code) {
        (KeyModifiers::ALT, KeyCode::Char('c')) => state.docker.details_focused = false,
        (KeyModifiers::ALT, KeyCode::Char('w')) => state.docker.expanded = !state.docker.expanded,
        (KeyModifiers::CONTROL, KeyCode::Char('o')) => {
            return crate::actions::perform(state, crate::Action::ToggleShell, now);
        }
        _ => {
            let Some(shell) = state.docker.visible_shell() else { return Vec::new() };
            let container = shell.container.clone();
            let Some(bytes) = encode_key(key, shell.status.observed.mode) else { return Vec::new() };
            return vec![Effect::DockerShell(ShellRequest::Input { container, bytes })];
        }
    }
    Vec::new()
}

/// Scrolls the visible shell's history with the wheel.
pub(crate) fn scroll_shell(state: &AppState, scroll: Scroll) -> Vec<Effect> {
    state
        .docker
        .visible_shell()
        .map(|shell| vec![Effect::DockerShell(ShellRequest::Scroll { container: shell.container.clone(), scroll })])
        .unwrap_or_default()
}

/// The container a panel shows, if it shows one's filesystem.
pub(crate) fn mounted(state: &AppState, side: Side) -> Option<&Mount> {
    state.panel(side).mount.as_ref()
}

fn file_work(state: &mut AppState, side: Side, container: String, op: FileOp) -> Vec<Effect> {
    let docker = &mut state.docker;
    docker.next_id += 1;
    let id = docker.next_id;
    docker.files.push(FileWork { id, side, op: op.clone() });
    vec![Effect::Docker { id, request: DockerRequest::Files { container, op } }]
}

/// Shows the selected container's filesystem in the Docker list's panel, starting in its
/// working directory, and closes the Docker view. The other panel keeps its host folder,
/// so files copy between the two.
pub(crate) fn browse(state: &mut AppState) -> Vec<Effect> {
    let Some(container) = state.docker.container().cloned() else {
        state.notice = Some(tr!(docker_choose_container).into());
        return Vec::new();
    };
    if container.state != State::Running {
        state.notice = Some(tr!(docker_start_to_browse).into());
        return Vec::new();
    }
    let workdir = state.docker.selected_details().map(|d| d.workdir.clone()).filter(|dir| dir.starts_with('/'));
    let side = state.docker.side.unwrap_or(state.active);
    close(state);
    state.aside = None;
    state.active = side;
    let panel = state.panel(side);
    let host = panel.mount.as_ref().map_or_else(|| panel.dir().to_path_buf(), |mount| mount.host.clone());
    state.panel_mut(side).mount = Some(Mount { id: container.id, name: container.name, host });
    vec![state.navigate(side, PathBuf::from(workdir.unwrap_or_else(|| "/".into())), None)]
}

/// Leaves a container's filesystem for the host folder the panel showed before.
pub(crate) fn unmount(state: &mut AppState, side: Side) -> Option<Effect> {
    let mount = state.panel_mut(side).mount.take()?;
    Some(state.navigate(side, mount.host, None))
}

/// Copies the selection between a container panel and a host panel as a copy job, which asks
/// before replacing what exists; `target` is a folder in the other panel, absolute or
/// relative to its folder.
pub(crate) fn copy(state: &mut AppState, sources: Vec<PathBuf>, target: &str, now: Instant) -> Vec<Effect> {
    let (from, to) = (state.active, state.active.other());
    let target = state.panel(to).dir().join(target);
    let (container, inbound) = match (mounted(state, from), mounted(state, to)) {
        (Some(mount), None) => (mount.id.clone(), false),
        (None, Some(mount)) => (mount.id.clone(), true),
        _ => {
            state.notice = Some(tr!(docker_copy_between_containers).into());
            return Vec::new();
        }
    };
    let (id, mut effects) = crate::jobs::begin(state, crate::JobKind::Copy, now);
    effects.push(Effect::StartDockerCopy { id, container, inbound, sources, target });
    effects
}

pub(crate) fn delete(state: &mut AppState, paths: Vec<PathBuf>) -> Vec<Effect> {
    let side = state.active;
    let Some(mount) = mounted(state, side).cloned() else { return Vec::new() };
    file_work(state, side, mount.id, FileOp::Delete { paths })
}

pub(crate) fn mkdir(state: &mut AppState, name: &str) -> Vec<Effect> {
    let side = state.active;
    let Some(mount) = mounted(state, side).cloned() else { return Vec::new() };
    let path = state.panel(side).dir().join(name);
    file_work(state, side, mount.id, FileOp::Mkdir { path })
}

/// Opens a container file in the viewer, once copied out.
pub(crate) fn view(state: &mut AppState, path: &Path) -> Vec<Effect> {
    let side = state.active;
    let Some(mount) = mounted(state, side).cloned() else { return Vec::new() };
    file_work(state, side, mount.id, FileOp::Fetch { path: path.to_path_buf() })
}

pub(crate) fn stats(state: &mut AppState, frame: Vec<(String, Stats)>) {
    if state.docker.watching {
        state.docker.sample(frame);
    }
}

/// The containers an action changes: the selected one, or every one in the selected project
/// that the action applies to.
fn targets(docker: &Docker, action: Action) -> Vec<String> {
    let busy = |c: &Container| docker.working_on(&c.id).is_some();
    match docker.row() {
        Some(DockerRow::Container(index)) => {
            let container = &docker.containers[index];
            if action.applies_to(container.state) && !busy(container) { vec![container.id.clone()] } else { Vec::new() }
        }
        Some(DockerRow::Group(index)) => docker.groups[index]
            .containers
            .iter()
            .map(|&i| &docker.containers[i])
            .filter(|c| action.applies_to(c.state) && !busy(c))
            .map(|c| c.id.clone())
            .collect(),
        None => Vec::new(),
    }
}

fn subject(docker: &Docker) -> String {
    match docker.row() {
        Some(DockerRow::Container(index)) => docker.containers[index].label().to_owned(),
        Some(DockerRow::Group(index)) => {
            docker.groups[index].project.clone().unwrap_or_else(|| tr!(docker_standalone).to_owned())
        }
        None => String::new(),
    }
}

/// Runs an action on the selection. Stopping or restarting what is running asks first.
pub(crate) fn act(state: &mut AppState, action: Action) -> Vec<Effect> {
    if !state.docker.is_open() {
        return Vec::new();
    }
    let docker = &state.docker;
    let ids = targets(docker, action);
    if ids.is_empty() {
        state.notice = Some(tr!(docker_nothing_to_do).into());
        return Vec::new();
    }
    if matches!(action, Action::Stop | Action::Restart) {
        let what = subject(docker);
        let message = match (action, ids.len()) {
            (Action::Stop, 1) => tr!(docker_confirm_stop_one, name = what),
            (Action::Stop, count) => tr!(docker_confirm_stop_many, name = what, count = count),
            (_, 1) => tr!(docker_confirm_restart_one, name = what),
            (_, count) => tr!(docker_confirm_restart_many, name = what, count = count),
        };
        state.overlay = Some(Overlay::Confirm(Confirm {
            title: if action == Action::Stop { tr!(docker_stop_title) } else { tr!(docker_restart_title) },
            message,
            purpose: ConfirmPurpose::Docker { action, ids },
            accept: true,
        }));
        return Vec::new();
    }
    start(state, action, ids)
}

pub(crate) fn start(state: &mut AppState, action: Action, ids: Vec<String>) -> Vec<Effect> {
    if !state.docker.is_open() {
        return Vec::new();
    }
    state.docker.request(DockerRequest::Act { action, ids })
}

fn pause_toggle(docker: &Docker) -> Action {
    let paused = match docker.row() {
        Some(DockerRow::Container(index)) => docker.containers[index].state == State::Paused,
        Some(DockerRow::Group(index)) => {
            docker.groups[index].containers.iter().any(|&i| docker.containers[i].state == State::Paused)
        }
        None => false,
    };
    if paused { Action::Unpause } else { Action::Pause }
}

/// Folds or unfolds the selected project; on a container, Left selects its project.
fn fold(state: &mut AppState, open: bool, now: Instant) {
    let docker = &mut state.docker;
    match docker.row() {
        Some(DockerRow::Group(index)) => {
            let project = docker.groups[index].project.clone();
            let collapsed = docker.collapsed.contains(&project);
            if open && !collapsed {
                docker.move_by(1, now);
            } else if open {
                docker.collapsed.remove(&project);
            } else {
                docker.collapsed.insert(project);
            }
        }
        Some(DockerRow::Container(_)) if !open => {
            if let Some(group) = docker.rows[..docker.cursor].iter().rposition(|r| matches!(r, DockerRow::Group(_))) {
                docker.select(group, now);
            }
        }
        _ => {}
    }
    docker.rebuild();
}

fn filter_key(state: &mut AppState, key: &KeyEvent, now: Instant) -> bool {
    let docker = &mut state.docker;
    match key.code {
        KeyCode::Char(c) if !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => {
            if docker.filter.chars().count() < FILTER_LIMIT {
                docker.filter.push(c);
            }
        }
        KeyCode::Backspace => {
            docker.filter.pop();
        }
        KeyCode::Enter => docker.filtering = false,
        KeyCode::Esc => {
            docker.filtering = false;
            docker.filter.clear();
        }
        KeyCode::Up | KeyCode::Down => {
            docker.move_by(if key.code == KeyCode::Up { -1 } else { 1 }, now);
            return true;
        }
        _ => return false,
    }
    docker.rebuild();
    docker.inspect_due = docker.container().map(|_| now + INSPECT_DELAY);
    true
}

pub(crate) fn paste(state: &mut AppState, text: &str) -> Vec<Effect> {
    if let Some(shell) = state.docker.visible_shell().filter(|_| state.docker.typing()) {
        let bytes = encode_paste(text, shell.status.observed.mode);
        return vec![Effect::DockerShell(ShellRequest::Input { container: shell.container.clone(), bytes })];
    }
    if state.docker.filtering {
        state.docker.filter.extend(text.chars().filter(|c| !c.is_control()).take(FILTER_LIMIT));
        state.docker.rebuild();
    }
    Vec::new()
}

/// Alt-1 and Alt-2 pick the details panel's tab, also while typing in a shell.
pub(crate) fn tab_key(key: &KeyEvent) -> Option<char> {
    match key.code {
        KeyCode::Char(tab @ ('1' | '2')) if key.modifiers == KeyModifiers::ALT => Some(tab),
        _ => None,
    }
}

/// The overview returns focus to the list; the shell tab opens or shows the shell.
fn select_tab(state: &mut AppState, tab: char, now: Instant) -> Vec<Effect> {
    if tab == '2' {
        return open_shell(state, now);
    }
    state.docker.shell_tab = false;
    state.docker.details_focused = false;
    Vec::new()
}

pub(crate) fn route_key(state: &mut AppState, key: &KeyEvent, now: Instant) -> Vec<Effect> {
    if let Some(tab) = tab_key(key) {
        return select_tab(state, tab, now);
    }
    if state.docker.typing() {
        return shell_key(state, key, now);
    }
    if key.modifiers == KeyModifiers::ALT.union(KeyModifiers::SHIFT)
        && matches!(key.code, KeyCode::Left | KeyCode::Right)
    {
        let step = if key.code == KeyCode::Right { crate::keymap::SPLIT_STEP } else { -crate::keymap::SPLIT_STEP };
        return crate::actions::perform(state, crate::Action::NudgeSplit(step), now);
    }
    if key.modifiers == KeyModifiers::ALT {
        match key.code {
            KeyCode::Char('w') => state.docker.expanded = !state.docker.expanded,
            KeyCode::Char('c') => close(state),
            KeyCode::Char('m') => return crate::actions::perform(state, crate::Action::Activity, now),
            _ => {}
        }
        return Vec::new();
    }
    if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('o') {
        return crate::actions::perform(state, crate::Action::ToggleShell, now);
    }
    match key.code {
        KeyCode::F(1) => return crate::actions::perform(state, crate::Action::About, now),
        KeyCode::F(9) => return crate::actions::perform(state, crate::Action::OpenMenuBar, now),
        KeyCode::F(10) => return crate::actions::perform(state, crate::Action::Quit, now),
        _ => {}
    }
    if state.docker.filtering && filter_key(state, key, now) {
        return Vec::new();
    }
    let docker = &mut state.docker;
    match key.code {
        KeyCode::Up => docker.move_by(-1, now),
        KeyCode::Down => docker.move_by(1, now),
        KeyCode::PageUp => docker.move_by(-(PAGE as isize), now),
        KeyCode::PageDown => docker.move_by(PAGE as isize, now),
        KeyCode::Home => docker.select(0, now),
        KeyCode::End => docker.select(usize::MAX, now),
        KeyCode::Left => fold(state, false, now),
        KeyCode::Right => fold(state, true, now),
        KeyCode::Enter => {
            if matches!(docker.row(), Some(DockerRow::Group(_))) {
                let open = docker.group().is_some_and(|g| docker.collapsed(g));
                fold(state, open, now);
            } else if docker.container().is_some() {
                docker.details_focused = !docker.details_focused;
            }
        }
        KeyCode::Tab => docker.details_focused = !docker.details_focused,
        KeyCode::Char('/') => {
            docker.filtering = true;
            docker.details_focused = false;
        }
        KeyCode::Esc => {
            if docker.details_focused {
                docker.details_focused = false;
            } else if !docker.filter.is_empty() {
                docker.filter.clear();
                docker.rebuild();
            } else {
                close(state);
            }
        }
        KeyCode::Char('s' | 'S') => return act(state, Action::Start),
        KeyCode::Char('x' | 'X') => return act(state, Action::Stop),
        KeyCode::Char('r' | 'R') => return act(state, Action::Restart),
        KeyCode::Char('p' | 'P') => {
            let action = pause_toggle(&state.docker);
            return act(state, action);
        }
        KeyCode::Char('f' | 'F') => return browse(state),
        KeyCode::Char('e' | 'E') => return open_shell(state, now),
        KeyCode::Char(tab @ ('1' | '2')) => return select_tab(state, tab, now),
        _ => {}
    }
    Vec::new()
}

/// A click on a row selects it; a double click acts like Enter.
pub(crate) fn click(state: &mut AppState, index: usize, open: bool, now: Instant) -> Vec<Effect> {
    if index >= state.docker.rows.len() {
        return Vec::new();
    }
    state.docker.details_focused = false;
    state.docker.filtering = false;
    state.docker.select(index, now);
    if open { route_key(state, &KeyCode::Enter.into(), now) } else { Vec::new() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn container(id: &str, project: Option<&str>, service: &str, state: State) -> Container {
        Container {
            id: id.into(),
            name: project.map_or_else(|| service.to_owned(), |p| format!("{p}-{service}-1")),
            image: format!("{service}:latest"),
            command: String::new(),
            created: String::new(),
            state,
            status: String::new(),
            ports: String::new(),
            networks: String::new(),
            project: project.map(str::to_owned),
            service: project.map(|_| service.to_owned()),
            project_dir: None,
            config_files: Vec::new(),
        }
    }

    fn docker() -> Docker {
        let mut docker = Docker {
            containers: vec![
                container("c1", None, "scratch", State::Exited),
                container("w1", Some("web"), "worker", State::Running),
                container("a1", Some("api"), "db", State::Running),
                container("w2", Some("web"), "app", State::Exited),
            ],
            ..Docker::default()
        };
        docker.rebuild();
        docker
    }

    #[test]
    fn projects_sort_by_name_with_other_containers_last() {
        let docker = docker();
        let labels: Vec<String> = docker
            .rows
            .iter()
            .map(|row| match *row {
                DockerRow::Group(i) => docker.groups[i].project.clone().unwrap_or_else(|| "-".into()),
                DockerRow::Container(i) => format!("  {}", docker.containers[i].label()),
            })
            .collect();
        assert_eq!(labels, ["api", "  db", "web", "  app", "  worker", "-", "  scratch"]);
        assert_eq!(docker.groups[1].running, 1);
    }

    #[test]
    fn the_selection_survives_refreshes_and_folding() {
        let now = Instant::now();
        let mut docker = docker();
        docker.select(4, now);
        assert_eq!(docker.container().map(|c| c.id.as_str()), Some("w1"));
        docker.containers.remove(0);
        docker.rebuild();
        assert_eq!(docker.container().map(|c| c.id.as_str()), Some("w1"), "rows above changed");
        docker.collapsed.insert(Some("web".into()));
        docker.rebuild();
        assert_eq!(docker.row(), Some(DockerRow::Group(1)), "a folded project keeps the cursor on itself");
        docker.filter = "work".into();
        docker.rebuild();
        assert!(docker.rows.contains(&DockerRow::Container(0)), "filtering unfolds matches");
        assert_eq!(docker.rows.len(), 2);
    }

    #[test]
    fn project_actions_target_only_the_containers_they_change() {
        let now = Instant::now();
        let mut docker = docker();
        docker.select(2, now);
        assert_eq!(targets(&docker, Action::Start), ["w2"]);
        assert_eq!(targets(&docker, Action::Stop), ["w1"]);
        docker.working.push(Working { id: 9, action: Action::Stop, ids: vec!["w1".into()] });
        assert!(targets(&docker, Action::Stop).is_empty(), "a container already stopping is left alone");
        docker.select(6, now);
        assert!(targets(&docker, Action::Restart).is_empty());
        assert_eq!(pause_toggle(&docker), Action::Pause);
    }

    #[test]
    fn stats_keep_a_minute_of_history() {
        let mut docker = docker();
        for second in 0..(HISTORY as u32 + 5) {
            docker.sample(vec![("w1".into(), Stats { cpu: second, memory: 1024, ..Stats::default() })]);
        }
        let usage = &docker.usage["w1"];
        assert_eq!(usage.cpu.len(), HISTORY);
        assert_eq!(usage.latest.cpu, HISTORY as u32 + 4);
        docker.select(4, Instant::now());
        assert_eq!(docker.group_usage(docker.group().unwrap()), (u64::from(HISTORY as u32 + 4), 1024));
    }
}
