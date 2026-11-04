use crate::Pattern;
use borg_vfs::Entry;
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use borg_vfs::EntryKind;

use super::*;

const ROWS: usize = 3;

fn entry(name: &str, kind: EntryKind, size: u64) -> Entry {
    Entry {
        source: None,
        missing: false,
        usage: None,
        name: name.into(),
        kind,
        size,
        modified: Some(UNIX_EPOCH + Duration::from_secs(size)),
        executable: false,
        permissions: 0o644,
    }
}

fn file(name: &str) -> Entry {
    entry(name, EntryKind::File, 0)
}

fn sized(name: &str, size: u64) -> Entry {
    entry(name, EntryKind::File, size)
}

fn dir(name: &str) -> Entry {
    entry(name, EntryKind::Dir, 0)
}

fn names(panel: &Panel) -> Vec<&str> {
    panel.entries().iter().map(|e| e.name.as_str()).collect()
}

fn marked(panel: &Panel) -> Vec<&str> {
    panel.marked().map(|e| e.name.as_str()).collect()
}

fn loaded(path: &str, batches: Vec<Vec<Entry>>) -> Panel {
    let mut panel = Panel::new(PathBuf::from("/"), ROWS, ListingMode::Full, Sort::default());
    panel.navigate(PathBuf::from(path), 1, None);
    batches.into_iter().for_each(|batch| panel.receive(1, batch, false));
    panel.finish(1, None);
    panel.settle();
    panel
}

#[test]
fn sorts_directories_first_case_insensitively() {
    let panel = loaded("/x", vec![vec![file("b"), dir("Zed"), file("A")], vec![dir("alpha")]]);
    assert_eq!(names(&panel), ["alpha", "Zed", "A", "b"]);
}

#[test]
fn sorts_by_size_extension_and_reverse_keeping_dirs_first() {
    let mut panel = loaded("/x", vec![vec![sized("a.rs", 30), sized("b.md", 10), sized("c.toml", 20), dir("z")]]);
    panel.set_sort(Sort { key: SortKey::Size, reverse: false });
    assert_eq!(names(&panel), ["z", "b.md", "c.toml", "a.rs"]);
    panel.set_sort(Sort { key: SortKey::Extension, reverse: false });
    assert_eq!(names(&panel), ["z", "b.md", "a.rs", "c.toml"]);
    panel.set_sort(Sort { key: SortKey::Name, reverse: true });
    assert_eq!(names(&panel), ["z", "c.toml", "b.md", "a.rs"]);
}

#[test]
fn resorting_keeps_cursor_on_same_file() {
    let mut panel = loaded("/x", vec![vec![sized("a", 30), sized("b", 10), sized("c", 20)]]);
    panel.move_by(1);
    assert_eq!(panel.hovered_entry().map(|e| e.name.as_str()), Some("a"));
    panel.set_sort(Sort { key: SortKey::Size, reverse: false });
    assert_eq!(panel.hovered_entry().map(|e| e.name.as_str()), Some("a"));
}

#[test]
fn hides_dotfiles_unless_requested() {
    let panel = loaded("/x", vec![vec![file(".env"), file("main.rs")]]);
    assert_eq!(names(&panel), ["main.rs"]);
}

#[test]
fn keeps_cursor_on_same_entry_while_batches_arrive() {
    let mut panel = Panel::new(PathBuf::from("/"), ROWS, ListingMode::Full, Sort::default());
    panel.navigate(PathBuf::from("/x"), 1, None);
    panel.receive(1, vec![file("m"), file("z")], false);
    panel.settle();
    panel.move_by(1);
    assert_eq!(panel.hovered(), Some(Row::Entry(&file("m"))));

    panel.receive(1, vec![file("a"), file("b")], false);
    panel.settle();
    assert_eq!(panel.hovered(), Some(Row::Entry(&file("m"))));
}

#[test]
fn focuses_child_after_leaving_it() {
    let mut panel = loaded("/x/sub", vec![vec![file("f")]]);
    panel.navigate(PathBuf::from("/x"), 2, Some("sub".into()));
    panel.receive(2, vec![dir("a"), dir("sub"), dir("z")], false);
    panel.settle();
    assert_eq!(panel.dir(), Path::new("/x"));
    assert_eq!(panel.hovered(), Some(Row::Entry(&dir("sub"))));
}

#[test]
fn keeps_previous_listing_until_destination_responds() {
    let mut panel = loaded("/x", vec![vec![file("f")]]);
    panel.navigate(PathBuf::from("/forbidden"), 2, None);
    assert_eq!(panel.dir(), Path::new("/x"));

    let error = panel.finish(2, Some("permission denied".into()));
    assert_eq!(error.as_deref(), Some("permission denied"));
    assert_eq!(panel.dir(), Path::new("/x"));
    assert_eq!(names(&panel), ["f"]);
}

