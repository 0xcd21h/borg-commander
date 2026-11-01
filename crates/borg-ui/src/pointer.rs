use std::time::Instant;

use borg_core::{AppState, Msg, Overlay, Pointer, Side, View, menus};
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Constraint, Layout, Position, Rect};
use unicode_width::UnicodeWidthStr;

use crate::{menu_view, panel_view};

mod dialogs;

use dialogs::dialog_mouse;

const WHEEL_ROWS: isize = 3;

pub fn mouse_input(state: &AppState, event: MouseEvent, double: bool, now: Instant) -> Option<Msg> {
    borg_i18n::with_locale(state.settings.preferences.language, || mouse_localized(state, event, double, now))
}

/// A mouse event with what the handlers ask of it.
struct Mouse {
    event: MouseEvent,
    point: Position,
    click: bool,
    double: bool,
}

impl Mouse {
    /// The rows a turn of the wheel scrolls, upward negative.
    fn wheel(&self) -> Option<isize> {
        match self.event.kind {
            MouseEventKind::ScrollUp => Some(-WHEEL_ROWS),
            MouseEventKind::ScrollDown => Some(WHEEL_ROWS),
            _ => None,
        }
    }
}

/// What the mouse does, asking each part of the screen in turn from the top: the agent's
/// dialogs, the editor and the viewer, then dialogs, menus, the monitors, and the panels.
fn mouse_localized(state: &AppState, event: MouseEvent, double: bool, now: Instant) -> Option<Msg> {
    let point = Position::new(event.column, event.row);
    let click = matches!(event.kind, MouseEventKind::Down(MouseButton::Left));
    let mouse = Mouse { event, point, click, double };
    // An agent's dialogs show over the console and the viewer too, and take the mouse there.
    if matches!(state.overlay, Some(Overlay::Approval(_) | Overlay::AgentLog { .. })) {
        return agent_dialog_mouse(state, &mouse, state.layout(now).screen);
    }
    if let Some(editor) = &state.editor {
        let screen = state.layout(now).screen;
        return crate::editor_view::mouse(editor, event, double, (screen, screen), WHEEL_ROWS);
    }
    if let Some(viewer) = state.viewer.as_ref().filter(|_| !state.shell.captured()) {
        return match mouse.wheel() {
            Some(delta) => Some(Msg::Pointer(Pointer::ScrollDocument(delta))),
            None if click => crate::viewer_view::click(viewer, state.layout(now).screen, point).map(Msg::Key),
            None => None,
        };
    }
    let layout = state.layout(now);
    if state.shell.captured() || state.effective_view() != View::Panels {
        return None;
    }
    if let Some(handled) = dialog_mouse(state, &mouse, layout.screen) {
        return handled;
    }
    if let Some(handled) = overlay_mouse(state, &mouse, layout.screen) {
        return handled;
    }
    if let Some(handled) = menu_mouse(state, &mouse, &layout) {
        return handled;
    }
    if state.activity.side.is_some() || state.docker.is_open() {
        return monitor_mouse(state, &mouse, &layout);
    }
    if let Some(handled) = chrome_mouse(state, &mouse, &layout) {
        return handled;
    }
    let side = Side::ALL.into_iter().find(|side| layout.panels[side.index()].contains(point))?;
    panel_mouse(state, side, &mouse, layout.panels[side.index()], (layout.screen, now))
}

/// The wheel scrolls an agent's dialog, and a click takes its buttons or the keys its border lists.
fn agent_dialog_mouse(state: &AppState, mouse: &Mouse, screen: Rect) -> Option<Msg> {
    if let Some(delta) = mouse.wheel() {
        return Some(Msg::Pointer(Pointer::DialogScroll(delta)));
    }
    if !mouse.click {
        return None;
    }
    if let Some(Overlay::Approval(approval)) = &state.overlay {
        return crate::agent_view::approval_button_at(approval, screen, mouse.point)
            .map(|accept| Msg::Pointer(Pointer::Confirm(accept)));
    }
    let (rect, _) = borg_core::log_layout(screen);
    hinted(rect, borg_i18n::tr!(agent_log_hint), true, mouse.point)
}

