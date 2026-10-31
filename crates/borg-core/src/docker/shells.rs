//! Shells in containers: opening one, the console it runs in, and the keys it takes.

use super::{ContainerShell, Key, ShellRequest, open};
use crate::{AppState, Console, Effect, ShellStatus, View};
use borg_docker::{Container, State};
use borg_i18n::tr;
use borg_term::{GridSize, Scroll, TermMode, encode_key};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

/// The rows of the details panel below its status and tabs, where a shell runs.
pub fn shell_rect(panel: Rect) -> Rect {
    pub(super) const HEADER: u16 = 2;
    let inner = Rect {
        x: panel.x + 1,
        y: panel.y + 1,
        width: panel.width.saturating_sub(2),
        height: panel.height.saturating_sub(2),
    };
    Rect { y: inner.y + HEADER.min(inner.height), height: inner.height.saturating_sub(HEADER), ..inner }
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

pub(super) fn start_shell(
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
pub(super) const FAILED_START: Duration = Duration::from_secs(3);

/// Whose shell the console should be: a container's while its filesystem panel is active and
/// the Docker command line is on, otherwise the host's. The Docker view keeps the host's.
pub(super) fn wanted_console(state: &AppState) -> Console {
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
pub(super) fn placeholder() -> Container {
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
pub(super) const MIN_SHELL: (u16, u16) = (20, 5);

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
pub(super) fn shell_key(state: &mut AppState, key: &KeyEvent, now: Instant) -> Vec<Effect> {
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
