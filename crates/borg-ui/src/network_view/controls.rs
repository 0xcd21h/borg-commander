//! The network screen's key bar, its status line, and what clicks and the wheel do in it.

use super::{FILTER_ROW, HEADER_ROW, TABS_ROW, cells, columns, tabs};
use crate::{
    Theme,
    activity_view::{body, inner, row, text},
};
use borg_core::{AppState, Msg, Network, NetworkPage, Pointer, TrafficPage, TrafficView};
use borg_i18n::tr;
use crossterm::event::{KeyCode, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
};
use unicode_width::UnicodeWidthStr;

const SCAN_KEY: char = 's';
const DISCOVER_KEY: char = 'd';
const PORTS_KEY: char = 'p';

fn buttons(network: &Network, gives_back: bool, area: Rect) -> Vec<(Rect, &'static str, KeyCode)> {
    let filtering = network.viewer.as_deref().map_or(network.filtering, |view| view.filtering);
    let specs: Vec<(&'static str, KeyCode)> = if filtering {
        vec![(tr!(activity_filter_keep_button), KeyCode::Enter), (tr!(activity_filter_clear_button), KeyCode::Esc)]
    } else if let Some(view) = network.viewer.as_deref() {
        let enter = match view.page {
            _ if view.len(view.page) == 0 => None,
            TrafficPage::Processes | TrafficPage::Hosts => Some(tr!(traffic_connections_button)),
            TrafficPage::Connections => Some(tr!(traffic_packets_button)),
            TrafficPage::Packets => None,
        };
        [
            enter.map(|label| (label, KeyCode::Enter)),
            Some((tr!(activity_filter_button), KeyCode::Char('/'))),
            Some(details_button(network)),
            Some(back_button(true)),
        ]
        .into_iter()
        .flatten()
        .collect()
    } else {
        let process = network.selected_pid().is_some();
        let capture =
            if network.recording.is_some() { tr!(network_stop_capture_button) } else { tr!(network_capture_button) };
        let copy = network.recording.as_ref().filter(|recording| recording.key_log.is_some()).map(|recording| {
            if recording.proxy.is_some() { tr!(network_copy_proxy_button) } else { tr!(network_copy_key_log_button) }
        });
        let scanning = network.scan.as_ref().is_some_and(|scan| !scan.stopping);
        let ports = network.loaded && network.page == NetworkPage::Neighbors;
        let scan = network.loaded && network.page == NetworkPage::Neighbors && network.scan.is_none();
        let esc = if scanning && !network.details_focused {
            (tr!(network_stop_scan_button), KeyCode::Esc)
        } else {
            back_button(network.details_focused || network.goes_back())
        };
        [
            ports.then_some((tr!(network_scan_ports_button), KeyCode::Char(PORTS_KEY))),
            scan.then_some((tr!(network_scan_button), KeyCode::Char(SCAN_KEY))),
            scan.then_some((tr!(network_discover_button), KeyCode::Char(DISCOVER_KEY))),
            process.then_some((tr!(network_show_button), KeyCode::Enter)),
            process.then_some((tr!(activity_signal_button), KeyCode::Char('k'))),
            network.loaded.then_some((capture, KeyCode::Char('r'))),
            copy.map(|copy| (copy, KeyCode::Char('l'))),
            Some((tr!(activity_filter_button), KeyCode::Char('/'))),
            Some((tr!(activity_pause_button), KeyCode::Char(' '))),
            Some(details_button(network)),
            Some(esc),
        ]
        .into_iter()
        .flatten()
        .collect()
    };
    let specs = crate::keybar::lent_esc(specs, gives_back);
    let mut x = area.x;
    specs
        .into_iter()
        .filter_map(|(label, key)| {
            let width = label.width() as u16 + 2;
            if x.saturating_add(width) > area.right() {
                return None;
            }
            let cell = Rect { x, width, ..area };
            x += width;
            Some((cell, label, key))
        })
        .collect()
}

