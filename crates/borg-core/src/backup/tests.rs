use super::*;
use crate::{AppState, Effect, Overlay, Prompt, PromptPurpose, Side};
use crate::{Msg, Settings, Startup, update};
use borg_i18n::tr;
use crossterm::event::{KeyCode, KeyEvent};
use std::path::{Path, PathBuf};
use std::time::Instant;

fn state() -> AppState {
    let now = Instant::now();
    let themes = vec![crate::ThemeChoice { id: "midnight".into(), name: "midnight".into() }];
    let appearance = crate::Appearance::new(themes, 0);
    let (mut state, _) = AppState::new(Startup::at("/work".into()), 100, 30, Settings::default(), appearance, now);
    state.backup.available = true;
    state
}

fn key(state: &mut AppState, code: KeyCode) -> Vec<Effect> {
    update(state, Msg::Key(KeyEvent::from(code)), Instant::now())
}

/// The window's requests, without the panels' own.
fn requests(effects: &[Effect]) -> Vec<&BackupRequest> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::Backup(BackupRequest::Listing { .. }) => None,
            Effect::Backup(request) => Some(request),
            _ => None,
        })
        .collect()
}

fn item(path: &str, exclusion: Exclusion) -> BackupItem {
    BackupItem { path: path.into(), dir: true, exclusion, ..BackupItem::default() }
}

fn opened_on_caches(state: &mut AppState) -> u64 {
    let effects = crate::actions::perform(state, crate::Action::TimeMachine, Instant::now());
    match requests(&effects)[..] {
        [BackupRequest::Caches { id, root }] => {
            assert_eq!(root, Path::new("/work"));
            *id
        }
        ref other => panic!("expected a cache search, got {other:?}"),
    }
}

#[test]
fn panels_ask_for_marks_once_per_listing_and_only_where_time_machine_is() {
    let mut state = state();
    let listings = |effects: &[Effect]| {
        effects.iter().filter(|effect| matches!(effect, Effect::Backup(BackupRequest::Listing { .. }))).count()
    };
    assert_eq!(listings(&sync(&mut state)), 2);
    assert!(sync(&mut state).is_empty(), "the same listing is not asked about again");
    let mut elsewhere = self::state();
    elsewhere.backup.available = false;
    assert!(sync(&mut elsewhere).is_empty());
}

#[test]
fn marks_from_an_older_listing_are_dropped() {
    let mut state = state();
    sync(&mut state);
    let listing = BackupListing { folder: Exclusion::Included, entries: vec![("target".into(), Exclusion::Sticky)] };
    let event = |generation| {
        Msg::Backup(BackupEvent::Listing {
            side: Side::Left,
            generation,
            dir: "/work".into(),
            result: Ok(listing.clone()),
        })
    };
    let generation = state.panel(Side::Left).generation();
    update(&mut state, event(generation + 7), Instant::now());
    assert!(!state.panel(Side::Left).backup.marks());
    update(&mut state, event(generation), Instant::now());
    assert_eq!(state.panel(Side::Left).backup.entry("target"), Some(&Exclusion::Sticky));
}

#[test]
fn off_macos_the_window_explains_itself_instead_of_opening() {
    let mut state = state();
    state.backup.available = false;
    let effects = crate::actions::perform(&mut state, crate::Action::TimeMachine, Instant::now());
    assert!(effects.is_empty());
    assert!(state.overlay.is_none());
    assert_eq!(state.notice.as_deref(), Some(tr!(backup_macos_only)));
}

#[test]
fn tabs_search_the_first_time_they_show_and_esc_stops_the_searches() {
    let mut state = state();
    opened_on_caches(&mut state);
    assert_eq!(state.backup.window.tab, BackupTab::Caches);
    let effects = key(&mut state, KeyCode::Tab);
    assert!(matches!(requests(&effects)[..], [BackupRequest::All { .. }]));
    assert!(requests(&key(&mut state, KeyCode::Char('2'))).is_empty(), "caches were already searched");
    let effects = key(&mut state, KeyCode::Esc);
    assert!(requests(&effects).contains(&&BackupRequest::Cancel));
    assert!(state.overlay.is_none());
}

