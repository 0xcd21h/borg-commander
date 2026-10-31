use super::*;
use crate::{Action, AppState, Effect, Overlay, Panel, Side};
use crate::{Appearance, ListingMode, Msg, Pointer, Settings, Sort, SortKey, Startup, ThemeChoice, update};
use borg_vfs::Entry;
use std::path::PathBuf;
use std::{path::Path, time::Instant};

fn file(path: &str, size: u64) -> Entry {
    Entry {
        source: Some(path.into()),
        missing: false,
        usage: None,
        name: Path::new(path).file_name().unwrap().to_string_lossy().into_owned(),
        kind: borg_vfs::EntryKind::File,
        size,
        modified: None,
        executable: false,
        permissions: 0o644,
    }
}

fn panel() -> Panel {
    let mut panel = Panel::new("/work".into(), 10, ListingMode::Full, Sort::default());
    panel.open_results(
        ResultSource::Collection("Bundle".into()),
        vec![file("/a/config.toml", 10), file("/b/config.toml", 20), file("/a/notes.md", 30)],
        10,
    );
    panel
}

fn state() -> (AppState, Instant) {
    let now = Instant::now();
    let (mut state, _) = AppState::new(
        Startup::at("/work".into()),
        100,
        30,
        Settings::default(),
        Appearance::new(vec![ThemeChoice { id: "midnight".into(), name: "Midnight".into() }], 0),
        now,
    );
    state.panels[0] = panel();
    state.collections.push(Collection { name: "Bundle".into(), entries: state.panels[0].entries().to_vec() });
    (state, now)
}

#[test]
fn new_searches_and_collections_open_in_their_pane_with_independent_expansion() {
    let (mut state, now) = state();
    assert!(!state.results_expanded());
    assert!(state.layout(now).panels.iter().all(|rect| !rect.is_empty()));
    perform(&mut state, Action::SetExpanded(true));
    assert!(state.results_expanded());
    state.active = Side::Right;
    collection(&mut state, "Other", Side::Right);
    assert!(!state.results_expanded());
    assert!(state.panels[0].results.as_ref().unwrap().expanded);
    assert!(state.layout(now).panels.iter().all(|rect| !rect.is_empty()));
    update(
        &mut state,
        Msg::Pointer(Pointer::ResultControl { side: Side::Left, action: Action::SetExpanded(true) }),
        now,
    );
    assert_eq!(state.active, Side::Left);
    assert!(state.results_expanded());
    start_search(
        &mut state,
        SearchRequest {
            skip: None,
            root: "/work".into(),
            query: "*".into(),
            hidden: false,
            options: Default::default(),
        },
    );
    assert!(!state.results_expanded());
    assert!(state.layout(now).panels.iter().all(|rect| !rect.is_empty()));
}

#[test]
fn fold_and_resume_keep_expansion_cursor_and_marks() {
    let (mut state, now) = state();
    state.result_history[0] = Some(Panel::new("/work".into(), 10, ListingMode::Full, Sort::default()));
    state.panels[0].move_by(1);
    state.panels[0].toggle_mark();
    let selected = state.panels[0].hovered_entry().unwrap().identity().to_path_buf();
    perform(&mut state, Action::SetExpanded(true));
    update(&mut state, Msg::Pointer(Pointer::ResultControl { side: Side::Left, action: Action::BackResults }), now);
    assert!(!state.panels[0].is_results());
    back(&mut state);
    assert!(state.results_expanded());
    assert_eq!(state.panels[0].hovered_entry().unwrap().identity(), selected);
    assert_eq!(state.panels[0].marked_count(), 1);
}

#[test]
fn cleanup_targets_include_a_marked_folder_once_without_dropping_similarly_named_siblings() {
    let mut panel = panel();
    let mut folder = file("/work/cache", 1);
    folder.kind = borg_vfs::EntryKind::Dir;
    panel.open_results(
        ResultSource::Collection("Cleanup".into()),
        vec![folder, file("/work/cache/nested/item", 2), file("/work/cache-other/item", 3)],
        20,
    );
    panel.mark_where(|_| true, true);
    assert_eq!(selected_paths(&panel), vec![PathBuf::from("/work/cache"), PathBuf::from("/work/cache-other/item")]);
}