/// The mouse in a window over the panels that takes all of it while it is open.
fn overlay_mouse(state: &AppState, mouse: &Mouse, screen: Rect) -> Option<Option<Msg>> {
    let (click, point) = (mouse.click, mouse.point);
    let pointer = match state.overlay.as_ref()? {
        Overlay::Prompt(prompt) => click
            .then(|| crate::dialog_view::prompt_field_at(prompt, screen, point))
            .flatten()
            .map(|option| Pointer::PromptField { option }),
        Overlay::Collections(picker) => collections_mouse(state, picker, mouse, screen),
        Overlay::Search(form) => {
            let recent = form.recent(state).len();
            let switch = (click && form.shows(borg_core::CONTENTS_FIELD))
                .then(|| crate::discovery_view::contents_switch(screen, recent, point));
            if let Some(key) = switch.flatten() {
                return Some(Some(Msg::Key(key)));
            }
            search_form_mouse(state, form, mouse, screen)
        }
        Overlay::Exclusions(editor) => click
            .then(|| crate::discovery_view::exclusion_row(editor, state, screen, point))
            .flatten()
            .map(Pointer::Exclusion),
        Overlay::Confirm(confirm) => {
            click.then(|| crate::dialog_view::confirm_at(confirm, screen, point)).flatten().map(Pointer::Confirm)
        }
        Overlay::Settings { .. } | Overlay::Plugins { .. } => {
            click.then(|| crate::settings_view::item_at(state, point, screen)).flatten().map(Pointer::Setting)
        }
        _ => return None,
    };
    Some(pointer.map(Msg::Pointer))
}

/// The collections picker: the wheel scrolls it, a click picks or opens a collection, and its
/// buttons search or start a new one.
fn collections_mouse(
    state: &AppState,
    picker: &borg_core::CollectionPicker,
    mouse: &Mouse,
    screen: Rect,
) -> Option<Pointer> {
    if let Some(delta) = mouse.wheel() {
        return Some(Pointer::CollectionScroll(delta));
    }
    let point = mouse.point;
    let list = crate::discovery_view::collection_list(screen);
    if mouse.click && list.contains(point) {
        let index = crate::discovery_view::collection_start(picker, screen) + usize::from(point.y - list.y);
        return (index < state.collections.len()).then_some(Pointer::Collection { index, open: mouse.double });
    }
    let area = crate::discovery_view::collection_area(screen);
    if !mouse.click || !area.contains(point) {
        return None;
    }
    if picker.creating.is_some() {
        return (point.y == area.bottom().saturating_sub(3) && point.x < area.x.saturating_add(14))
            .then_some(Pointer::SearchStart);
    }
    if point.y != area.bottom().saturating_sub(4) {
        return None;
    }
    let new_start = area.x.saturating_add(2 + borg_i18n::tr!(collections_open_button).width() as u16 + 5);
    Some(if point.x < new_start { Pointer::SearchStart } else { Pointer::NewCollection })
}

/// The search form: a click takes a field, a recent search, the exclusions or the start button.
fn search_form_mouse(state: &AppState, form: &borg_core::SearchForm, mouse: &Mouse, screen: Rect) -> Option<Pointer> {
    if !mouse.click {
        return None;
    }
    let point = mouse.point;
    let recent = form.recent(state);
    let sizes = matches!(form.kind, borg_core::SearchKind::FolderTotals | borg_core::SearchKind::FolderTree);
    if !sizes && crate::discovery_view::edit_exclusions(screen, recent.len()).contains(point) {
        return Some(Pointer::EditExclusions);
    }
    if let Some(field) = crate::discovery_view::search_field(screen, recent.len(), point) {
        return Some(Pointer::SearchField(field));
    }
    if let Some(at) = crate::discovery_view::search_recent(screen, &recent, point) {
        return Some(Pointer::SearchRecent(at));
    }
    crate::discovery_view::search_button(screen, recent.len()).contains(point).then_some(Pointer::SearchStart)
}

