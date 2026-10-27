use borg_i18n::tr;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

use crossterm::event::{KeyCode, KeyEvent};

use crate::{AppState, Effect, Overlay, RemoteMount, Side};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RemoteHostInfo {
    pub version: Option<String>,
    pub platform: Option<String>,
    pub service: Option<bool>,
    pub latency_ms: Option<u64>,
    pub relay: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RemoteChoice {
    pub identity: String,
    pub host_label: Option<String>,
    pub session_label: Option<String>,
    pub machine: String,
    pub session: Option<String>,
    pub name: String,
    pub info: RemoteHostInfo,
}

impl RemoteChoice {
    pub fn host_name(&self) -> &str {
        self.host_label.as_deref().unwrap_or(&self.machine)
    }
    pub fn session_name(&self) -> &str {
        self.session_label.as_deref().unwrap_or(&self.name)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteNames(pub BTreeMap<String, RemoteHostNames>);

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteHostNames {
    pub host: Option<String>,
    #[serde(default)]
    pub sessions: BTreeMap<String, String>,
}

impl RemoteNames {
    pub fn validate(name: &str) -> Result<(), String> {
        if name.chars().count() > 40 || name.chars().any(char::is_control) {
            return Err(tr!(remote_name_invalid).into());
        }
        Ok(())
    }

    pub fn set(&mut self, identity: &str, session: Option<&str>, name: &str) -> Result<(), String> {
        Self::validate(name)?;
        let name = name.trim();
        let labels = self.0.entry(identity.into()).or_default();
        match session {
            None => labels.host = (!name.is_empty()).then(|| name.into()),
            Some(session) => {
                if name.is_empty() {
                    labels.sessions.remove(session);
                } else {
                    labels.sessions.insert(session.into(), name.into());
                }
            }
        }
        if labels.host.is_none() && labels.sessions.is_empty() {
            self.0.remove(identity);
        }
        Ok(())
    }

    pub fn apply(&self, choice: &mut RemoteChoice) {
        let labels = self.0.get(&choice.identity);
        choice.host_label = labels.and_then(|labels| labels.host.clone());
        choice.session_label =
            labels.and_then(|labels| choice.session.as_ref().and_then(|id| labels.sessions.get(id))).cloned();
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteClient {
    pub name: String,
    pub fingerprint: String,
    pub connections: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteBrowser {
    pub side: Side,
    pub choices: Vec<RemoteChoice>,
    pub selected: usize,
    pub loading: bool,
    pub connecting: bool,
    pub error: Option<String>,
    pub clients: Option<Vec<RemoteClient>>,
    pub show_clients: bool,
    pub client_selected: usize,
    pub deleting: Option<(String, String)>,
}

pub(crate) fn on_key(state: &mut AppState, mut browser: RemoteBrowser, key: &KeyEvent) -> Vec<Effect> {
    if browser.deleting.is_some() {
        if key.code == KeyCode::Esc {
            state.overlay = None;
        }
        return Vec::new();
    }
    if browser.connecting {
        if key.code == KeyCode::Esc {
            browser.connecting = false;
            state.overlay = Some(Overlay::Remotes(browser));
            return vec![Effect::CancelRemoteConnection];
        }
        return Vec::new();
    }
    if matches!(key.code, KeyCode::Tab | KeyCode::BackTab) {
        browser.show_clients = !browser.show_clients;
        state.overlay = Some(Overlay::Remotes(browser));
        return Vec::new();
    }
    if browser.show_clients {
        let last = browser.clients.as_ref().map_or(0, |clients| clients.len().saturating_sub(1));
        match key.code {
            KeyCode::Esc => state.overlay = None,
            KeyCode::Up => {
                browser.client_selected = browser.client_selected.saturating_sub(1);
                state.overlay = Some(Overlay::Remotes(browser));
            }
            KeyCode::Down => {
                browser.client_selected = (browser.client_selected + 1).min(last);
                state.overlay = Some(Overlay::Remotes(browser));
            }
            KeyCode::Home => {
                browser.client_selected = 0;
                state.overlay = Some(Overlay::Remotes(browser));
            }
            KeyCode::End => {
                browser.client_selected = last;
                state.overlay = Some(Overlay::Remotes(browser));
            }
            KeyCode::Char('r') | KeyCode::Char('R') => {
                browser.loading = true;
                state.overlay = Some(Overlay::Remotes(browser));
                return vec![Effect::OpenRemotes];
            }
            _ => {}
        }
        return Vec::new();
    }
    let last = browser.choices.len().saturating_sub(1);
    if matches!(key.code, KeyCode::Up | KeyCode::Down | KeyCode::Home | KeyCode::End) {
        browser.error = None;
    }
    match key.code {
        KeyCode::Delete | KeyCode::F(8) => {
            let Some(choice) = browser.choices.get(browser.selected) else { return Vec::new() };
            let Some(session) = choice.session.clone() else { return Vec::new() };
            if session == "default" {
                browser.error = Some(tr!(remote_session_default_protected).into());
                state.overlay = Some(Overlay::Remotes(browser));
                return Vec::new();
            }
            state.overlay = Some(Overlay::Confirm(crate::Confirm {
                title: tr!(remote_session_delete_title),
                message: format!(
                    tr!(remote_session_delete_confirm),
                    session = choice.session_name(),
                    machine = choice.host_name()
                ),
                purpose: crate::ConfirmPurpose::RemoteSessionDelete {
                    machine: choice.machine.clone(),
                    session,
                    browser: Box::new(browser),
                },
                accept: false,
            }));
        }
        KeyCode::F(2) => {
            let Some(choice) = browser.choices.get(browser.selected) else { return Vec::new() };
            if choice.identity.is_empty() {
                return Vec::new();
            }
            let session = key.modifiers.contains(crossterm::event::KeyModifiers::SHIFT);
            if session && choice.session.is_none() {
                return Vec::new();
            }
            let value = if session { choice.session_name() } else { choice.host_name() };
            state.overlay = Some(Overlay::Prompt(crate::Prompt {
                title: if session { tr!(remote_rename_session) } else { tr!(remote_rename_host) },
                label: tr!(remote_name_label).into(),
                input: crate::LineInput::new(value),
                purpose: crate::PromptPurpose::RemoteName {
                    identity: choice.identity.clone(),
                    session: if session { choice.session.clone() } else { None },
                    original: if session { choice.name.clone() } else { choice.machine.clone() },
                    browser: Box::new(browser),
                },
                option_focused: false,
            }));
        }
        KeyCode::Esc => state.overlay = None,
        KeyCode::Up => {
            browser.selected = browser.selected.saturating_sub(1);
            state.overlay = Some(Overlay::Remotes(browser));
        }
        KeyCode::Down => {
            browser.selected = (browser.selected + 1).min(last);
            state.overlay = Some(Overlay::Remotes(browser));
        }
        KeyCode::Home => {
            browser.selected = 0;
            state.overlay = Some(Overlay::Remotes(browser));
        }
        KeyCode::End => {
            browser.selected = last;
            state.overlay = Some(Overlay::Remotes(browser));
        }
        KeyCode::Char('r') | KeyCode::Char('R') => {
            browser.loading = true;
            browser.error = None;
            state.overlay = Some(Overlay::Remotes(browser));
            return vec![Effect::OpenRemotes];
        }
        KeyCode::Enter | KeyCode::Char('m') | KeyCode::Char('M') => {
            let Some(choice) = browser.choices.get(browser.selected) else { return Vec::new() };
            let Some(session) = choice.session.clone() else { return Vec::new() };
            let (machine, name) = (choice.machine.clone(), choice.name.clone());
            if matches!(key.code, KeyCode::Enter) {
                state.overlay = None;
                return vec![Effect::MountRemote { side: browser.side, machine, session, name }];
            }
            state.overlay = Some(Overlay::Remotes(browser));
            return mirror(state, machine, session);
        }
        _ => state.overlay = Some(Overlay::Remotes(browser)),
    }
    Vec::new()
}

pub(crate) fn mirror(state: &mut AppState, machine: String, session: String) -> Vec<Effect> {
    if let Some(Overlay::Remotes(browser)) = state.overlay.as_mut() {
        browser.show_clients = false;
        browser.connecting = true;
        browser.error = None;
    } else {
        let name = state
            .panels
            .iter()
            .filter_map(|panel| panel.remote_mount.as_ref())
            .find(|mount| mount.machine == machine && mount.session == session)
            .map_or_else(|| session.clone(), |mount| mount.name.clone());
        let mut choice = RemoteChoice {
            identity: state.remote_identities.get(&machine).cloned().unwrap_or_default(),
            machine: machine.clone(),
            session: Some(session.clone()),
            name,
            ..Default::default()
        };
        state.remote_names.apply(&mut choice);
        state.overlay = Some(Overlay::Remotes(RemoteBrowser {
            side: state.active,
            choices: vec![choice],
            selected: 0,
            loading: false,
            connecting: true,
            error: None,
            clients: None,
            show_clients: false,
            client_selected: 0,
            deleting: None,
        }));
    }
    vec![Effect::MirrorRemote { machine, session }]
}

pub(crate) fn deleted(
    state: &mut AppState,
    machine: String,
    session: String,
    result: Result<(), String>,
) -> Vec<Effect> {
    let key = (machine.clone(), session.clone());
    let mut effects = Vec::new();
    if result.is_ok() {
        state.notice = Some(tr!(remote_session_deleted).into());
        for side in Side::ALL {
            if state
                .panel(side)
                .remote_mount
                .as_ref()
                .is_some_and(|mount| mount.machine == machine && mount.session == session)
                && let Some(effect) = unmount(state, side)
            {
                effects.push(effect);
            }
        }
        state.remote_consoles.remove(&key);
        if state.console == (crate::Console::Remote { machine: machine.clone(), session: session.clone() }) {
            state.shell = state.parked_host.take().unwrap_or_else(|| crate::ShellStatus::new(Default::default()));
            state.console = crate::Console::Host;
            state.view = crate::View::Panels;
            effects.push(Effect::UseConsole(crate::Console::Host));
        }
        effects.push(Effect::CloseRemoteConsole { machine: machine.clone(), session: session.clone() });
        if state.failed_remote_console.as_ref() == Some(&key) {
            state.failed_remote_console = None;
            state.remote_console_failure = None;
        }
    }
    if let Some(Overlay::Remotes(browser)) = state.overlay.as_mut() {
        if browser.deleting.as_ref() == Some(&key) {
            browser.deleting = None;
        }
        match result {
            Ok(()) => {
                browser
                    .choices
                    .retain(|choice| choice.machine != machine || choice.session.as_deref() != Some(&session));
                browser.selected = browser.selected.min(browser.choices.len().saturating_sub(1));
                browser.error = None;
                browser.loading = true;
                effects.push(Effect::OpenRemotes);
            }
            Err(error) => browser.error = Some(error),
        }
    } else {
        state.notice = Some(match result {
            Ok(()) => tr!(remote_session_deleted).into(),
            Err(error) => error,
        });
    }
    effects
}

pub(crate) fn connected(state: &mut AppState, result: Result<(), String>) {
    let Some(Overlay::Remotes(browser)) = state.overlay.as_mut() else { return };
    if !browser.connecting {
        return;
    }
    match result {
        Ok(()) => state.overlay = None,
        Err(error) => {
            browser.connecting = false;
            browser.error = Some(error);
        }
    }
}

/// Mount a paired session in one panel; the other remains on this computer.
pub fn mount(state: &mut AppState, side: Side, machine: String, session: String, name: String) -> Vec<Effect> {
    if state.failed_remote_console.as_ref() == Some(&(machine.clone(), session.clone())) {
        state.failed_remote_console = None;
        state.remote_console_failure = None;
    }
    let host = state
        .panel(side)
        .remote_mount
        .as_ref()
        .map_or_else(|| state.panel(side).dir().to_path_buf(), |old| old.host.clone());
    let old = state.panel(side);
    let mut panel = crate::Panel::new(PathBuf::from("/"), old.rows(), old.mode(), old.sort());
    panel.remote_mount = Some(RemoteMount { machine, session, name, host });
    *state.panel_mut(side) = panel;
    vec![state.navigate(side, PathBuf::from("/"), None)]
}

pub fn unmount(state: &mut AppState, side: Side) -> Option<Effect> {
    let mount = state.panel_mut(side).remote_mount.take()?;
    let old = state.panel(side);
    *state.panel_mut(side) = crate::Panel::new(mount.host.clone(), old.rows(), old.mode(), old.sort());
    Some(state.navigate(side, mount.host, None))
}

pub(crate) fn blocked_console_notice(state: &AppState) -> String {
    if let Some(mount) = &state.active_panel().remote_mount
        && state.failed_remote_console.as_ref() == Some(&(mount.machine.clone(), mount.session.clone()))
        && let Some(error) = &state.remote_console_failure
    {
        return error.clone();
    }
    borg_i18n::tr!(docker_command_line_host).into()
}

pub(crate) fn apply_names(state: &mut AppState) {
    let browser = match state.overlay.as_mut() {
        Some(Overlay::Remotes(browser)) => Some(browser),
        Some(Overlay::Prompt(crate::Prompt { purpose: crate::PromptPurpose::RemoteName { browser, .. }, .. })) => {
            Some(browser.as_mut())
        }
        _ => None,
    };
    if let Some(browser) = browser {
        for choice in &mut browser.choices {
            state.remote_names.apply(choice);
        }
    }
}

pub(crate) fn catalog(state: &mut AppState, mut choices: Vec<RemoteChoice>, complete: bool) {
    for choice in &mut choices {
        if !choice.identity.is_empty() {
            state.remote_identities.insert(choice.machine.clone(), choice.identity.clone());
        }
        state.remote_names.apply(choice);
    }
    let browser = match state.overlay.as_mut() {
        Some(Overlay::Remotes(browser)) => Some(browser),
        Some(Overlay::Prompt(crate::Prompt { purpose: crate::PromptPurpose::RemoteName { browser, .. }, .. })) => {
            Some(browser.as_mut())
        }
        _ => None,
    };
    if let Some(browser) = browser {
        if browser.loading
            && complete
            && choices.is_empty()
            && browser.clients.as_ref().is_some_and(|clients| !clients.is_empty())
        {
            browser.show_clients = true;
        }
        let selected =
            browser.choices.get(browser.selected).map(|choice| (choice.machine.clone(), choice.session.clone()));
        browser.choices = choices;
        browser.selected = selected
            .and_then(|(machine, session)| {
                browser.choices.iter().position(|choice| choice.machine == machine && choice.session == session)
            })
            .unwrap_or_else(|| browser.selected.min(browser.choices.len().saturating_sub(1)));
        browser.loading = !complete;
    }
}

pub(crate) fn clients(state: &mut AppState, clients: Vec<RemoteClient>) {
    state.agent.computers = clients.iter().map(|client| client.name.clone()).collect();
    let browser = match state.overlay.as_mut() {
        Some(Overlay::Remotes(browser)) => browser,
        Some(Overlay::Prompt(crate::Prompt { purpose: crate::PromptPurpose::RemoteName { browser, .. }, .. })) => {
            browser
        }
        _ => return,
    };
    if browser.clients.is_none() && !browser.loading && browser.choices.is_empty() && !clients.is_empty() {
        browser.show_clients = true;
    }
    let selected = browser
        .clients
        .as_ref()
        .and_then(|clients| clients.get(browser.client_selected))
        .map(|client| client.fingerprint.clone());
    browser.client_selected = selected
        .and_then(|selected| clients.iter().position(|client| client.fingerprint == selected))
        .unwrap_or_else(|| browser.client_selected.min(clients.len().saturating_sub(1)));
    browser.clients = Some(clients);
}