#[test]
fn stopping_a_search_never_discards_an_inflight_collection_refresh() {
    let (mut state, _) = state();
    let generation = state.panels[0].generation();
    state.panels[0].begin_results_refresh(generation);
    assert!(perform(&mut state, Action::CancelSearch).is_empty());
    assert_eq!(state.panels[0].entries().len(), 3);
    assert_eq!(state.panels[0].listing(), &crate::Listing::Loading);
}

#[test]
fn editing_a_collection_updates_a_hidden_copy_without_resurrecting_removed_references() {
    let (mut state, _) = state();
    state.panels[1] = state.panels[0].clone();
    state.result_history[0] = Some(Panel::new("/work".into(), 10, ListingMode::Full, Sort::default()));
    back(&mut state);
    assert!(!state.panels[0].is_results());
    state.active = Side::Right;
    perform(&mut state, Action::RemoveReferences);
    assert_eq!(state.collections[0].entries.len(), 2);
    state.active = Side::Left;
    back(&mut state);
    assert_eq!(state.panels[0].entries().len(), 2);
}

#[test]
fn hidden_search_keeps_its_position_marks_filter_and_receives_late_results() {
    let (mut state, now) = state();
    state.panels[0] = Panel::new("/work".into(), 10, ListingMode::Full, Sort::default());
    let effects = start_search(
        &mut state,
        SearchRequest {
            skip: None,
            root: "/work".into(),
            query: "*".into(),
            hidden: true,
            options: Default::default(),
        },
    );
    let generation = match &effects[0] {
        Effect::Search { generation, .. } => *generation,
        _ => panic!("search"),
    };
    update(
        &mut state,
        Msg::Batch { side: Side::Left, generation, entries: vec![file("/work/a", 10), file("/work/b", 20)] },
        now,
    );
    state.settle();
    state.panels[0].move_by(1);
    state.panels[0].toggle_mark();
    state.panels[0].filter_results("/work".into());
    let path = state.panels[0].hovered_entry().unwrap().identity().to_path_buf();
    update(&mut state, Msg::Key(crossterm::event::KeyCode::Esc.into()), now);
    assert!(!state.panels[0].is_results());
    update(&mut state, Msg::Batch { side: Side::Left, generation, entries: vec![file("/work/c", 30)] }, now);
    update(&mut state, Msg::Listed { side: Side::Left, generation, error: None }, now);
    assert!(back(&mut state).is_empty());
    assert_eq!(state.panels[0].entries().len(), 3);
    assert_eq!(state.panels[0].hovered_entry().unwrap().identity(), path);
    assert_eq!(state.panels[0].marked_count(), 1);
    assert_eq!(state.panels[0].results.as_ref().unwrap().filter, "/work");
    assert!(back(&mut state).is_empty());
    assert!(back(&mut state).is_empty());
    assert_eq!(state.panels[0].hovered_entry().unwrap().identity(), path);
}

#[test]
fn virtual_panels_own_tab_even_when_shell_reports_a_typed_line() {
    let (mut state, now) = state();
    state.panels[1] = state.panels[0].clone();
    state.shell.mark_typed();
    let effects = update(&mut state, Msg::Key(crossterm::event::KeyCode::Tab.into()), now);
    assert_eq!(state.active, Side::Right);
    assert!(!effects.iter().any(|e| matches!(e, Effect::Write(_))));
    update(&mut state, Msg::Key(crossterm::event::KeyCode::Tab.into()), now);
    assert_eq!(state.active, Side::Left);
}

#[test]
fn quit_always_confirms_and_defaults_to_staying() {
    let (mut state, now) = state();
    assert!(
        update(&mut state, Msg::Key(crossterm::event::KeyCode::F(10).into()), now).iter().all(|e| *e != Effect::Quit)
    );
    assert!(matches!(state.overlay, Some(Overlay::Confirm(crate::Confirm { accept: false, .. }))));
    assert!(!update(&mut state, Msg::Key(crossterm::event::KeyCode::Enter.into()), now).contains(&Effect::Quit));
    update(&mut state, Msg::Key(crossterm::event::KeyCode::F(10).into()), now);
    assert!(update(&mut state, Msg::Pointer(Pointer::Confirm(true)), now).contains(&Effect::Quit));
}

