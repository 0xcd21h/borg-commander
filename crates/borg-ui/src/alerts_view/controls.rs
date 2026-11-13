//! The Alerts page's key bar, its status line, and what clicks and the wheel do in it.

use super::{FILTER_ROW, SEPARATOR};
use crate::{
    Theme,
    activity_view::{body, inner, row, text},
};
use borg_core::{Alerts, AppState, Msg, Pointer};
use borg_i18n::tr;
use crossterm::event::{KeyCode, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
};
use unicode_width::UnicodeWidthStr;

fn buttons(alerts: &Alerts, gives_back: bool, area: Rect) -> Vec<(Rect, &'static str, KeyCode)> {
    let specs: Vec<(&'static str, KeyCode)> = if alerts.filtering {
        vec![(tr!(activity_filter_keep_button), KeyCode::Enter), (tr!(activity_filter_clear_button), KeyCode::Esc)]
    } else {
        let selected = alerts.selected();
        [
            selected.map(|_| (tr!(watch_activity_button), KeyCode::Enter)),
            selected.filter(|alert| alert.resumable()).map(|_| (tr!(watch_resume_button), KeyCode::Char('c'))),
            selected.map(|_| (tr!(watch_kill_button), KeyCode::Char('k'))),
            selected.map(|_| (tr!(watch_allow_button), KeyCode::Char('a'))),
            Some((tr!(watch_again_button), KeyCode::Char('r'))),
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
    let alerts = &state.alerts;
    buf.set_style(area, theme.styles.key_label);
    for (rect, label, _) in buttons(alerts, state.esc_gives_back(), row(area, 0)) {
        text(buf, rect, label, theme.styles.key);
    }
    let hint = if alerts.filtering { tr!(watch_filter_hint) } else { tr!(watch_hint) };
    text(buf, crate::branding::content(row(area, 1)), hint, theme.styles.key_label);
}

/// The status line: the selected alert's rule, its program and what has become of it.
pub(crate) fn status(alerts: &Alerts) -> String {
    alerts.selected().map_or_else(String::new, |alert| {
        format!("{}{SEPARATOR}{}{SEPARATOR}{}", alert.rule().label(), alert.program(), alert.state.label())
    })
}

pub(crate) fn panel_mouse(
    state: &AppState,
    event: MouseEvent,
    double: bool,
    layout: &borg_core::Layout,
) -> Option<Msg> {
    let alerts = &state.alerts;
    let point = Position::new(event.column, event.row);
    let left_click = event.kind == MouseEventKind::Down(MouseButton::Left);
    if layout.keybar.contains(point) {
        if !left_click {
            return None;
        }
        return buttons(alerts, state.esc_gives_back(), row(layout.keybar, 0))
            .into_iter()
            .find(|(rect, _, _)| rect.contains(point))
            .map(|(_, _, code)| Msg::Pointer(Pointer::AlertsControl(code)));
    }
    let side = alerts.side?;
    let rect =
        [side, side.other()].into_iter().map(|panel| layout.panels[panel.index()]).find(|r| r.contains(point))?;
    if left_click
        && let Some((_, _, action)) =
            crate::results_view::controls(rect, alerts.expanded).into_iter().find(|(r, _, _)| r.contains(point))
    {
        return Some(Msg::Pointer(match action {
            borg_core::Action::SetExpanded(expand) => Pointer::AlertsExpand(expand),
            _ => Pointer::AlertsHide,
        }));
    }
    let wheel = match event.kind {
        MouseEventKind::ScrollUp => Some(KeyCode::Up),
        MouseEventKind::ScrollDown => Some(KeyCode::Down),
        _ => None,
    };
    if let Some(code) = wheel {
        return Some(Msg::Pointer(Pointer::AlertsControl(code)));
    }
    if !left_click || rect != layout.panels[side.index()] {
        return None;
    }
    if row(inner(rect), FILTER_ROW).contains(point) {
        return Some(Msg::Pointer(Pointer::AlertsControl(KeyCode::Char('/'))));
    }
    let body = body(rect);
    if body.contains(point) {
        let index = alerts.list_start(usize::from(body.height)) + usize::from(point.y - body.y);
        return Some(Msg::Pointer(Pointer::AlertsRow { index, open: double }));
    }
    None
}
