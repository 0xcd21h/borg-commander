//! Preferences turned on and off from the menus and keys, and the folders indexed for search.

use std::time::Instant;

use borg_i18n::tr;

use crate::{AfterCommand, AppState, Effect, Preferences};

/// Turns a preference on or off and saves it.
pub(super) fn flip(state: &mut AppState, flag: impl FnOnce(&mut Preferences) -> &mut bool) -> Vec<Effect> {
    let flag = flag(&mut state.settings.preferences);
    *flag = !*flag;
    vec![Effect::SaveConfig]
}

/// Changes a preference that changes the screen's layout, and saves it.
pub(super) fn preference(state: &mut AppState, change: impl FnOnce(&mut Preferences)) -> Vec<Effect> {
    change(&mut state.settings.preferences);
    state.refit();
    vec![Effect::SaveConfig]
}

/// Turns the agent's log on or off; turned off, it forgets what it held.
pub(super) fn toggle_agent_log(state: &mut AppState) -> Vec<Effect> {
    let effects = flip(state, |prefs| &mut prefs.agent_log);
    if !state.settings.preferences.agent_log {
        state.agent.log.clear();
    }
    effects
}

pub(super) fn toggle_hidden(state: &mut AppState) -> Vec<Effect> {
    let prefs = &mut state.settings.preferences;
    prefs.show_hidden = !prefs.show_hidden;
    let mut effects = state.reload_all();
    effects.push(Effect::SaveConfig);
    effects
}

/// Turns the screensaver on or off, counting the idle time from now.
pub(super) fn toggle_screensaver(state: &mut AppState, now: Instant) -> Vec<Effect> {
    state.idle_since = now;
    preference(state, |prefs| prefs.screensaver = !prefs.screensaver)
}

/// Whether a command typed in the panels leaves the shell showing or comes back to the panels.
pub(super) fn toggle_after_command(state: &mut AppState) -> Vec<Effect> {
    preference(state, |prefs| {
        prefs.after_command = match prefs.after_command {
            AfterCommand::ShowShell => AfterCommand::ReturnToPanels,
            AfterCommand::ReturnToPanels => AfterCommand::ShowShell,
        }
    })
}

/// Adds the active folder to the folders indexed for instant search, or removes it.
pub(super) fn toggle_index(state: &mut AppState) -> Vec<Effect> {
    let panel = state.active_panel();
    if panel.is_results() || panel.mount.is_some() || panel.remote_mount.is_some() {
        return Vec::new();
    }
    let dir = panel.dir().to_path_buf();
    let folder = dir.display().to_string();
    let roots = &mut state.settings.index_roots;
    if let Some(at) = roots.iter().position(|root| *root == dir) {
        roots.remove(at);
        state.indexing = None;
        state.notice = Some(tr!(index_removed, folder = folder));
    } else {
        roots.push(dir.clone());
        state.indexing = Some(dir);
        state.notice = Some(tr!(index_started, folder = folder));
    }
    vec![Effect::SetIndexRoots(state.settings.index_roots.clone())]
}