#[test]
fn collection_picker_scrolls_creates_and_opens_without_typing_saved_names() {
    let (mut state, now) = state();
    state.collections =
        (0..30).map(|index| Collection { name: format!("Batch {index}"), entries: Vec::new() }).collect();
    open_collection_prompt(&mut state);
    update(&mut state, Msg::Key(crossterm::event::KeyCode::End.into()), now);
    assert!(matches!(state.overlay, Some(Overlay::Collections(crate::CollectionPicker { selected: 29, .. }))));
    update(&mut state, Msg::Key(crossterm::event::KeyCode::Enter.into()), now);
    assert!(
        matches!(state.active_panel().results.as_ref().unwrap().source, ResultSource::Collection(ref name) if name == "Batch 29")
    );
    open_collection_prompt(&mut state);
    update(&mut state, Msg::Pointer(Pointer::NewCollection), now);
    update(&mut state, Msg::Paste("Cleanup".into()), now);
    update(&mut state, Msg::Key(crossterm::event::KeyCode::Enter.into()), now);
    assert!(state.collections.iter().any(|c| c.name == "Cleanup"));
}

#[test]
fn same_names_keep_distinct_marks_and_focus_through_sort_and_filter() {
    let mut panel = panel();
    panel.toggle_mark();
    let selected = panel.hovered_entry().unwrap().identity().to_path_buf();
    panel.set_sort(Sort { key: SortKey::Size, reverse: true });
    assert_eq!(panel.hovered_entry().unwrap().identity(), selected);
    assert_eq!(panel.marked_count(), 1);
    panel.filter_results("/b/".into());
    assert_eq!(panel.len(), 1);
    assert_eq!(panel.hovered_entry().unwrap().identity(), Path::new("/b/config.toml"));
    assert_eq!(panel.marked().next().unwrap().identity(), selected);
    assert_eq!(selected_paths(&panel), vec![selected]);
    panel.filter_results(String::new());
    assert_eq!(panel.entries().len(), 3);
}

#[test]
fn grouped_headers_fold_but_never_become_file_targets() {
    let mut panel = panel();
    panel.group_results(ResultGroup::Folder);
    panel.to_start();
    assert!(matches!(panel.hovered(), Some(crate::Row::Group { count: 2, .. })));
    assert!(selected_paths(&panel).is_empty());
    panel.toggle_mark();
    assert_eq!(panel.marked_count(), 2);
    assert_eq!(selected_paths(&panel).len(), 2);
    panel.clear_marks();
    let before = panel.len();
    panel.toggle_group();
    assert_eq!(panel.len(), before - 2);
    panel.toggle_group();
    assert_eq!(panel.len(), before);
}

#[test]
fn f8_removes_only_references_and_preserves_other_same_named_member() {
    let (mut state, _) = state();
    let effects = perform(&mut state, Action::RemoveReferences);
    assert!(effects.contains(&Effect::SaveCollections));
    assert!(!effects.iter().any(|e| matches!(e, Effect::StartJob { .. })));
    assert_eq!(state.collections[0].entries.len(), 2);
    assert!(state.collections[0].entries.iter().any(|e| e.identity() == Path::new("/b/config.toml")));
}

#[test]
fn cancelled_search_rejects_already_queued_batches() {
    let (mut state, now) = state();
    let effects = start_search(
        &mut state,
        SearchRequest {
            skip: None,
            root: "/work".into(),
            query: "*".into(),
            hidden: false,
            options: Default::default(),
        },
    );
    let generation = effects
        .iter()
        .find_map(|e| match e {
            Effect::Search { generation, .. } => Some(*generation),
            _ => None,
        })
        .unwrap();
    update(&mut state, Msg::Batch { side: Side::Left, generation, entries: vec![file("/work/first", 1)] }, now);
    state.settle();
    assert_eq!(state.active_panel().entries().len(), 1);
    perform(&mut state, Action::CancelSearch);
    update(&mut state, Msg::Batch { side: Side::Left, generation, entries: vec![file("/work/late", 1)] }, now);
    state.settle();
    assert_eq!(state.active_panel().entries().len(), 1);
    assert!(
        matches!(state.active_panel().results.as_ref().unwrap().source, ResultSource::Search { ref progress, .. } if progress.cancelled)
    );
}

