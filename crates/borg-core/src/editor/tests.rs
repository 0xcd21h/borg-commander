use super::*;
use crate::{AppState, Effect, Pointer, Side, View};
use crate::{Appearance, Msg, Settings, Startup, ThemeChoice, update};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::time::Instant;

fn state() -> (AppState, Instant) {
    let now = Instant::now();
    let themes = vec![ThemeChoice { id: "midnight".into(), name: "Midnight".into() }];
    let (state, _) =
        AppState::new(Startup::at("/work".into()), 80, 12, Settings::default(), Appearance::new(themes, 0), now);
    (state, now)
}

fn stamp(len: u64) -> FileStamp {
    FileStamp { len, modified: None, inode: 7 }
}

fn opened(state: &mut AppState, now: Instant, text: &str) {
    let file = EditorFile { path: "/work/notes.rs".into(), text: text.into(), stamp: stamp(text.len() as u64) };
    update(state, Msg::EditorOpened(EditorSlot::Screen, Ok(file)), now);
}

fn press(state: &mut AppState, now: Instant, key: KeyEvent) -> Vec<Effect> {
    update(state, Msg::Key(key), now)
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

#[test]
fn typing_saves_with_the_stamp_read_and_lands_clean() {
    let (mut state, now) = state();
    opened(&mut state, now, "fn main() {}\n");
    for c in "// hi\n".chars() {
        let key = if c == '\n' { KeyCode::Enter.into() } else { KeyCode::Char(c).into() };
        press(&mut state, now, key);
    }
    let effects = press(&mut state, now, ctrl('s'));
    let [Effect::SaveFile { bytes, expected, force: false, .. }] = &effects[..] else { panic!("{effects:?}") };
    assert_eq!(String::from_utf8_lossy(bytes), "// hi\nfn main() {}\n");
    assert_eq!(*expected, stamp(13));
    update(&mut state, Msg::EditorSaved(EditorSlot::Screen, Ok(stamp(19))), now);
    let editor = state.editor.as_ref().unwrap();
    assert!(!editor.buffer.modified() && editor.stamp == stamp(19));
    assert!(editor.painted.is_some(), "Rust is highlighted");
}

#[test]
fn closing_with_changes_asks_and_a_changed_file_can_be_overwritten() {
    let (mut state, now) = state();
    opened(&mut state, now, "a");
    press(&mut state, now, KeyCode::Char('b').into());
    press(&mut state, now, KeyCode::Esc.into());
    assert_eq!(state.editor.as_ref().unwrap().prompt, Some(EditorPrompt::Unsaved { focus: 0 }));
    let effects = press(&mut state, now, KeyCode::Enter.into());
    assert!(matches!(&effects[..], [Effect::SaveFile { force: false, .. }]));
    update(&mut state, Msg::EditorSaved(EditorSlot::Screen, Err(SaveError::Changed)), now);
    assert_eq!(state.editor.as_ref().unwrap().prompt, Some(EditorPrompt::Changed { focus: 0 }));
    press(&mut state, now, KeyCode::Right.into());
    press(&mut state, now, KeyCode::Right.into());
    assert!(press(&mut state, now, KeyCode::Enter.into()).is_empty(), "Cancel keeps editing");
    assert!(state.editor.as_ref().unwrap().prompt.is_none());
    update(&mut state, Msg::EditorSaved(EditorSlot::Screen, Err(SaveError::Changed)), now);
    let effects = press(&mut state, now, KeyCode::Char('o').into());
    assert!(matches!(&effects[..], [Effect::SaveFile { force: true, .. }]));
    let effects = update(&mut state, Msg::EditorSaved(EditorSlot::Screen, Ok(stamp(2))), now);
    assert!(state.editor.is_none(), "the save that closing asked for closes the editor");
    assert!(
        effects.iter().any(|effect| matches!(effect, Effect::List { .. })),
        "the panel shows the new size: {effects:?}"
    );
}

#[test]
fn the_editor_closes_over_search_results_and_collections() {
    let (mut state, now) = state();
    let side = state.active;
    state.panel_mut(side).open_results(crate::ResultSource::Collection("notes".into()), Vec::new(), 1);
    opened(&mut state, now, "a");
    let effects = press(&mut state, now, KeyCode::Esc.into());
    assert!(state.editor.is_none(), "Esc closes an unchanged file: {effects:?}");
    assert!(matches!(&effects[..], [Effect::RefreshCollection { .. }]), "{effects:?}");

    opened(&mut state, now, "a");
    press(&mut state, now, KeyCode::Char('b').into());
    press(&mut state, now, KeyCode::Esc.into());
    press(&mut state, now, KeyCode::Right.into());
    press(&mut state, now, KeyCode::Enter.into());
    assert!(state.editor.is_none(), "Discard closes the editor");

    opened(&mut state, now, "a");
    press(&mut state, now, KeyCode::Char('b').into());
    press(&mut state, now, KeyCode::Esc.into());
    update(&mut state, Msg::Pointer(Pointer::EditorControl(EditorSlot::Screen, 1)), now);
    assert!(state.editor.is_none(), "a click on Discard closes it too");
}

#[test]
fn an_edit_colours_the_text_as_colouring_it_afresh_would() {
    let (mut state, now) = state();
    let text: String = (0..60).map(|n| format!("let value_{n} = \"{n}\"; // line {n}\n")).collect();
    opened(&mut state, now, &text);
    let rows = usize::from(state.rows - EDITOR_CHROME);
    let colours = |editor: &Editor| {
        (0..editor.buffer.lines()).map(|line| editor.runs(line).map(<[_]>::to_vec)).collect::<Vec<_>>()
    };
    let afresh = |editor: &Editor| {
        let mut fresh = Editor::new(
            EditorFile { path: editor.path.clone(), text: editor.buffer.text(), stamp: stamp(0) },
            EditorSlot::Screen,
        );
        fresh.top = editor.top;
        fresh.paint(rows);
        colours(&fresh)
    };
    for _ in 0..3 {
        press(&mut state, now, KeyCode::Down.into());
    }
    // A comment opened above the rest changes the colour of every line below it.
    for key in ["/", "*", " "].map(|c| KeyCode::Char(c.chars().next().unwrap())).into_iter().chain([KeyCode::Enter]) {
        press(&mut state, now, key.into());
        let editor = state.editor.as_ref().unwrap();
        assert_eq!(colours(editor), afresh(editor), "after {key:?}");
    }
    for key in [KeyCode::Backspace, KeyCode::Backspace, KeyCode::Backspace, KeyCode::Backspace, KeyCode::PageDown] {
        press(&mut state, now, key.into());
        let editor = state.editor.as_ref().unwrap();
        assert_eq!(colours(editor), afresh(editor), "after {key:?}");
    }
    press(&mut state, now, ctrl('z'));
    let editor = state.editor.as_ref().unwrap();
    assert_eq!(colours(editor), afresh(editor), "after an undo");
}

#[test]
fn a_line_as_long_as_minified_code_is_left_uncoloured_and_the_rest_colours_as_without_it() {
    let (mut other, _) = state();
    let (mut state, now) = state();
    opened(&mut other, now, "/* open\n\nclosed */ let value = 1;\n");
    let without = other.editor.as_ref().unwrap().runs(2).map(<[_]>::to_vec);
    let long = format!("let data = \"{}\";", "x".repeat(PAINT_LINE_LIMIT));
    opened(&mut state, now, &format!("/* open\n{long}\nclosed */ let value = 1;\n"));
    let editor = state.editor.as_ref().unwrap();
    assert_eq!(editor.runs(1), Some(&[][..]));
    assert_eq!(editor.runs(2).map(<[_]>::to_vec), without, "the comment opened above it still closes below");
}

#[test]
fn alt_shift_f_lays_out_json_in_one_undo_with_the_cursor_on_its_character() {
    let (mut state, now) = state();
    let format = KeyEvent::new(KeyCode::Char('F'), KeyModifiers::ALT | KeyModifiers::SHIFT);
    let open = |state: &mut AppState, path: &str, text: &str| {
        let file = EditorFile { path: path.into(), text: text.into(), stamp: stamp(text.len() as u64) };
        update(state, Msg::EditorOpened(EditorSlot::Screen, Ok(file)), now);
    };
    let text = r#"{"name":"Ada","tags":["x","y"]}"#;
    open(&mut state, "/work/data.json", text);
    state.editor.as_mut().unwrap().buffer.place(0, 23, false);
    press(&mut state, now, format);
    let editor = state.editor.as_ref().unwrap();
    assert_eq!(
        editor.buffer.text(),
        "{\n    \"name\": \"Ada\",\n    \"tags\": [\n        \"x\",\n        \"y\"\n    ]\n}"
    );
    assert_eq!(editor.buffer.position(), (3, 9), "still before the x");
    assert_eq!(editor.note.as_deref(), Some("Formatted as JSON"));
    press(&mut state, now, ctrl('z'));
    assert_eq!(state.editor.as_ref().unwrap().buffer.text(), text, "one undo puts it back");

    open(&mut state, "/work/broken.json", r#"{"a":"#);
    press(&mut state, now, format);
    let note = state.editor.as_ref().unwrap().note.clone();
    assert_eq!(note.as_deref(), Some("Not JSON: EOF while parsing a value at line 1 column 5"));
    opened(&mut state, now, "fn main() {}\n");
    press(&mut state, now, format);
    assert_eq!(state.editor.as_ref().unwrap().note.as_deref(), Some("Only JSON is formatted"));
}

#[test]
fn copy_cut_and_paste_go_through_borgs_clipboard() {
    let (mut state, now) = state();
    opened(&mut state, now, "one two");
    press(&mut state, now, KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL | KeyModifiers::SHIFT));
    assert_eq!(press(&mut state, now, ctrl('x')), vec![Effect::Copy("one".into())]);
    press(&mut state, now, KeyCode::End.into());
    press(&mut state, now, ctrl('v'));
    update(&mut state, Msg::Paste(" three".into()), now);
    assert_eq!(state.editor.as_ref().unwrap().buffer.text(), " twoone three");
}

#[test]
fn closing_asks_in_a_dialog_whose_buttons_take_clicks() {
    let (mut state, now) = state();
    opened(&mut state, now, "a");
    press(&mut state, now, KeyCode::Char('b').into());
    press(&mut state, now, KeyCode::Esc.into());
    let dialog = dialog(state.editor.as_ref().unwrap()).unwrap();
    assert_eq!(dialog.buttons, ["Save", "Discard", "Cancel"]);
    assert_eq!(dialog.message.as_deref(), Some("Save the changes to notes.rs?"));
    update(&mut state, Msg::Pointer(Pointer::EditorControl(EditorSlot::Screen, 2)), now);
    assert!(state.editor.as_ref().is_some_and(|editor| editor.prompt.is_none()), "Cancel keeps editing");
    press(&mut state, now, KeyCode::Esc.into());
    let effects = update(&mut state, Msg::Pointer(Pointer::EditorControl(EditorSlot::Screen, 1)), now);
    assert!(state.editor.is_none() && effects.iter().any(|effect| matches!(effect, Effect::List { .. })));
}

#[test]
fn replace_steps_through_matches_and_all_replaces_the_rest_in_one_undo() {
    let (mut state, now) = state();
    opened(&mut state, now, "cat Cat\ncat dog\n");
    press(&mut state, now, ctrl('r'));
    update(&mut state, Msg::Paste("cat".into()), now);
    press(&mut state, now, KeyCode::Tab.into());
    update(&mut state, Msg::Paste("cow".into()), now);
    press(&mut state, now, KeyCode::Enter.into());
    assert_eq!(state.editor.as_ref().unwrap().buffer.selection(), Some(0..3), "the first Enter finds a match");
    press(&mut state, now, KeyCode::Enter.into());
    let editor = state.editor.as_ref().unwrap();
    assert_eq!(editor.buffer.text(), "cow Cat\ncat dog\n");
    assert_eq!(editor.buffer.selection(), Some(4..7), "replacing moves on to the next match, in any case");
    assert!(matches!(editor.prompt, Some(EditorPrompt::Replace { .. })), "the dialog stays open");
    press(&mut state, now, KeyEvent::new(KeyCode::Char('a'), KeyModifiers::ALT));
    let editor = state.editor.as_ref().unwrap();
    assert_eq!(editor.buffer.text(), "cow cow\ncow dog\n");
    assert_eq!(editor.note.as_deref(), Some("Replaced 2"));
    assert!(editor.prompt.is_none());
    press(&mut state, now, ctrl('z'));
    assert_eq!(state.editor.as_ref().unwrap().buffer.text(), "cow Cat\ncat dog\n");
    press(&mut state, now, KeyCode::Right.into());
    press(&mut state, now, KeyCode::F(4).into());
    let Some(EditorPrompt::Replace { find, with, .. }) = &state.editor.as_ref().unwrap().prompt else { panic!() };
    assert_eq!((find.text(), with.text()), ("cat", "cow"), "the dialog remembers what it replaced");
}

#[test]
fn clicks_place_the_cursor_drags_select_and_the_wheel_moves_the_view_only() {
    let (mut state, now) = state();
    opened(&mut state, now, &(1..=40).map(|n| format!("line {n}\n")).collect::<String>());
    let click = |line, column, select, word| {
        Msg::Pointer(Pointer::EditorClick { slot: EditorSlot::Screen, line, column, select, word })
    };
    update(&mut state, click(2, 3, false, false), now);
    assert_eq!(state.editor.as_ref().unwrap().buffer.position(), (2, 3));
    update(&mut state, click(3, 6, true, false), now);
    assert_eq!(state.editor.as_ref().unwrap().buffer.selected_text().as_deref(), Some("e 3\nline 4"));
    update(&mut state, click(5, 1, false, true), now);
    assert_eq!(state.editor.as_ref().unwrap().buffer.selected_text().as_deref(), Some("line"));
    update(&mut state, Msg::Pointer(Pointer::EditorScroll(EditorSlot::Screen, 20)), now);
    let editor = state.editor.as_ref().unwrap();
    assert_eq!((editor.top, editor.buffer.position()), (20, (5, 4)), "the cursor stays behind");
    update(
        &mut state,
        Msg::Pointer(Pointer::Panel { side: crate::Side::Left, index: 2, open: true, mark: false }),
        now,
    );
    assert!(state.editor.is_some(), "clicks never reach the panels behind the editor");
    press(&mut state, now, KeyCode::Right.into());
    assert!(state.editor.as_ref().unwrap().top <= 5, "a key brings the cursor back into view");
}

#[test]
fn go_to_line_and_the_find_bar_search_on_from_the_cursor() {
    let (mut state, now) = state();
    opened(&mut state, now, &(1..=40).map(|n| format!("line {n}\n")).collect::<String>());
    press(&mut state, now, ctrl('g'));
    for c in "30".chars() {
        press(&mut state, now, KeyCode::Char(c).into());
    }
    press(&mut state, now, KeyCode::Enter.into());
    let editor = state.editor.as_ref().unwrap();
    assert_eq!(editor.buffer.position(), (29, 0));
    assert!(editor.top <= 29 && 29 < editor.top + 10, "the cursor stays on screen");
    press(&mut state, now, ctrl('f'));
    update(&mut state, Msg::Paste("line 4".into()), now);
    let editor = state.editor.as_ref().unwrap();
    assert_eq!(editor.buffer.selection(), Some(303..309), "typing finds line 40, the first after line 30");
    assert_eq!(crate::find_bar::summary(&editor.find).as_deref(), Some("2 of 2"));
    assert_eq!(editor.matches_on("line 4 and line 40"), [0..6, 11..17], "the bar marks every match");
    press(&mut state, now, KeyCode::Enter.into());
    let editor = state.editor.as_ref().unwrap();
    assert_eq!(editor.buffer.position(), (3, 6), "Enter goes on past the end to line 4");
    assert_eq!(crate::find_bar::summary(&editor.find).as_deref(), Some("1 of 2 · from the other end"));
    press(&mut state, now, KeyCode::Up.into());
    assert_eq!(state.editor.as_ref().unwrap().buffer.position(), (39, 6), "Up goes back");
    for _ in 0.."line 4".len() {
        press(&mut state, now, KeyCode::Backspace.into());
    }
    let editor = state.editor.as_ref().unwrap();
    assert_eq!((editor.buffer.position(), editor.buffer.selection()), ((29, 0), None), "no query, no move");
    update(&mut state, Msg::Paste("line 9".into()), now);
    press(&mut state, now, KeyCode::Esc.into());
    let editor = state.editor.as_ref().unwrap();
    assert!(!editor.find.open && editor.matches_on("line 9").is_empty(), "Esc takes the marks away");
    assert_eq!(editor.buffer.selected_text().as_deref(), Some("line 9"), "and keeps the match selected");
    assert_eq!(state.find_recent, ["line 9", "line 4"], "queries stepped through or closed on are kept");
    press(&mut state, now, KeyEvent::new(KeyCode::F(3), KeyModifiers::SHIFT));
    let editor = state.editor.as_ref().unwrap();
    assert_eq!(editor.buffer.position(), (8, 6), "Shift-F3 steps back with the bar closed");
    assert_eq!(editor.note.as_deref(), Some("1 of 1 · from the other end"));
    press(&mut state, now, ctrl('f'));
    press(&mut state, now, KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
    let editor = state.editor.as_ref().unwrap();
    assert_eq!(editor.note, None);
    assert_eq!(crate::find_bar::summary(&editor.find).as_deref(), Some("No matches"));
}

#[test]
fn whole_words_and_regular_expressions_find_and_replace_with_groups() {
    let (mut state, now) = state();
    opened(&mut state, now, "cat concat cat\nlet cat = 1;\n");
    press(&mut state, now, ctrl('f'));
    update(&mut state, Msg::Paste("cat".into()), now);
    assert_eq!(state.editor.as_ref().unwrap().find.count.map(|count| count.total), Some(4));
    press(&mut state, now, KeyEvent::new(KeyCode::Char('w'), KeyModifiers::ALT));
    assert_eq!(state.editor.as_ref().unwrap().find.count.map(|count| count.total), Some(3), "concat is no word");
    press(&mut state, now, KeyCode::Esc.into());
    press(&mut state, now, ctrl('r'));
    for _ in 0..3 {
        press(&mut state, now, KeyCode::Backspace.into());
    }
    update(&mut state, Msg::Paste(r"(\w+) = (\d)".into()), now);
    press(&mut state, now, KeyEvent::new(KeyCode::Char('r'), KeyModifiers::ALT));
    assert!(state.editor.as_ref().unwrap().find.regex && state.editor.as_ref().unwrap().find.words);
    press(&mut state, now, KeyCode::Tab.into());
    update(&mut state, Msg::Paste("$2 = $1".into()), now);
    press(&mut state, now, KeyEvent::new(KeyCode::Char('a'), KeyModifiers::ALT));
    let editor = state.editor.as_ref().unwrap();
    assert_eq!(editor.buffer.text(), "cat concat cat\nlet 1 = cat;\n");
    assert_eq!(editor.note.as_deref(), Some("Replaced 1"));
    press(&mut state, now, ctrl('z'));
    assert_eq!(state.editor.as_ref().unwrap().buffer.text(), "cat concat cat\nlet cat = 1;\n");
}

fn docked(state: &mut AppState, now: Instant, side: Side, name: &str, text: &str) {
    let path = format!("/work/{name}").into();
    let file = EditorFile { path, text: text.into(), stamp: stamp(text.len() as u64) };
    update(state, Msg::EditorOpened(EditorSlot::Panel(side), Ok(file)), now);
}

fn text_in(state: &AppState, side: Side) -> String {
    state.docked[side.index()].as_ref().map(|editor| editor.buffer.text()).unwrap_or_default()
}

#[test]
fn each_panel_holds_an_editor_f6_moves_between_them_and_text_copies_across() {
    let (mut state, now) = state();
    docked(&mut state, now, Side::Left, "left.txt", "alpha\n");
    docked(&mut state, now, Side::Right, "right.txt", "beta\n");
    assert_eq!((state.active, focused(&state)), (Side::Right, Some(EditorSlot::Panel(Side::Right))));
    press(&mut state, now, ctrl('a'));
    assert_eq!(press(&mut state, now, ctrl('c')), vec![Effect::Copy("beta\n".into())]);

    press(&mut state, now, KeyCode::F(6).into());
    assert_eq!(focused(&state), Some(EditorSlot::Panel(Side::Left)));
    press(&mut state, now, ctrl('v'));
    press(&mut state, now, KeyCode::Tab.into());
    assert_eq!(state.active, Side::Left, "Tab indents in an editor and stays");
    let indent = state.docked[0].as_ref().unwrap().buffer.indent.unit();
    assert_eq!(text_in(&state, Side::Left), format!("beta\n{indent}alpha\n"));
    assert_eq!(text_in(&state, Side::Right), "beta\n", "the other editor is as it was");

    let [Effect::SaveFile { path, slot, .. }] = &press(&mut state, now, KeyCode::F(2).into())[..] else {
        panic!("F2 saves the left file");
    };
    assert_eq!((path.as_path(), *slot), (std::path::Path::new("/work/left.txt"), EditorSlot::Panel(Side::Left)));
    update(&mut state, Msg::EditorSaved(EditorSlot::Panel(Side::Left), Ok(stamp(13))), now);
    let left = state.docked[0].as_ref().unwrap();
    assert!(!left.buffer.modified() && left.note.as_deref() == Some("Saved"));

    let effects = press(&mut state, now, KeyCode::Esc.into());
    assert!(state.docked[0].is_none(), "Esc gives the panel its files back");
    assert!(effects.iter().any(|effect| matches!(effect, Effect::List { side: Side::Left, .. })));
    assert_eq!(focused(&state), None, "the panel's own keys work again");
}

#[test]
fn quit_asks_about_each_panel_editor_with_changes_first() {
    let (mut state, now) = state();
    docked(&mut state, now, Side::Left, "draft.txt", "draft");
    press(&mut state, now, KeyCode::Char('!').into());
    press(&mut state, now, KeyCode::F(6).into());
    assert_eq!((state.active, focused(&state)), (Side::Right, None));

    press(&mut state, now, KeyCode::F(10).into());
    assert!(state.overlay.is_none());
    assert_eq!(state.active, Side::Left, "the editor with changes comes forward");
    let dialog = dialog(state.docked[0].as_ref().unwrap()).unwrap();
    assert_eq!(dialog.message.as_deref(), Some("Save the changes to draft.txt?"));
    update(&mut state, Msg::Pointer(Pointer::EditorControl(EditorSlot::Panel(Side::Left), 1)), now);
    assert!(state.docked[0].is_none(), "Discard closes it");

    press(&mut state, now, KeyCode::F(10).into());
    assert!(matches!(state.overlay, Some(crate::Overlay::Confirm(_))), "with none left, Quit asks as ever");
}

#[test]
fn ctrl_o_shows_the_console_from_a_panel_editor_and_brings_it_back() {
    let (mut state, now) = state();
    docked(&mut state, now, Side::Left, "notes.txt", "notes");
    press(&mut state, now, ctrl('o'));
    assert_eq!((state.view, focused(&state)), (View::Shell, None));
    press(&mut state, now, ctrl('o'));
    assert_eq!((state.view, focused(&state)), (View::Panels, Some(EditorSlot::Panel(Side::Left))));
    assert_eq!(text_in(&state, Side::Left), "notes", "neither Ctrl-O typed into the file");
}

#[test]
fn panel_editors_fit_their_panels_follow_a_swap_and_take_clicks_there() {
    let (mut state, now) = state();
    docked(&mut state, now, Side::Left, "one.txt", &"wide line\n".repeat(30));
    docked(&mut state, now, Side::Right, "two.txt", "two\n");
    let layout = crate::Layout::new(state.cols, state.rows, state.chrome(), state.split_target());
    let left = state.docked[0].as_ref().unwrap();
    assert_eq!(text_size(&state, left).0, usize::from(layout.panels[0].height - EDITOR_CHROME));

    update(&mut state, Msg::Pointer(Pointer::EditorScroll(EditorSlot::Panel(Side::Left), 5)), now);
    assert_eq!(state.active, Side::Right, "the wheel scrolls without taking focus");
    assert_eq!(state.docked[0].as_ref().unwrap().top, 5);
    let click =
        Pointer::EditorClick { slot: EditorSlot::Panel(Side::Left), line: 7, column: 2, select: false, word: false };
    update(&mut state, Msg::Pointer(click), now);
    assert_eq!(state.active, Side::Left, "a click does");
    assert_eq!(state.docked[0].as_ref().unwrap().buffer.position(), (7, 2));

    crate::actions::perform(&mut state, crate::Action::SwapPanels, now);
    let right = state.docked[1].as_ref().unwrap();
    assert_eq!((right.path.to_str(), right.slot), (Some("/work/one.txt"), EditorSlot::Panel(Side::Right)));
    assert_eq!(state.docked[0].as_ref().unwrap().slot, EditorSlot::Panel(Side::Left));
}

#[test]
fn previews_give_way_to_panel_editors_and_their_hidden_files_take_no_menu_actions() {
    let (mut state, now) = state();
    state.aside = Some(crate::Aside::QuickView);
    docked(&mut state, now, Side::Left, "notes.txt", "notes");
    assert_eq!(state.aside, None);
    press(&mut state, now, KeyCode::F(6).into());
    press(&mut state, now, ctrl('q'));
    assert_eq!(state.aside, None);
    assert_eq!(state.notice.as_deref(), Some("The other panel holds an editor; close it to show a preview"));

    // From the files, Tab goes back to the editor; F6 there moves files, as ever.
    press(&mut state, now, KeyCode::Tab.into());
    assert_eq!(focused(&state), Some(EditorSlot::Panel(Side::Left)));
    let file = crate::menus(&state).into_iter().find(|menu| menu.title == "File").unwrap();
    let enabled = |action: crate::Action| {
        file.entries
            .iter()
            .any(|entry| matches!(entry, crate::MenuEntry::Item(item) if item.action == action && item.enabled))
    };
    assert!(!enabled(crate::Action::Mkdir) && !enabled(crate::Action::SelectGroup));
    assert!(enabled(crate::Action::Quit));
}
