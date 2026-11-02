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

#[test]
fn a_formatted_documents_source_is_coloured_as_it_opens_and_each_view_keeps_its_colours() {
    for (name, text) in [
        ("guide.md", "# Guide\n\nSome `code` and a [link](https://example.com).\n"),
        ("data.json", "{\"name\":\"borg\",\"tags\":[\"one\",\"two\"],\"size\":42}\n"),
    ] {
        let source: Arc<dyn Source> = Arc::new(text.as_bytes().to_vec());
        let mut view = Pager::for_path(source, Path::new(name), 40, 8).unwrap_or_else(|e| panic!("{e}"));
        assert!(view.formatted(), "{name}");
        let formatted = view.decoration.clone().unwrap_or_else(|| panic!("{name}: the formatted view is coloured"));
        let coloured = view.document.as_ref().and_then(|document| document.hidden.decoration.clone());
        let source = coloured.unwrap_or_else(|| panic!("{name}: the source is coloured as the document opens"));
        ok(view.toggle_source());
        let shown = view.decoration.clone().unwrap_or_else(|| panic!("{name}: the source shows coloured"));
        assert!(Arc::ptr_eq(&shown, &source), "{name}: showing the source paints nothing new");
        assert!(view.rich_rows()[0].iter().any(|run| run.paint != Paint::default()), "{name}");
        ok(view.toggle_source());
        assert!(view.decoration.as_ref().is_some_and(|shown| Arc::ptr_eq(shown, &formatted)), "{name}");
        ok(view.toggle_source());
        assert!(view.decoration.as_ref().is_some_and(|shown| Arc::ptr_eq(shown, &source)), "{name}");
    }
}

fn bytes(count: usize) -> Vec<u8> {
    (0..count).map(|i| i as u8).collect()
}

#[test]
fn hex_rows_scroll_by_row_and_page_and_stop_at_either_end() {
    let mut view = Pager::new(Arc::new(bytes(100)), 40, 3).unwrap_or_else(|e| panic!("{e}"));
    ok(view.toggle_hex());
    assert!(view.hex());
    assert_eq!(
        view.rows(),
        [
            "0000  00 01 02 03 04 05 06 07  ........",
            "0008  08 09 0a 0b 0c 0d 0e 0f  ........",
            "0010  10 11 12 13 14 15 16 17  ........",
        ]
    );
    ok(view.scroll_down(1));
    assert_eq!(view.offset(), 8);
    ok(view.page_down());
    assert_eq!(view.offset(), 24);
    ok(view.end());
    assert_eq!(view.rows().last().map(String::as_str), Some("0060  60 61 62 63              `abc"));
    assert_eq!((view.offset(), view.percent()), (80, 100));
    ok(view.scroll_down(5));
    assert_eq!(view.offset(), 80);
    ok(view.page_up());
    assert_eq!(view.offset(), 64);
    ok(view.scroll_up(20));
    assert_eq!(view.offset(), 0);
    ok(view.end());
    ok(view.home());
    assert_eq!(view.offset(), 0);
}

#[test]
fn hex_rows_keep_their_place_through_resizes_and_back_to_text() {
    let mut view = pager(&numbered(40), 80, 4);
    ok(view.scroll_down(10));
    assert_eq!(view.rows()[0], "line 11");
    let line = view.offset();
    ok(view.toggle_hex());
    assert_eq!((line, view.offset()), (71, 64), "the row the page's first line starts on");
    ok(view.toggle_wrap());
    ok(view.scroll_right());
    ok(view.resize(40, 4));
    assert_eq!(view.rows()[0], "0040  69 6e 65 20 31 30 0a 6c  ine 10.l", "fewer bytes a row in less room");
    ok(view.end());
    ok(view.resize(80, 4));
    assert_eq!(view.percent(), 100, "the end stays on the last row");
    ok(view.toggle_hex());
    assert!(!view.hex() && view.wrapped() && view.column() == 0, "w and the arrows leave hex rows alone");
    assert_eq!(view.rows()[0], "line 34", "the line the top row's first byte is on, at byte 256");

    let mut document = Pager::for_path(Arc::new(b"# Title\n\nText\n".to_vec()), Path::new("notes.md"), 80, 4)
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(document.formatted());
    ok(document.toggle_hex());
    assert!(!document.formatted(), "hex rows show the document's source");
    assert_eq!(document.rows()[0], format!("0000  23 20 54 69 74 6c 65 0a 0a 54 65 78 74 0a{:8}# Title..Text.", ""));
    ok(document.toggle_source());
    assert!(!document.formatted() && document.hex(), "s leaves hex rows alone");
}

#[test]
fn hex_rows_mark_matches_on_the_page_and_scroll_to_one_off_it() {
    let text: String =
        (0..64).map(|i| if i == 40 { "needle!\n".to_owned() } else { format!("row {i:03}\n") }).collect();
    let mut view = pager(&text, 80, 4);
    ok(view.toggle_hex());
    let search = crate::Query { text: "needle".into(), ..crate::Query::default() }.compile().unwrap().unwrap();
    ok(view.show_matches(Some(search.clone())));
    assert!(view.rich_rows().iter().flatten().all(|run| run.mark == Mark::None));
    let found = search.next(&*view.source(), 0, &|_| true).unwrap().unwrap();
    assert_eq!(found, 320..326);
    ok(view.reveal(found));
    assert_eq!(view.offset(), 320 - 16, "a third of the page shows above the match");
    let current: String =
        view.rich_rows()[1].iter().filter(|run| run.mark == Mark::Current).map(|run| run.text.as_str()).collect();
    assert_eq!(current, "6e 65 65 64 6c 65needle");
    ok(view.forget_match());
    let marked: String =
        view.rich_rows()[1].iter().filter(|run| run.mark == Mark::Match).map(|run| run.text.as_str()).collect();
    assert_eq!(marked, "6e 65 65 64 6c 65needle");
    ok(view.reveal(330..331));
    assert_eq!(view.offset(), 320 - 16, "a match on the page leaves it where it is");
}
