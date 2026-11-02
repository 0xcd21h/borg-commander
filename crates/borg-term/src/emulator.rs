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

/// Output concealed after Borg types a command, beyond which it is shown after all.
const CONCEAL_LIMIT: usize = 16 * 1024;

pub struct Emulator {
    term: Term<Replies>,
    parser: Processor,
    scanner: OscScanner,
    replies: Receiver<String>,
    input_start: Option<Point>,
    /// Where the shell's current prompt began.
    prompt_start: Option<Point>,
    /// Output held back while a command Borg typed runs, until the shell's next prompt.
    concealed: Option<Concealed>,
}

struct Concealed {
    prompt: Point,
    bytes: Vec<u8>,
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
            prompt_start: None,
            concealed: None,
        }
    }

    /// Keeps the shell's output out of sight until its next prompt, which then replaces the
    /// current one, so a command Borg types, such as a `cd`, leaves no trace on screen.
    /// Without a prompt to return to, or after `CONCEAL_LIMIT` bytes, output shows as usual.
    pub fn conceal(&mut self) {
        self.concealed = self.prompt_start.map(|prompt| Concealed { prompt, bytes: Vec::new() });
    }

    /// Whether output is held back, so the screen still shows the prompt from before Borg typed.
    pub fn concealing(&self) -> bool {
        self.concealed.is_some()
    }

    pub fn feed(&mut self, bytes: &[u8]) -> Vec<ShellSignal> {
        let marks = self.scanner.scan(bytes);
        let mut consumed = 0;
        for (end, signal) in &marks {
            if signal == &ShellSignal::PromptStart
                && let Some(concealed) = self.concealed.take()
            {
                // The new prompt takes the old one's place; what ran in between stays hidden.
                let row = concealed.prompt.line.0.max(0) + 1;
                let column = concealed.prompt.column.0 + 1;
                self.advance(format!("\x1b[{row};{column}H\x1b[J").as_bytes());
                self.track(signal);
                consumed = *end;
                continue;
            }
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
        self.prompt_start = None;
        if let Some(concealed) = self.concealed.take() {
            self.advance(&concealed.bytes);
        }
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

    /// What is typed at the prompt, without autosuggestions. Blanks before the cursor were typed,
    /// so they stay.
    pub fn command_line(&self) -> String {
        let Some(start) = self.input_start else { return String::new() };
        let grid = self.term.grid();
        let cursor = grid.cursor.point;
        let bottom = self.input_bottom();
        let mut text = String::new();
        let mut typed = 0;
        let mut line = start.line;
        loop {
            let first = if line == start.line { start.column.0 } else { 0 };
            let mut row = String::new();
            let mut before_cursor = 0;
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
                if point < cursor {
                    before_cursor = row.len();
                }
            }
            let wrapped = grid[Point::new(line, Column(grid.columns() - 1))].flags.contains(Flags::WRAPLINE);
            let mut kept = if wrapped { row.len() } else { row.trim_end().len() };
            if line == cursor.line {
                kept = kept.max(before_cursor);
                typed = text.len() + before_cursor;
            }
            text.push_str(&row[..kept]);
            if line >= bottom {
                let end = text.trim_end().len().max(typed);
                text.truncate(end);
                return text;
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
        if let Some(concealed) = self.concealed.as_mut() {
            concealed.bytes.extend_from_slice(bytes);
            if concealed.bytes.len() <= CONCEAL_LIMIT {
                return;
            }
            let held = self.concealed.take().map(|concealed| concealed.bytes).unwrap_or_default();
            return self.advance(&held);
        }
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
        if signal == &ShellSignal::PromptStart {
            self.prompt_start = Some(self.term.grid().cursor.point);
        }
        self.input_start = match signal {
            ShellSignal::InputStart => Some(self.term.grid().cursor.point),
            ShellSignal::PromptStart | ShellSignal::CommandStart | ShellSignal::CommandEnd { .. } => None,
            ShellSignal::Cwd { .. } | ShellSignal::SilentCd | ShellSignal::ExitRequested { .. } => self.input_start,
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
mod tests;
