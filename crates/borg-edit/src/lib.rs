//! The editor's text: a rope with a cursor, a selection and undo. Positions are character
//! indices into the text; the cursor moves by grapheme clusters, so an emoji or a letter with
//! combining marks is one step.

use std::ops::Range;

use ropey::Rope;
use unicode_segmentation::{GraphemeCursor, UnicodeSegmentation};
use unicode_width::UnicodeWidthStr;

pub const TAB_WIDTH: usize = 4;
const DEFAULT_INDENT: usize = 4;

/// How lines end in the file: Borg edits with `\n` and writes `\r\n` back where the file had it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Eol {
    Lf,
    Crlf,
}

/// What Tab inserts, following the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Indent {
    Tabs,
    Spaces(usize),
}

impl Indent {
    fn unit(self) -> String {
        match self {
            Self::Tabs => "\t".into(),
            Self::Spaces(width) => " ".repeat(width),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    Left,
    Right,
    Up,
    Down,
    WordLeft,
    WordRight,
    /// To the first character that is not blank, or to the start of the line from there.
    Home,
    End,
    PageUp(usize),
    PageDown(usize),
    Start,
    Finish,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Change {
    Insert { at: usize, text: String },
    Delete { at: usize, text: String },
}

#[derive(Debug, Clone)]
struct Step {
    id: u64,
    changes: Vec<Change>,
    before: (usize, Option<usize>),
    after: (usize, Option<usize>),
    /// Characters typed one at a time join the step before them.
    typing: bool,
}

#[derive(Debug, Clone)]
pub struct Buffer {
    rope: Rope,
    cursor: usize,
    anchor: Option<usize>,
    /// The display column vertical moves keep to.
    goal: Option<usize>,
    undo: Vec<Step>,
    redo: Vec<Step>,
    revision: u64,
    next_step: u64,
    /// The step the file was saved at; `None` for the text as opened.
    saved: Option<u64>,
    /// The first character a change touched since `take_changed_line` last asked.
    changed: Option<usize>,
    pub eol: Eol,
    pub indent: Indent,
}

impl Buffer {
    pub fn new(text: &str) -> Self {
        let crlf = text.contains("\r\n") && text.matches('\n').count() == text.matches("\r\n").count();
        let text = if crlf { text.replace("\r\n", "\n") } else { text.to_owned() };
        Self {
            indent: detect_indent(&text),
            rope: Rope::from_str(&text),
            cursor: 0,
            anchor: None,
            goal: None,
            undo: Vec::new(),
            redo: Vec::new(),
            revision: 0,
            next_step: 0,
            saved: None,
            changed: None,
            eol: if crlf { Eol::Crlf } else { Eol::Lf },
        }
    }

    /// The text as it is written to the file, with the file's line endings.
    pub fn text(&self) -> String {
        let text = self.rope.to_string();
        if self.eol == Eol::Crlf { text.replace('\n', "\r\n") } else { text }
    }

    /// Counts every change, undo and redo included, to tell when the shown text is stale.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn modified(&self) -> bool {
        self.undo.last().map(|step| step.id) != self.saved
    }

    pub fn mark_saved(&mut self) {
        self.saved = self.undo.last().map(|step| step.id);
    }

    /// The first line a change touched since the last call; the lines above it are as they were.
    pub fn take_changed_line(&mut self) -> Option<usize> {
        self.changed.take().map(|at| self.rope.char_to_line(at.min(self.rope.len_chars())))
    }

    pub fn lines(&self) -> usize {
        self.rope.len_lines()
    }

    /// Where line `n` starts, as a character index.
    pub fn line_start(&self, n: usize) -> usize {
        self.rope.line_to_char(n.min(self.rope.len_lines()))
    }

    /// Line `n` without its line break.
    pub fn line(&self, n: usize) -> String {
        if n >= self.rope.len_lines() {
            return String::new();
        }
        let line = self.rope.line(n).to_string();
        line.strip_suffix('\n').map(str::to_owned).unwrap_or(line)
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// The cursor's line and its column in characters.
    pub fn position(&self) -> (usize, usize) {
        let line = self.rope.char_to_line(self.cursor);
        (line, self.cursor - self.rope.line_to_char(line))
    }

    /// The cursor's column on screen, with tabs expanded and wide characters counted twice.
    pub fn screen_column(&self) -> usize {
        let (line, column) = self.position();
        width(&self.line(line).chars().take(column).collect::<String>())
    }

    pub fn selection(&self) -> Option<Range<usize>> {
        let anchor = self.anchor.filter(|anchor| *anchor != self.cursor)?;
        Some(anchor.min(self.cursor)..anchor.max(self.cursor))
    }

    pub fn selected_text(&self) -> Option<String> {
        self.selection().map(|range| self.rope.slice(range).to_string())
    }

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

    fn line_text(&self, line: usize) -> (usize, String) {
        (self.rope.line_to_char(line), self.line(line))
    }

    fn grapheme_step(&self, forward: bool) -> usize {
        let (line, column) = self.position();
        let (start, text) = self.line_text(line);
        let byte = text.char_indices().nth(column).map_or(text.len(), |(byte, _)| byte);
        let mut graphemes = GraphemeCursor::new(byte, text.len(), true);
        let next = if forward { graphemes.next_boundary(&text, 0) } else { graphemes.prev_boundary(&text, 0) };
        match next.ok().flatten() {
            Some(byte) => start + text[..byte].chars().count(),
            None if forward => (self.cursor + 1).min(self.rope.len_chars()),
            None => self.cursor.saturating_sub(1),
        }
    }

    /// The character index on `line` closest to screen column `goal`.
    fn at_column(&self, line: usize, goal: usize) -> usize {
        let (start, text) = self.line_text(line);
        let mut column = 0;
        let mut chars = 0;
        for grapheme in text.graphemes(true) {
            let next = advance(column, grapheme);
            if next > goal {
                break;
            }
            column = next;
            chars += grapheme.chars().count();
        }
        start + chars
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
                let (start, text) = self.line_text(line);
                let blank = text.chars().take_while(|c| c.is_whitespace()).count();
                start + if column == blank { 0 } else { blank }
            }
            Motion::End => {
                let (line, _) = self.position();
                let (start, text) = self.line_text(line);
                start + text.chars().count()
            }
            Motion::Start => 0,
            Motion::Finish => self.rope.len_chars(),
            Motion::Up | Motion::Down | Motion::PageUp(_) | Motion::PageDown(_) => self.cursor,
        };
    }

    fn word_boundary(&self, forward: bool) -> usize {
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

    fn break_typing(&mut self) {
        if let Some(step) = self.undo.last_mut() {
            step.typing = false;
        }
    }

    fn apply(&mut self, change: &Change) {
        let (Change::Insert { at, .. } | Change::Delete { at, .. }) = change;
        self.changed = Some(self.changed.map_or(*at, |changed| changed.min(*at)));
        match change {
            Change::Insert { at, text } => self.rope.insert(*at, text),
            Change::Delete { at, text } => self.rope.remove(*at..*at + text.chars().count()),
        }
    }

    fn record(&mut self, changes: Vec<Change>, before: (usize, Option<usize>), typing: bool) {
        self.revision += 1;
        self.redo.clear();
        let after = (self.cursor, self.anchor);
        let saved = self.saved;
        match self.undo.last_mut() {
            Some(last) if typing && last.typing && last.after.0 == before.0 && Some(last.id) != saved => {
                last.changes.extend(changes);
                last.after = after;
            }
            _ => {
                self.next_step += 1;
                self.undo.push(Step { id: self.next_step, changes, before, after, typing });
            }
        }
    }

    /// Replaces the selection, if any, with `text`.
    fn replace(&mut self, text: &str, typing: bool) {
        let before = (self.cursor, self.anchor);
        let mut changes = Vec::new();
        if let Some(range) = self.selection() {
            let removed = self.rope.slice(range.clone()).to_string();
            let change = Change::Delete { at: range.start, text: removed };
            self.apply(&change);
            changes.push(change);
            self.cursor = range.start;
        }
        self.anchor = None;
        self.goal = None;
        if !text.is_empty() {
            let change = Change::Insert { at: self.cursor, text: text.into() };
            self.apply(&change);
            changes.push(change);
            self.cursor += text.chars().count();
        }
        if !changes.is_empty() {
            self.record(changes, before, typing && self.anchor.is_none());
        }
    }

    /// Types one character: runs of letters undo together, a space or a selection ends them.
    pub fn type_char(&mut self, c: char) {
        let typing = self.selection().is_none() && !c.is_whitespace();
        self.replace(&c.to_string(), typing);
    }

    /// Inserts text as one step, as a paste does; `\r\n` becomes `\n`.
    pub fn insert(&mut self, text: &str) {
        self.replace(&text.replace("\r\n", "\n").replace('\r', "\n"), false);
    }

    /// Breaks the line, keeping the current line's indentation.
    pub fn newline(&mut self) {
        let (line, _) = self.position();
        let indent: String = self.line(line).chars().take_while(|c| *c == ' ' || *c == '\t').collect();
        self.replace(&format!("\n{indent}"), false);
    }

    fn remove(&mut self, range: Range<usize>) {
        if range.is_empty() {
            return;
        }
        let before = (self.cursor, self.anchor);
        let change = Change::Delete { at: range.start, text: self.rope.slice(range.clone()).to_string() };
        self.apply(&change);
        self.cursor = range.start;
        self.anchor = None;
        self.goal = None;
        self.record(vec![change], before, false);
    }

    pub fn backspace(&mut self) {
        match self.selection() {
            Some(range) => self.remove(range),
            None => {
                let start = self.grapheme_step(false);
                self.remove(start..self.cursor);
            }
        }
    }

    pub fn delete(&mut self) {
        match self.selection() {
            Some(range) => self.remove(range),
            None => {
                let end = self.grapheme_step(true);
                self.remove(self.cursor..end);
            }
        }
    }

    pub fn delete_word_back(&mut self) {
        let start = self.word_boundary(false);
        self.anchor = None;
        self.remove(start..self.cursor);
    }

    pub fn cut(&mut self) -> Option<String> {
        let text = self.selected_text()?;
        self.backspace();
        Some(text)
    }

    /// Tab: with a selection over lines, indents or outdents each of them; otherwise inserts
    /// the file's indentation, or outdents the line.
    pub fn indent(&mut self, outdent: bool) {
        let unit = self.indent.unit();
        let selected = self.selection();
        if selected.is_none() && !outdent {
            return self.replace(&unit, false);
        }
        let range = selected.clone().unwrap_or(self.cursor..self.cursor);
        let first = self.rope.char_to_line(range.start);
        let mut last = self.rope.char_to_line(range.end);
        if last > first && range.end == self.rope.line_to_char(last) {
            last -= 1;
        }
        let before = (self.cursor, self.anchor);
        let mut changes = Vec::new();
        let (mut cursor, mut anchor) = (self.cursor, self.anchor.unwrap_or(self.cursor));
        for line in (first..=last).rev() {
            let start = self.rope.line_to_char(line);
            let change = if outdent {
                let text = self.line(line);
                let width = match self.indent {
                    Indent::Tabs if text.starts_with('\t') => 1,
                    _ => text.chars().take(unit.chars().count().max(1)).take_while(|c| *c == ' ').count(),
                };
                if width == 0 {
                    continue;
                }
                Change::Delete { at: start, text: text.chars().take(width).collect() }
            } else {
                Change::Insert { at: start, text: unit.clone() }
            };
            let shift = |at: usize| match &change {
                Change::Insert { text, .. } if at > start => at + text.chars().count(),
                Change::Delete { text, .. } if at > start => at.saturating_sub(text.chars().count()).max(start),
                _ => at,
            };
            (cursor, anchor) = (shift(cursor), shift(anchor));
            self.apply(&change);
            changes.push(change);
        }
        if changes.is_empty() {
            return;
        }
        self.cursor = cursor;
        self.anchor = selected.map(|_| anchor);
        self.goal = None;
        self.record(changes, before, false);
    }

    pub fn undo(&mut self) -> bool {
        let Some(step) = self.undo.pop() else { return false };
        for change in step.changes.iter().rev() {
            self.apply(&match change.clone() {
                Change::Insert { at, text } => Change::Delete { at, text },
                Change::Delete { at, text } => Change::Insert { at, text },
            });
        }
        (self.cursor, self.anchor) = step.before;
        self.goal = None;
        self.revision += 1;
        self.redo.push(Step { typing: false, ..step });
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(step) = self.redo.pop() else { return false };
        for change in &step.changes {
            self.apply(change);
        }
        (self.cursor, self.anchor) = step.after;
        self.goal = None;
        self.revision += 1;
        self.undo.push(step);
        true
    }

    /// Selects the next match of `needle` after the cursor, or the previous one before the
    /// selection, wrapping around. A needle in lowercase ignores case.
    pub fn find(&mut self, needle: &str, forward: bool) -> bool {
        if needle.is_empty() {
            return false;
        }
        let fold = !needle.chars().any(char::is_uppercase);
        let text = self.rope.to_string();
        let (haystack, needle) = if fold && text.is_ascii() && needle.is_ascii() {
            (text.to_ascii_lowercase(), needle.to_ascii_lowercase())
        } else if fold {
            // Lowercasing can change lengths outside ASCII, so match on characters instead.
            return self.find_chars(&text, needle, forward);
        } else {
            (text, needle.to_owned())
        };
        let char_at = |byte: usize| haystack[..byte].chars().count();
        let from = self.rope.char_to_byte(if forward {
            self.cursor
        } else {
            self.selection().map_or(self.cursor, |r| r.start)
        });
        let found = if forward {
            haystack[from..].find(&needle).map(|at| at + from).or_else(|| haystack.find(&needle))
        } else {
            haystack[..from].rfind(&needle).or_else(|| haystack.rfind(&needle))
        };
        found.is_some_and(|byte| {
            let start = char_at(byte);
            self.select(start..start + needle.chars().count());
            true
        })
    }

    fn find_chars(&mut self, text: &str, needle: &str, forward: bool) -> bool {
        let chars: Vec<char> = text.chars().flat_map(char::to_lowercase).collect();
        let wanted: Vec<char> = needle.chars().flat_map(char::to_lowercase).collect();
        if chars.len() != text.chars().count() {
            return false;
        }
        let starts: Vec<usize> =
            (0..=chars.len().saturating_sub(wanted.len())).filter(|&at| chars[at..].starts_with(&wanted)).collect();
        let from = if forward { self.cursor } else { self.selection().map_or(self.cursor, |r| r.start) };
        let found = if forward {
            starts.iter().find(|&&at| at >= from).or_else(|| starts.first())
        } else {
            starts.iter().rev().find(|&&at| at < from).or_else(|| starts.last())
        };
        found.copied().is_some_and(|start| {
            self.select(start..start + wanted.len());
            true
        })
    }

    /// Every match of `needle` from the start, as `find` matches them, none overlapping.
    pub fn matches(&self, needle: &str) -> Vec<Range<usize>> {
        let fold = !needle.chars().any(char::is_uppercase);
        let normal = |c: char| if fold { c.to_lowercase().next().unwrap_or(c) } else { c };
        let wanted: Vec<char> = needle.chars().map(normal).collect();
        let chars: Vec<char> = self.rope.chars().map(normal).collect();
        let mut found = Vec::new();
        let mut at = 0;
        while !wanted.is_empty() && at + wanted.len() <= chars.len() {
            if chars[at..at + wanted.len()] == wanted[..] {
                found.push(at..at + wanted.len());
                at += wanted.len();
            } else {
                at += 1;
            }
        }
        found
    }

    /// Whether the selection is a match of `needle`, as a replace finds it.
    pub fn selection_matches(&self, needle: &str) -> bool {
        self.selection().is_some_and(|range| self.matches(needle).contains(&range))
    }

    /// Replaces every match of `needle` with `with` in one step, returning how many there were.
    pub fn replace_all(&mut self, needle: &str, with: &str) -> usize {
        let found = self.matches(needle);
        if found.is_empty() {
            return 0;
        }
        let before = (self.cursor, self.anchor);
        let mut changes = Vec::new();
        // From the end, so the matches before each one keep their places.
        for range in found.iter().rev() {
            let removed = Change::Delete { at: range.start, text: self.rope.slice(range.clone()).to_string() };
            self.apply(&removed);
            changes.push(removed);
            if !with.is_empty() {
                let added = Change::Insert { at: range.start, text: with.into() };
                self.apply(&added);
                changes.push(added);
            }
        }
        let shift = |at: usize| {
            let before: Vec<_> = found.iter().take_while(|range| range.end <= at).collect();
            (at + before.len() * with.chars().count()).saturating_sub(before.iter().map(|range| range.len()).sum())
        };
        self.cursor = shift(self.cursor).min(self.rope.len_chars());
        self.anchor = None;
        self.goal = None;
        self.record(changes, before, false);
        found.len()
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
        let word = |c: char| c.is_alphanumeric() || c == '_';
        let (start, text) = self.line_text(self.rope.char_to_line(self.cursor));
        let chars: Vec<char> = text.chars().collect();
        let at = self.cursor - start;
        if !chars.get(at).is_some_and(|c| word(*c)) {
            return;
        }
        let first = chars[..at].iter().rposition(|c| !word(*c)).map_or(0, |i| i + 1);
        let last = chars[at..].iter().position(|c| !word(*c)).map_or(chars.len(), |i| at + i);
        self.select(start + first..start + last);
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

/// The screen column after `grapheme` when it starts at `column`.
fn advance(column: usize, grapheme: &str) -> usize {
    if grapheme == "\t" { (column / TAB_WIDTH + 1) * TAB_WIDTH } else { column + grapheme.width().max(1) }
}

/// The screen width of `text` from the start of a line.
pub fn width(text: &str) -> usize {
    text.graphemes(true).fold(0, advance)
}

/// The line as screen cells: each grapheme's first character index in the line, how it shows,
/// and its width. Tabs become spaces to the next tab stop; control characters show as `·`.
pub fn cells(text: &str) -> Vec<(usize, String, usize)> {
    let mut cells = Vec::new();
    let (mut column, mut index) = (0, 0);
    for grapheme in text.graphemes(true) {
        let next = advance(column, grapheme);
        let shown = match grapheme {
            "\t" => " ".repeat(next - column),
            g if g.chars().all(char::is_control) => "·".into(),
            g => g.into(),
        };
        cells.push((index, shown, next - column));
        index += grapheme.chars().count();
        column = next;
    }
    cells
}

/// The line as it shows on screen: tabs expanded to spaces and control characters as symbols.
pub fn expand(text: &str) -> String {
    let mut shown = String::with_capacity(text.len());
    let mut column = 0;
    for grapheme in text.graphemes(true) {
        let next = advance(column, grapheme);
        match grapheme {
            "\t" => shown.extend(std::iter::repeat_n(' ', next - column)),
            g if g.chars().all(char::is_control) => shown.push('·'),
            g => shown.push_str(g),
        }
        column = next;
    }
    shown
}

fn detect_indent(text: &str) -> Indent {
    let (mut tabs, mut spaces) = (0, Vec::new());
    for line in text.lines().take(10_000) {
        if line.starts_with('\t') {
            tabs += 1;
        } else {
            let count = line.chars().take_while(|c| *c == ' ').count();
            if count > 0 && line.len() > count {
                spaces.push(count);
            }
        }
    }
    if tabs > spaces.len() {
        return Indent::Tabs;
    }
    let width = [2, 4, 8].into_iter().filter(|w| spaces.iter().all(|n| n % w == 0)).max();
    Indent::Spaces(match width {
        Some(width) if !spaces.is_empty() => width,
        _ => DEFAULT_INDENT,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn typed(buffer: &mut Buffer, text: &str) {
        text.chars().for_each(|c| if c == '\n' { buffer.newline() } else { buffer.type_char(c) });
    }

    #[test]
    fn replacing_all_matches_is_one_step_and_clicks_place_the_cursor_and_select_words() {
        let mut buffer = Buffer::new("Café cafe\ncafé\tlatte");
        assert_eq!(buffer.matches("café"), [0..4, 10..14], "a lowercase needle ignores case");
        buffer.place(1, 8, false);
        assert_eq!(buffer.position(), (1, 5), "the tab spans columns 4 to 8");
        assert_eq!(buffer.replace_all("café", "tea"), 2);
        assert_eq!(buffer.text(), "tea cafe\ntea\tlatte");
        assert_eq!(buffer.position(), (1, 4), "the cursor keeps its place after the shorter text");
        assert!(buffer.undo());
        assert_eq!(buffer.text(), "Café cafe\ncafé\tlatte");
        buffer.select_word(0, 6);
        assert_eq!(buffer.selected_text().as_deref(), Some("cafe"));
        assert!(!buffer.selection_matches("café"));
        buffer.place(1, 2, true);
        assert_eq!(buffer.selected_text().as_deref(), Some("cafe\nca"), "a drag extends from the word");
        assert_eq!(buffer.replace_all("missing", "x"), 0);
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
    fn find_selects_matches_wrapping_around_with_smart_case() {
        let mut buffer = Buffer::new("Alpha beta ALPHA gamma alpha");
        assert!(buffer.find("alpha", true));
        assert_eq!(buffer.selection(), Some(0..5));
        assert!(buffer.find("alpha", true));
        assert_eq!(buffer.selection(), Some(11..16));
        assert!(buffer.find("Alpha", true));
        assert_eq!(buffer.selection(), Some(0..5), "an uppercase needle matches case, wrapping to the start");
        assert!(buffer.find("alpha", false));
        assert_eq!(buffer.selection(), Some(23..28));
        assert!(!buffer.find("delta", true));
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
        let shown = cells("e\u{301}\t日");
        assert_eq!(shown, [(0, "e\u{301}".into(), 1), (2, "   ".into(), 3), (3, "日".into(), 2)]);
    }

    proptest! {
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
}
