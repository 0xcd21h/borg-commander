//! The container list's keys and clicks, and the actions they take on containers.

use super::{
    Docker, DockerRequest, DockerRow, FILTER_LIMIT, INSPECT_DELAY, PAGE, ShellRequest, browse, close, open_shell,
    shell_key,
};
use crate::{AppState, Confirm, ConfirmPurpose, Effect, Overlay};
use borg_docker::{Action, Container, State, Stats};
use borg_i18n::tr;
use borg_term::encode_paste;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::time::Instant;

pub(crate) fn stats(state: &mut AppState, frame: Vec<(String, Stats)>) {
    if state.docker.watching {
        state.docker.sample(frame);
    }
}

/// The containers an action changes: the selected one, or every one in the selected project
/// that the action applies to.
pub(super) fn targets(docker: &Docker, action: Action) -> Vec<String> {
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

pub(super) fn subject(docker: &Docker) -> String {
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

pub(super) fn pause_toggle(docker: &Docker) -> Action {
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
pub(super) fn fold(state: &mut AppState, open: bool, now: Instant) {
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

pub(super) fn filter_key(state: &mut AppState, key: &KeyEvent, now: Instant) -> bool {
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
pub(super) fn select_tab(state: &mut AppState, tab: char, now: Instant) -> Vec<Effect> {
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
