//! The Hosts page's key bar, its status line, and what clicks and the wheel do in it.

use borg_core::{AppState, Hosts, HostsLayout, Msg, Pointer};
use borg_i18n::tr;
use crossterm::event::{KeyCode, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
};
use unicode_width::UnicodeWidthStr;

use super::{FILTER_ROW, SEPARATOR, card_lines, card_rect, cards_body};
use crate::{
    Theme,
    activity_view::{body, inner, row, text},
};

fn buttons(hosts: &Hosts, gives_back: bool, area: Rect) -> Vec<(Rect, &'static str, KeyCode)> {
    let specs: Vec<(&'static str, KeyCode)> = if hosts.filtering {
        vec![(tr!(activity_filter_keep_button), KeyCode::Enter), (tr!(activity_filter_clear_button), KeyCode::Esc)]
    } else {
        let selected = hosts.selected();
        let layout = match hosts.layout {
            HostsLayout::Cards => tr!(hosts_rows_button),
            HostsLayout::Rows => tr!(hosts_cards_button),
        };
        [
            selected.map(|_| (tr!(hosts_files_button), KeyCode::Enter)),
            selected.map(|_| (tr!(hosts_mirror_button), KeyCode::Char('m'))),
            selected.filter(|card| card.older()).map(|_| (tr!(hosts_update_button), KeyCode::Char('u'))),
            selected.map(|_| (tr!(hosts_rename_button), KeyCode::F(2))),
            Some((tr!(activity_refresh_button), KeyCode::Char('r'))),
            Some((layout, KeyCode::Char('v'))),
            Some((tr!(activity_filter_button), KeyCode::Char('/'))),
            Some((tr!(activity_close_button), KeyCode::Esc)),
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

pub(crate) fn keybar(state: &AppState, area: Rect, theme: &Theme, buf: &mut Buffer) {
    let hosts = &state.hosts;
    buf.set_style(area, theme.styles.key_label);
    for (rect, label, _) in buttons(hosts, state.esc_gives_back(), row(area, 0)) {
        text(buf, rect, label, theme.styles.key);
    }
    let hint = if hosts.filtering { tr!(watch_filter_hint) } else { tr!(hosts_hint) };
    text(buf, crate::branding::content(row(area, 1)), hint, theme.styles.key_label);
}

/// The status line: the selected host's name, its Borg and its system.
pub(crate) fn status(hosts: &Hosts) -> String {
    hosts.selected().map_or_else(String::new, |card| {
        [Some(card.name()), card.info.version.as_deref(), card.info.platform.as_deref()]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(SEPARATOR)
    })
}

pub(crate) fn panel_mouse(
    state: &AppState,
    event: MouseEvent,
    double: bool,
    layout: &borg_core::Layout,
) -> Option<Msg> {
    let hosts = &state.hosts;
    let point = Position::new(event.column, event.row);
    let left_click = event.kind == MouseEventKind::Down(MouseButton::Left);
    if layout.keybar.contains(point) {
        if !left_click {
            return None;
        }
        return buttons(hosts, state.esc_gives_back(), row(layout.keybar, 0))
            .into_iter()
            .find(|(rect, _, _)| rect.contains(point))
            .map(|(_, _, code)| Msg::Pointer(Pointer::HostsControl(code)));
    }
    let side = hosts.side?;
    let rect =
        [side, side.other()].into_iter().map(|panel| layout.panels[panel.index()]).find(|r| r.contains(point))?;
    if left_click
        && let Some((_, _, action)) =
            crate::results_view::controls(rect, hosts.expanded).into_iter().find(|(r, _, _)| r.contains(point))
    {
        return Some(Msg::Pointer(match action {
            borg_core::Action::SetExpanded(expand) => Pointer::HostsExpand(expand),
            _ => Pointer::HostsHide,
        }));
    }
    let wheel = match event.kind {
        MouseEventKind::ScrollUp => Some(KeyCode::Up),
        MouseEventKind::ScrollDown => Some(KeyCode::Down),
        _ => None,
    };
    if let Some(code) = wheel {
        return Some(Msg::Pointer(Pointer::HostsControl(code)));
    }
    if !left_click || rect != layout.panels[side.index()] {
        return None;
    }
    if row(inner(rect), FILTER_ROW).contains(point) {
        return Some(Msg::Pointer(Pointer::HostsControl(KeyCode::Char('/'))));
    }
    let index = match hosts.columns() {
        Some(columns) => {
            let area = cards_body(rect);
            let first = hosts.list_start(card_lines(area));
            (first * columns..hosts.reachable())
                .find(|&row| card_rect(area, columns, first, row).is_some_and(|card| card.contains(point)))?
        }
        None => {
            let body = body(rect);
            if !body.contains(point) {
                return None;
            }
            hosts.list_start(usize::from(body.height)) + usize::from(point.y - body.y)
        }
    };
    (index < hosts.rows.len()).then_some(Msg::Pointer(Pointer::HostsRow { index, open: double }))
}
