//! The traffic viewer's keys and clicks: pages, the filter, moving, Enter into a row and Esc
//! back out of it, and F3 for the selected bytes in the viewer.

use super::{TrafficPage, TrafficView};
use crate::{AppState, Effect, network::FILTER_LIMIT, network::Network, network::PAGE};
use borg_i18n::tr;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::time::Instant;

/// Cells the frame takes from a pane's width, one on each side.
const FRAME_CELLS: u16 = 2;
const NEXT_TURN_KEY: char = ']';
const PREVIOUS_TURN_KEY: char = '[';

pub(crate) fn route_key(state: &mut AppState, key: &KeyEvent, now: Instant) -> Vec<Effect> {
    let width = details_width(state, now);
    let network = &mut state.network;
    let Some(view) = network.viewer.as_deref_mut() else { return Vec::new() };
    if view.filtering && filter_key(view, key) {
        return Vec::new();
    }
    let key = crate::keyboard::latin(key);
    let (code, modifiers) = (key.code, key.modifiers);
    if code == KeyCode::Char('c') && modifiers == KeyModifiers::CONTROL {
        return copy(state);
    }
    if !(modifiers.is_empty() || modifiers == KeyModifiers::SHIFT) {
        return Vec::new();
    }
    if code == KeyCode::F(3) {
        return view_bytes(state);
    }
    match code {
        KeyCode::Tab => network.details_focused = !network.details_focused,
        KeyCode::Esc => escape(network),
        KeyCode::Enter => {
            if !view.drill() && view.len(view.page) > 0 {
                network.details_focused = true;
            }
        }
        KeyCode::Char('/') => {
            view.filtering = true;
            network.details_focused = false;
        }
        KeyCode::Char(digit @ '1'..='4') => show_page(network, TrafficPage::ALL[usize::from(digit as u8 - b'1')]),
        code if network.details_focused => scroll_details(view, code, width),
        code => move_cursor(view, code),
    }
    Vec::new()
}

/// How many cells wide the details are inside their frame.
fn details_width(state: &AppState, now: Instant) -> usize {
    let side = state.network.side.unwrap_or(state.active);
    usize::from(state.layout(now).panels[side.other().index()].width.saturating_sub(FRAME_CELLS))
}

/// Shows another page, with the list taking the keys.
pub(crate) fn show_page(network: &mut Network, page: TrafficPage) {
    network.details_focused = false;
    if let Some(view) = network.viewer.as_deref_mut() {
        view.page = page;
        view.filtering = false;
        view.details_top = 0;
    }
}

/// Pastes into the filter while it is typed.
pub(crate) fn paste(view: &mut TrafficView, text: &str) {
    if view.filtering {
        let room = FILTER_LIMIT.saturating_sub(view.filter.chars().count());
        view.filter.extend(text.chars().filter(|c| !c.is_control()).take(room));
        view.rebuild();
    }
}

/// A row clicked in the list: the cursor moves to it, and a double click acts as Enter does.
pub(crate) fn click(state: &mut AppState, index: usize, open: bool) -> Vec<Effect> {
    let network = &mut state.network;
    network.details_focused = false;
    let Some(view) = network.viewer.as_deref_mut() else { return Vec::new() };
    view.select(index);
    if open && !view.drill() && view.len(view.page) > 0 {
        network.details_focused = true;
    }
    Vec::new()
}

fn filter_key(view: &mut TrafficView, key: &KeyEvent) -> bool {
    match key.code {
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            if view.filter.chars().count() < FILTER_LIMIT {
                view.filter.push(c);
            }
        }
        KeyCode::Backspace => _ = view.filter.pop(),
        KeyCode::Enter => view.filtering = false,
        KeyCode::Esc => {
            view.filtering = false;
            view.filter.clear();
        }
        _ => return false,
    }
    view.rebuild();
    true
}