#[test]
fn ignores_stale_batches() {
    let mut panel = loaded("/x", vec![vec![file("f")]]);
    panel.receive(0, vec![file("ghost")], false);
    panel.settle();
    assert_eq!(names(&panel), ["f"]);
}

#[test]
fn defers_merges_geometrically_while_loading() {
    let batch = |range: std::ops::Range<usize>| range.map(|i| file(&format!("f{i:06}"))).collect::<Vec<_>>();
    let mut panel = Panel::new(PathBuf::from("/"), ROWS, ListingMode::Full, Sort::default());
    panel.navigate(PathBuf::from("/x"), 1, None);
    panel.receive(1, batch(0..EAGER_MERGE_LIMIT), false);
    panel.settle();
    assert_eq!(panel.entries().len(), EAGER_MERGE_LIMIT);

    panel.receive(1, batch(EAGER_MERGE_LIMIT..EAGER_MERGE_LIMIT + 10), false);
    panel.settle();
    assert_eq!(panel.entries().len(), EAGER_MERGE_LIMIT);
    assert_eq!(panel.received(), EAGER_MERGE_LIMIT + 10);

    panel.finish(1, None);
    panel.settle();
    assert_eq!(panel.entries().len(), EAGER_MERGE_LIMIT + 10);
}

#[test]
fn scrolls_to_keep_cursor_visible() {
    let mut panel = loaded("/x", vec![(0..10).map(|i| file(&format!("f{i}"))).collect()]);
    panel.to_end();
    assert_eq!(panel.cursor(), 10);
    assert_eq!(panel.scroll(), 10 + 1 - ROWS);
    panel.to_start();
    assert_eq!((panel.cursor(), panel.scroll()), (0, 0));
    assert_eq!(panel.hovered(), Some(Row::Parent));
}

#[test]
fn brief_mode_scrolls_by_whole_columns() {
    let mut panel = loaded("/x", vec![(0..20).map(|i| file(&format!("f{i:02}"))).collect()]);
    panel.set_mode(ListingMode::Brief);
    assert_eq!(panel.capacity(), ROWS * BRIEF_COLUMNS);

    panel.move_column(1);
    assert_eq!(panel.cursor(), ROWS);
    panel.move_column(1);
    assert_eq!(panel.cursor(), ROWS * 2);
    assert_eq!(panel.scroll() % ROWS, 0);
    assert!(panel.cursor() < panel.scroll() + panel.capacity());
}

#[test]
fn marks_toggle_match_and_invert() {
    let mut panel = loaded("/x", vec![vec![file("a.rs"), file("b.rs"), file("notes.md"), dir("src")]]);
    panel.move_by(2);
    panel.toggle_mark();
    assert_eq!(panel.marked_count(), 1);

    let rust = Pattern::parse("*.RS").unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(panel.mark_matching(&rust, true), 2);
    assert_eq!(marked(&panel), ["a.rs", "b.rs"]);

    panel.invert_marks();
    assert_eq!(marked(&panel), ["src", "notes.md"]);
}

#[test]
fn selection_falls_back_to_hovered_entry() {
    let mut panel = loaded("/x", vec![vec![file("a"), file("b")]]);
    assert!(panel.selection().is_empty());
    panel.move_by(2);
    assert_eq!(panel.selection().iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["b"]);
}

#[test]
fn marks_survive_reload_but_not_navigation() {
    let mut panel = loaded("/x", vec![vec![file("a"), file("b")]]);
    panel.move_by(1);
    panel.toggle_mark();

    panel.reload(2);
    panel.receive(2, vec![file("a"), file("b")], false);
    panel.finish(2, None);
    panel.settle();
    assert_eq!(panel.marked_count(), 1);

    panel.navigate(PathBuf::from("/y"), 3, None);
    panel.finish(3, None);
    assert_eq!(panel.marked_count(), 0);
}

#[test]
fn cursor_moves_to_next_entry_when_hovered_file_vanishes() {
    let mut panel = loaded("/x", vec![vec![file("a"), file("b"), file("c")]]);
    panel.move_by(2);
    assert_eq!(panel.hovered_entry().map(|e| e.name.as_str()), Some("b"));

    panel.reload(2);
    panel.receive(2, vec![file("a"), file("c")], false);
    panel.finish(2, None);
    panel.settle();
    assert_eq!(panel.hovered_entry().map(|e| e.name.as_str()), Some("c"));
}