#[test]
fn caches_sort_by_size_once_found_and_keep_the_cursor_on_its_folder() {
    let mut state = state();
    let id = opened_on_caches(&mut state);
    let found = vec![item("/work/a/target", Exclusion::Included), item("/work/node_modules", Exclusion::Sticky)];
    receive(&mut state, BackupEvent::Found { id, items: found });
    receive(&mut state, BackupEvent::Sized { id, path: "/work/a/target".into(), bytes: 10 });
    receive(&mut state, BackupEvent::Sized { id, path: "/work/node_modules".into(), bytes: 900 });
    assert_eq!(state.backup.window.list().hovered().unwrap().path, Path::new("/work/a/target"));
    receive(&mut state, BackupEvent::Finished { id, error: None });
    let list = state.backup.window.list();
    assert_eq!(list.items[0].path, Path::new("/work/node_modules"));
    assert_eq!(list.hovered().unwrap().path, Path::new("/work/a/target"));
    assert!(!list.loading);
}

#[test]
fn exclude_sends_only_what_is_still_backed_up_and_reports_the_outcome() {
    let mut state = state();
    let id = opened_on_caches(&mut state);
    let found = vec![item("/work/target", Exclusion::Sticky), item("/work/node_modules", Exclusion::Included)];
    receive(&mut state, BackupEvent::Found { id, items: found });
    key(&mut state, KeyCode::Char('*'));
    let effects = key(&mut state, KeyCode::Char('e'));
    let [BackupRequest::Change { id, change: BackupChange::Exclude, items }] = &requests(&effects)[..] else {
        panic!("expected one change");
    };
    assert_eq!(items.iter().map(|item| item.path.as_path()).collect::<Vec<_>>(), [Path::new("/work/node_modules")]);
    assert_eq!(state.backup.window.working, Some((*id, false)));
    assert!(requests(&key(&mut state, KeyCode::Char('e'))).is_empty(), "one change at a time");

    let changed = vec![item("/work/node_modules", Exclusion::Sticky)];
    receive(&mut state, BackupEvent::Changed { id: *id, items: changed, error: None });
    let window = &state.backup.window;
    assert!(window.working.is_none());
    assert!(window.list().marks.is_empty());
    assert!(window.list().items.iter().all(|item| item.exclusion == Exclusion::Sticky));
    assert_eq!(window.outcome.as_deref(), Some(tr!(backup_now_excluded, items = "1 item").as_str()));
    assert!(state.panels.iter().all(|panel| panel.backup.key.is_none()), "the panels' marks are asked again");
}

#[test]
fn excluding_a_path_or_including_one_waits_for_the_password() {
    let mut state = state();
    let id = opened_on_caches(&mut state);
    receive(&mut state, BackupEvent::Found { id, items: vec![item("/work/node_modules", Exclusion::Included)] });
    key(&mut state, KeyCode::Char('p'));
    assert!(state.backup.window.working.is_some_and(|(_, password)| password));

    let mut state = self::state();
    let id = opened_on_caches(&mut state);
    let found = vec![item("/work/a", Exclusion::Sticky), item("/work/b", Exclusion::Path)];
    receive(&mut state, BackupEvent::Found { id, items: found });
    key(&mut state, KeyCode::Char('*'));
    let effects = key(&mut state, KeyCode::Char('i'));
    let [BackupRequest::Change { change: BackupChange::Include, items, .. }] = &requests(&effects)[..] else {
        panic!("expected one change");
    };
    assert_eq!(items.len(), 2);
    assert!(state.backup.window.working.is_some_and(|(_, password)| password));
}

#[test]
fn including_what_is_not_its_own_says_what_to_do_instead() {
    let mut state = state();
    let id = opened_on_caches(&mut state);
    let found = vec![item("/work/target/debug", Exclusion::Inside("/work/target".into()))];
    receive(&mut state, BackupEvent::Found { id, items: found });
    assert!(requests(&key(&mut state, KeyCode::Char('i'))).is_empty());
    assert_eq!(state.backup.window.outcome, Some(tr!(backup_include_folder, folder = "target")));

    receive(&mut state, BackupEvent::Found { id, items: vec![item("/System", Exclusion::System)] });
    key(&mut state, KeyCode::Down);
    assert!(requests(&key(&mut state, KeyCode::Char('i'))).is_empty());
    assert_eq!(state.backup.window.outcome.as_deref(), Some(tr!(backup_system_fixed)));
}

