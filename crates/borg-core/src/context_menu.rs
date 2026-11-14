//! The menu a right-click or Shift-F10 opens on the row under the cursor: what can be copied
//! from the item, such as its path, name or PID, then the keys that act on it.

use std::time::Instant;

use borg_i18n::tr;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{Action, AppState, Effect, Menu, MenuEntry, MenuItem, Overlay, Pointer, View};

mod files;
mod screens;

const PREVIOUS: isize = -1;
const NEXT: isize = 1;

/// What an entry of the context menu does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextCommand {
    /// An action of the key map, as its key does it.
    Perform(Action),
    /// A key of the list under the menu, as if it was pressed there.
    Press(KeyEvent),
    /// Puts `text` on the clipboard and says so with `notice`.
    Copy { text: String, notice: String },
}

/// The context menu: the entries for the item it opened on, under that item's name, the entry
/// the cursor is on, and the cell a right-click opened it at. Shift-F10 opens it mid-screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextMenu {
    pub menu: Menu<ContextCommand>,
    pub selected: Option<usize>,
    pub at: Option<(u16, u16)>,
}

/// Whether `key` opens the context menu: Shift-F10, or the Menu key where the terminal sends it.
pub(crate) fn opens(key: &KeyEvent) -> bool {
    matches!((key.code, key.modifiers), (KeyCode::F(10), KeyModifiers::SHIFT) | (KeyCode::Menu, _))
}

/// A right-click: `pick` moves the cursor to the row under it, then the menu opens there.
pub(crate) fn on_right_click(state: &mut AppState, pick: Pointer, at: (u16, u16), now: Instant) -> Vec<Effect> {
    let mut effects = crate::pointer::on_pointer(state, pick, now);
    if state.overlay.is_none() {
        effects.extend(open(state, Some(at)));
    }
    effects
}

/// Opens the menu for the item under the cursor, or says there is none.
pub(crate) fn open(state: &mut AppState, at: Option<(u16, u16)>) -> Vec<Effect> {
    let Some(menu) = for_focus(state) else {
        state.notice = Some(tr!(nothing_selected).into());
        return Vec::new();
    };
    let selected = menu.first_enabled();
    state.overlay = Some(Overlay::ContextMenu(ContextMenu { menu, selected, at }));
    Vec::new()
}

pub(crate) fn on_key(state: &mut AppState, mut menu: ContextMenu, key: &KeyEvent, now: Instant) -> Vec<Effect> {
    menu.selected = match key.code {
        KeyCode::Up => menu.menu.step(menu.selected, PREVIOUS),
        KeyCode::Down => menu.menu.step(menu.selected, NEXT),
        KeyCode::Home => menu.menu.first_enabled(),
        KeyCode::End => menu.menu.step(None, PREVIOUS),
        KeyCode::Enter => return menu.selected.map(|index| choose(state, menu, index, now)).unwrap_or_default(),
        KeyCode::Esc | KeyCode::F(_) | KeyCode::Menu => {
            state.overlay = None;
            return Vec::new();
        }
        _ => menu.selected,
    };
    state.overlay = Some(Overlay::ContextMenu(menu));
    Vec::new()
}

/// Does what the entry at `index` does; the menu stays open on an entry that cannot be chosen.
/// When the item under the cursor is no longer the one the menu was made for, as when a list
/// sorts itself again while the menu is open, nothing happens: an action reaches only the item
/// the menu named.
pub(crate) fn choose(state: &mut AppState, menu: ContextMenu, index: usize, now: Instant) -> Vec<Effect> {
    let Some(command) = menu.menu.item(index).filter(|item| item.enabled).map(|item| item.action.clone()) else {
        state.overlay = Some(Overlay::ContextMenu(menu));
        return Vec::new();
    };
    state.overlay = None;
    if for_focus(state).as_ref() != Some(&menu.menu) {
        state.notice = Some(tr!(context_menu_stale).into());
        return Vec::new();
    }
    match command {
        ContextCommand::Perform(action) => crate::actions::perform(state, action, now),
        ContextCommand::Press(key) => crate::update::press(state, &key, now),
        ContextCommand::Copy { text, notice } => {
            state.notice = Some(notice);
            crate::clipboard::copy(state, text)
        }
    }
}

/// Chooses the entry of the open context menu named `label`, as a click on it does.
#[cfg(test)]
pub(crate) fn choose_labelled(state: &mut AppState, label: &str, now: Instant) -> Vec<Effect> {
    let Some(Overlay::ContextMenu(menu)) = state.overlay.clone() else {
        panic!("no context menu: {:?}", state.overlay)
    };
    let index = menu
        .menu
        .entries
        .iter()
        .position(|entry| matches!(entry, MenuEntry::Item(item) if item.label == label))
        .unwrap_or_else(|| panic!("no {label} in {:?}", menu.menu.entries));
    choose(state, menu, index, now)
}

/// The menu for what has the keys: a screen's selected row, or the active panel's selection.
fn for_focus(state: &AppState) -> Option<Menu<ContextCommand>> {
    if state.view != View::Panels {
        return None;
    }
    if state.monitor_open() && !state.files_beside_focused() {
        return for_screen(state);
    }
    if state.docked[state.active.index()].is_some() {
        return None;
    }
    files::entries(state)
}

fn for_screen(state: &AppState) -> Option<Menu<ContextCommand>> {
    if state.docker.is_open() {
        return screens::container(state);
    }
    if state.activity.side.is_some() {
        return screens::activity(&state.activity);
    }
    if state.network.is_open() {
        let scanned = state
            .network
            .neighbor()
            .is_some_and(|(neighbor, _)| crate::port_scan::scan_of(state, neighbor.address).is_some());
        return screens::network(&state.network, scanned);
    }
    if state.alerts.is_open() {
        return screens::alert(&state.alerts);
    }
    screens::startup_item(&state.startup_items)
}

fn entry(label: &'static str, hint: String, command: ContextCommand) -> MenuEntry<ContextCommand> {
    MenuEntry::Item(MenuItem { label, hint, action: command, checked: None, enabled: true })
}

/// An entry that copies one value and names it in the notice.
fn copy(label: &'static str, text: impl Into<String>) -> MenuEntry<ContextCommand> {
    let text = text.into();
    let notice = tr!(context_copied, text = text);
    entry(label, String::new(), ContextCommand::Copy { text, notice })
}

/// An entry that copies several lines, with a notice that does not repeat them.
fn copy_lines(label: &'static str, lines: &[String], notice: &'static str) -> MenuEntry<ContextCommand> {
    entry(label, String::new(), ContextCommand::Copy { text: lines.join("\n"), notice: notice.into() })
}

/// An entry for an action of the key map, with its key beside it.
fn perform(label: &'static str, action: Action) -> MenuEntry<ContextCommand> {
    entry(label, crate::keymap::hint(action).unwrap_or_default(), ContextCommand::Perform(action))
}

/// An entry for a key only the list under the menu takes, named by `hint`.
fn press(label: &'static str, hint: &'static str, key: impl Into<KeyEvent>) -> MenuEntry<ContextCommand> {
    entry(label, hint.into(), ContextCommand::Press(key.into()))
}
