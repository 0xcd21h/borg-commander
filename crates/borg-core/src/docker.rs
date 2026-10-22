//! The Docker view: Compose projects with their containers in one panel, and the selected
//! project or container with its live resource use in the other. Ctrl-O stays the host
//! console.

use borg_i18n::tr;
use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};

use borg_docker::{Action, Container, Details, State, Stats};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{AppState, Confirm, ConfirmPurpose, Effect, HISTORY, Overlay, Side, View};

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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DockerData {
    Listed { context: Option<String>, containers: Vec<Container> },
    Inspected { id: String, details: Box<Details> },
    Done,
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
        if found.is_none() {
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

pub(crate) fn receive(state: &mut AppState, id: u64, result: Result<DockerData, String>, now: Instant) {
    let docker = &mut state.docker;
    if let Some(index) = docker.working.iter().position(|w| w.id == id) {
        let work = docker.working.remove(index);
        if let Err(error) = result {
            state.notice = Some(format!(tr!(docker_action_failed), error = error));
        } else if work.action == Action::Stop || work.action == Action::Restart || work.action == Action::Start {
            docker.inspect_due = Some(now);
        }
        docker.next = Some(now);
        return;
    }
    if docker.pending.as_ref().is_none_or(|(pending, _)| *pending != id) {
        return;
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
        Ok(DockerData::Done) => {}
        Err(error) => {
            docker.loaded = true;
            docker.error = Some(error);
            docker.next = Some(now + RETRY);
        }
    }
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
    let docker = &state.docker;
    let ids = targets(docker, action);
    if ids.is_empty() {
        state.notice = Some(tr!(docker_nothing_to_do).into());
        return Vec::new();
    }
    if matches!(action, Action::Stop | Action::Restart) {
        let what = subject(docker);
        let message = match (action, ids.len()) {
            (Action::Stop, 1) => format!(tr!(docker_confirm_stop_one), name = what),
            (Action::Stop, count) => format!(tr!(docker_confirm_stop_many), name = what, count = count),
            (_, 1) => format!(tr!(docker_confirm_restart_one), name = what),
            (_, count) => format!(tr!(docker_confirm_restart_many), name = what, count = count),
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

pub(crate) fn paste(state: &mut AppState, text: &str) {
    if state.docker.filtering {
        state.docker.filter.extend(text.chars().filter(|c| !c.is_control()).take(FILTER_LIMIT));
        state.docker.rebuild();
    }
}

pub(crate) fn route_key(state: &mut AppState, key: &KeyEvent, now: Instant) -> Vec<Effect> {
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
