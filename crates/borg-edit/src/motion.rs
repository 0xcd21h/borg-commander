//! Moving the cursor and the selection: by grapheme, word, line and column, and to a line by number.

use super::{Buffer, GRAPHEME_REACH, Motion, Spot, Until, walk};
use std::ops::Range;
use unicode_segmentation::GraphemeCursor;

impl Buffer {
    pub fn select_all(&mut self) {
        self.anchor = Some(0);
        self.cursor = self.rope.len_chars();
        self.goal = None;
    }

    /// Selects `range`, with the cursor at its end, as a found match is shown.
    pub fn select(&mut self, range: Range<usize>) {
        let end = self.rope.len_chars();
        self.anchor = Some(range.start.min(end));
        self.cursor = range.end.min(end);
        self.goal = None;
        self.break_typing();
    }

    pub(super) fn grapheme_step(&self, forward: bool) -> usize {
        let (line, column) = self.position();
        let start = self.rope.line_to_char(line);
        // Only the text near the cursor is looked at, so a step costs the same on a line of any length.
        let from = column.saturating_sub(GRAPHEME_REACH);
        let to = (column + GRAPHEME_REACH).min(self.line_len(line));
        let text = self.rope.slice(start + from..start + to).to_string();
        let byte = text.char_indices().nth(column - from).map_or(text.len(), |(byte, _)| byte);
        let mut graphemes = GraphemeCursor::new(byte, text.len(), true);
        let next = if forward { graphemes.next_boundary(&text, 0) } else { graphemes.prev_boundary(&text, 0) };
        match next.ok().flatten() {
            Some(byte) => start + from + text[..byte].chars().count(),
            None if forward => (self.cursor + 1).min(self.rope.len_chars()),
            None => self.cursor.saturating_sub(1),
        }
    }

    /// The character index on `line` closest to screen column `goal`.
    pub(super) fn at_column(&self, line: usize, goal: usize) -> usize {
        self.rope.line_to_char(line) + walk(&self.line(line), Spot::default(), Until::Column(goal)).char
    }

    pub fn move_to(&mut self, motion: Motion, select: bool) {
        if select {
            self.anchor.get_or_insert(self.cursor);
        } else if let Some(range) = self.selection().filter(|_| matches!(motion, Motion::Left | Motion::Right)) {
            self.cursor = if motion == Motion::Left { range.start } else { range.end };
            self.anchor = None;
            self.goal = None;
            return;
        } else {
            self.anchor = None;
        }
        self.break_typing();
        let vertical = |buffer: &mut Self, lines: isize| {
            let goal = buffer.goal.unwrap_or_else(|| buffer.screen_column());
            buffer.goal = Some(goal);
            let (line, _) = buffer.position();
            let target = line.saturating_add_signed(lines).min(buffer.rope.len_lines().saturating_sub(1));
            buffer.cursor = if lines < 0 && line == 0 {
                0
            } else if lines > 0 && line == target {
                buffer.rope.len_chars()
            } else {
                buffer.at_column(target, goal)
            };
        };
        match motion {
            Motion::Up => return vertical(self, -1),
            Motion::Down => return vertical(self, 1),
            Motion::PageUp(rows) => return vertical(self, -(rows.max(1) as isize)),
            Motion::PageDown(rows) => return vertical(self, rows.max(1) as isize),
            _ => self.goal = None,
        }
        self.cursor = match motion {
            Motion::Left => self.grapheme_step(false),
            Motion::Right => self.grapheme_step(true),
            Motion::WordLeft => self.word_boundary(false),
            Motion::WordRight => self.word_boundary(true),
            Motion::Home => {
                let (line, column) = self.position();
                let blank = self.rope.line(line).chars().take_while(|c| c.is_whitespace() && *c != '\n').count();
                self.rope.line_to_char(line) + if column == blank { 0 } else { blank }
            }
            Motion::End => {
                let (line, _) = self.position();
                self.rope.line_to_char(line) + self.line_len(line)
            }
            Motion::Start => 0,
            Motion::Finish => self.rope.len_chars(),
            Motion::Up | Motion::Down | Motion::PageUp(_) | Motion::PageDown(_) => self.cursor,
        };
    }

    pub(super) fn word_boundary(&self, forward: bool) -> usize {
        let word = |c: char| c.is_alphanumeric() || c == '_';
        let len = self.rope.len_chars();
        let mut at = self.cursor;
        if forward {
            while at < len && !word(self.rope.char(at)) {
                at += 1;
            }
            while at < len && word(self.rope.char(at)) {
                at += 1;
            }
        } else {
            while at > 0 && !word(self.rope.char(at - 1)) {
                at -= 1;
            }
            while at > 0 && word(self.rope.char(at - 1)) {
                at -= 1;
            }
        }
        at
    }

    /// Moves the cursor to screen column `column` on `line`, as a click does; `select` keeps
    /// the selection's other end where it was, as a drag or a Shift-click does.
    pub fn place(&mut self, line: usize, column: usize, select: bool) {
        if select {
            self.anchor.get_or_insert(self.cursor);
        } else {
            self.anchor = None;
        }
        let line = line.min(self.rope.len_lines().saturating_sub(1));
        self.cursor = self.at_column(line, column);
        self.goal = None;
        self.break_typing();
    }

    /// Selects the word at screen column `column` on `line`, as a double click does.
    pub fn select_word(&mut self, line: usize, column: usize) {
        self.place(line, column, false);
        let word = |c: &char| c.is_alphanumeric() || *c == '_';
        let at = self.cursor;
        if self.rope.get_char(at).is_none_or(|c| !word(&c)) {
            return;
        }
        // A line break is no word character, so the word ends on its line.
        let before = self.rope.chars_at(at).reversed().take_while(word).count();
        let after = self.rope.chars_at(at).take_while(word).count();
        self.select(at - before..at + after);
    }

    /// Moves to the start of line `line`, counted from 1.
    pub fn go_to_line(&mut self, line: usize) {
        let line = line.saturating_sub(1).min(self.rope.len_lines().saturating_sub(1));
        self.anchor = None;
        self.goal = None;
        self.cursor = self.rope.line_to_char(line);
        self.break_typing();
    }
}
