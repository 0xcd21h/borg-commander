//! The network screen's keys and clicks: pages, the filter, sorting, moving, and what acts on
//! the selected row's process.

use std::{net::SocketAddr, time::Instant};

use borg_i18n::tr;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{Exposure, FILTER_LIMIT, Network, NetworkPage, NetworkSort, Order, PAGE, close};
use crate::{Action, AppState, Effect};

const LOCALHOST: &str = "localhost";

pub(crate) fn route_key(state: &mut AppState, key: &KeyEvent, now: Instant) -> Vec<Effect> {
    let (code, modifiers) = (key.code, key.modifiers);
    if modifiers == KeyModifiers::ALT.union(KeyModifiers::SHIFT) && matches!(code, KeyCode::Left | KeyCode::Right) {
        let step = if code == KeyCode::Right { crate::keymap::SPLIT_STEP } else { -crate::keymap::SPLIT_STEP };
        return crate::actions::perform(state, Action::NudgeSplit(step), now);
    }
    if modifiers == KeyModifiers::ALT {
        return match code {
            KeyCode::Char('w') => {
                state.network.expanded = !state.network.expanded;
                Vec::new()
            }
            KeyCode::Char('n') => {
                close(state);
                Vec::new()
            }
            KeyCode::Char('m') => crate::actions::perform(state, Action::Activity, now),
            KeyCode::Char('c') => crate::actions::perform(state, Action::Docker, now),
            _ => Vec::new(),
        };
    }
    if let Some(action) = crate::activity::panels_action(key) {
        return crate::actions::perform(state, action, now);
    }
    if state.network.filtering && filter_key(&mut state.network, key) {
        return Vec::new();
    }
    if code == KeyCode::Char('c') && modifiers == KeyModifiers::CONTROL {
        return copy(state);
    }
    if !(modifiers.is_empty() || modifiers == KeyModifiers::SHIFT) {
        return Vec::new();
    }
    let network = &mut state.network;
    match code {
        KeyCode::Tab => network.details_focused = !network.details_focused,
        KeyCode::Esc => escape(state),
        KeyCode::Enter => return enter(state),
        KeyCode::Char('/') => {
            network.filtering = true;
            network.details_focused = false;
        }
        KeyCode::Char(' ') => network.paused = !network.paused,
        KeyCode::Char('k' | 'K') => return show_process(state, true),
        KeyCode::Char(digit @ '1'..='4') => show_page(network, NetworkPage::ALL[usize::from(digit as u8 - b'1')]),
        KeyCode::Char(c) => {
            if let Some(sort) = NetworkSort::from_key(c) {
                sort_by(state, sort);
            }
        }
        code if network.details_focused => scroll_details(network, code),
        code => move_cursor(network, code),
    }
    Vec::new()
}

/// Shows another page, with the list taking the keys.
pub(crate) fn show_page(network: &mut Network, page: NetworkPage) {
    network.page = page;
    network.filtering = false;
    network.details_focused = false;
    network.details_top = 0;
}

/// Pastes into the filter while it is typed.
pub(crate) fn paste(state: &mut AppState, text: &str) {
    let network = &mut state.network;
    if network.filtering {
        let room = FILTER_LIMIT.saturating_sub(network.filter.chars().count());
        network.filter.extend(text.chars().filter(|c| !c.is_control()).take(room));
        network.rebuild();
    }
}

/// A row clicked in the list: the cursor moves to it, and a double click acts as Enter does.
pub(crate) fn click(state: &mut AppState, index: usize, open: bool) -> Vec<Effect> {
    let network = &mut state.network;
    network.details_focused = false;
    network.select(index);
    if open { enter(state) } else { Vec::new() }
}

/// Sorts the page by a column; the same column again turns the order round.
pub(crate) fn sort_by(state: &mut AppState, sort: NetworkSort) {
    let network = &mut state.network;
    let page = network.page;
    if !sort.fits(page) {
        return;
    }
    let order = &mut network.orders[page.index()];
    *order = if order.sort == sort {
        Order { descending: !order.descending, ..*order }
    } else {
        Order { sort, descending: sort.descends() }
    };
    network.rebuild();
}

