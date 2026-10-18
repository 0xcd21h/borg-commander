use std::sync::mpsc::{Receiver, Sender, channel};

use alacritty_terminal::{
    event::{Event, EventListener},
    grid::Dimensions,
    index::{Column, Line, Point},
    term::{
        Config, Term, TermMode,
        cell::{Cell, Flags},
    },
    vte::ansi::{Color, NamedColor, Processor, Rgb},
};

use crate::{GridSize, OscScanner, ShellSignal};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptState {
    Empty,
    Typed,
    Unknown,
}

#[derive(Clone)]
pub struct Replies(Sender<String>);

impl EventListener for Replies {
    fn send_event(&self, event: Event) {
        if let Event::PtyWrite(text) = event {
            let _ = self.0.send(text);
        }
    }
}

pub struct Emulator {
    term: Term<Replies>,
    parser: Processor,
    scanner: OscScanner,
    replies: Receiver<String>,
    input_start: Option<Point>,
}

impl Emulator {
    pub fn new(size: GridSize, scrollback: usize) -> Self {
        let (tx, replies) = channel();
        let config = Config { scrolling_history: scrollback, ..Config::default() };
        Self {
            term: Term::new(config, &size, Replies(tx)),
            parser: Processor::new(),
            scanner: OscScanner::default(),
            replies,
            input_start: None,
        }
    }

    pub fn feed(&mut self, bytes: &[u8]) -> Vec<ShellSignal> {
        let marks = self.scanner.scan(bytes);
        let mut consumed = 0;
        for (end, signal) in &marks {
            self.parser.advance(&mut self.term, &bytes[consumed..*end]);
            consumed = *end;
            self.track(signal);
        }
        self.parser.advance(&mut self.term, &bytes[consumed..]);
        marks.into_iter().map(|(_, signal)| signal).collect()
    }

    pub fn drain_replies(&self) -> Vec<String> {
        self.replies.try_iter().collect()
    }

    pub fn resize(&mut self, size: GridSize) {
        self.term.resize(size);
        self.input_start = None;
    }

    pub fn mode(&self) -> TermMode {
        *self.term.mode()
    }

    pub fn export_ansi(&self, max_lines: usize) -> Vec<u8> {
        crate::export::export(&self.term, max_lines)
    }

    pub fn screen(&self) -> Screen<'_> {
        Screen { term: &self.term }
    }

    pub fn prompt_state(&self) -> PromptState {
        let Some(start) = self.input_start else { return PromptState::Unknown };
        let cursor = self.term.grid().cursor.point;
        let blank = |cell: &Cell| cell.c == ' ' || cell.c == '\0';

        if cursor == start && blank(&self.term.grid()[cursor]) { PromptState::Empty } else { PromptState::Typed }
    }

    pub fn command_line(&self) -> String {
        let Some(start) = self.input_start else { return String::new() };
        let grid = self.term.grid();
        let cursor = grid.cursor.point;
        let last_column = Column(grid.columns().saturating_sub(1));
        let bottom = Line(i32::try_from(grid.screen_lines()).unwrap_or(i32::MAX) - 1);
        let mut text = String::new();
        let mut line = start.line;
        loop {
            let first = if line == start.line { start.column.0 } else { 0 };
            for column in first..grid.columns() {
                let point = Point::new(line, Column(column));
                let cell = &grid[point];
                if point >= cursor && suggested(cell) {
                    return text.trim_end().to_owned();
                }
                if !cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                    text.push(if cell.c == BLANK { ' ' } else { cell.c });
                }
            }
            if line >= bottom || !grid[Point::new(line, last_column)].flags.contains(Flags::WRAPLINE) {
                return text.trim_end().to_owned();
            }
            line += 1;
        }
    }

    fn track(&mut self, signal: &ShellSignal) {
        self.input_start = match signal {
            ShellSignal::InputStart => Some(self.term.grid().cursor.point),
            ShellSignal::PromptStart | ShellSignal::CommandStart | ShellSignal::CommandEnd { .. } => None,
            ShellSignal::Cwd { .. } => self.input_start,
        };
    }
}

const BLANK: char = '\0';
const SUGGESTION_INDEX: u8 = 8;

fn suggested(cell: &Cell) -> bool {
    cell.flags.contains(Flags::DIM)
        || matches!(
            cell.fg,
            Color::Named(NamedColor::BrightBlack | NamedColor::DimForeground) | Color::Indexed(SUGGESTION_INDEX)
        )
}

pub struct Screen<'a> {
    term: &'a Term<Replies>,
}

impl<'a> Screen<'a> {
    pub fn cols(&self) -> u16 {
        u16::try_from(self.term.grid().columns()).unwrap_or(u16::MAX)
    }

    pub fn rows(&self) -> u16 {
        u16::try_from(self.term.grid().screen_lines()).unwrap_or(u16::MAX)
    }

    pub fn cell(&self, col: u16, row: u16) -> &'a Cell {
        &self.term.grid()[Point::new(Line(i32::from(row)), Column(usize::from(col)))]
    }

    pub fn cursor(&self) -> (u16, u16) {
        let point = self.term.grid().cursor.point;
        let row = u16::try_from(point.line.0).unwrap_or_default();
        let col = u16::try_from(point.column.0).unwrap_or_default();
        (col, row)
    }

    pub fn cursor_visible(&self) -> bool {
        self.term.mode().contains(TermMode::SHOW_CURSOR)
    }

    pub fn color(&self, index: usize) -> Option<Rgb> {
        self.term.colors()[index]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: GridSize = GridSize { cols: 20, rows: 4 };

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
    fn reports_signals_in_order() {
        let mut emulator = Emulator::new(SIZE, 0);
        let signals = emulator.feed(b"\x1b]133;D;0\x07\x1b]133;A\x07");
        assert_eq!(signals, vec![ShellSignal::CommandEnd { exit: Some(0) }, ShellSignal::PromptStart]);
    }
}
