use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::*;
use crate::{Appearance, Effect, Msg, Pointer, Settings, Startup, ThemeChoice, VaultMount, update};

const HOME: &str = "/work";
const CIPHER: &str = "/work/Papers";
const ROOT: &str = "/run/user/1000/borg-commander/vaults/Papers";

fn state() -> (AppState, Instant) {
    let now = Instant::now();
    let themes = vec![ThemeChoice { id: "midnight".into(), name: "Midnight".into() }];
    let (state, _) =
        AppState::new(Startup::at(HOME.into()), 80, 12, Settings::default(), Appearance::new(themes, 0), now);
    (state, now)
}

fn press(state: &mut AppState, now: Instant, code: KeyCode) -> Vec<Effect> {
    update(state, Msg::Key(KeyEvent::new(code, KeyModifiers::NONE)), now)
}

fn presses(state: &mut AppState, now: Instant, codes: &[KeyCode]) {
    codes.iter().for_each(|&code| {
        press(state, now, code);
    });
}

fn open(state: &AppState) -> MenuCursor {
    let Some(Overlay::Menu(cursor)) = state.overlay else { panic!("an open menu") };
    cursor
}

/// The title of the open menu and the label of its chosen entry.
fn shown(state: &AppState) -> (String, Option<&'static str>) {
    let cursor = open(state);
    let menu = &menus(state)[cursor.menu];
    (menu.title.clone(), cursor.item.and_then(|item| menu.item(item)).map(|item| item.label))
}

fn entry(label: &'static str, action: Action, enabled: bool) -> MenuEntry {
    MenuEntry::Item(MenuItem { label, hint: String::new(), action, checked: None, enabled })
}

fn menu(title: &str, entries: Vec<MenuEntry>) -> Menu {
    Menu { title: title.into(), entries }
}

/// A menu with an entry that can be chosen, a separator, one that cannot, and two that can.
fn mixed() -> Menu {
    menu(
        "Mixed",
        vec![
            entry("First", Action::Search, true),
            MenuEntry::Separator,
            entry("Off", Action::AnalyzeSpace, false),
            entry("Third", Action::FolderTree, true),
            entry("Last", Action::CalculateSizes, true),
        ],
    )
}

#[test]
fn the_wheel_moves_through_the_entries_that_can_be_chosen_and_stops_at_the_ends() {
    let mixed = mixed();
    assert_eq!(mixed.scroll(Some(0), 1), Some(3));
    assert_eq!(mixed.scroll(Some(3), 1), Some(4));
    assert_eq!(mixed.scroll(Some(4), 1), Some(4));
    assert_eq!(mixed.scroll(Some(0), -1), Some(0));
    assert_eq!(mixed.scroll(Some(4), -2), Some(0));
    assert_eq!(mixed.scroll(Some(0), 5), Some(4));
    assert_eq!(mixed.scroll(None, 1), Some(0));
}

#[test]
fn the_nearest_entry_that_can_be_chosen_stands_in_for_one_that_cannot() {
    let mixed = mixed();
    assert_eq!(mixed.nearest(2), Some(3));
    assert_eq!(mixed.nearest(1), Some(0));
    assert_eq!(mixed.nearest(4), Some(4));
    assert_eq!(mixed.nearest(9), Some(4));
    assert_eq!(menu("Empty", vec![entry("Off", Action::Search, false)]).nearest(0), None);
}

#[test]
fn a_place_is_found_again_by_its_menu_and_entry_when_menus_and_entries_come_and_go() {
    let before = [menu("Left", vec![entry("Rescan", Action::Reload, true)]), mixed()];
    let place = MenuPlace::of(&before, MenuCursor { menu: 1, item: Some(3) }).unwrap();

    let mut moved = mixed();
    moved.entries.insert(0, entry("New", Action::OpenCollection, true));
    let after = [before[0].clone(), menu("Git", vec![entry("Status", Action::Reload, true)]), moved];
    assert_eq!(place.resume(&after), Some(MenuCursor { menu: 2, item: Some(4) }));

    let mut off = mixed();
    off.entries[3] = entry("Third", Action::FolderTree, false);
    assert_eq!(place.resume(&[before[0].clone(), off]), Some(MenuCursor { menu: 1, item: Some(4) }));

    let renamed = [before[0].clone(), menu("Renamed", mixed().entries)];
    assert_eq!(place.resume(&renamed), Some(MenuCursor { menu: 1, item: Some(3) }));
    assert_eq!(place.resume(&before[..1]), None);
}