/// The filter's keys while it is typed. Moving keys still move the list.
fn filter_key(network: &mut Network, key: &KeyEvent) -> bool {
    match key.code {
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            if network.filter.chars().count() < FILTER_LIMIT {
                network.filter.push(c);
            }
        }
        KeyCode::Backspace => _ = network.filter.pop(),
        KeyCode::Enter => network.filtering = false,
        KeyCode::Esc => {
            network.filtering = false;
            network.filter.clear();
        }
        _ => return false,
    }
    network.rebuild();
    true
}

/// Esc gives the list back the keys, then clears a filter, then closes the screen.
fn escape(state: &mut AppState) {
    let network = &mut state.network;
    if network.details_focused {
        network.details_focused = false;
    } else if !network.filter.is_empty() {
        network.filter.clear();
        network.rebuild();
    } else {
        close(state);
    }
}

/// Enter shows the selected row's process in the activity monitor; on a row without one it
/// gives the details the keys.
fn enter(state: &mut AppState) -> Vec<Effect> {
    if state.network.selected_pid().is_some() {
        return show_process(state, false);
    }
    state.network.details_focused = true;
    Vec::new()
}

/// Opens the activity monitor in this panel on the selected row's process, and with `control`
/// its signals and priority.
fn show_process(state: &mut AppState, control: bool) -> Vec<Effect> {
    let Some(pid) = state.network.selected_pid() else {
        state.notice = Some(tr!(network_no_process).into());
        return Vec::new();
    };
    if let Some(side) = state.network.side {
        state.active = side;
    }
    close(state);
    let effects = crate::activity::open(state);
    if state.activity.side.is_some() {
        state.activity.reveal(pid, control);
    }
    effects
}

fn move_cursor(network: &mut Network, code: KeyCode) {
    match code {
        KeyCode::Up => network.move_by(-1),
        KeyCode::Down => network.move_by(1),
        KeyCode::PageUp => network.move_by(-(PAGE as isize)),
        KeyCode::PageDown => network.move_by(PAGE as isize),
        KeyCode::Home => network.select(0),
        KeyCode::End => network.select(network.len(network.page).saturating_sub(1)),
        _ => {}
    }
}

fn scroll_details(network: &mut Network, code: KeyCode) {
    let last = network.related().len().saturating_sub(1);
    network.details_top = match code {
        KeyCode::Up => network.details_top.saturating_sub(1),
        KeyCode::Down => network.details_top + 1,
        KeyCode::PageUp => network.details_top.saturating_sub(PAGE),
        KeyCode::PageDown => network.details_top + PAGE,
        KeyCode::Home => 0,
        KeyCode::End => last,
        _ => network.details_top,
    }
    .min(last);
}

/// Ctrl-C copies what the selected row is reached by: a port's address, a process's PID, a
/// connection's other end, or an interface's address.
fn copy(state: &mut AppState) -> Vec<Effect> {
    let Some(text) = copied_text(&state.network) else { return Vec::new() };
    state.notice = Some(tr!(network_copied, text = text));
    crate::clipboard::copy(state, text)
}

fn copied_text(network: &Network) -> Option<String> {
    match network.page {
        NetworkPage::Ports => {
            let row = network.port()?;
            Some(match row.exposure {
                Exposure::Everywhere | Exposure::ThisMachine => format!("{LOCALHOST}:{}", row.port),
                Exposure::Addresses => SocketAddr::new(*row.addresses.first()?, row.port).to_string(),
            })
        }
        NetworkPage::Processes => network.process()?.pid.map(|pid| pid.to_string()),
        NetworkPage::Connections => network.connection()?.0.remote.map(|remote| remote.to_string()),
        NetworkPage::Interfaces => {
            let (interface, _) = network.interface()?;
            let address = interface
                .addresses
                .iter()
                .find(|(address, _)| address.is_ipv4())
                .or(interface.addresses.first())
                .map(|(address, _)| address.to_string());
            Some(address.unwrap_or_else(|| interface.name.clone()))
        }
    }
}
