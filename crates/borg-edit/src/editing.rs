//! Changing the text in steps that undo and redo whole: typing, deleting, cutting, indenting and replacing.

use super::{Buffer, Change, Indent, Step};
use std::ops::Range;

impl Buffer {
    pub(super) fn break_typing(&mut self) {
        if let Some(step) = self.undo.last_mut() {
            step.typing = false;
        }
    }

    pub(super) fn apply(&mut self, change: &Change) {
        let (Change::Insert { at, .. } | Change::Delete { at, .. }) = change;
        self.changed = Some(self.changed.map_or(*at, |changed| changed.min(*at)));
        match change {
            Change::Insert { at, text } => self.rope.insert(*at, text),
            Change::Delete { at, text } => self.rope.remove(*at..*at + text.chars().count()),
        }
    }

    pub(super) fn record(&mut self, changes: Vec<Change>, before: (usize, Option<usize>), typing: bool) {
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
    pub(super) fn replace(&mut self, text: &str, typing: bool) {
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
        let indent: String = self.rope.line(line).chars().take_while(|c| *c == ' ' || *c == '\t').collect();
        self.replace(&format!("\n{indent}"), false);
    }

    pub(super) fn remove(&mut self, range: Range<usize>) {
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

    /// Replaces each of `edits`' ranges, in order and none overlapping, with its text, in one
    /// step, returning how many there were.
    pub fn replace_ranges(&mut self, edits: &[(Range<usize>, String)]) -> usize {
        if edits.is_empty() {
            return 0;
        }
        let before = (self.cursor, self.anchor);
        let mut changes = Vec::new();
        // From the end, so the ranges before each one keep their places.
        for (range, with) in edits.iter().rev() {
            let removed = Change::Delete { at: range.start, text: self.rope.slice(range.clone()).to_string() };
            self.apply(&removed);
            changes.push(removed);
            if !with.is_empty() {
                let added = Change::Insert { at: range.start, text: with.clone() };
                self.apply(&added);
                changes.push(added);
            }
        }
        let shift = |at: usize| {
            let before = edits.iter().take_while(|(range, _)| range.end <= at);
            let (added, removed) = before
                .fold((0, 0), |(added, removed), (range, with)| (added + with.chars().count(), removed + range.len()));
            (at + added).saturating_sub(removed)
        };
        self.cursor = shift(self.cursor).min(self.rope.len_chars());
        self.anchor = None;
        self.goal = None;
        self.record(changes, before, false);
        edits.len()
    }
}