#[test]
fn collecting_into_a_new_panel_cannot_be_replaced_by_its_initial_refresh() {
    let (mut state, now) = state();
    let effects = collect(&mut state);
    assert!(!effects.iter().any(|e| matches!(e, Effect::RefreshCollection { .. })));
    state.settle();
    assert_eq!(state.panel(Side::Right).entries().len(), 1);
    update(&mut state, Msg::Listed { side: Side::Right, generation: 0, error: None }, now);
    state.settle();
    assert_eq!(state.panel(Side::Right).entries().len(), 1);
}

#[test]
fn column_sorting_keeps_selection_and_expanded_layout_inside_screen() {
    let (mut state, now) = state();
    state.panels[0].results.as_mut().unwrap().expanded = true;
    let path = state.active_panel().hovered_entry().unwrap().identity().to_path_buf();
    update(&mut state, Msg::Pointer(Pointer::ResultSort { side: Side::Left, key: SortKey::Size }), now);
    assert_eq!(state.active_panel().hovered_entry().unwrap().identity(), path);
    assert!(state.active_panel().sort().reverse);
    assert_eq!(state.layout(now).panels[0].width, state.cols);
    assert!(state.layout(now).panels[1].is_empty());
    state.aside = Some(crate::Aside::QuickView);
    let layout = state.layout(now);
    assert_eq!(layout.panels[0].bottom(), layout.panels[1].y);
    assert!(layout.panels[1].bottom() <= layout.prompt.y);
    assert_eq!(state.active_panel().dir(), Path::new("/work"));
}
#[test]
fn removing_a_reference_invalidates_inflight_refreshes_on_both_panels() {
    let (mut state, now) = state();
    state.panels[1] = state.panels[0].clone();
    let old = state.panels[0].generation();
    state.panels[0].begin_results_refresh(old);
    state.panels[1].begin_results_refresh(old);
    perform(&mut state, Action::RemoveReferences);
    for side in Side::ALL {
        update(&mut state, Msg::Batch { side, generation: old, entries: vec![file("/a/config.toml", 10)] }, now);
        update(&mut state, Msg::Listed { side, generation: old, error: None }, now);
    }
    state.settle();
    assert_eq!(state.panels[0].entries().len(), 2);
    assert_eq!(state.panels[1].entries().len(), 2);
    assert!(state.collections[0].entries.iter().all(|entry| entry.identity() != Path::new("/a/config.toml")));
}

#[test]
fn collection_limit_does_not_append_references_to_a_real_directory() {
    let (mut state, _) = state();
    state.collections =
        (0..COLLECTION_LIMIT).map(|index| Collection { name: format!("Saved {index}"), entries: Vec::new() }).collect();
    let before = state.panel(Side::Right).entries().to_vec();
    collect(&mut state);
    assert!(!state.panel(Side::Right).is_results());
    assert_eq!(state.panel(Side::Right).entries(), before);
    assert!(state.notice.as_ref().unwrap().contains("limit"));
}

#[test]
fn refreshing_a_search_preserves_filter_grouping_and_marks() {
    let (mut state, _) = state();
    let entries = state.active_panel().entries().to_vec();
    state.panels[0].open_results(
        ResultSource::Search {
            request: SearchRequest {
                skip: None,
                root: "/work".into(),
                query: "*.toml".into(),
                hidden: false,
                options: Default::default(),
            },
            progress: SearchProgress::default(),
        },
        entries,
        10,
    );
    state.panels[0].toggle_mark();
    state.panels[0].filter_results("config".into());
    state.panels[0].group_results(ResultGroup::Folder);
    state.settings.preferences.show_hidden = true;
    let effect = state.reload(Side::Left);
    assert!(matches!(effect, Effect::Search { request, .. } if request.hidden));
    assert_eq!(state.active_panel().marked_count(), 1);
    let results = state.active_panel().results.as_ref().unwrap();
    assert_eq!(results.filter, "config");
    assert_eq!(results.group, ResultGroup::Folder);
}
fn tree_panel() -> Panel {
    let mut panel = Panel::new("/work".into(), 12, ListingMode::Full, Sort::default());
    let entries = [("/work", 120), ("/work/cache", 100), ("/work/cache/deep", 80), ("/work/src", 20)]
        .into_iter()
        .map(|(path, size)| {
            let mut entry = file(path, size);
            entry.kind = borg_vfs::EntryKind::Dir;
            entry.usage = Some(borg_vfs::FolderUsage { files: size, ..Default::default() });
            entry
        })
        .collect();
    panel.open_results(
        ResultSource::Search {
            request: SearchRequest {
                skip: None,
                root: "/work".into(),
                query: "*".into(),
                hidden: true,
                options: crate::SearchOptions { kind: crate::SearchKind::FolderTree, ..Default::default() },
            },
            progress: SearchProgress::default(),
        },
        entries,
        10,
    );
    panel.set_sort(Sort { key: SortKey::Size, reverse: true });
    panel
}

