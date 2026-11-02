//! The network screen's key bar, its status line, and what clicks and the wheel do in it.

use super::{FILTER_ROW, HEADER_ROW, TABS_ROW, cells, columns, tabs};
use crate::{
    Theme,
    activity_view::{body, inner, row, text},
};
use borg_core::{AppState, Msg, Network, NetworkPage, Pointer};
use borg_i18n::tr;
use crossterm::event::{KeyCode, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
};
use unicode_width::UnicodeWidthStr;

fn buttons(network: &Network, area: Rect) -> Vec<(Rect, &'static str, KeyCode)> {
    let specs: Vec<(&'static str, KeyCode)> = if network.filtering {
        vec![(tr!(activity_filter_keep_button), KeyCode::Enter), (tr!(activity_filter_clear_button), KeyCode::Esc)]
    } else {
        let process = network.selected_pid().is_some();
        [
            process.then_some((tr!(network_show_button), KeyCode::Enter)),
            process.then_some((tr!(activity_signal_button), KeyCode::Char('k'))),
            Some((tr!(activity_filter_button), KeyCode::Char('/'))),
            Some((tr!(activity_pause_button), KeyCode::Char(' '))),
            Some((
                if network.details_focused { tr!(docker_list_button) } else { tr!(docker_details_button) },
                KeyCode::Tab,
            )),
            Some((
                if network.details_focused { tr!(activity_back_button) } else { tr!(activity_close_button) },
                KeyCode::Esc,
            )),
        ]
        .into_iter()
        .flatten()
        .collect()
    };
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

fn hint(network: &Network) -> &'static str {
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
    }
}

pub(crate) fn keybar(state: &AppState, area: Rect, theme: &Theme, buf: &mut Buffer) {
    let network = &state.network;
    buf.set_style(area, theme.styles.key_label);
    for (rect, label, _) in buttons(network, row(area, 0)) {
        text(buf, rect, label, theme.styles.key);
    }
    text(buf, crate::branding::content(row(area, 1)), hint(network), theme.styles.key_label);
}

/// The status line: the selected row's process and where it is reached.
pub(crate) fn status(network: &Network) -> String {
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
        return buttons(network, row(layout.keybar, 0))
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
