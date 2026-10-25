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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scroll {
    Delta(i32),
    PageUp,
    PageDown,
    Top,
    Bottom,
}

impl From<Scroll> for alacritty_terminal::grid::Scroll {
    fn from(scroll: Scroll) -> Self {
        match scroll {
            Scroll::Delta(lines) => Self::Delta(lines),
            Scroll::PageUp => Self::PageUp,
            Scroll::PageDown => Self::PageDown,
            Scroll::Top => Self::Top,
            Scroll::Bottom => Self::Bottom,
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
            self.advance(&bytes[consumed..*end]);
            consumed = *end;
            self.track(signal);
        }
        self.advance(&bytes[consumed..]);
        marks.into_iter().map(|(_, signal)| signal).collect()
    }

    pub fn drain_replies(&self) -> Vec<String> {
        self.replies.try_iter().collect()
    }

    pub fn resize(&mut self, size: GridSize) {
        self.term.resize(size);
        self.input_start = None;
    }

    pub fn scroll(&mut self, scroll: Scroll) {
        self.term.scroll_display(scroll.into());
    }

    pub fn mode(&self) -> TermMode {
        *self.term.mode()
    }

    pub fn export_ansi(&self, max_lines: usize) -> Vec<u8> {
        crate::export::export(&self.term, max_lines)
    }

    pub fn snapshot_ansi(&self) -> Vec<u8> {
        crate::export::snapshot(&self.term)
    }

    pub fn screen(&self) -> Screen<'_> {
        Screen { term: &self.term, input_start: self.input_start }
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
        let bottom = self.input_bottom();
        let mut text = String::new();
        let mut line = start.line;
        loop {
            let first = if line == start.line { start.column.0 } else { 0 };
            let mut row = String::new();
            for column in first..grid.columns() {
                let point = Point::new(line, Column(column));
                let cell = &grid[point];
                if point >= cursor && suggested(cell) {
                    break;
                }
                if !cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                    row.push(if cell.c == BLANK { ' ' } else { cell.c });
                    if let Some(zero) = cell.zerowidth() {
                        row.extend(zero);
                    }
                }
            }
            let wrapped = grid[Point::new(line, Column(grid.columns() - 1))].flags.contains(Flags::WRAPLINE);
            text.push_str(if wrapped { &row } else { row.trim_end() });
            if line >= bottom {
                return text.trim_end().to_owned();
            }
            if !wrapped {
                text.push('\n');
            }
            line += 1;
        }
    }

    pub fn command_rows(&self) -> u16 {
        self.screen().input_region().map_or(1, |(first, last)| last.saturating_sub(first) + 1)
    }

    fn input_bottom(&self) -> Line {
        let (_, last) = self.screen().input_region().unwrap_or_else(|| {
            let (_, row) = self.screen().cursor();
            (row, row)
        });
        Line(i32::from(last))
    }

    fn advance(&mut self, bytes: &[u8]) {
        if self.input_start.is_none() {
            self.parser.advance(&mut self.term, bytes);
            return;
        }
        for byte in bytes {
            let bottom = self.term.grid().screen_lines() as i32 - 1;
            self.term.vi_mode_cursor.point = Point::new(Line(bottom), Column(0));
            self.parser.advance(&mut self.term, std::slice::from_ref(byte));
            let scrolled = bottom - self.term.vi_mode_cursor.point.line.0;
            if let Some(start) = self.input_start.as_mut() {
                start.line -= scrolled;
                let oldest = -(self.term.grid().history_size() as i32);
                if start.line.0 < oldest {
                    start.line = Line(oldest);
                    start.column = Column(0);
                }
            }
        }
    }

    fn track(&mut self, signal: &ShellSignal) {
        self.input_start = match signal {
            ShellSignal::InputStart => Some(self.term.grid().cursor.point),
            ShellSignal::PromptStart | ShellSignal::CommandStart | ShellSignal::CommandEnd { .. } => None,
            ShellSignal::Cwd { .. } | ShellSignal::SilentCd => self.input_start,
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
    input_start: Option<Point>,
}

impl<'a> Screen<'a> {
    pub fn input_region(&self) -> Option<(u16, u16)> {
        let start = self.input_start?;
        let grid = self.term.grid();
        let cursor = grid.cursor.point;
        let mut last = cursor.line.0.max(start.line.0).max(0) as u16;
        while last + 1 < self.rows() {
            let next = last + 1;
            let nonblank = (0..self.cols()).any(|col| {
                let c = self.live_cell(col, next).c;
                c != ' ' && c != BLANK
            });
            if !nonblank {
                break;
            }
            last = next;
        }
        Some((start.line.0.max(0) as u16, last))
    }

    pub fn prompt_top(&self, height: u16) -> u16 {
        let (_, cursor) = self.cursor();
        self.input_region().map_or(cursor, |(first, last)| {
            first
                .max(cursor.saturating_add(1).saturating_sub(height))
                .min(last.saturating_add(1).saturating_sub(height).max(first))
        })
    }

    pub fn cols(&self) -> u16 {
        u16::try_from(self.term.grid().columns()).unwrap_or(u16::MAX)
    }

    pub fn rows(&self) -> u16 {
        u16::try_from(self.term.grid().screen_lines()).unwrap_or(u16::MAX)
    }

    pub fn cell(&self, col: u16, row: u16) -> &'a Cell {
        let offset = i32::try_from(self.scrollback_offset()).unwrap_or(i32::MAX);
        &self.term.grid()[Point::new(Line(i32::from(row) - offset), Column(usize::from(col)))]
    }

    pub fn live_cell(&self, col: u16, row: u16) -> &'a Cell {
        &self.term.grid()[Point::new(Line(i32::from(row)), Column(usize::from(col)))]
    }

    pub fn scrollback_offset(&self) -> usize {
        self.term.grid().display_offset()
    }

    pub fn cursor(&self) -> (u16, u16) {
        let point = self.term.grid().cursor.point;
        let row = u16::try_from(point.line.0).unwrap_or_default();
        let col = u16::try_from(point.column.0).unwrap_or_default();
        (col, row)
    }

    pub fn cursor_visible(&self) -> bool {
        self.scrollback_offset() == 0 && self.term.mode().contains(TermMode::SHOW_CURSOR)
    }

    pub fn color(&self, index: usize) -> Option<Rgb> {
        self.term.colors()[index]
    }

    /// The live screen's text row by row, not scrolled back, with trailing spaces left out.
    pub fn live_text(&self) -> Vec<String> {
        (0..self.rows())
            .map(|row| {
                let mut line = String::new();
                for col in 0..self.cols() {
                    let cell = self.live_cell(col, row);
                    if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                        continue;
                    }
                    line.push(if cell.c == '\0' { ' ' } else { cell.c });
                    if let Some(zero) = cell.zerowidth() {
                        line.extend(zero);
                    }
                }
                line.trim_end().to_owned()
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
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

#[cfg(test)]
mod multiline_tests {
    use super::*;

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
}
