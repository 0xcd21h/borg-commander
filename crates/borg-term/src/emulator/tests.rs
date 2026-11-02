use super::*;

const SIZE: GridSize = GridSize { cols: 20, rows: 4 };

#[test]
fn scrollback_moves_the_viewport_without_changing_the_prompt() {
    let mut emulator = Emulator::new(SIZE, 100);
    for n in 1..=20 {
        emulator.feed(format!("output {n}\r\n").as_bytes());
    }
    emulator.feed(b"\x1b]133;A\x07$ \x1b]133;B\x07");
    let live = emulator.screen().cell(0, 0).c;
    emulator.scroll(Scroll::Top);
    assert!(emulator.screen().scrollback_offset() > 0);
    assert_eq!(emulator.screen().cell(0, 0).c, 'o');
    assert_eq!(emulator.screen().cell(7, 0).c, '1');
    assert_eq!(emulator.screen().live_cell(0, 0).c, live);
    assert_eq!(emulator.prompt_state(), PromptState::Empty);
    assert_eq!(emulator.command_line(), "");
    assert!(!emulator.screen().cursor_visible());
    emulator.scroll(Scroll::Bottom);
    assert_eq!(emulator.screen().scrollback_offset(), 0);
    assert!(emulator.screen().cursor_visible());
}

#[test]
fn tracks_empty_and_typed_prompt() {
    let mut emulator = Emulator::new(SIZE, 0);
    assert_eq!(emulator.prompt_state(), PromptState::Unknown);

    emulator.feed(b"\x1b]133;A\x07$ \x1b]133;B\x07");
    assert_eq!(emulator.prompt_state(), PromptState::Empty);

    emulator.feed(b"ls");
    assert_eq!(emulator.prompt_state(), PromptState::Typed);

    emulator.feed(b"\x08\x08");
    assert_eq!(emulator.prompt_state(), PromptState::Typed);

    emulator.feed(b"\x1b[K");
    assert_eq!(emulator.prompt_state(), PromptState::Empty);
}

#[test]
fn reads_the_typed_command_without_autosuggestions() {
    let mut emulator = Emulator::new(GridSize { cols: 24, rows: 4 }, 0);
    emulator.feed(b"\x1b]133;A\x07$ \x1b]133;B\x07");
    assert_eq!(emulator.command_line(), "");
    emulator.feed(b"echo a long line that wraps");
    assert_eq!(emulator.command_line(), "echo a long line that wraps");
    emulator.feed(b"\x1b[90m --suggested\x1b[0m\x1b[12D");
    assert_eq!(emulator.command_line(), "echo a long line that wraps");
}

#[test]
fn keeps_the_blanks_typed_before_the_cursor() {
    let mut emulator = Emulator::new(SIZE, 0);
    emulator.feed(b"\x1b]133;A\x07$ \x1b]133;B\x07cd ");
    assert_eq!(emulator.command_line(), "cd ");
    emulator.feed(b"\x1b[90m~/src\x1b[0m\x1b[5D");
    assert_eq!(emulator.command_line(), "cd ");
    emulator.feed(b"\x1b[K\x08\x08");
    assert_eq!(emulator.command_line(), "cd", "blanks after the cursor were not typed");
}

#[test]
fn forgets_prompt_when_command_runs() {
    let mut emulator = Emulator::new(SIZE, 0);
    emulator.feed(b"$ \x1b]133;B\x07ls\r\n\x1b]133;C\x07");
    assert_eq!(emulator.prompt_state(), PromptState::Unknown);
}

#[test]
fn exported_output_replays_into_identical_screen() {
    let mut original = Emulator::new(SIZE, 100);
    original.feed(b"plain\r\n\x1b[1;31mred bold\x1b[0m and \x1b[38;5;208morange\x1b[0m\r\n\x1b[44m bg \x1b[0m");
    let exported = original.export_ansi(100);

    let mut restored = Emulator::new(SIZE, 100);
    restored.feed(&exported);

    let text = |e: &Emulator, row: u16| -> String {
        (0..SIZE.cols).map(|col| e.screen().cell(col, row).c).collect::<String>().trim_end().to_owned()
    };
    for row in 0..3 {
        assert_eq!(text(&restored, row), text(&original, row));
        for col in 0..SIZE.cols {
            let (a, b) = (original.screen().cell(col, row), restored.screen().cell(col, row));
            assert_eq!((a.fg, a.bg, a.flags), (b.fg, b.bg, b.flags), "cell {col},{row}");
        }
    }
}

