//! The startup items screen's key bar, its status line, and what clicks and the wheel do in it.

use super::{FILTER_ROW, SEPARATOR, file_line};
use crate::{
    Theme,
    activity_view::{body, inner, row, text},
};
use borg_core::{AppState, Msg, Pointer, StartupItems};
use borg_i18n::tr;
use crossterm::event::{KeyCode, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
};
use unicode_width::UnicodeWidthStr;

fn buttons(startup: &StartupItems, area: Rect) -> Vec<(Rect, &'static str, KeyCode)> {
    let specs: Vec<(&'static str, KeyCode)> = if startup.filtering {
        vec![(tr!(activity_filter_keep_button), KeyCode::Enter), (tr!(activity_filter_clear_button), KeyCode::Esc)]
    } else {
        let selected = startup.selected().is_some();
        [
            selected.then_some((tr!(startup_show_button), KeyCode::Enter)),
            selected.then_some((tr!(startup_view_button), KeyCode::F(3))),
            Some((tr!(startup_again_button), KeyCode::Char('r'))),
            Some((tr!(activity_filter_button), KeyCode::Char('/'))),
            Some((tr!(activity_close_button), KeyCode::Esc)),
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

pub(crate) fn keybar(state: &AppState, area: Rect, theme: &Theme, buf: &mut Buffer) {
    let startup = &state.startup_items;
    buf.set_style(area, theme.styles.key_label);
    for (rect, label, _) in buttons(startup, row(area, 0)) {
        text(buf, rect, label, theme.styles.key);
    }
    let hint = if startup.filtering { tr!(startup_filter_hint) } else { tr!(startup_hint) };
    text(buf, crate::branding::content(row(area, 1)), hint, theme.styles.key_label);
}

/// The status line: the selected item's name and its file.
pub(crate) fn status(startup: &StartupItems) -> String {
    startup.selected().map_or_else(String::new, |item| format!("{}{SEPARATOR}{}", item.found.name, file_line(item)))
}

pub(crate) fn panel_mouse(
    state: &AppState,
    event: MouseEvent,
    double: bool,
    layout: &borg_core::Layout,
) -> Option<Msg> {
    let startup = &state.startup_items;
    let point = Position::new(event.column, event.row);
    let left_click = event.kind == MouseEventKind::Down(MouseButton::Left);
    if layout.keybar.contains(point) {
        if !left_click {
            return None;
        }
        return buttons(startup, row(layout.keybar, 0))
            .into_iter()
            .find(|(rect, _, _)| rect.contains(point))
            .map(|(_, _, code)| Msg::Pointer(Pointer::StartupControl(code)));
    }
    let side = startup.side?;
    let rect =
        [side, side.other()].into_iter().map(|panel| layout.panels[panel.index()]).find(|r| r.contains(point))?;
    if left_click
        && let Some((_, _, action)) =
            crate::results_view::controls(rect, startup.expanded).into_iter().find(|(r, _, _)| r.contains(point))
    {
        return Some(Msg::Pointer(match action {
            borg_core::Action::SetExpanded(expand) => Pointer::StartupExpand(expand),
            _ => Pointer::StartupHide,
        }));
    }
    let wheel = match event.kind {
        MouseEventKind::ScrollUp => Some(KeyCode::Up),
        MouseEventKind::ScrollDown => Some(KeyCode::Down),
        _ => None,
    };
    if let Some(code) = wheel {
        return Some(Msg::Pointer(Pointer::StartupControl(code)));
    }
    if !left_click || rect != layout.panels[side.index()] {
        return None;
    }
    if row(inner(rect), FILTER_ROW).contains(point) {
        return Some(Msg::Pointer(Pointer::StartupControl(KeyCode::Char('/'))));
    }
    let body = body(rect);
    if body.contains(point) {
        let index = startup.list_start(usize::from(body.height)) + usize::from(point.y - body.y);
        return Some(Msg::Pointer(Pointer::StartupRow { index, open: double }));
    }
    None
}
