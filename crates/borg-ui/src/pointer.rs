use std::time::Instant;

use borg_core::{AppState, Msg, Overlay, Pointer, Side, View, menus};
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Constraint, Layout, Position, Rect};
use unicode_width::UnicodeWidthStr;

use crate::{menu_view, panel_view};

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
    if let Some(Overlay::Prompt(prompt)) = &state.overlay {
        return click
            .then(|| crate::dialog_view::prompt_field_at(prompt, layout.screen, point))
            .flatten()
            .map(|option| Msg::Pointer(Pointer::PromptField { option }));
    }
    if let Some(Overlay::Collections(picker)) = &state.overlay {
        let delta = match event.kind {
            MouseEventKind::ScrollUp => Some(-WHEEL_ROWS),
            MouseEventKind::ScrollDown => Some(WHEEL_ROWS),
            _ => None,
        };
        if let Some(delta) = delta {
            return Some(Msg::Pointer(Pointer::CollectionScroll(delta)));
        }
        let list = crate::discovery_view::collection_list(layout.screen);
        if click && list.contains(point) {
            let index = crate::discovery_view::collection_start(picker, layout.screen) + usize::from(point.y - list.y);
            return (index < state.collections.len())
                .then_some(Msg::Pointer(Pointer::Collection { index, open: double }));
        }
        let area = crate::discovery_view::collection_area(layout.screen);
        if click && area.contains(point) {
            if picker.creating.is_some() {
                if point.y == area.bottom().saturating_sub(3) && point.x < area.x.saturating_add(14) {
                    return Some(Msg::Pointer(Pointer::SearchStart));
                }
            } else if point.y == area.bottom().saturating_sub(4) {
                use unicode_width::UnicodeWidthStr;
                let new_start = area.x.saturating_add(2 + borg_i18n::tr!(collections_open_button).width() as u16 + 5);
                return Some(Msg::Pointer(if point.x < new_start {
                    Pointer::SearchStart
                } else {
                    Pointer::NewCollection
                }));
            }
        }
        return None;
    }
    if let Some(Overlay::Search(_)) = &state.overlay {
        if !click {
            return None;
        }
        if let Some(field) = crate::discovery_view::search_field(layout.screen, point) {
            return Some(Msg::Pointer(Pointer::SearchField(field)));
        }
        return crate::discovery_view::search_button(layout.screen)
            .contains(point)
            .then_some(Msg::Pointer(Pointer::SearchStart));
    }
    if let Some(Overlay::Confirm(confirm)) = &state.overlay {
        return click
            .then(|| crate::dialog_view::confirm_at(confirm, layout.screen, point))
            .flatten()
            .map(|accept| Msg::Pointer(Pointer::Confirm(accept)));
    }
    if matches!(state.overlay, Some(Overlay::Settings { .. } | Overlay::Plugins { .. })) {
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
        return positions
            .iter()
            .zip(&all)
            .enumerate()
            .find_map(|(index, (&x, menu))| {
                let width = menu.title.width() as u16 + 4;
                (point.x >= x && point.x < x + width).then_some(Msg::Pointer(Pointer::Menu(index)))
            })
            .or(Some(Msg::Pointer(Pointer::Menu(0))));
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
    if state.activity.side.is_some() {
        let control = layout.panels.iter().any(|rect| {
            crate::results_view::controls(*rect, state.activity.expanded).iter().any(|(r, _, _)| r.contains(point))
        });
        if click && event.row == 0 && !control {
            return Some(Msg::Pointer(Pointer::Action(borg_core::Action::OpenMenuBar)));
        }
        return crate::activity_view::panel_mouse(state, event, double, &layout);
    }
    if state.docker.is_open() {
        let control = layout.panels.iter().any(|rect| {
            crate::results_view::controls(*rect, state.docker.expanded).iter().any(|(r, _, _)| r.contains(point))
        });
        if click && event.row == 0 && !control {
            return Some(Msg::Pointer(Pointer::Action(borg_core::Action::OpenMenuBar)));
        }
        return crate::docker_view::panel_mouse(state, event, double, &layout);
    }
    if click {
        for side in Side::ALL {
            if !state.panel(side).is_results() || state.aside.is_some() && side != state.active {
                continue;
            }
            let expanded = side == state.active && state.results_expanded();
            if let Some((_, _, action)) = crate::results_view::controls(layout.panels[side.index()], expanded)
                .into_iter()
                .find(|(rect, _, _)| rect.contains(point))
            {
                return Some(Msg::Pointer(Pointer::ResultControl { side, action }));
            }
        }
    }
    if click && event.row == 0 {
        return Some(Msg::Pointer(Pointer::Action(borg_core::Action::OpenMenuBar)));
    }
    if click && layout.keybar.contains(point) {
        return crate::keybar::action_at(state, layout.keybar, point).map(Msg::Pointer);
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
        // The footer shows a running size scan; clicking it stops the scan, like Alt-I.
        if click && point.y + 1 == rect.bottom() && state.sizing.as_ref().is_some_and(|scan| scan.side == side) {
            return Some(Msg::Pointer(Pointer::Action(borg_core::Action::MeasureSizes)));
        }
        if state.panel(side).is_results() {
            let (header, body) = crate::results_view::geometry(rect);
            if click && header.contains(point) {
                return crate::results_view::panel_columns(state.panel(side), header)
                    .into_iter()
                    .find(|(_, cell)| cell.contains(point))
                    .map(|(key, _)| Msg::Pointer(Pointer::ResultSort { side, key }));
            }
            if body.contains(point) {
                let index = state.panel(side).scroll() + usize::from(point.y - body.y);
                if click
                    && state.panel(side).is_tree()
                    && let Some(borg_core::Row::Entry(entry)) = state.panel(side).row(index)
                    && let Some((_, name)) = crate::results_view::panel_columns(state.panel(side), body)
                        .into_iter()
                        .find(|(key, _)| *key == borg_core::SortKey::Name)
                {
                    let arrow = name.x + crate::results_view::tree_indent(state.panel(side), entry, name.width) as u16;
                    if (arrow..arrow + 2).contains(&point.x) {
                        return Some(Msg::Pointer(Pointer::TreeNode { side, index }));
                    }
                }
                return Some(Msg::Pointer(Pointer::Panel { side, index, open: double && click, mark }));
            }
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
