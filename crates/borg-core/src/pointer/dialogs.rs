//! Clicks and the wheel in the window open over the panels: its rows, fields and buttons, and
//! a click beside it.

use std::time::Instant;

use crossterm::event::KeyCode;

use super::Pointer;
use crate::{AppState, Effect, MenuCursor, Overlay, actions, menus};

pub(super) fn on_pointer(state: &mut AppState, pointer: Pointer, overlay: Overlay, now: Instant) -> Vec<Effect> {
    match (pointer, overlay) {
        (Pointer::PromptField { option }, Overlay::Prompt(mut prompt)) => {
            prompt.option_focused = option;
            if option {
                prompt.purpose.toggle_option();
            }
            show(state, Overlay::Prompt(prompt))
        }
        (Pointer::DialogScroll(delta), Overlay::Approval(_) | Overlay::AgentLog { .. }) => {
            crate::agent::scroll_dialog(state, delta);
            Vec::new()
        }
        (Pointer::DialogScroll(delta), overlay) => scroll_list(state, overlay, delta, now),
        (Pointer::DialogRow(row), overlay) => {
            select_row(state, overlay, row, now);
            Vec::new()
        }
        (Pointer::DialogMark(row), Overlay::TimeMachine) => {
            state.backup.window.point(row, true);
            Vec::new()
        }
        (Pointer::DismissDialog, _) => press(state, KeyCode::Esc, now),
        (Pointer::Confirm(accept), Overlay::Confirm(_) | Overlay::Approval(_)) => {
            press(state, KeyCode::Char(if accept { 'y' } else { 'n' }), now)
        }
        (Pointer::Collection { index, open }, Overlay::Collections(picker)) if picker.creating.is_none() => {
            pick_collection(state, picker, index, open, now)
        }
        (Pointer::CollectionScroll(delta), Overlay::Collections(mut picker)) if picker.creating.is_none() => {
            picker.selected =
                picker.selected.saturating_add_signed(delta).min(state.collections.len().saturating_sub(1));
            show(state, Overlay::Collections(picker))
        }
        (Pointer::NewCollection, Overlay::Collections(mut picker)) => {
            picker.creating = Some(crate::LineInput::new(String::new()));
            show(state, Overlay::Collections(picker))
        }
        (Pointer::SearchField(selected), Overlay::Search(form)) => pick_search_field(state, form, selected, now),
        (Pointer::SearchRecent(at), Overlay::Search(mut form)) => {
            form.take_recent(state, at);
            show(state, Overlay::Search(form))
        }
        (Pointer::EditExclusions, Overlay::Search(form)) => crate::exclusions::open(state, form),
        (Pointer::Exclusion(row), Overlay::Exclusions(mut editor)) if editor.editing.is_none() => {
            let effects = editor.activate(state, row);
            state.overlay = Some(Overlay::Exclusions(editor));
            effects
        }
        (Pointer::SearchStart, Overlay::Search(_) | Overlay::Collections(_)) => press(state, KeyCode::Enter, now),
        (Pointer::MenuItem(index), Overlay::Menu(cursor)) => choose_menu_item(state, cursor, index, now),
        (Pointer::Setting(selected), Overlay::Settings { .. } | Overlay::Plugins { .. }) => {
            crate::overlays::toggle_setting(state, selected, now)
        }
        (Pointer::DismissMenu, Overlay::Menu(_)) => {
            state.overlay = None;
            Vec::new()
        }
        _ => Vec::new(),
    }
}

fn show(state: &mut AppState, overlay: Overlay) -> Vec<Effect> {
    state.overlay = Some(overlay);
    Vec::new()
}

/// Does what `code` does in the open window.
fn press(state: &mut AppState, code: KeyCode, now: Instant) -> Vec<Effect> {
    crate::overlays::on_key(state, &code.into(), now)
}

/// A collection in the list of them, which a double click opens.
fn pick_collection(
    state: &mut AppState,
    mut picker: crate::CollectionPicker,
    index: usize,
    open: bool,
    now: Instant,
) -> Vec<Effect> {
    picker.selected = index.min(state.collections.len().saturating_sub(1));
    state.overlay = Some(Overlay::Collections(picker));
    if open { press(state, KeyCode::Enter, now) } else { Vec::new() }
}

