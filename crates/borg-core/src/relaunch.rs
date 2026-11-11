//! Moving the session to a newer Borg that an update installed. Once nothing here would be
//! lost, the app starts the new program file in place and hands it the shell.

use borg_i18n::tr;

use crate::{AppState, Effect};

/// What keeps the session from moving now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Hold {
    Jobs,
    Unsaved,
    Agent,
}

pub(crate) fn hold(state: &AppState) -> Option<Hold> {
    let unsaved = state.editor.iter().chain(state.docked.iter().flatten()).any(|editor| editor.buffer.modified());
    if !state.jobs.is_empty() {
        Some(Hold::Jobs)
    } else if unsaved {
        Some(Hold::Unsaved)
    } else if state.approval.is_some() || state.agent.busy() {
        Some(Hold::Agent)
    } else {
        None
    }
}

/// Borg `version` is installed and runs in the service: the session moves to it now, or says
/// what it waits for.
pub(crate) fn requested(state: &mut AppState, version: String) -> Vec<Effect> {
    state.notice = hold(state).map(|hold| match hold {
        Hold::Jobs => tr!(relaunch_waits_jobs, version = version),
        Hold::Unsaved => tr!(relaunch_waits_unsaved, version = version),
        Hold::Agent => tr!(relaunch_waits_agent, version = version),
    });
    state.relaunch = Some(version);
    Vec::new()
}

/// The move, once one waits and nothing holds it.
pub(crate) fn sync(state: &mut AppState) -> Vec<Effect> {
    if hold(state).is_some() || state.relaunch.take().is_none() {
        return Vec::new();
    }
    vec![Effect::Relaunch]
}

#[cfg(test)]
mod tests;
