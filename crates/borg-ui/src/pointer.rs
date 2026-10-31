use std::time::Instant;

use borg_core::{AppState, Msg, Overlay, Pointer, Side, View, menus};
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Constraint, Layout, Position, Rect};
use unicode_width::UnicodeWidthStr;

use crate::{menu_view, panel_view};

const WHEEL_ROWS: isize = 3;

pub fn mouse_input(state: &AppState, event: MouseEvent, double: bool, now: Instant) -> Option<Msg> {
    borg_i18n::with_locale(state.settings.preferences.language, || mouse_localized(state, event, double, now))
}

fn mouse_localized(state: &AppState, event: MouseEvent, double: bool, now: Instant) -> Option<Msg> {
    let point = Position::new(event.column, event.row);
    let click = matches!(event.kind, MouseEventKind::Down(MouseButton::Left));
    // An agent's dialogs show over the console and the viewer too, and take the mouse there.
    if let Some(Overlay::Approval(_) | Overlay::AgentLog { .. }) = &state.overlay {
        match event.kind {
            MouseEventKind::ScrollUp => return Some(Msg::Pointer(Pointer::DialogScroll(-WHEEL_ROWS))),
            MouseEventKind::ScrollDown => return Some(Msg::Pointer(Pointer::DialogScroll(WHEEL_ROWS))),
            _ => {}
        }
        if let Some(Overlay::Approval(approval)) = &state.overlay {
            return click
                .then(|| crate::agent_view::approval_button_at(approval, state.layout(now).screen, point))
                .flatten()
                .map(|accept| Msg::Pointer(Pointer::Confirm(accept)));
        }
        let (rect, _) = borg_core::log_layout(state.layout(now).screen);
        return click.then(|| hinted(rect, borg_i18n::tr!(agent_log_hint), true, point)).flatten();
    }
    if let Some(editor) = &state.editor {
        return crate::editor_view::mouse(editor, event, double, state.layout(now).screen, WHEEL_ROWS);
    }
    if state.viewer.is_some() && !state.shell.captured() {
        return match event.kind {
            MouseEventKind::ScrollUp => Some(Msg::Pointer(Pointer::ScrollDocument(-WHEEL_ROWS))),
            MouseEventKind::ScrollDown => Some(Msg::Pointer(Pointer::ScrollDocument(WHEEL_ROWS))),
            _ => None,
        };
    }
    let layout = state.layout(now);
    if state.shell.captured() || state.effective_view() != View::Panels {
        return None;
    }
    let all = menus(state);
    if let Some(handled) = dialog_mouse(state, event, layout.screen) {
        return handled;
    }
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
    if let Some(Overlay::Search(form)) = &state.overlay {
        if !click {
            return None;
        }
        let recent = form.recent(state);
        let sizes = matches!(form.kind, borg_core::SearchKind::FolderTotals | borg_core::SearchKind::FolderTree);
        if !sizes && crate::discovery_view::edit_exclusions(layout.screen, recent.len()).contains(point) {
            return Some(Msg::Pointer(Pointer::EditExclusions));
        }
        if let Some(field) = crate::discovery_view::search_field(layout.screen, recent.len(), point) {
            return Some(Msg::Pointer(Pointer::SearchField(field)));
        }
        if let Some(at) = crate::discovery_view::search_recent(layout.screen, &recent, point) {
            return Some(Msg::Pointer(Pointer::SearchRecent(at)));
        }
        return crate::discovery_view::search_button(layout.screen, recent.len())
            .contains(point)
            .then_some(Msg::Pointer(Pointer::SearchStart));
    }
    if let Some(Overlay::Exclusions(editor)) = &state.overlay {
        return click
            .then(|| crate::discovery_view::exclusion_row(editor, state, layout.screen, point))
            .flatten()
            .map(|row| Msg::Pointer(Pointer::Exclusion(row)));
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
        // Finding in a panel ends at a click, which then acts on the panels.
        Some(Overlay::Find(_)) | None => None,
        Some(_) => return None,
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

/// A click on a key that `rect`'s border lists, or a click beside a dialog that closes on one.
fn hinted(rect: Rect, hint: &str, centered: bool, point: Position) -> Option<Msg> {
    if !rect.contains(point) {
        return Some(Msg::Pointer(Pointer::DismissDialog));
    }
    crate::hints::bottom(rect, hint, centered, point).map(Msg::Key)
}

/// The mouse in dialogs: the keys their borders list take clicks, the wheel moves through lists,
/// rows and buttons take clicks, and a click beside a list or a notice closes it. Dialogs that
/// hold typing or ask a question stay open. `None` leaves the event to the other handlers.
fn dialog_mouse(state: &AppState, event: MouseEvent, screen: Rect) -> Option<Option<Msg>> {
    use crate::discovery_view;
    use borg_i18n::tr;
    use crossterm::event::KeyCode;
    let point = Position::new(event.column, event.row);
    let click = matches!(event.kind, MouseEventKind::Down(MouseButton::Left));
    let wheel = match event.kind {
        MouseEventKind::ScrollUp => Some(-1),
        MouseEventKind::ScrollDown => Some(1),
        _ => None,
    };
    let scroll = |delta: isize| Some(Some(Msg::Pointer(Pointer::DialogScroll(delta))));
    // A key in the border of a dialog that stays open when clicked beside.
    let keys = |rect: Rect, hint: &str, centered: bool| {
        crate::hints::bottom(rect, hint, centered, point).map(|key| Some(Msg::Key(key)))
    };
    // A row chosen with a click, and taken with a click once chosen.
    let row = |row: usize, selected: usize| {
        Some(Some(if row == selected {
            Msg::Key(KeyCode::Enter.into())
        } else {
            Msg::Pointer(Pointer::DialogRow(row))
        }))
    };
    match state.overlay.as_ref()? {
        Overlay::Themes(picker) | Overlay::Languages(picker) => {
            let themes = matches!(state.overlay, Some(Overlay::Themes(_)));
            let names = crate::picker_view::names(themes.then(|| state.appearance.choices()));
            if let Some(delta) = wheel {
                return scroll(delta);
            }
            let (rect, at) = crate::picker_view::name_at(&names, screen, point);
            match (click, at) {
                (true, Some(at)) => row(at, picker.selected),
                (true, None) => Some(hinted(rect, tr!(enter_keep_esc_cancel), true, point)),
                _ => Some(None),
            }
        }
        Overlay::Settings { .. } | Overlay::Plugins { .. } => {
            if let Some(delta) = wheel {
                return scroll(delta);
            }
            let rect = crate::settings_view::rect(state, screen);
            (click && crate::settings_view::item_at(state, point, screen).is_none())
                .then(|| hinted(rect, tr!(space_toggle_esc_close_saved_immediately), false, point))
        }
        Overlay::Jobs(_) => {
            if let Some(delta) = wheel {
                return scroll(delta);
            }
            let (rect, at) = crate::job_view::job_at(state.jobs.len(), screen, point);
            match (click, at) {
                (true, Some(at)) => Some(Some(Msg::Pointer(Pointer::DialogRow(at)))),
                (true, None) => Some(hinted(rect, tr!(p_pause_c_cancel_esc_close), true, point)),
                _ => Some(None),
            }
        }
        Overlay::Question(question) => {
            let job = state.jobs.iter().find(|job| job.id == question.id)?;
            let (_, buttons) = crate::job_view::question_buttons(job, screen);
            let hit = buttons.into_iter().find(|(rect, _)| click && rect.contains(point));
            Some(hit.map(|(_, hotkey)| Msg::Key(KeyCode::Char(hotkey).into())))
        }
        Overlay::About(_) => {
            let rect = crate::art_view::about_rect(screen);
            Some(click.then(|| hinted(rect, tr!(about_keys), true, point)).flatten())
        }
        Overlay::Game2048 => {
            let rect = crate::game2048_view::rect(screen);
            if !click {
                return Some(None);
            }
            if rect.contains(point) && point.y > rect.y && point.y + 1 < rect.bottom() {
                // A click toward a side of the board slides the tiles that way.
                let dx = (f32::from(point.x) - f32::from(rect.x + rect.width / 2)) / f32::from(rect.width.max(1));
                let dy = (f32::from(point.y) - f32::from(rect.y + rect.height / 2)) / f32::from(rect.height.max(1));
                let code = match (dx.abs() >= dy.abs(), dx < 0.0, dy < 0.0) {
                    (true, true, _) => KeyCode::Left,
                    (true, false, _) => KeyCode::Right,
                    (false, _, true) => KeyCode::Up,
                    (false, _, false) => KeyCode::Down,
                };
                return Some(Some(Msg::Key(code.into())));
            }
            Some(hinted(rect, crate::game2048_view::hint(rect), true, point))
        }
        Overlay::Remotes(browser) => {
            if let Some(delta) = wheel {
                return scroll(delta);
            }
            if !click {
                return Some(None);
            }
            let rect = crate::remote_view::rect(screen);
            Some(match crate::remote_view::spot(browser, screen, point) {
                Some(crate::remote_view::RemoteSpot::Tab { clients }) => {
                    (clients != browser.show_clients).then(|| Msg::Key(KeyCode::Tab.into()))
                }
                Some(crate::remote_view::RemoteSpot::Row(at)) if browser.show_clients => {
                    Some(Msg::Pointer(Pointer::DialogRow(at)))
                }
                Some(crate::remote_view::RemoteSpot::Row(at)) => row(at, browser.selected).flatten(),
                None => hinted(rect, crate::remote_view::keys(browser, rect), true, point),
            })
        }
        Overlay::Connect(connect) => {
            if !click {
                return Some(None);
            }
            let (rect, picks) = crate::agent_view::connect_picks(state, connect, screen);
            match picks.into_iter().find(|(_, rows)| rows.contains(&point.y) && rect.contains(point)) {
                Some((pick, _)) => row(pick, connect.pick),
                None => Some(hinted(rect, tr!(agent_connect_hint), true, point)),
            }
        }
        Overlay::TimeMachine => {
            use crate::backup_view::BackupSpot;
            let window = &state.backup.window;
            if let Some(delta) = wheel {
                return scroll(delta);
            }
            let (rect, at) = crate::backup_view::spot(window, screen, point);
            let right = matches!(event.kind, MouseEventKind::Down(MouseButton::Right));
            Some(match at {
                Some(BackupSpot::Item(index)) if right => Some(Msg::Pointer(Pointer::DialogMark(index))),
                _ if !click => None,
                Some(BackupSpot::Tab(digit)) => Some(Msg::Key(KeyCode::Char(digit).into())),
                Some(BackupSpot::Item(index)) => row(index, window.list().cursor).flatten(),
                None => hinted(rect, crate::backup_view::keys(window, rect), true, point),
            })
        }
        Overlay::Collections(picker) => {
            let rect = discovery_view::collection_area(screen);
            if click && picker.creating.is_none() && !rect.contains(point) {
                return Some(Some(Msg::Pointer(Pointer::DismissDialog)));
            }
            click.then(|| keys(rect, tr!(pgup_pgdn_enter_open_f7_n_new), false)).flatten()
        }
        Overlay::Search(form) => {
            let rect = discovery_view::search_area(screen, form.recent(state).len());
            click.then(|| keys(rect, tr!(tab_fields_mode_space_toggle_esc_cancel), false)).flatten()
        }
        Overlay::Exclusions(editor) => {
            if let Some(delta) = wheel.filter(|_| editor.editing.is_none()) {
                return scroll(delta);
            }
            let rect = discovery_view::exclusions_area(editor, state, screen);
            click.then(|| keys(rect, tr!(exclusions_keys), false)).flatten()
        }
        Overlay::Prompt(prompt) => {
            let (rect, hint) = crate::dialog_view::prompt_hint(prompt, screen);
            click.then(|| keys(rect, hint, true)).flatten()
        }
        _ => None,
    }
}
