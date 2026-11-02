//! The mouse in the dialogs: choosing their rows, the wheel moving through their lists, their
//! buttons, and the keys their borders list.

use borg_core::{AppState, Connect, Msg, Overlay, Pointer, RemoteBrowser};
use borg_i18n::tr;
use crossterm::event::{KeyCode, MouseButton, MouseEventKind};
use ratatui::layout::Rect;

use super::{Mouse, hinted};
use crate::discovery_view;

/// The mouse in dialogs: the keys their borders list take clicks, the wheel moves through lists,
/// rows and buttons take clicks, and a click beside a list or a notice closes it. Dialogs that
/// hold typing or ask a question stay open. `None` leaves the event to the other handlers.
pub(super) fn dialog_mouse(state: &AppState, mouse: &Mouse, screen: Rect) -> Option<Option<Msg>> {
    let (point, click) = (mouse.point, mouse.click);
    let overlay = state.overlay.as_ref()?;
    if let Some(delta) = step(mouse)
        && scrolls(overlay)
    {
        return Some(Some(Msg::Pointer(Pointer::DialogScroll(delta))));
    }
    match overlay {
        Overlay::Themes(picker) | Overlay::Languages(picker) => {
            let themes = matches!(overlay, Overlay::Themes(_));
            let names = crate::picker_view::names(themes.then(|| state.appearance.choices()));
            let (rect, at) = crate::picker_view::name_at(&names, screen, point);
            Some(match (click, at) {
                (true, Some(at)) => Some(pick_row(at, picker.selected)),
                (true, None) => hinted(rect, tr!(enter_keep_esc_cancel), true, point),
                _ => None,
            })
        }
        Overlay::Settings { .. } | Overlay::Plugins { .. } => {
            let rect = crate::settings_view::rect(state, screen);
            (click && crate::settings_view::item_at(state, point, screen).is_none())
                .then(|| hinted(rect, tr!(space_toggle_esc_close_saved_immediately), false, point))
        }
        Overlay::Jobs(_) => {
            let (rect, at) = crate::job_view::job_at(state.jobs.len(), screen, point);
            Some(match (click, at) {
                (true, Some(at)) => Some(Msg::Pointer(Pointer::DialogRow(at))),
                (true, None) => hinted(rect, tr!(p_pause_c_cancel_esc_close), true, point),
                _ => None,
            })
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
        Overlay::Game2048 => Some(game_mouse(mouse, screen)),
        Overlay::Folders(dialog) => Some(folders_mouse(state, dialog, mouse, screen)),
        Overlay::Remotes(browser) => Some(remotes_mouse(browser, mouse, screen)),
        Overlay::Connect(connect) => Some(connect_mouse(state, connect, mouse, screen)),
        Overlay::TimeMachine => Some(time_machine_mouse(state, mouse, screen)),
        Overlay::Collections(picker) => {
            let rect = discovery_view::collection_area(screen);
            if click && picker.creating.is_none() && !rect.contains(point) {
                return Some(Some(Msg::Pointer(Pointer::DismissDialog)));
            }
            border_key(mouse, rect, tr!(pgup_pgdn_enter_open_f7_n_new), false)
        }
        Overlay::Search(form) => {
            let rect = discovery_view::search_area(screen, form.recent(state).len());
            border_key(mouse, rect, tr!(tab_fields_mode_space_toggle_esc_cancel), false)
        }
        Overlay::Exclusions(editor) => {
            let rect = discovery_view::exclusions_area(editor, state, screen);
            border_key(mouse, rect, tr!(exclusions_keys), false)
        }
        Overlay::Prompt(prompt) => {
            let (rect, hint) = crate::dialog_view::prompt_hint(prompt, screen);
            border_key(mouse, rect, hint, true)
        }
        _ => None,
    }
}

/// The row a turn of the wheel moves a list's choice by, upward negative.
fn step(mouse: &Mouse) -> Option<isize> {
    match mouse.event.kind {
        MouseEventKind::ScrollUp => Some(-1),
        MouseEventKind::ScrollDown => Some(1),
        _ => None,
    }
}

/// Whether the wheel moves through the dialog's list.
fn scrolls(overlay: &Overlay) -> bool {
    match overlay {
        Overlay::Themes(_)
        | Overlay::Languages(_)
        | Overlay::Settings { .. }
        | Overlay::Plugins { .. }
        | Overlay::Jobs(_)
        | Overlay::Folders(_)
        | Overlay::Remotes(_)
        | Overlay::TimeMachine => true,
        Overlay::Exclusions(editor) => editor.editing.is_none(),
        _ => false,
    }
}

/// A row chosen with a click, and taken with a click once chosen.
fn pick_row(row: usize, selected: usize) -> Msg {
    if row == selected { Msg::Key(KeyCode::Enter.into()) } else { Msg::Pointer(Pointer::DialogRow(row)) }
}

/// A click on a key in the border of a dialog that stays open when clicked beside.
fn border_key(mouse: &Mouse, rect: Rect, hint: &str, centered: bool) -> Option<Option<Msg>> {
    if !mouse.click {
        return None;
    }
    crate::hints::bottom(rect, hint, centered, mouse.point).map(|key| Some(Msg::Key(key)))
}

/// A click toward a side of the 2048 board slides the tiles that way.
fn game_mouse(mouse: &Mouse, screen: Rect) -> Option<Msg> {
    let (point, rect) = (mouse.point, crate::game2048_view::rect(screen));
    if !mouse.click {
        return None;
    }
    if !(rect.contains(point) && point.y > rect.y && point.y + 1 < rect.bottom()) {
        return hinted(rect, crate::game2048_view::hint(rect), true, point);
    }
    let dx = (f32::from(point.x) - f32::from(rect.x + rect.width / 2)) / f32::from(rect.width.max(1));
    let dy = (f32::from(point.y) - f32::from(rect.y + rect.height / 2)) / f32::from(rect.height.max(1));
    let code = match (dx.abs() >= dy.abs(), dx < 0.0, dy < 0.0) {
        (true, true, _) => KeyCode::Left,
        (true, false, _) => KeyCode::Right,
        (false, _, true) => KeyCode::Up,
        (false, _, false) => KeyCode::Down,
    };
    Some(Msg::Key(code.into()))
}

/// The remote Borgs: a tab switches between the sessions and the clients, and a click chooses
/// a row.
fn remotes_mouse(browser: &RemoteBrowser, mouse: &Mouse, screen: Rect) -> Option<Msg> {
    use crate::remote_view::{RemoteSpot, keys, rect, spot};
    if !mouse.click {
        return None;
    }
    match spot(browser, screen, mouse.point) {
        Some(RemoteSpot::Tab { clients }) => (clients != browser.show_clients).then(|| Msg::Key(KeyCode::Tab.into())),
        Some(RemoteSpot::Row(at)) if browser.show_clients => Some(Msg::Pointer(Pointer::DialogRow(at))),
        Some(RemoteSpot::Row(at)) => Some(pick_row(at, browser.selected)),
        None => {
            let rect = rect(screen);
            hinted(rect, keys(browser, rect), true, mouse.point)
        }
    }
}

/// The ways to connect an agent, which a click chooses.
fn connect_mouse(state: &AppState, connect: &Connect, mouse: &Mouse, screen: Rect) -> Option<Msg> {
    if !mouse.click {
        return None;
    }
    let point = mouse.point;
    let (rect, picks) = crate::agent_view::connect_picks(state, connect, screen);
    match picks.into_iter().find(|(_, rows)| rows.contains(&point.y) && rect.contains(point)) {
        Some((pick, _)) => Some(pick_row(pick, connect.pick)),
        None => hinted(rect, tr!(agent_connect_hint), true, point),
    }
}

/// Time Machine: its tabs, and its items, which a right click marks as in the panels.
fn time_machine_mouse(state: &AppState, mouse: &Mouse, screen: Rect) -> Option<Msg> {
    use crate::backup_view::BackupSpot;
    let window = &state.backup.window;
    let (rect, at) = crate::backup_view::spot(window, screen, mouse.point);
    let right = matches!(mouse.event.kind, MouseEventKind::Down(MouseButton::Right));
    match at {
        Some(BackupSpot::Item(index)) if right => Some(Msg::Pointer(Pointer::DialogMark(index))),
        _ if !mouse.click => None,
        Some(BackupSpot::Tab(digit)) => Some(Msg::Key(KeyCode::Char(digit).into())),
        Some(BackupSpot::Item(index)) => Some(pick_row(index, window.list().cursor)),
        None => hinted(rect, crate::backup_view::keys(window, rect), true, mouse.point),
    }
}

/// The folders dialog: a click chooses a folder and a second one opens it, the keys in its
/// border take clicks, and a click beside it closes it.
fn folders_mouse(state: &AppState, dialog: &borg_core::FoldersDialog, mouse: &Mouse, screen: Rect) -> Option<Msg> {
    if !mouse.click {
        return None;
    }
    let point = mouse.point;
    let list = crate::folders_view::dialog_list(state, screen);
    if list.contains(point) {
        let index = crate::folders_view::dialog_start(state, dialog, screen) + usize::from(point.y - list.y);
        let count = borg_core::folder_choices(state, dialog.filter.text()).len();
        return (index < count).then(|| pick_row(index, dialog.selected));
    }
    let rect = crate::folders_view::dialog_area(state, screen);
    if !rect.contains(point) {
        return Some(Msg::Pointer(Pointer::DismissDialog));
    }
    crate::hints::bottom(rect, tr!(folders_keys), false, point).map(Msg::Key)
}