/// The menu bar and an open menu: a click on a title opens that menu, one on an item chooses
/// it, and one beside the menu closes it. Other windows over the panels take no clicks.
fn menu_mouse(state: &AppState, mouse: &Mouse, layout: &borg_core::Layout) -> Option<Option<Msg>> {
    let open = match state.overlay {
        Some(Overlay::Menu(cursor)) => Some(cursor),
        // Finding in a panel ends at a click, which then acts on the panels.
        Some(Overlay::Find(_)) | None => None,
        Some(_) => return Some(None),
    };
    let point = mouse.point;
    let all = menus(state);
    let bar = layout.menu.or_else(|| open.map(|_| Rect { height: 1, ..layout.screen }));
    if mouse.click && bar.is_some_and(|bar| bar.contains(point)) {
        let positions = menu_view::title_positions(&all);
        let index = positions
            .iter()
            .zip(&all)
            .position(|(&x, menu)| point.x >= x && point.x < x + menu.title.width() as u16 + 4);
        return Some(Some(Msg::Pointer(Pointer::Menu(index.unwrap_or(0)))));
    }
    let cursor = open?;
    if !mouse.click {
        return Some(None);
    }
    let dropdown = || {
        let menu = all.get(cursor.menu)?;
        let x = menu_view::title_positions(&all)[cursor.menu];
        let rect = menu_view::dropdown_rect(menu, x, bar?.bottom(), layout.screen);
        let inner = Rect {
            x: rect.x + 1,
            y: rect.y + 1,
            width: rect.width.saturating_sub(2),
            height: rect.height.saturating_sub(2),
        };
        if inner.contains(point) {
            Some(Pointer::MenuItem(usize::from(point.y - inner.y)))
        } else {
            (!rect.contains(point)).then_some(Pointer::DismissMenu)
        }
    };
    Some(dropdown().map(Msg::Pointer))
}

/// The activity monitor and the container view take the mouse in the panels; a click on the
/// top row opens the menus, unless it is on a pane's controls.
fn monitor_mouse(state: &AppState, mouse: &Mouse, layout: &borg_core::Layout) -> Option<Msg> {
    let activity = state.activity.side.is_some();
    let expanded = if activity { state.activity.expanded } else { state.docker.expanded };
    let control = layout
        .panels
        .iter()
        .any(|rect| crate::results_view::controls(*rect, expanded).iter().any(|(r, _, _)| r.contains(mouse.point)));
    if mouse.click && mouse.event.row == 0 && !control {
        return Some(Msg::Pointer(Pointer::Action(borg_core::Action::OpenMenuBar)));
    }
    if activity {
        crate::activity_view::panel_mouse(state, mouse.event, mouse.double, layout)
    } else {
        crate::docker_view::panel_mouse(state, mouse.event, mouse.double, layout)
    }
}

/// What sits around and over the panels: results' controls, the menu bar's row, the key bar,
/// and the dialog of the active panel's editor, which may reach over the other panel.
fn chrome_mouse(state: &AppState, mouse: &Mouse, layout: &borg_core::Layout) -> Option<Option<Msg>> {
    let (click, point) = (mouse.click, mouse.point);
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
                return Some(Some(Msg::Pointer(Pointer::ResultControl { side, action })));
            }
        }
    }
    if click && mouse.event.row == 0 {
        return Some(Some(Msg::Pointer(Pointer::Action(borg_core::Action::OpenMenuBar))));
    }
    let docked = borg_core::focused_editor(state).is_some();
    if click && layout.keybar.contains(point) {
        return Some(if docked {
            crate::editor_view::keybar_click(state, layout.keybar, point)
        } else {
            crate::keybar::action_at(state, layout.keybar, point).map(Msg::Pointer)
        });
    }
    let active = state.docked[state.active.index()].as_ref().filter(|_| docked);
    let editor = active.filter(|editor| borg_core::editor_dialog(editor).is_some())?;
    let area = (layout.panels[state.active.index()], layout.screen);
    Some(crate::editor_view::mouse(editor, mouse.event, mouse.double, area, WHEEL_ROWS))
}

