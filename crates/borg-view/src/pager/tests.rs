use super::*;
use crate::{
    Mark, Source,
    paint::{self},
};
use std::{io, path::Path, sync::Arc};

fn pager(content: &str, width: usize, height: usize) -> Pager {
    Pager::new(Arc::new(content.as_bytes().to_vec()), width, height).unwrap_or_else(|e| panic!("{e}"))
}

fn numbered(count: usize) -> String {
    (1..=count).map(|i| format!("line {i}\n")).collect()
}

fn ok(result: io::Result<()>) {
    result.unwrap_or_else(|e| panic!("{e}"));
}

#[test]
fn shows_first_page_and_scrolls() {
    let mut view = pager(&numbered(10), 20, 3);
    assert_eq!(view.rows(), ["line 1", "line 2", "line 3"]);
    ok(view.scroll_down(2));
    assert_eq!(view.rows(), ["line 3", "line 4", "line 5"]);
    ok(view.scroll_up(1));
    assert_eq!(view.rows()[0], "line 2");
}

#[test]
fn end_shows_last_page_and_stops_there() {
    let mut view = pager(&numbered(10), 20, 3);
    ok(view.end());
    assert_eq!(view.rows(), ["line 8", "line 9", "line 10"]);
    assert_eq!(view.percent(), 100);
    ok(view.scroll_down(5));
    assert_eq!(view.rows(), ["line 8", "line 9", "line 10"]);
    ok(view.page_up());
    assert_eq!(view.rows(), ["line 6", "line 7", "line 8"]);
    ok(view.home());
    assert_eq!(view.rows()[0], "line 1");
}

#[test]
fn wraps_long_lines_and_scrolls_by_visual_row() {
    let mut view = pager("abcdefghij\nxy\n", 4, 2);
    assert_eq!(view.rows(), ["abcd", "efgh"]);
    ok(view.scroll_down(1));
    assert_eq!(view.rows(), ["efgh", "ij"]);
    ok(view.scroll_down(1));
    assert_eq!(view.rows(), ["ij", "xy"]);
    ok(view.scroll_up(2));
    assert_eq!(view.rows(), ["abcd", "efgh"]);
}

#[test]
fn unwrapped_view_scrolls_horizontally() {
    let mut view = pager("0123456789abcdef\n", 6, 1);
    ok(view.toggle_wrap());
    assert_eq!(view.rows(), ["012345"]);
    ok(view.scroll_right());
    assert_eq!(view.rows(), ["89abcd"]);
    ok(view.scroll_left());
    assert_eq!(view.rows(), ["012345"]);
}

#[test]
fn resizing_keeps_the_wrapped_position_and_the_bottom_of_a_document() {
    let mut view = pager("abcdefghij\nklmnopqrst\nuvwxyz\n", 2, 2);
    ok(view.scroll_down(3));
    assert_eq!(view.rows()[0], "gh");
    ok(view.resize(4, 2));
    assert_eq!(view.rows()[0], "efgh");
    ok(view.resize(2, 3));
    assert_eq!(view.rows()[0], "ef");
    ok(view.end());
    ok(view.resize(4, 2));
    assert_eq!(view.rows().last().unwrap(), "yz");
    assert_eq!(view.percent(), 100);
}

#[test]
fn handles_files_without_trailing_newline_and_empty_files() {
    let mut view = pager("a\nb", 5, 5);
    assert_eq!(view.rows(), ["a", "b"]);
    ok(view.end());
    assert_eq!(view.rows(), ["a", "b"]);

    let empty = pager("", 5, 5);
    assert!(empty.rows().is_empty());
    assert_eq!(empty.percent(), 100);
}

#[test]
fn splits_giant_lines_into_bounded_chunks() {
    let content = format!("{}\nend\n", "x".repeat(10));
    let source: Arc<dyn Source> = Arc::new(content.into_bytes());
    let mut view = Pager::with_limits(source, 3, 10, 4, 64).unwrap_or_else(|e| panic!("{e}"));
    ok(view.toggle_wrap());
    assert_eq!(view.rows(), ["xxx", "xxx", "xx", "end"]);
    ok(view.end());
    ok(view.scroll_up(1));
    assert_eq!(view.rows()[0], "xxx");
}