#[test]
fn export_keeps_only_the_most_recent_lines() {
    let mut emulator = Emulator::new(SIZE, 100);
    (1..=10).for_each(|i| {
        emulator.feed(format!("line {i}\r\n").as_bytes());
    });
    let exported = String::from_utf8(emulator.export_ansi(3)).unwrap_or_default();
    assert_eq!(exported.matches("line").count(), 3);
    assert!(exported.starts_with("line 8"));
}

#[test]
fn a_concealed_command_leaves_no_trace_and_the_next_prompt_takes_the_old_ones_place() {
    let mut emulator = Emulator::new(SIZE, 100);
    emulator.feed(b"done\r\n\x1b]133;A\x07~/a $ \x1b]133;B\x07");
    emulator.conceal();
    let signals = emulator.feed(b" cd '/b'\r\n\x1b]133;D;0\x07\x1b]7;file://host/b\x07");
    assert!(signals.contains(&ShellSignal::CommandEnd { exit: Some(0) }), "markers still arrive: {signals:?}");
    assert!(!emulator.screen().live_text().iter().any(|line| line.contains("cd")));
    emulator.feed(b"\x1b]133;A\x07/b $ \x1b]133;B\x07");
    let text = emulator.screen().live_text();
    assert_eq!(text.iter().filter(|line| !line.trim().is_empty()).collect::<Vec<_>>(), ["done", "/b $"]);
    assert_eq!(emulator.prompt_state(), PromptState::Empty);
}

#[test]
fn concealing_gives_up_without_a_prompt_and_shows_what_it_held() {
    let mut emulator = Emulator::new(SIZE, 100);
    emulator.conceal();
    emulator.feed(b"no prompt yet");
    assert!(emulator.screen().live_text().iter().any(|line| line.contains("no prompt yet")));
    emulator.feed(b"\r\n\x1b]133;A\x07$ ");
    emulator.conceal();
    emulator.feed(&vec![b'x'; CONCEAL_LIMIT + 1]);
    assert!(emulator.screen().live_text().iter().any(|line| line.contains("xxx")), "long output is not lost");
}

#[test]
fn reports_signals_in_order() {
    let mut emulator = Emulator::new(SIZE, 0);
    let signals = emulator.feed(b"\x1b]133;D;0\x07\x1b]133;A\x07");
    assert_eq!(signals, vec![ShellSignal::CommandEnd { exit: Some(0) }, ShellSignal::PromptStart]);
}

#[test]
fn tracks_wrapped_input_when_prompt_scrolls_up() {
    let mut emulator = Emulator::new(GridSize { cols: 12, rows: 4 }, 100);
    emulator.feed(b"output\r\noutput\r\noutput\r\n\x1b]133;A\x07$ \x1b]133;B\x07");
    emulator.feed(b"echo a command with several wrapped rows");
    assert_eq!(emulator.command_line(), "echo a command with several wrapped rows");
    assert_eq!(emulator.command_rows(), 4);
    assert_eq!(emulator.screen().input_region(), Some((0, 3)));
    emulator.feed(b"\x1b[2A\x1b[3G");
    assert_eq!(emulator.command_rows(), 4, "moving the cursor up preserves following input rows");
    emulator.feed(b"\x1b]133;C\x07");
    assert_eq!(emulator.command_rows(), 1);
}

#[test]
fn explicit_newlines_survive_command_recall() {
    let mut emulator = Emulator::new(GridSize { cols: 40, rows: 6 }, 100);
    emulator.feed(b"\x1b]133;A\x07$ \x1b]133;B\x07printf first\r\nprintf second");
    assert_eq!(emulator.command_line(), "printf first\nprintf second");
    assert_eq!(emulator.command_rows(), 2);
    emulator.feed(b"\x1b[1A\r\x1b[2C\x1b[J");
    assert_eq!(emulator.command_rows(), 1);
    assert_eq!(emulator.command_line(), "");
}
