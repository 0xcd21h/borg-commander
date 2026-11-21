//! The hosts screen's keys and clicks: moving among the cards, the filter, and what a card
//! offers: the host's files, a mirror of its session, an update and a new name, through the same
//! steps as the remote hosts list.

use std::time::Instant;

use borg_i18n::tr;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{FILTER_LIMIT, HostCard, Hosts, HostsLayout, PAGE, close};
use crate::{Action, AppState, Effect, Overlay, RemoteBrowser};

pub(crate) fn route_key(state: &mut AppState, key: &KeyEvent, now: Instant) -> Vec<Effect> {
    let (code, modifiers) = (key.code, key.modifiers);
    if modifiers == KeyModifiers::ALT.union(KeyModifiers::SHIFT) && matches!(code, KeyCode::Left | KeyCode::Right) {
        let step = if code == KeyCode::Right { crate::keymap::SPLIT_STEP } else { -crate::keymap::SPLIT_STEP };
        return crate::actions::perform(state, Action::NudgeSplit(step), now);
    }
    if modifiers == KeyModifiers::ALT {
        return match code {
            KeyCode::Char('w') => {
                state.hosts.expanded = !state.hosts.expanded;
                Vec::new()
            }
            KeyCode::Char('h') => crate::actions::perform(state, Action::BrowseRemotes, now),
            KeyCode::Char('m') => crate::actions::perform(state, Action::Activity, now),
            KeyCode::Char('n') => crate::actions::perform(state, Action::Network, now),
            KeyCode::Char('c') => crate::actions::perform(state, Action::Docker, now),
            KeyCode::Char('l') => crate::actions::perform(state, Action::Alerts, now),
            KeyCode::Char('u') => crate::actions::perform(state, Action::StartupItems, now),
            _ => Vec::new(),
        };
    }
    if let Some(action) = crate::activity::panels_action(key) {
        return crate::actions::perform(state, action, now);
    }
    if state.hosts.filtering && filter_key(&mut state.hosts, key) {
        return Vec::new();
    }
    let key = &crate::keyboard::latin(key);
    if key.code == KeyCode::F(2) && modifiers.is_empty() {
        return with_choices(state, Pick::Rename);
    }
    if !(modifiers.is_empty() || modifiers == KeyModifiers::SHIFT) {
        return Vec::new();
    }
    let hosts = &mut state.hosts;
    match key.code {
        KeyCode::Esc => {
            close(state);
            return Vec::new();
        }
        KeyCode::Enter => return with_choices(state, Pick::Files),
        KeyCode::Char('m' | 'M') => return with_choices(state, Pick::Mirror),
        KeyCode::Char('u' | 'U') => return with_choices(state, Pick::Update),
        KeyCode::Char('r' | 'R') => return vec![Effect::RefreshHosts],
        KeyCode::Char('v' | 'V') => {
            hosts.layout = match hosts.layout {
                HostsLayout::Cards => HostsLayout::Rows,
                HostsLayout::Rows => HostsLayout::Cards,
            };
        }
        KeyCode::Char('/') => hosts.filtering = true,
        code => move_cursor(hosts, code),
    }
    Vec::new()
}

/// Pastes into the filter while it is typed.
pub(crate) fn paste(state: &mut AppState, text: &str) {
    let hosts = &mut state.hosts;
    if hosts.filtering {
        let room = FILTER_LIMIT.saturating_sub(hosts.filter.chars().count());
        hosts.filter.extend(text.chars().filter(|c| !c.is_control()).take(room));
        hosts.rebuild();
    }
}

/// A card or row clicked: the cursor moves to it, and a double click opens its files.
pub(crate) fn click(state: &mut AppState, row: usize, open: bool) -> Vec<Effect> {
    state.hosts.select(row);
    if open { with_choices(state, Pick::Files) } else { Vec::new() }
}

/// The filter's keys while it is typed. Moving keys still move the cursor.
fn filter_key(hosts: &mut Hosts, key: &KeyEvent) -> bool {
    match key.code {
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            if hosts.filter.chars().count() < FILTER_LIMIT {
                hosts.filter.push(c);
            }
        }
        KeyCode::Backspace => _ = hosts.filter.pop(),
        KeyCode::Enter => hosts.filtering = false,
        KeyCode::Esc => {
            hosts.filtering = false;
            hosts.filter.clear();
        }
        _ => return false,
    }
    hosts.rebuild();
    true
}

fn move_cursor(hosts: &mut Hosts, code: KeyCode) {
    let step = hosts.step();
    let cards = hosts.columns.is_some();
    match code {
        KeyCode::Up => hosts.move_by(-step),
        KeyCode::Down => hosts.move_by(step),
        KeyCode::Left if cards => hosts.move_by(-1),
        KeyCode::Right if cards => hosts.move_by(1),
        KeyCode::PageUp => hosts.move_by(-(PAGE as isize) * step),
        KeyCode::PageDown => hosts.move_by(PAGE as isize * step),
        KeyCode::Home => hosts.select(0),
        KeyCode::End => hosts.select(hosts.reachable().saturating_sub(1)),
        _ => {}
    }
}

/// What a card's key does with the host's sessions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pick {
    Files,
    Mirror,
    Update,
    Rename,
}

/// The remote hosts list holding only this host's sessions, which returns to the cards.
fn browser(state: &AppState, card: &HostCard) -> RemoteBrowser {
    let mut choices = card.sessions.clone();
    if choices.is_empty() {
        choices.push(crate::RemoteChoice {
            identity: card.identity.clone(),
            machine: card.machine.clone(),
            info: card.info.clone(),
            ..Default::default()
        });
    }
    for choice in &mut choices {
        state.remote_names.apply(choice);
    }
    RemoteBrowser {
        side: state.hosts.side.unwrap_or(state.active),
        choices,
        selected: 0,
        loading: false,
        connecting: false,
        error: None,
        clients: None,
        show_clients: false,
        client_selected: 0,
        deleting: None,
        forgetting: None,
        updating: None,
        from_overview: true,
    }
}

/// Enter opens the host's files and M mirrors its session, straight away when it has one and
/// from the remote hosts list when it has several; U updates the host and F2 renames it, after
/// the list's own questions.
fn with_choices(state: &mut AppState, pick: Pick) -> Vec<Effect> {
    let Some(card) = state.hosts.selected().cloned() else { return Vec::new() };
    let browser = browser(state, &card);
    match pick {
        Pick::Rename => crate::remote::ask_rename(state, browser, false),
        _ if card.sessions.is_empty() => {
            state.notice = Some(tr!(remote_host_no_session, machine = card.name()));
            Vec::new()
        }
        Pick::Update => crate::remote::ask_update(state, browser),
        Pick::Files | Pick::Mirror if card.sessions.len() > 1 => {
            state.overlay = Some(Overlay::Remotes(browser));
            Vec::new()
        }
        Pick::Files => {
            close(state);
            crate::remote::open_choice(state, browser, true)
        }
        Pick::Mirror => crate::remote::open_choice(state, browser, false),
    }
}