/// A field of the search dialog; a click on one of its switches flips it, as Right does.
fn pick_search_field(state: &mut AppState, mut form: crate::SearchForm, selected: usize, now: Instant) -> Vec<Effect> {
    form.selected = selected.min(form.fields() - 1);
    let toggle = matches!(form.selected, 2 | 5 | 6);
    state.overlay = Some(Overlay::Search(form));
    if toggle { press(state, KeyCode::Right, now) } else { Vec::new() }
}

/// An item of the open menu, which closes it and does what the item names, if it can be chosen.
fn choose_menu_item(state: &mut AppState, cursor: MenuCursor, index: usize, now: Instant) -> Vec<Effect> {
    let chosen = menus(state)
        .get(cursor.menu)
        .and_then(|menu| menu.item(index))
        .filter(|item| item.enabled)
        .map(|item| item.action);
    let Some(action) = chosen else { return Vec::new() };
    state.last_menu = Some(MenuCursor { item: Some(index), ..cursor });
    state.overlay = None;
    actions::perform(state, action, now)
}

/// The wheel over a list dialog moves its choice by rows.
fn scroll_list(state: &mut AppState, overlay: Overlay, delta: isize, now: Instant) -> Vec<Effect> {
    if let Some((selected, count)) = list_position(state, &overlay) {
        let row = selected.saturating_add_signed(delta).min(count.saturating_sub(1));
        select_row(state, overlay, row, now);
    }
    Vec::new()
}

/// The chosen row of a list dialog and how many rows it has.
fn list_position(state: &AppState, overlay: &Overlay) -> Option<(usize, usize)> {
    Some(match overlay {
        Overlay::Themes(picker) => (picker.selected, state.appearance.choices().len()),
        Overlay::Languages(picker) => (picker.selected, borg_i18n::Locale::ALL.len()),
        Overlay::Settings { selected } | Overlay::Plugins { selected } => {
            (*selected, crate::menu::checkbox_items(state).len())
        }
        Overlay::Jobs(list) => (list.selected, state.jobs.len()),
        Overlay::Remotes(browser) if browser.show_clients => {
            (browser.client_selected, browser.clients.as_ref().map_or(0, Vec::len))
        }
        Overlay::Remotes(browser) => (browser.selected, browser.choices.len()),
        Overlay::Exclusions(editor) if editor.editing.is_none() => {
            (editor.selected, crate::ExclusionEditor::rows(state))
        }
        Overlay::TimeMachine => {
            let list = state.backup.window.list();
            (list.cursor, list.items.len())
        }
        _ => return None,
    })
}

/// Chooses `row` in a list dialog as the arrow keys would, previewing a theme as they do.
fn select_row(state: &mut AppState, overlay: Overlay, row: usize, now: Instant) {
    let Some((_, count)) = list_position(state, &overlay).or(matches!(overlay, Overlay::Connect(_)).then_some((0, 3)))
    else {
        return;
    };
    let row = row.min(count.saturating_sub(1));
    state.overlay = Some(match overlay {
        Overlay::Themes(picker) => {
            let fade = state.motion().fade;
            state.appearance.select(row, now, fade);
            Overlay::Themes(crate::Picker { selected: row, ..picker })
        }
        Overlay::Languages(picker) => Overlay::Languages(crate::Picker { selected: row, ..picker }),
        Overlay::Settings { .. } => Overlay::Settings { selected: row },
        Overlay::Plugins { .. } => Overlay::Plugins { selected: row },
        Overlay::Jobs(_) => Overlay::Jobs(crate::JobList { selected: row }),
        Overlay::Remotes(mut browser) => {
            if browser.show_clients {
                browser.client_selected = row;
            } else {
                browser.selected = row;
                browser.error = None;
            }
            Overlay::Remotes(browser)
        }
        Overlay::Connect(mut connect) => {
            connect.pick = row;
            Overlay::Connect(connect)
        }
        Overlay::Exclusions(mut editor) => {
            editor.selected = row;
            Overlay::Exclusions(editor)
        }
        Overlay::TimeMachine => {
            state.backup.window.point(row, false);
            Overlay::TimeMachine
        }
        overlay => overlay,
    });
}
