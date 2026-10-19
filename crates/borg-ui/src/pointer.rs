use std::time::Instant;

use borg_core::{AppState, KEYMAP, Msg, Overlay, Pointer, Side, View, menus};
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Constraint, Layout, Position, Rect};
use unicode_width::UnicodeWidthStr;

use crate::{keybar::actions_area, menu_view, panel_view};

const WHEEL_ROWS: isize = 3;

pub fn mouse_input(state: &AppState, event: MouseEvent, double: bool, now: Instant) -> Option<Msg> {
    if state.viewer.is_some() && !state.shell.captured() {
        return match event.kind {
            MouseEventKind::ScrollUp => Some(Msg::Pointer(Pointer::ScrollDocument(-WHEEL_ROWS))),
            MouseEventKind::ScrollDown => Some(Msg::Pointer(Pointer::ScrollDocument(WHEEL_ROWS))),
            _ => None,
        };
    }
    if state.shell.captured() || state.effective_view() != View::Panels {
        return None;
    }
    let point = Position::new(event.column, event.row);
    let layout = state.layout(now);
    let click = matches!(event.kind, MouseEventKind::Down(MouseButton::Left));
    let all = menus(state);
    if matches!(state.overlay, Some(Overlay::Settings { .. })) {
        return click
            .then(|| crate::settings_view::item_at(state, point, layout.screen))
            .flatten()
            .map(|index| Msg::Pointer(Pointer::Setting(index)));
    }
    let open = match state.overlay {
        Some(Overlay::Menu(cursor)) => Some(cursor),
        Some(_) => return None,
        None => None,
    };
    let bar = layout.menu.or_else(|| open.map(|_| Rect { height: 1, ..layout.screen }));
    if click && bar.is_some_and(|bar| bar.contains(point)) {
        let positions = menu_view::title_positions(&all);
        return positions.iter().zip(&all).enumerate().find_map(|(index, (&x, menu))| {
            let width = menu.title.width() as u16 + 4;
            (point.x >= x && point.x < x + width).then_some(Msg::Pointer(Pointer::Menu(index)))
        });
    }
    if let Some(cursor) = open {
        if !click {
            return None;
        }
        let menu = all.get(cursor.menu)?;
        let x = menu_view::title_positions(&all)[cursor.menu];
        let rect = menu_view::dropdown_rect(menu, x, bar?.bottom(), layout.screen);
        let inner = Rect {
            x: rect.x + 1,
            y: rect.y + 1,
            width: rect.width.saturating_sub(2),
            height: rect.height.saturating_sub(2),
        };
        return Some(Msg::Pointer(if inner.contains(point) {
            Pointer::MenuItem(usize::from(point.y - inner.y))
        } else if !rect.contains(point) {
            Pointer::DismissMenu
        } else {
            return None;
        }));
    }
    if click && layout.keybar.contains(point) {
        if state.notice.is_some() {
            return Some(Msg::Pointer(Pointer::DismissNotice));
        }
        let actions = actions_area(layout.keybar);
        let bindings: Vec<_> = KEYMAP.iter().filter(|binding| binding.label.is_some()).collect();
        let slot = actions.width / bindings.len() as u16;
        if slot == 0 || !actions.contains(point) {
            return None;
        }
        return bindings
            .get(usize::from((point.x - actions.x) / slot))
            .map(|binding| Msg::Pointer(Pointer::Action(binding.action)));
    }
    for side in Side::ALL {
        let rect = layout.panels[side.index()];
        if !rect.contains(point) {
            continue;
        }
        if state.aside.is_some() && side != state.active {
            return match event.kind {
                MouseEventKind::ScrollUp => Some(Msg::Pointer(Pointer::ScrollDocument(-WHEEL_ROWS))),
                MouseEventKind::ScrollDown => Some(Msg::Pointer(Pointer::ScrollDocument(WHEEL_ROWS))),
                _ if click => Some(Msg::Pointer(Pointer::FocusPreview)),
                _ => None,
            };
        }
        let delta = match event.kind {
            MouseEventKind::ScrollUp => Some(-WHEEL_ROWS),
            MouseEventKind::ScrollDown => Some(WHEEL_ROWS),
            _ => None,
        };
        if let Some(delta) = delta {
            return Some(Msg::Pointer(Pointer::ScrollPanel { side, delta }));
        }
        let mark = event.kind == MouseEventKind::Down(MouseButton::Right);
        if !click && !mark {
            return None;
        }
        let (_, content) = panel_view::geometry(rect);
        if !content.contains(point) {
            return None;
        }
        let panel = state.panel(side);
        let columns = panel.mode().columns();
        let cells = Layout::horizontal((0..columns).map(|_| Constraint::Ratio(1, columns as u32))).split(content);
        let column = cells.iter().position(|cell| cell.contains(point))?;
        let scroll = state.motions[side.index()].scroll(now, panel.scroll());
        let index = scroll + column * panel.rows().max(1) + usize::from(point.y - content.y);
        return Some(Msg::Pointer(Pointer::Panel { side, index, open: double && click, mark }));
    }
    None
}