/// The mouse in the panel on `side`: its editor or the preview take it when they show there;
/// otherwise the wheel scrolls the panel, a click picks an entry and a right click marks one.
fn panel_mouse(state: &AppState, side: Side, mouse: &Mouse, rect: Rect, (screen, now): (Rect, Instant)) -> Option<Msg> {
    if let Some(editor) = &state.docked[side.index()] {
        return crate::editor_view::mouse(editor, mouse.event, mouse.double, (rect, screen), WHEEL_ROWS);
    }
    if state.aside.is_some() && side != state.active {
        return match mouse.wheel() {
            Some(delta) => Some(Msg::Pointer(Pointer::ScrollDocument(delta))),
            None => mouse.click.then_some(Msg::Pointer(Pointer::FocusPreview)),
        };
    }
    if let Some(delta) = mouse.wheel() {
        return Some(Msg::Pointer(Pointer::ScrollPanel { side, delta }));
    }
    let (click, point) = (mouse.click, mouse.point);
    let mark = mouse.event.kind == MouseEventKind::Down(MouseButton::Right);
    if !click && !mark {
        return None;
    }
    // The footer shows a running size scan; clicking it stops the scan, like Alt-I.
    if click && point.y + 1 == rect.bottom() && state.sizing.as_ref().is_some_and(|scan| scan.side == side) {
        return Some(Msg::Pointer(Pointer::Action(borg_core::Action::MeasureSizes)));
    }
    let open = mouse.double && click;
    if state.panel(side).is_results() {
        return results_mouse(state, side, rect, mouse, mark);
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
    Some(Msg::Pointer(Pointer::Panel { side, index, open, mark }))
}

/// A click in results: on a column heading it sorts by it, on a tree's arrow it opens or closes
/// the branch, and on a row it picks the entry.
fn results_mouse(state: &AppState, side: Side, rect: Rect, mouse: &Mouse, mark: bool) -> Option<Msg> {
    let (click, point) = (mouse.click, mouse.point);
    let panel = state.panel(side);
    let (header, body) = crate::results_view::geometry(rect);
    if click && header.contains(point) {
        return crate::results_view::panel_columns(panel, header)
            .into_iter()
            .find(|(_, cell)| cell.contains(point))
            .map(|(key, _)| Msg::Pointer(Pointer::ResultSort { side, key }));
    }
    if !body.contains(point) {
        return None;
    }
    let index = panel.scroll() + usize::from(point.y - body.y);
    if click
        && panel.is_tree()
        && let Some(borg_core::Row::Entry(entry)) = panel.row(index)
        && let Some((_, name)) = crate::results_view::panel_columns(panel, body)
            .into_iter()
            .find(|(key, _)| *key == borg_core::SortKey::Name)
    {
        let arrow = name.x + crate::results_view::tree_indent(panel, entry, name.width) as u16;
        if (arrow..arrow + 2).contains(&point.x) {
            return Some(Msg::Pointer(Pointer::TreeNode { side, index }));
        }
    }
    Some(Msg::Pointer(Pointer::Panel { side, index, open: mouse.double && click, mark }))
}

/// A click on a key that `rect`'s border lists, or a click beside a dialog that closes on one.
fn hinted(rect: Rect, hint: &str, centered: bool, point: Position) -> Option<Msg> {
    if !rect.contains(point) {
        return Some(Msg::Pointer(Pointer::DismissDialog));
    }
    crate::hints::bottom(rect, hint, centered, point).map(Msg::Key)
}