#[test]
fn reload_keeps_old_listing_until_new_one_is_complete() {
    let mut panel = loaded("/x", vec![vec![file("a"), file("b")]]);
    panel.move_by(2);
    panel.reload(2);
    panel.receive(2, vec![file("b")], false);
    panel.settle();
    assert_eq!(names(&panel), ["a", "b"]);
    assert_eq!(panel.hovered_entry().map(|e| e.name.as_str()), Some("b"));
    assert_eq!(panel.received(), 2);

    panel.receive(2, vec![file("c")], false);
    panel.finish(2, None);
    panel.settle();
    assert_eq!(names(&panel), ["b", "c"]);
    assert_eq!(panel.hovered_entry().map(|e| e.name.as_str()), Some("b"));
}

#[test]
fn a_move_made_while_a_refresh_loads_is_kept() {
    let mut panel = loaded("/x", vec![vec![file("a"), file("b")]]);
    panel.move_by(2);
    panel.reload(2);
    panel.to_start();
    panel.receive(2, vec![file("a"), file("b")], false);
    panel.finish(2, None);
    panel.settle();
    assert!(matches!(panel.hovered(), Some(Row::Parent)));
}

#[test]
fn first_listing_streams_even_into_the_starting_directory() {
    let mut panel = Panel::new(PathBuf::from("/x"), ROWS, ListingMode::Full, Sort::default());
    panel.navigate(PathBuf::from("/x"), 1, None);
    panel.receive(1, vec![file("a")], false);
    panel.settle();
    assert_eq!(names(&panel), ["a"]);
}

#[test]
fn marks_of_vanished_files_are_dropped_after_reload() {
    let mut panel = loaded("/x", vec![vec![file("a"), file("b")]]);
    panel.move_by(1);
    panel.toggle_mark();

    panel.reload(2);
    panel.receive(2, vec![file("b")], false);
    panel.finish(2, None);
    panel.settle();
    assert_eq!(panel.marked_count(), 0);
}

const TALL: usize = 10;
const FOLDERS: usize = 40;

fn folders(range: std::ops::Range<usize>) -> Vec<Entry> {
    range.map(|i| dir(&format!("d{i:02}"))).collect()
}

fn list(panel: &mut Panel, path: &str, generation: u64, focus: Option<&str>, batches: Vec<Vec<Entry>>) {
    panel.navigate(PathBuf::from(path), generation, focus.map(str::to_owned));
    for batch in batches {
        panel.receive(generation, batch, false);
        panel.settle();
    }
    panel.finish(generation, None);
    panel.settle();
}

fn hovered(panel: &Panel) -> Option<&str> {
    panel.hovered_entry().map(|entry| entry.name.as_str())
}

fn screen_row(panel: &Panel) -> usize {
    panel.cursor() - panel.scroll()
}

/// A tall panel in `/x` scrolled down, with the cursor on `d20` in the middle of the screen.
fn scrolled() -> Panel {
    let mut panel = Panel::new(PathBuf::from("/"), TALL, ListingMode::Full, Sort::default());
    list(&mut panel, "/x", 1, None, vec![folders(0..FOLDERS)]);
    panel.move_by(25);
    panel.move_by(-4);
    assert_eq!((hovered(&panel), panel.cursor(), panel.scroll()), (Some("d20"), 21, 16));
    panel
}

#[test]
fn returning_from_a_folder_restores_its_scroll_and_row() {
    let mut panel = scrolled();
    list(&mut panel, "/x/d20", 2, None, vec![folders(0..FOLDERS)]);
    assert_eq!((panel.cursor(), panel.scroll()), (0, 0), "a folder entered opens at the top");

    list(&mut panel, "/x", 3, Some("d20"), vec![folders(0..FOLDERS)]);
    assert_eq!((hovered(&panel), panel.cursor(), panel.scroll()), (Some("d20"), 21, 16));
}

#[test]
fn returning_to_the_root_restores_every_folder_on_the_way() {
    let mut panel = scrolled();
    list(&mut panel, "/x/d20", 2, None, vec![folders(0..FOLDERS)]);
    panel.move_by(33);
    let inner = (panel.cursor(), panel.scroll());
    list(&mut panel, "/x/d20/d32", 3, None, vec![folders(0..FOLDERS)]);
    panel.move_by(5);
    list(&mut panel, "/x/d20/d32/d04", 4, None, vec![vec![file("f")]]);

    list(&mut panel, "/x/d20/d32", 5, Some("d04"), vec![folders(0..FOLDERS)]);
    assert_eq!((panel.cursor(), panel.scroll()), (5, 0));
    list(&mut panel, "/x/d20", 6, Some("d32"), vec![folders(0..FOLDERS)]);
    assert_eq!((panel.cursor(), panel.scroll()), inner);
    list(&mut panel, "/x", 7, Some("d20"), vec![folders(0..FOLDERS)]);
    assert_eq!((panel.cursor(), panel.scroll()), (21, 16));
}