#[test]
fn all_exclusions_group_by_kind_and_keep_the_group_after_a_change() {
    let mut state = state();
    opened_on_caches(&mut state);
    key(&mut state, KeyCode::Char('3'));
    let id = state.backup.window.list().id;
    let found = vec![item("/z", Exclusion::System), item("/b", Exclusion::Path), item("/c", Exclusion::Sticky)];
    receive(&mut state, BackupEvent::Found { id, items: found });
    let paths = |state: &AppState| -> Vec<PathBuf> {
        state.backup.window.list().items.iter().map(|item| item.path.clone()).collect()
    };
    assert_eq!(paths(&state), [PathBuf::from("/c"), "/b".into(), "/z".into()]);
    let effects = key(&mut state, KeyCode::Char('i'));
    let [BackupRequest::Change { id, .. }] = &requests(&effects)[..] else { panic!("expected one change") };
    receive(&mut state, BackupEvent::Changed { id: *id, items: vec![item("/c", Exclusion::Included)], error: None });
    let first = &state.backup.window.list().items[0];
    assert_eq!((first.exclusion.clone(), first.group.clone()), (Exclusion::Included, Exclusion::Sticky));
}

#[test]
fn the_list_scrolls_only_when_the_cursor_would_leave_it() {
    let mut state = state();
    let id = opened_on_caches(&mut state);
    let found = (0..40).map(|n| item(&format!("/work/{n:02}/target"), Exclusion::Included)).collect();
    receive(&mut state, BackupEvent::Found { id, items: found });
    let rows = list_rows(state.rows);
    let at = |state: &AppState| (state.backup.window.list().cursor, state.backup.window.list().top);
    for _ in 0..25 {
        key(&mut state, KeyCode::Down);
    }
    assert_eq!(at(&state), (25, 25 - (rows - 1)), "moving down past the end scrolls one row at a time");
    key(&mut state, KeyCode::Up);
    assert_eq!(at(&state), (24, 25 - (rows - 1)), "moving back up inside the list keeps it still");
    for _ in 0..rows {
        key(&mut state, KeyCode::Up);
    }
    assert_eq!(at(&state), (24 - rows, 24 - rows), "moving up past the top scrolls");
    key(&mut state, KeyCode::PageDown);
    assert_eq!(at(&state), (24 - rows + rows - 1, 24 - rows), "a page down lands on the last row shown");
}

#[test]
fn scrolling_up_to_the_first_item_of_a_kind_shows_its_heading() {
    let mut state = state();
    opened_on_caches(&mut state);
    key(&mut state, KeyCode::Char('3'));
    let id = state.backup.window.list().id;
    let mut found = vec![item("/work/kept", Exclusion::Sticky)];
    found.extend((0..30).map(|n| item(&format!("/fixed/{n:02}"), Exclusion::Path)));
    receive(&mut state, BackupEvent::Found { id, items: found });
    let window = |state: &AppState| state.backup.window.clone();
    assert_eq!(
        (window(&state).heading(0), window(&state).heading(1)),
        (Some(&Exclusion::Sticky), Some(&Exclusion::Path))
    );
    assert_eq!(window(&state).heading(2), None);
    key(&mut state, KeyCode::End);
    let rows = list_rows(state.rows);
    assert_eq!(window(&state).list().top, 30 + 2 - (rows - 1), "the headings count as rows");
    key(&mut state, KeyCode::Home);
    assert_eq!(window(&state).list().top, 0, "the first item shows with its heading");
}

#[test]
fn a_path_typed_into_add_becomes_an_exclusion_of_the_chosen_kind() {
    let mut state = state();
    opened_on_caches(&mut state);
    key(&mut state, KeyCode::Char('3'));
    key(&mut state, KeyCode::Char('a'));
    assert!(matches!(state.overlay, Some(Overlay::Prompt(Prompt { purpose: PromptPurpose::BackupAdd { .. }, .. }))));
    let effects = add_path(&mut state, "relative/path", false);
    assert!(effects.is_empty());
    assert_eq!(state.backup.window.outcome.as_deref(), Some(tr!(backup_add_absolute)));
    let effects = add_path(&mut state, "/work/logs/", true);
    let [BackupRequest::Change { change: BackupChange::ExcludePath, items, .. }] = &requests(&effects)[..] else {
        panic!("expected one change");
    };
    assert_eq!(items[0].path, Path::new("/work/logs"));
    assert!(matches!(state.overlay, Some(Overlay::TimeMachine)));
    assert_eq!(state.backup.window.list().hovered().unwrap().group, Exclusion::Path);
}