fn marks(view: &Pager, row: usize) -> Vec<(String, Mark)> {
    view.rich_rows()[row].iter().map(|run| (run.text.clone(), run.mark)).collect()
}

#[test]
fn matches_on_the_page_are_marked_and_one_off_the_page_scrolls_into_view() {
    let mut view = pager(&numbered(100), 20, 6);
    let search = crate::Query { text: "line 9".into(), ..crate::Query::default() }.compile().unwrap().unwrap();
    ok(view.show_matches(Some(search.clone())));
    assert!(view.rich_rows().iter().flatten().all(|run| run.mark == Mark::None));
    let first = search.next(&*view.source(), 0, &|_| true).unwrap().unwrap();
    ok(view.reveal(first.clone()));
    assert_eq!(view.rows()[0], "line 7", "a third of the page shows above the match");
    assert_eq!(marks(&view, 2), [("line 9".to_owned(), Mark::Current)]);
    let next = search.next(&*view.source(), first.end, &|_| true).unwrap().unwrap();
    ok(view.reveal(next.clone()));
    assert_eq!(view.rows()[0], "line 88");
    assert_eq!(marks(&view, 2), [("line 9".to_owned(), Mark::Current), ("0".to_owned(), Mark::None)]);
    assert_eq!(marks(&view, 3), [("line 9".to_owned(), Mark::Match), ("1".to_owned(), Mark::None)]);
    let shown = search.next(&*view.source(), next.end, &|_| true).unwrap().unwrap();
    ok(view.reveal(shown));
    assert_eq!(view.rows()[0], "line 88", "a match on the page leaves it where it is");
    assert_eq!(view.current_match().map(|range| range.start), Some(next.end + 2));

    let mut wide = pager(&format!("{}needle\n", "x".repeat(50)), 10, 2);
    ok(wide.toggle_wrap());
    let needle = crate::Query { text: "needle".into(), ..crate::Query::default() }.compile().unwrap().unwrap();
    ok(wide.show_matches(Some(needle)));
    ok(wide.reveal(50..56));
    assert_eq!(wide.rows()[0], "xxneedle", "the view scrolls sideways to the match");
}

#[test]
fn colours_reached_by_paging_match_colouring_the_whole_file() {
    // A comment and a string that span lines, so colours depend on the text above the page.
    let chunk = "/* a comment\n   over lines */\nfn main() {\n    let text = \"two\n lines\";\n}\n";
    let text = chunk.repeat(600);
    let source: Arc<dyn Source> = Arc::new(text.clone().into_bytes());
    let mut view = Pager::for_path(source.clone(), Path::new("main.rs"), 40, 12).unwrap_or_else(|e| panic!("{e}"));
    let mut whole = Pager::for_path(source, Path::new("main.rs"), 40, 12).unwrap_or_else(|e| panic!("{e}"));
    whole.decoration = Some(Arc::new(paint::highlight(&text, "Rust", 0, &|| false).unwrap_or_else(|e| panic!("{e}"))));
    let mut same = |view: &Pager| {
        whole.top = view.top;
        ok(whole.refresh());
        assert_eq!(view.rich_rows(), whole.rich_rows(), "at {:?}", view.top);
    };
    while !view.at_end() {
        ok(view.page_down());
        same(&view);
    }
    assert!(view.resumes().len() <= text.len() / RESUME_EVERY as usize + 2, "{} places", view.resumes().len());
    ok(view.home());
    ok(view.end());
    same(&view);
    for _ in 0..40 {
        ok(view.page_up());
        same(&view);
    }
}

#[test]
fn paging_down_far_into_a_file_carries_colouring_on_instead_of_starting_again_above() {
    let text = numbered(40_000);
    let at = text.find("\nline 30000\n").unwrap() as u64 + 1;
    let source: Arc<dyn Source> = Arc::new(text.into_bytes());
    let mut view = Pager::for_path(source, Path::new("notes.txt"), 40, 12).unwrap_or_else(|e| panic!("{e}"));
    assert!(view.language().is_some());
    view.top = Position { line: at, row: 0 };
    ok(view.refresh());
    let base = view.decoration.as_ref().unwrap().base;
    assert!(at - base <= SYNTAX_CONTEXT, "a jump colours some way above the page");
    for _ in 0..50 {
        ok(view.page_down());
    }
    assert_eq!(view.decoration.as_ref().unwrap().base, base, "moving down colours only the new text");
}
