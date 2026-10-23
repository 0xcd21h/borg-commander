//! An AI agent connected through `borg mcp`: it sees what the user sees and can show things
//! in a panel, while every command it wants to run waits for the user's approval here.

use borg_i18n::tr;
use std::path::PathBuf;

use crate::{AppState, Confirm, ConfirmPurpose, Effect, Overlay, Side};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentEvent {
    /// How many agents are connected now, and where the log of what they did is.
    Connected {
        clients: usize,
        log: PathBuf,
    },
    /// An agent asks to run `command`; `place` says where, for the question.
    Ask {
        id: u64,
        command: String,
        place: String,
    },
    /// An approved command is about to run in a container's shell: the Docker view shows it.
    ShowShell {
        container: String,
        name: String,
    },
    /// An agent asks to show a folder in a panel, with `focus` selected in it.
    Reveal {
        dir: PathBuf,
        focus: Option<String>,
    },
    Finished {
        id: u64,
        command: String,
        exit: Option<i32>,
    },
}

#[derive(Debug, Clone, Default)]
pub struct Agent {
    pub clients: usize,
    pub log: Option<PathBuf>,
    /// The approved command running now.
    pub running: Option<(u64, String)>,
    /// Commands waiting for the user's answer, oldest first, with where they would run.
    asks: Vec<(u64, String, String)>,
}

impl Agent {
    pub fn connected(&self) -> bool {
        self.clients > 0
    }
}

pub(crate) fn receive(state: &mut AppState, event: AgentEvent) -> Vec<Effect> {
    match event {
        AgentEvent::Connected { clients, log } => {
            if clients > state.agent.clients {
                state.notice = Some(tr!(agent_connected).into());
            }
            state.agent.clients = clients;
            state.agent.log = Some(log);
        }
        AgentEvent::Ask { id, command, place } => state.agent.asks.push((id, command, place)),
        AgentEvent::ShowShell { container, name } => return crate::docker::show_shell(state, &container, &name),
        AgentEvent::Reveal { dir, focus } => return reveal(state, dir, focus),
        AgentEvent::Finished { id, command, exit } => {
            if state.agent.running.as_ref().is_some_and(|(running, _)| *running == id) {
                state.agent.running = None;
            }
            state.notice = Some(match exit {
                Some(code) => format!(tr!(agent_ran), command = command, code = code),
                None => format!(tr!(agent_stopped), command = command),
            });
            return state.reload_all();
        }
    }
    Vec::new()
}

/// Asks about the oldest waiting command once nothing else is on screen.
pub(crate) fn surface(state: &mut AppState) {
    if state.overlay.is_some() || state.agent.asks.is_empty() {
        return;
    }
    let (id, command, place) = state.agent.asks.remove(0);
    state.overlay = Some(Overlay::Confirm(Confirm {
        title: tr!(agent_title),
        message: format!(tr!(agent_asks_to_run), command = command, place = place),
        purpose: ConfirmPurpose::Agent { id, command },
        accept: false,
    }));
}

pub(crate) fn answer(state: &mut AppState, id: u64, command: String, approved: bool) -> Vec<Effect> {
    if approved {
        state.agent.running = Some((id, command));
    }
    vec![Effect::AgentAnswer { id, approved }]
}

/// Shows a folder in the inactive panel, so the user's own panel stays where it is.
fn reveal(state: &mut AppState, dir: PathBuf, focus: Option<String>) -> Vec<Effect> {
    let side: Side = state.active.other();
    state.aside = None;
    state.panel_mut(side).mount = None;
    vec![state.navigate(side, dir, focus)]
}
