use borg_i18n::tr;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use borg_git::{Repository, Tool};

use crate::{AppState, Effect, Side, View, Viewer};

const REFRESH: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Default)]
pub struct GitPanel {
    pub repository: Option<Repository>,
    pub error: Option<String>,
    key: Option<(u64, PathBuf)>,
    pending: bool,
    next: Option<Instant>,
}

#[derive(Debug, Clone, Default)]
pub struct Plugins {
    pub git_task: Option<(u64, Tool)>,
    next_task: u64,
}

fn can_refresh(state: &AppState) -> bool {
    state.settings.preferences.git_plugin
        && state.activity.side.is_none()
        && !state.docker.is_open()
        && state.view == View::Panels
        && state.viewer.is_none()
        && state.overlay.is_none()
        && !state.screensaver
        && state.plugins.git_task.is_none()
}

pub(crate) fn wake_at(state: &AppState) -> Option<Instant> {
    can_refresh(state).then(|| state.panels.iter().filter_map(|panel| panel.git.next).min()).flatten()
}

pub(crate) fn sync(state: &mut AppState, now: Instant) -> Vec<Effect> {
    if !state.settings.preferences.git_plugin {
        state.panels.iter_mut().for_each(|panel| panel.git = GitPanel::default());
        return Vec::new();
    }
    let refresh = can_refresh(state);
    let mut effects = Vec::new();
    for side in Side::ALL {
        let panel = state.panel_mut(side);
        if panel.is_results() || panel.mount.is_some() || panel.remote_mount.is_some() {
            panel.git = GitPanel::default();
            continue;
        }
        let key = (panel.generation(), panel.destination().to_path_buf());
        let changed = panel.git.key.as_ref() != Some(&key);
        let due = refresh && panel.git.next.is_some_and(|next| now >= next);
        if changed || (!panel.git.pending && due) {
            if changed {
                panel.git.repository = None;
                panel.git.error = None;
            }
            panel.git.key = Some(key.clone());
            panel.git.pending = true;
            panel.git.next = None;
            effects.push(Effect::GitScan { side, generation: key.0, dir: key.1 });
        }
    }
    effects
}

pub(crate) fn receive(
    state: &mut AppState,
    side: Side,
    generation: u64,
    dir: PathBuf,
    result: Result<Option<Repository>, String>,
    now: Instant,
) {
    if !state.settings.preferences.git_plugin {
        return;
    }
    let panel = state.panel_mut(side);
    if panel.generation() != generation
        || panel.destination() != dir
        || panel.git.key.as_ref() != Some(&(generation, dir))
    {
        return;
    }
    panel.git.pending = false;
    match result {
        Ok(repository) => {
            panel.git.next = repository.as_ref().map(|_| now + REFRESH);
            panel.git.repository = repository;
            panel.git.error = None;
        }
        Err(error) => {
            panel.git.repository = None;
            panel.git.error = Some(error);
        }
    }
}

pub(crate) fn run(state: &mut AppState, tool: Tool) -> Vec<Effect> {
    if !state.settings.preferences.git_plugin
        || state.plugins.git_task.is_some()
        || state.active_panel().git_repository().is_none()
    {
        return Vec::new();
    }
    let panel = state.active_panel();
    let paths = if tool.needs_selection() {
        panel.selection().into_iter().map(|entry| entry.path(panel.dir())).collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    if tool.needs_selection() && paths.is_empty() {
        return Vec::new();
    }
    let dir = panel.dir().to_path_buf();
    state.plugins.next_task += 1;
    let id = state.plugins.next_task;
    state.plugins.git_task = Some((id, tool));
    state.notice = Some(tr!(git_running).into());
    vec![Effect::GitTool { id, tool, dir, paths }]
}

pub(crate) fn finish(state: &mut AppState, id: u64, result: Result<Option<Viewer>, String>) -> Vec<Effect> {
    let Some((current, tool)) = state.plugins.git_task else { return Vec::new() };
    if id != current {
        return Vec::new();
    }
    state.plugins.git_task = None;
    state.notice = None;
    match result {
        Ok(Some(viewer)) if state.settings.preferences.git_plugin => {
            state.viewer = Some(viewer);
            crate::viewer::resize(state);
        }
        Ok(None) => state.notice = Some(tr!(git_index_updated).into()),
        Err(error) => state.notice = Some(format!(tr!(git_error), error = error)),
        _ => {}
    }
    if tool.changes_index() { state.reload_all() } else { Vec::new() }
}