fn details_button(network: &Network) -> (&'static str, KeyCode) {
    (if network.details_focused { tr!(docker_list_button) } else { tr!(docker_details_button) }, KeyCode::Tab)
}

/// Esc, which goes back when there is somewhere to go back to, and closes the screen otherwise.
fn back_button(back: bool) -> (&'static str, KeyCode) {
    (if back { tr!(activity_back_button) } else { tr!(activity_close_button) }, KeyCode::Esc)
}

fn traffic_hint(network: &Network, view: &TrafficView) -> &'static str {
    if view.filtering {
        return tr!(traffic_filter_hint);
    }
    if network.details_focused {
        return match view.page {
            TrafficPage::Connections => tr!(traffic_stream_hint),
            _ => tr!(network_details_hint),
        };
    }
    match view.page {
        TrafficPage::Processes => tr!(traffic_pid_hint),
        TrafficPage::Hosts => tr!(traffic_address_hint),
        TrafficPage::Connections => tr!(traffic_connection_hint),
        TrafficPage::Packets => tr!(traffic_packets_hint),
    }
}

fn hint(network: &Network) -> &'static str {
    if let Some(view) = network.viewer.as_deref() {
        return traffic_hint(network, view);
    }
    if network.filtering && network.page == NetworkPage::Neighbors {
        return tr!(network_neighbors_filter_hint);
    }
    if network.filtering {
        return tr!(network_filter_hint);
    }
    if network.details_focused {
        return tr!(network_details_hint);
    }
    match network.page {
        NetworkPage::Ports => tr!(network_ports_hint),
        NetworkPage::Processes => tr!(network_processes_hint),
        NetworkPage::Connections => tr!(network_connections_hint),
        NetworkPage::Interfaces => tr!(network_interfaces_hint),
        NetworkPage::Neighbors => tr!(network_neighbors_hint),
    }
}

pub(crate) fn keybar(state: &AppState, area: Rect, theme: &Theme, buf: &mut Buffer) {
    let network = &state.network;
    buf.set_style(area, theme.styles.key_label);
    for (rect, label, _) in buttons(network, state.esc_gives_back(), row(area, 0)) {
        text(buf, rect, label, theme.styles.key);
    }
    text(buf, crate::branding::content(row(area, 1)), hint(network), theme.styles.key_label);
}

/// The status line: the selected row's process and where it is reached.
pub(crate) fn status(network: &Network) -> String {
    if let Some(view) = network.viewer.as_deref() {
        return super::traffic::status(view);
    }
    let process = |pid: Option<u32>| match pid {
        Some(pid) => tr!(process_control_subject, name = network.name(Some(pid)), pid = pid),
        None => network.name(None),
    };
    match network.page {
        NetworkPage::Ports => network
            .port()
            .map_or_else(String::new, |row| format!("{} · {} {}", process(row.pid), row.transport.name(), row.port)),
        NetworkPage::Processes => network.process().map_or_else(String::new, |row| process(row.pid)),
        NetworkPage::Connections => network.connection().map_or_else(String::new, |(socket, _)| {
            let remote = socket.remote.map_or_else(String::new, |remote| remote.to_string());
            format!("{} · {} → {remote}", process(socket.pid), socket.local)
        }),
        NetworkPage::Interfaces => network.interface().map_or_else(String::new, |(interface, _)| {
            format!("{} · {}", interface.name, borg_core::kind_label(interface.kind))
        }),
        NetworkPage::Neighbors => match network.neighbor() {
            Some((neighbor, _)) => {
                let parts = [Some(neighbor.address.to_string()), neighbor.mac.map(|mac| mac.to_string())];
                parts.into_iter().flatten().chain([neighbor.interface.clone()]).collect::<Vec<_>>().join(" · ")
            }
            None => network.entry().map_or_else(String::new, |_| super::map::status(network)),
        },
    }
}

