use super::*;
use crate::columns::advance;
use proptest::prelude::*;
use unicode_segmentation::UnicodeSegmentation;

fn typed(buffer: &mut Buffer, text: &str) {
    text.chars().for_each(|c| if c == '\n' { buffer.newline() } else { buffer.type_char(c) });
}

#[test]
fn replacing_ranges_is_one_step_and_clicks_place_the_cursor_and_select_words() {
    let mut buffer = Buffer::new("Café cafe\ncafé\tlatte");
    buffer.place(1, 8, false);
    assert_eq!(buffer.position(), (1, 5), "the tab spans columns 4 to 8");
    assert_eq!(buffer.replace_ranges(&[(0..4, "tea".into()), (10..14, "tea".into())]), 2);
    assert_eq!(buffer.text(), "tea cafe\ntea\tlatte");
    assert_eq!(buffer.position(), (1, 4), "the cursor keeps its place after the shorter text");
    assert!(buffer.undo());
    assert_eq!(buffer.text(), "Café cafe\ncafé\tlatte");
    buffer.select_word(0, 6);
    assert_eq!(buffer.selected_text().as_deref(), Some("cafe"));
    buffer.place(1, 2, true);
    assert_eq!(buffer.selected_text().as_deref(), Some("cafe\nca"), "a drag extends from the word");
    assert_eq!(buffer.replace_ranges(&[]), 0);
}

#[test]
fn the_text_reads_as_bytes_from_any_offset() {
    let text = "naïve\n".repeat(400);
    let buffer = Buffer::new(&text);
    assert_eq!(buffer.len_bytes(), text.len());
    let mut buf = vec![0; 1000];
    assert_eq!(buffer.read_at(3, &mut buf), 1000);
    assert_eq!(&buf[..], &text.as_bytes()[3..1003]);
    assert_eq!(buffer.read_at(text.len() - 2, &mut buf), 2);
    assert_eq!(buffer.byte_to_char(7), 6, "ï takes two bytes");
    assert_eq!(buffer.char_to_byte(6), 7);
    assert_eq!(buffer.line_of(6), 1);
}

#[test]
fn the_first_changed_line_waits_until_it_is_asked_for() {
    let mut buffer = Buffer::new("a\nb\nc\nd");
    assert_eq!(buffer.take_changed_line(), None);
    buffer.place(2, 1, false);
    buffer.type_char('x');
    buffer.place(1, 0, false);
    buffer.newline();
    assert_eq!(buffer.take_changed_line(), Some(1), "the earliest of the changes");
    assert_eq!(buffer.take_changed_line(), None);
    assert!(buffer.undo());
    assert_eq!(buffer.take_changed_line(), Some(1), "an undo changes the text too");
}

#[test]
fn typing_undoes_by_words_and_redo_brings_them_back() {
    let mut buffer = Buffer::new("");
    typed(&mut buffer, "hello world");
    assert_eq!(buffer.text(), "hello world");
    assert!(buffer.undo());
    assert_eq!(buffer.text(), "hello ");
    assert!(buffer.undo() && buffer.undo());
    assert_eq!(buffer.text(), "");
    assert!(!buffer.modified(), "undoing everything returns to the opened text");
    assert!(buffer.redo() && buffer.redo());
    assert_eq!(buffer.text(), "hello ");
}

#[test]
fn saving_marks_the_text_clean_until_it_changes_again() {
    let mut buffer = Buffer::new("a");
    buffer.move_to(Motion::Finish, false);
    buffer.type_char('b');
    buffer.mark_saved();
    assert!(!buffer.modified());
    buffer.type_char('c');
    assert!(buffer.modified(), "typing after a save is a new change");
    buffer.undo();
    assert!(!buffer.modified());
    buffer.undo();
    assert!(buffer.modified(), "undoing past the save changes the file again");
}

#[test]
fn crlf_files_edit_with_line_feeds_and_save_their_endings() {
    let mut buffer = Buffer::new("one\r\ntwo\r\n");
    assert_eq!((buffer.eol, buffer.lines()), (Eol::Crlf, 3));
    buffer.move_to(Motion::End, false);
    buffer.newline();
    buffer.insert("new");
    assert_eq!(buffer.text(), "one\r\nnew\r\ntwo\r\n");
    assert_eq!(Buffer::new("mixed\r\nends\n").text(), "mixed\r\nends\n", "mixed endings stay as they are");
}

#[test]
fn a_byte_of_the_file_as_read_finds_its_character_whatever_the_line_endings() {
    let crlf = Buffer::new("one\r\ntwø\r\nthree\r\n");
    let found = [0, 5, 7, 9, 11, 100].map(|byte| crlf.file_byte_to_char(byte));
    assert_eq!(found, [0, 4, 6, 7, 8, crlf.len_chars()], "each line ending before the byte took one more");
    let lf = Buffer::new("one\ntwø\nthree\n");
    assert_eq!([0, 4, 6, 9].map(|byte| lf.file_byte_to_char(byte)), [0, 4, 6, 8]);
}

#[test]
fn the_cursor_steps_over_whole_graphemes_and_keeps_its_column_on_short_lines() {
    let mut buffer = Buffer::new("a👩‍💻b\nx\nwide 日本語");
    buffer.move_to(Motion::Right, false);
    buffer.move_to(Motion::Right, false);
    assert_eq!(buffer.position(), (0, 4), "the emoji sequence is one step");
    assert_eq!(buffer.screen_column(), 3);
    buffer.move_to(Motion::Down, false);
    assert_eq!(buffer.position(), (1, 1));
    buffer.move_to(Motion::Down, false);
    assert_eq!(buffer.screen_column(), 3, "the goal column survives the short line");
    buffer.backspace();
    assert_eq!(buffer.line(2), "wie 日本語");
}