#[test]
fn f9_opens_where_the_menu_was_left_however_it_closed() {
    let (mut state, now) = state();
    press(&mut state, now, KeyCode::F(9));
    presses(&mut state, now, &[KeyCode::Right, KeyCode::Right, KeyCode::Down, KeyCode::Down]);
    let left = open(&state);
    assert_eq!(left.menu, 2);

    update(&mut state, Msg::Pointer(Pointer::DismissMenu), now);
    assert!(state.overlay.is_none());
    press(&mut state, now, KeyCode::F(9));
    assert_eq!(open(&state), left, "a click beside the menu closed it");

    press(&mut state, now, KeyCode::Down);
    let moved = open(&state);
    press(&mut state, now, KeyCode::Esc);
    press(&mut state, now, KeyCode::F(9));
    assert_eq!(open(&state), moved, "Esc closed it");

    press(&mut state, now, KeyCode::F(10));
    press(&mut state, now, KeyCode::Esc);
    press(&mut state, now, KeyCode::F(9));
    assert_eq!(open(&state), moved, "another function key closed it");
}

#[test]
fn f9_opens_at_the_entry_chosen_last_by_key_or_click() {
    let (mut state, now) = state();
    press(&mut state, now, KeyCode::F(9));
    presses(&mut state, now, &[KeyCode::Down, KeyCode::Enter]);
    press(&mut state, now, KeyCode::F(9));
    assert_eq!(shown(&state), ("Left".into(), Some("Brief listing")));
    press(&mut state, now, KeyCode::Esc);

    update(&mut state, Msg::Pointer(Pointer::Menu(FILE_MENU)), now);
    let make_directory = menus(&state)[FILE_MENU]
        .entries
        .iter()
        .position(|entry| matches!(entry, MenuEntry::Item(item) if item.action == Action::Mkdir));
    update(&mut state, Msg::Pointer(Pointer::MenuItem(make_directory.unwrap())), now);
    press(&mut state, now, KeyCode::Esc);
    press(&mut state, now, KeyCode::F(9));
    assert_eq!(shown(&state), ("File".into(), Some("Make directory")));
}

#[test]
fn f9_comes_back_to_the_same_entry_after_a_menu_leaves_the_bar() {
    let (mut state, now) = state();
    press(&mut state, now, KeyCode::F(9));
    presses(&mut state, now, &[KeyCode::Left, KeyCode::Left, KeyCode::Down, KeyCode::Down, KeyCode::Down]);
    assert_eq!(shown(&state), ("Search".into(), Some("Calculate sizes")));
    press(&mut state, now, KeyCode::Esc);

    state.settings.preferences.git_plugin = !state.settings.preferences.git_plugin;
    press(&mut state, now, KeyCode::F(9));
    assert_eq!(shown(&state), ("Search".into(), Some("Calculate sizes")));
}

#[test]
fn the_wheel_moves_through_an_open_menu() {
    let (mut state, now) = state();
    press(&mut state, now, KeyCode::F(9));
    update(&mut state, Msg::Pointer(Pointer::DialogScroll(1)), now);
    assert_eq!(shown(&state), ("Left".into(), Some("Brief listing")));
    (0..20).for_each(|_| {
        update(&mut state, Msg::Pointer(Pointer::DialogScroll(1)), now);
    });
    assert_eq!(shown(&state), ("Left".into(), Some("Rescan")), "the wheel stops at the last entry");
}

#[test]
fn home_and_end_go_to_the_first_and_last_entries_that_can_be_chosen() {
    let (mut state, now) = state();
    press(&mut state, now, KeyCode::F(9));
    press(&mut state, now, KeyCode::End);
    assert_eq!(shown(&state), ("Left".into(), Some("Rescan")));
    press(&mut state, now, KeyCode::Home);
    assert_eq!(shown(&state), ("Left".into(), Some("Full listing")));
}

#[test]
fn the_encryption_menu_holds_the_vault_commands() {
    let (mut state, now) = state();
    let labels = |state: &AppState, title: &str| -> Vec<(&'static str, bool)> {
        let all = menus(state);
        let menu = all.iter().find(|menu| menu.title == title).expect("the menu");
        menu.entries
            .iter()
            .filter_map(|entry| match entry {
                MenuEntry::Item(item) => Some((item.label, item.enabled)),
                MenuEntry::Separator => None,
            })
            .collect()
    };
    assert_eq!(labels(&state, "Encryption"), [("Create vault…", true), ("Close vault", false)]);
    assert!(labels(&state, "File").iter().all(|(label, _)| !label.contains("vault")));

    state.panel_mut(Side::Left).vault = Some(VaultMount { cipher: CIPHER.into(), root: ROOT.into() });
    assert_eq!(labels(&state, "Encryption"), [("Create vault…", true), ("Close vault", true)]);

    press(&mut state, now, KeyCode::F(9));
    press(&mut state, now, KeyCode::Left);
    assert_eq!(shown(&state), ("Encryption".into(), Some("Create vault…")));
    press(&mut state, now, KeyCode::Enter);
    assert!(matches!(state.overlay, Some(Overlay::Prompt(_))), "Create vault asks for the name");
}