fn paths(panel: &Panel) -> Vec<PathBuf> {
    (0..panel.len())
        .filter_map(|i| match panel.row(i) {
            Some(crate::Row::Entry(e)) => Some(e.identity().to_path_buf()),
            _ => None,
        })
        .collect()
}

#[test]
fn tree_navigation_keeps_root_first_sorts_siblings_and_preserves_hidden_marks() {
    let mut panel = tree_panel();
    assert_eq!(paths(&panel), ["/work", "/work/cache", "/work/src"].map(PathBuf::from));
    panel.move_by(1);
    panel.tree_step(true);
    assert_eq!(paths(&panel), ["/work", "/work/cache", "/work/cache/deep", "/work/src"].map(PathBuf::from));
    panel.tree_step(true);
    assert_eq!(panel.hovered_entry().unwrap().identity(), Path::new("/work/cache/deep"));
    panel.toggle_mark();
    panel.tree_step(false);
    assert_eq!(panel.hovered_entry().unwrap().identity(), Path::new("/work/cache"));
    panel.tree_step(false);
    assert_eq!(panel.marked_count(), 1);
    assert_eq!(panel.visible_entries().filter(|e| panel.is_marked(e)).count(), 0);
    panel.toggle_mark();
    assert_eq!(selected_paths(&panel), vec![PathBuf::from("/work/cache")]);
    panel.filter_results("deep".into());
    assert_eq!(paths(&panel), ["/work", "/work/cache", "/work/cache/deep"].map(PathBuf::from));
    panel.filter_results(String::new());
    assert_eq!(panel.len(), 3);
    panel.set_sort(Sort { key: SortKey::Size, reverse: false });
    assert_eq!(paths(&panel), ["/work", "/work/src", "/work/cache"].map(PathBuf::from));
}

#[test]
fn tree_stream_updates_replace_by_identity_and_cancelled_refresh_preserves_previous_tree() {
    let mut panel = tree_panel();
    let mut root = panel.entries().iter().find(|e| e.identity() == Path::new("/work")).unwrap().clone();
    root.size = 200;
    panel.receive(10, vec![root.clone(), root], true);
    panel.settle();
    assert_eq!(panel.entries().len(), 4);
    assert_eq!(panel.entries().iter().find(|e| e.identity() == Path::new("/work")).unwrap().size, 200);
    panel.begin_results_refresh(11);
    panel.receive(11, vec![file("/work/new", 50)], true);
    panel.settle();
    assert_eq!(panel.entries().len(), 4);
    panel.cancel_refresh();
    panel.finish(11, None);
    panel.settle();
    assert_eq!(panel.entries().len(), 4);
    assert!(panel.entries().iter().all(|e| e.usage.as_ref().unwrap().cached));
    panel.begin_results_refresh(12);
    panel.receive(12, vec![file("/work/new", 60)], true);
    panel.finish(12, Some("unavailable".into()));
    panel.settle();
    assert_eq!(panel.entries().len(), 4);
}