/// Esc gives the list back the keys, then goes back to the page Enter came from, then clears
/// the filter, then closes the viewer for the live screen.
fn escape(network: &mut Network) {
    let Some(view) = network.viewer.as_deref_mut() else { return };
    if network.details_focused {
        network.details_focused = false;
        return;
    }
    if view.back() {
        return;
    }
    if !view.filter.is_empty() {
        view.set_filter("");
        return;
    }
    network.viewer = None;
    network.sampled = None;
    network.next = None;
}

fn move_cursor(view: &mut TrafficView, code: KeyCode) {
    match code {
        KeyCode::Up => view.move_by(-1),
        KeyCode::Down => view.move_by(1),
        KeyCode::PageUp => view.move_by(-(PAGE as isize)),
        KeyCode::PageDown => view.move_by(PAGE as isize),
        KeyCode::Home => view.select(0),
        KeyCode::End => view.select(view.len(view.page).saturating_sub(1)),
        _ => {}
    }
}

fn scroll_details(view: &mut TrafficView, code: KeyCode, width: usize) {
    if let KeyCode::Char(key @ (NEXT_TURN_KEY | PREVIOUS_TURN_KEY)) = code {
        view.step_turn(width, key == NEXT_TURN_KEY);
        return;
    }
    let last = view.detail_rows(width).saturating_sub(1);
    view.details_top = match code {
        KeyCode::Up => view.details_top.saturating_sub(1),
        KeyCode::Down => view.details_top + 1,
        KeyCode::PageUp => view.details_top.saturating_sub(PAGE),
        KeyCode::PageDown => view.details_top + PAGE,
        KeyCode::Home => 0,
        KeyCode::End => last,
        _ => view.details_top,
    }
    .min(last);
}

/// F3 shows the selected conversation's stream, or the selected packet's bytes, in the viewer
/// as hex rows.
fn view_bytes(state: &mut AppState) -> Vec<Effect> {
    if let Some((name, bytes)) = state.network.viewer.as_deref().and_then(selected_bytes) {
        crate::viewer::open_bytes(state, name, bytes);
    }
    Vec::new()
}

/// The bytes F3 shows, named for where they come from: a conversation by its ends, a packet
/// by its number.
fn selected_bytes(view: &TrafficView) -> Option<(String, Vec<u8>)> {
    let (name, bytes) = match view.page {
        TrafficPage::Connections => {
            let ends = view.ends[view.flow()?.0];
            let name = format!("{} → {}", view.endpoint(ends.local), view.endpoint(ends.remote));
            (name, view.stream()?.bytes.clone())
        }
        TrafficPage::Packets => {
            let (index, packet) = view.packet()?;
            (tr!(traffic_packet_title, number = index + 1), packet.data.clone())
        }
        TrafficPage::Processes | TrafficPage::Hosts => return None,
    };
    (!bytes.is_empty()).then_some((name, bytes))
}

/// Ctrl-C copies what the selected row is reached by: a PID, a host's address, a
/// conversation's other end, or a packet's bytes in hex.
fn copy(state: &mut AppState) -> Vec<Effect> {
    let Some(view) = state.network.viewer.as_deref() else { return Vec::new() };
    let Some(text) = copied_text(view) else { return Vec::new() };
    let shown = text.lines().next().unwrap_or_default().to_owned();
    state.notice = Some(tr!(network_copied, text = shown));
    crate::clipboard::copy(state, text)
}

fn copied_text(view: &TrafficView) -> Option<String> {
    match view.page {
        TrafficPage::Processes => view.talker()?.pid.map(|pid| pid.to_string()),
        TrafficPage::Hosts => Some(view.host_row()?.address.to_string()),
        TrafficPage::Connections => view.flow().map(|(index, _)| view.ends[index].remote.to_string()),
        TrafficPage::Packets => view
            .packet()
            .map(|(_, packet)| borg_view::HexLayout::full(packet.data.len() as u64).lines(&packet.data).join("\n")),
    }
}