#[test]
fn home_goes_to_the_indentation_first_and_newline_keeps_it() {
    let mut buffer = Buffer::new("    let x = 1;");
    buffer.move_to(Motion::End, false);
    buffer.move_to(Motion::Home, false);
    assert_eq!(buffer.position(), (0, 4));
    buffer.move_to(Motion::Home, false);
    assert_eq!(buffer.position(), (0, 0));
    buffer.move_to(Motion::End, false);
    buffer.newline();
    assert_eq!(buffer.line(1), "    ");
    assert_eq!(buffer.indent, Indent::Spaces(4));
}

#[test]
fn tab_indents_selected_lines_and_shift_tab_outdents_them() {
    let mut buffer = Buffer::new("a\n  b\nc");
    assert_eq!(buffer.indent, Indent::Spaces(2));
    buffer.move_to(Motion::Down, true);
    buffer.move_to(Motion::End, true);
    buffer.indent(false);
    assert_eq!(buffer.text(), "  a\n    b\nc");
    assert_eq!(buffer.selected_text().as_deref(), Some("  a\n    b"));
    buffer.indent(true);
    buffer.indent(true);
    assert_eq!(buffer.text(), "a\nb\nc");
    assert_eq!(Buffer::new("\tx\n\ty").indent, Indent::Tabs);
}

#[test]
fn typing_over_a_selection_replaces_it_and_cut_removes_it() {
    let mut buffer = Buffer::new("one two three");
    buffer.move_to(Motion::WordRight, false);
    buffer.move_to(Motion::WordRight, true);
    assert_eq!(buffer.selected_text().as_deref(), Some(" two"));
    buffer.type_char('!');
    assert_eq!(buffer.text(), "one! three");
    buffer.select_all();
    assert_eq!(buffer.cut().as_deref(), Some("one! three"));
    assert_eq!(buffer.text(), "");
    assert!(buffer.undo());
    assert_eq!(buffer.text(), "one! three");
}

#[test]
fn tabs_expand_to_their_columns_and_controls_become_visible() {
    assert_eq!(expand("a\tb"), "a   b");
    assert_eq!(expand("ab\u{7}c"), "ab·c");
    assert_eq!(width("日本\t"), 8);
    let shown = cells("e\u{301}\t日", 0);
    assert_eq!(shown, [(0, "e\u{301}".into(), 1), (2, "   ".into(), 3), (3, "日".into(), 2)]);
    assert_eq!(cells("\tx", 2), [(0, "  ".into(), 2), (1, "x".into(), 1)], "tabs stop where the part starts");
}

#[test]
fn a_long_line_is_drawn_and_walked_by_the_part_that_shows() {
    let line = format!("{}日本{}", "x".repeat(70_000), "y".repeat(10_000));
    let mut buffer = Buffer::new(&format!("{line}\nshort"));
    let window = buffer.window(0, 70_000, 80);
    assert_eq!((window.start, window.column), (70_000 - MARGIN, 70_000 - MARGIN));
    assert_eq!(width(&window.text), 2 * MARGIN + 80);
    assert!(window.text.contains("日本"));
    assert_eq!(buffer.window(1, 0, 80).text, "short", "a short line comes whole");

    buffer.place(0, 70_001, false);
    assert_eq!(buffer.position(), (0, 70_000), "a click inside a wide character lands before it");
    buffer.move_to(Motion::Right, false);
    assert_eq!((buffer.position(), buffer.screen_column()), ((0, 70_001), 70_002));
    buffer.move_to(Motion::End, false);
    assert_eq!(buffer.screen_column(), 80_004);
    buffer.move_to(Motion::Down, false);
    buffer.move_to(Motion::Up, false);
    assert_eq!(buffer.position(), (0, 80_002), "the goal column holds across the short line");
    buffer.select_word(0, 3);
    assert_eq!(buffer.selection(), Some(0..80_002), "letters of any script are one word");
}

proptest! {
    #[test]
    fn walking_runs_at_once_agrees_with_walking_by_graphemes(
        text in "[a-c \t\u{7}\u{301}\u{600}日é👍🏽\u{200d}🇩🇪]{0,40}",
        goal in 0usize..60,
    ) {
        let (mut column, mut chars) = (0, 0);
        for grapheme in text.graphemes(true) {
            let next = advance(column, grapheme);
            if next > goal {
                break;
            }
            (column, chars) = (next, chars + grapheme.chars().count());
        }
        let spot = walk(&text, Spot::default(), Until::Column(goal));
        prop_assert_eq!((spot.char, spot.column), (chars, column));
        prop_assert_eq!(walk(&text, Spot::default(), Until::Char(chars)).column, column);
        prop_assert_eq!(width(&text), text.graphemes(true).fold(0, advance));
    }

    #[test]
    fn undoing_every_edit_restores_the_text_and_redo_replays_them(
        start in "[a-c \\n]{0,20}",
        edits in proptest::collection::vec((0u8..6, "[a-z\\n]{1,3}"), 1..30),
    ) {
        let mut buffer = Buffer::new(&start);
        let mut texts = Vec::new();
        for (kind, text) in edits {
            match kind {
                0 => buffer.insert(&text),
                1 => buffer.backspace(),
                2 => buffer.move_to(Motion::Left, text.len() > 1),
                3 => buffer.move_to(Motion::Up, false),
                4 => buffer.indent(text.len() == 2),
                _ => buffer.type_char(text.chars().next().unwrap()),
            }
            texts.push(buffer.text());
        }
        let end = buffer.text();
        while buffer.undo() {}
        prop_assert_eq!(buffer.text(), start);
        while buffer.redo() {}
        prop_assert_eq!(buffer.text(), end);
    }
}