#[test]
fn a_folder_that_lost_entries_keeps_the_cursor_on_its_row_without_space_below_the_end() {
    let mut panel = scrolled();
    list(&mut panel, "/x/d20", 2, None, vec![vec![file("f")]]);
    list(&mut panel, "/x", 3, Some("d20"), vec![folders(10..FOLDERS)]);
    assert_eq!((hovered(&panel), screen_row(&panel)), (Some("d20"), 5), "entries above it went");

    list(&mut panel, "/x/d20", 4, None, vec![vec![file("f")]]);
    list(&mut panel, "/x", 5, Some("d20"), vec![folders(0..22)]);
    assert_eq!(hovered(&panel), Some("d20"));
    assert_eq!(panel.scroll(), panel.len() - TALL, "the last row stays at the bottom of the panel");
}

#[test]
fn a_resized_panel_keeps_the_row_when_it_fits_and_the_bottom_row_when_not() {
    const SHORT: usize = 4;
    const TALLER: usize = 20;
    let mut panel = scrolled();
    list(&mut panel, "/x/d20", 2, None, vec![vec![file("f")]]);
    panel.set_rows(TALLER);
    list(&mut panel, "/x", 3, Some("d20"), vec![folders(0..FOLDERS)]);
    assert_eq!((hovered(&panel), screen_row(&panel)), (Some("d20"), 5));

    list(&mut panel, "/x/d20", 4, None, vec![vec![file("f")]]);
    panel.set_rows(SHORT);
    list(&mut panel, "/x", 5, Some("d20"), vec![folders(0..FOLDERS)]);
    assert_eq!((hovered(&panel), screen_row(&panel)), (Some("d20"), SHORT - 1));
}

#[test]
fn the_row_holds_while_a_returning_listing_arrives_in_batches() {
    let mut panel = scrolled();
    list(&mut panel, "/x/d20", 2, None, vec![vec![file("f")]]);
    list(&mut panel, "/x", 3, Some("d20"), vec![folders(20..FOLDERS), folders(0..20)]);
    assert_eq!((hovered(&panel), panel.cursor(), panel.scroll()), (Some("d20"), 21, 16));
}

#[test]
fn a_move_made_while_a_returning_listing_arrives_is_kept() {
    let mut panel = scrolled();
    list(&mut panel, "/x/d20", 2, None, vec![vec![file("f")]]);
    panel.navigate(PathBuf::from("/x"), 3, Some("d20".into()));
    panel.receive(3, folders(0..30), false);
    panel.settle();
    panel.move_by(1);
    let moved = (panel.cursor(), panel.scroll());
    panel.receive(3, folders(30..FOLDERS), false);
    panel.finish(3, None);
    panel.settle();
    assert_eq!((hovered(&panel), panel.cursor(), panel.scroll()), (Some("d21"), moved.0, moved.1));
}

#[test]
fn brief_listings_return_to_the_same_columns() {
    let mut panel = Panel::new(PathBuf::from("/"), TALL, ListingMode::Brief, Sort::default());
    list(&mut panel, "/x", 1, None, vec![folders(0..FOLDERS)]);
    panel.to_end();
    panel.move_by(-15);
    let before = (panel.cursor(), panel.scroll());
    assert_eq!((hovered(&panel), before), (Some("d24"), (25, 20)));
    list(&mut panel, "/x/d24", 2, None, vec![vec![file("f")]]);
    list(&mut panel, "/x", 3, Some("d24"), vec![folders(0..FOLDERS)]);
    assert_eq!((panel.cursor(), panel.scroll()), before);
}

#[test]
fn a_container_keeps_its_own_trail_apart_from_this_machines_folders() {
    let mut panel = Panel::new(PathBuf::from("/"), TALL, ListingMode::Full, Sort::default());
    list(&mut panel, "/", 1, None, vec![folders(0..FOLDERS)]);
    panel.move_by(25);
    panel.move_by(-4);
    panel.mount = Some(crate::Mount { id: "web".into(), name: "web".into(), host: PathBuf::from("/") });
    list(&mut panel, "/d20", 2, None, vec![vec![file("f")]]);
    list(&mut panel, "/", 3, Some("d20"), vec![folders(0..FOLDERS)]);
    assert_eq!((hovered(&panel), panel.scroll()), (Some("d20"), 21 + 1 - TALL), "the host's row stays with the host");
}