#[test]
fn hiding_a_tree_reloads_normal_totals_and_resumes_branches_cursor_and_marks() {
    let (mut state, now) = state();
    state.panels[0] = tree_panel();
    state.result_history[0] = Some(Panel::new("/work".into(), 10, ListingMode::Full, Sort::default()));
    state.panels[0].move_by(1);
    state.panels[0].tree_step(true);
    state.panels[0].tree_step(true);
    state.panels[0].toggle_mark();
    let effects = update(
        &mut state,
        Msg::Key(crossterm::event::KeyEvent::new(crossterm::event::KeyCode::Esc, crossterm::event::KeyModifiers::NONE)),
        now,
    );
    assert!(effects.iter().any(|e| matches!(e, Effect::List { .. })));
    assert!(!state.active_panel().is_results());
    back(&mut state);
    assert_eq!(state.active_panel().len(), 4);
    assert_eq!(state.active_panel().hovered_entry().unwrap().identity(), Path::new("/work/cache/deep"));
    assert_eq!(state.active_panel().marked_count(), 1);
    let effect = state.reload(Side::Left);
    assert!(matches!(effect, Effect::Search { request, .. } if request.options.refresh && request.hidden));
}
#[test]
fn flat_size_results_expose_the_same_rescan_and_cached_folder_handoff() {
    let (mut state, now) = state();
    state.panels[0] = tree_panel();
    let Some(Results { tree, source: ResultSource::Search { request, .. }, .. }) = &mut state.panels[0].results else {
        unreachable!()
    };
    *tree = None;
    request.options.kind = crate::SearchKind::FolderTotals;
    state.result_history[0] = Some(Panel::new("/work".into(), 10, ListingMode::Full, Sort::default()));
    let effects = update(
        &mut state,
        Msg::Key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('r'),
            crossterm::event::KeyModifiers::CONTROL,
        )),
        now,
    );
    assert!(effects.iter().any(
        |e| matches!(e, Effect::Search { request, side: Side::Left, .. } if request.options.refresh && request.hidden)
    ));
    perform(&mut state, Action::CancelSearch);
    let effects = back(&mut state);
    assert!(matches!(&effects[..], [Effect::List { .. }]));
    back(&mut state);
    assert!(state.active_panel().entries().iter().all(|e| e.usage.as_ref().unwrap().cached));
}
#[test]
fn lazy_tree_children_reject_old_generations_and_keep_fresh_scan_status() {
    let (mut state, now) = state();
    let mut panel = tree_panel();
    let mut entries =
        panel.entries().iter().filter(|e| e.identity() != Path::new("/work/cache/deep")).cloned().collect::<Vec<_>>();
    let branch = entries.iter_mut().find(|e| e.identity() == Path::new("/work/cache")).unwrap();
    branch.usage.as_mut().unwrap().folders = 1;
    branch.usage.as_mut().unwrap().direct_bytes = 20;
    branch.usage.as_mut().unwrap().direct_files = 2;
    panel.replace_collection(entries);
    state.panels[0] = panel;
    state.panels[0].move_by(1);
    state.panels[0].tree_step(true);
    let effects = sync_sizes(&mut state);
    let generation = state.panels[0].generation();
    assert!(
        effects.iter().any(|e| matches!(e, Effect::SizeChildren { parent, .. } if parent == Path::new("/work/cache")))
    );
    let mut child = file("/work/cache/deep", 80);
    child.kind = borg_vfs::EntryKind::Dir;
    child.usage = Some(borg_vfs::FolderUsage { cached: true, ..Default::default() });
    update(
        &mut state,
        Msg::SizeChildren {
            side: Side::Left,
            generation: generation + 1,
            parent: "/work/cache".into(),
            result: Ok((vec![child.clone()], false)),
        },
        now,
    );
    assert_eq!(state.panels[0].entries().len(), 3);
    update(
        &mut state,
        Msg::SizeChildren {
            side: Side::Left,
            generation,
            parent: "/work/cache".into(),
            result: Ok((vec![child], false)),
        },
        now,
    );
    assert_eq!(state.panels[0].entries().len(), 4);
    assert!(state.panels[0].entries().iter().all(|e| !e.usage.as_ref().unwrap().cached));
    assert!(matches!(state.panels[0].row(2), Some(crate::Row::Group { count: 2, .. })));
    assert!(
        matches!(&state.panels[0].results.as_ref().unwrap().source, ResultSource::Search { progress, .. } if !progress.cached)
    );
}