pub(crate) fn panel_mouse(
    state: &AppState,
    event: MouseEvent,
    double: bool,
    layout: &borg_core::Layout,
) -> Option<Msg> {
    let network = &state.network;
    let point = Position::new(event.column, event.row);
    let left_click = event.kind == MouseEventKind::Down(MouseButton::Left);
    if layout.keybar.contains(point) {
        if !left_click {
            return None;
        }
        return buttons(network, state.esc_gives_back(), row(layout.keybar, 0))
            .into_iter()
            .find(|(rect, _, _)| rect.contains(point))
            .map(|(_, _, code)| Msg::Pointer(Pointer::NetworkControl { details: network.details_focused, code }));
    }
    let side = network.side?;
    for panel in [side, side.other()] {
        let rect = layout.panels[panel.index()];
        if !rect.contains(point) {
            continue;
        }
        let details = panel != side;
        if left_click
            && let Some((_, _, action)) =
                crate::results_view::controls(rect, network.expanded).into_iter().find(|(r, _, _)| r.contains(point))
        {
            return Some(Msg::Pointer(match action {
                borg_core::Action::SetExpanded(expand) => Pointer::NetworkExpand { details, expand },
                _ => Pointer::NetworkHide,
            }));
        }
        let wheel = match event.kind {
            MouseEventKind::ScrollUp => Some(KeyCode::Up),
            MouseEventKind::ScrollDown => Some(KeyCode::Down),
            _ => None,
        };
        if let Some(code) = wheel {
            return Some(Msg::Pointer(Pointer::NetworkControl { details, code }));
        }
        if !left_click {
            return None;
        }
        if details {
            return Some(Msg::Pointer(Pointer::NetworkControl { details: true, code: KeyCode::Null }));
        }
        let inner = inner(rect);
        if let Some(view) = network.viewer.as_deref() {
            return Some(Msg::Pointer(traffic_click(view, rect, inner, point, double)));
        }
        if let Some((_, _, page)) = tabs(row(inner, TABS_ROW)).into_iter().find(|(r, _, _)| r.contains(point)) {
            return Some(Msg::Pointer(Pointer::NetworkPage(page)));
        }
        if row(inner, FILTER_ROW).contains(point) {
            return Some(Msg::Pointer(Pointer::NetworkControl { details: false, code: KeyCode::Char('/') }));
        }
        let header = row(inner, HEADER_ROW);
        if header.contains(point) {
            let columns = columns(network.page, header.width);
            return columns
                .iter()
                .zip(cells(&columns, header))
                .find(|(_, cell)| cell.contains(point))
                .and_then(|(column, _)| column.sort)
                .map(|sort| Msg::Pointer(Pointer::NetworkSort(sort)));
        }
        let body = body(rect);
        if body.contains(point) {
            let index = network.list_start(usize::from(body.height)) + usize::from(point.y - body.y);
            return Some(Msg::Pointer(Pointer::NetworkRow { index, open: double }));
        }
        return Some(Msg::Pointer(Pointer::NetworkControl { details: false, code: KeyCode::Null }));
    }
    None
}

/// A click in the traffic viewer's list pane: on a tab, the filter or a row.
fn traffic_click(view: &TrafficView, rect: Rect, inner: Rect, point: Position, double: bool) -> Pointer {
    let tab = super::traffic::tabs(row(inner, TABS_ROW)).into_iter().find(|(r, _, _)| r.contains(point));
    if let Some((_, _, page)) = tab {
        return Pointer::TrafficPage(page);
    }
    if row(inner, FILTER_ROW).contains(point) {
        return Pointer::NetworkControl { details: false, code: KeyCode::Char('/') };
    }
    let body = body(rect);
    if body.contains(point) {
        let index = view.list_start(usize::from(body.height)) + usize::from(point.y - body.y);
        return Pointer::NetworkRow { index, open: double };
    }
    Pointer::NetworkControl { details: false, code: KeyCode::Null }
}
