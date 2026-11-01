//! The editor's text: a rope with a cursor, a selection and undo. Positions are character
//! indices into the text; the cursor moves by grapheme clusters, so an emoji or a letter with
//! combining marks is one step.

use std::ops::Range;

use ropey::Rope;

mod columns;
mod editing;
mod motion;

use columns::{Spot, Until, walk};
pub use columns::{Window, cells, expand, width};

pub const TAB_WIDTH: usize = 4;
const DEFAULT_INDENT: usize = 4;
/// Characters on either side of the cursor a step looks at for where a grapheme ends: a longer
/// cluster is stepped through in parts.
const GRAPHEME_REACH: usize = 128;
/// Lines up to this many bytes are drawn whole; of longer ones, what shows and `MARGIN` columns
/// on either side.
const WHOLE_LINE: usize = 64 * 1024;
const MARGIN: usize = 1024;

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
    /// What one level of indentation is.
    pub fn unit(self) -> String {
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

    /// The characters on line `n`, its line break left out.
    pub fn line_len(&self, n: usize) -> usize {
        if n >= self.rope.len_lines() {
            return 0;
        }
        let line = self.rope.line(n);
        let len = line.len_chars();
        if len > 0 && line.char(len - 1) == '\n' { len - 1 } else { len }
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
        let start = self.rope.line_to_char(self.rope.char_to_line(self.cursor));
        width(&self.rope.slice(start..self.cursor).to_string())
    }

    /// Line `n` around screen columns `left..left + width`, for drawing what of it shows: a
    /// long line costs little more than the screen it fills.
    pub fn window(&self, n: usize, left: usize, width: usize) -> Window {
        let line = self.line(n);
        if line.len() <= WHOLE_LINE {
            return Window { text: line, start: 0, column: 0 };
        }
        let first = walk(&line, Spot::default(), Until::Column(left.saturating_sub(MARGIN)));
        let last = walk(&line, first, Until::Column(left.saturating_add(width).saturating_add(MARGIN)));
        Window { text: line[first.byte..last.byte].to_owned(), start: first.char, column: first.column }
    }

    pub fn selection(&self) -> Option<Range<usize>> {
        let anchor = self.anchor.filter(|anchor| *anchor != self.cursor)?;
        Some(anchor.min(self.cursor)..anchor.max(self.cursor))
    }

    pub fn selected_text(&self) -> Option<String> {
        self.selection().map(|range| self.rope.slice(range).to_string())
    }

    pub fn len_chars(&self) -> usize {
        self.rope.len_chars()
    }

    /// The characters in `range`, with line feeds whatever the file's line endings.
    pub fn slice(&self, range: Range<usize>) -> String {
        self.rope.slice(range).to_string()
    }

    /// The text's length in bytes, with line feeds for line breaks.
    pub fn len_bytes(&self) -> usize {
        self.rope.len_bytes()
    }

    /// Copies the text's bytes from `offset` into `buf`, as far as it fills, returning how many.
    pub fn read_at(&self, offset: usize, buf: &mut [u8]) -> usize {
        if offset >= self.rope.len_bytes() {
            return 0;
        }
        let (chunks, chunk_start, _, _) = self.rope.chunks_at_byte(offset);
        let mut filled = 0;
        let mut skip = offset - chunk_start;
        for chunk in chunks {
            let bytes = &chunk.as_bytes()[skip..];
            skip = 0;
            let n = bytes.len().min(buf.len() - filled);
            buf[filled..filled + n].copy_from_slice(&bytes[..n]);
            filled += n;
            if filled == buf.len() {
                break;
            }
        }
        filled
    }

    pub fn byte_to_char(&self, byte: usize) -> usize {
        self.rope.byte_to_char(byte.min(self.rope.len_bytes()))
    }

    /// The character at byte `byte` of the file as read, where each `\r\n` the text holds as
    /// `\n` took a byte more.
    pub fn file_byte_to_char(&self, byte: usize) -> usize {
        if self.eol == Eol::Lf {
            return self.byte_to_char(byte);
        }
        let starts_by = |line: usize| self.rope.line_to_byte(line) + line <= byte;
        let (mut line, mut after) = (0, self.rope.len_lines());
        while line + 1 < after {
            let middle = line + (after - line) / 2;
            if starts_by(middle) {
                line = middle;
            } else {
                after = middle;
            }
        }
        self.byte_to_char(byte - line)
    }

    pub fn char_to_byte(&self, char: usize) -> usize {
        self.rope.char_to_byte(char.min(self.rope.len_chars()))
    }

    /// The line the character at `char` is on.
    pub fn line_of(&self, char: usize) -> usize {
        self.rope.char_to_line(char.min(self.rope.len_chars()))
    }
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
mod tests;
