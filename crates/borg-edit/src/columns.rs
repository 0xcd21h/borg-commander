//! Columns and graphemes: how wide text is on screen, and which characters a window of columns holds.

use super::TAB_WIDTH;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// The screen column after `grapheme` when it starts at `column`.
pub(super) fn advance(column: usize, grapheme: &str) -> usize {
    if grapheme == "\t" { (column / TAB_WIDTH + 1) * TAB_WIDTH } else { column + grapheme.width().max(1) }
}

/// The screen width of `text` from the start of a line.
pub fn width(text: &str) -> usize {
    walk(text, Spot::default(), Until::Char(usize::MAX)).column
}

/// Where a walk along a line stands: at a byte and a character of it, and the screen column there.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Spot {
    pub(super) byte: usize,
    pub(super) char: usize,
    pub(super) column: usize,
}

/// How far a walk along a line goes: to the last grapheme that ends by a screen column, or by a
/// character.
#[derive(Debug, Clone, Copy)]
pub(super) enum Until {
    Column(usize),
    Char(usize),
}

/// Printable ASCII: one column wide, and a grapheme on its own unless combining marks follow.
pub(super) fn plain(byte: u8) -> bool {
    (b' '..=b'~').contains(&byte)
}

/// How many bytes `bytes` starts with that are plain.
pub(super) fn plain_prefix(bytes: &[u8]) -> usize {
    pub(super) const BLOCK: usize = 32;
    let mut at = 0;
    // A block checked whole, without stopping at the first byte that fails, compiles to a few
    // vector instructions.
    while at + BLOCK <= bytes.len() && bytes[at..at + BLOCK].iter().fold(true, |all, &byte| all & plain(byte)) {
        at += BLOCK;
    }
    at + bytes[at..].iter().position(|&byte| !plain(byte)).unwrap_or(bytes.len() - at)
}

/// Walks `text`, a line or its start, from `from` a grapheme at a time as far as `until` lets,
/// or to its end. Runs of plain ASCII go at once, so a long line of it costs about a read.
pub(super) fn walk(text: &str, from: Spot, until: Until) -> Spot {
    let mut spot = from;
    while spot.byte < text.len() {
        let run = plain_prefix(&text.as_bytes()[spot.byte..]);
        // The run's last byte may take combining marks after it, so it goes with them.
        let whole = if spot.byte + run == text.len() { run } else { run.saturating_sub(1) };
        let room = match until {
            Until::Column(column) => column.saturating_sub(spot.column),
            Until::Char(char) => char.saturating_sub(spot.char),
        };
        let step = whole.min(room);
        spot = Spot { byte: spot.byte + step, char: spot.char + step, column: spot.column + step };
        if step < whole {
            break;
        }
        let Some(grapheme) = text[spot.byte..].graphemes(true).next() else { break };
        let next = Spot {
            byte: spot.byte + grapheme.len(),
            char: spot.char + grapheme.chars().count(),
            column: advance(spot.column, grapheme),
        };
        let past = match until {
            Until::Column(column) => next.column > column,
            Until::Char(char) => next.char > char,
        };
        if past {
            break;
        }
        spot = next;
    }
    spot
}

/// The part of a line `Buffer::window` gives for drawing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Window {
    pub text: String,
    /// The character of the line `text` starts at, and the screen column there.
    pub start: usize,
    pub column: usize,
}

/// `text`, part of a line from screen column `column`, as screen cells: each grapheme's first
/// character index in `text`, how it shows, and its width. Tabs become spaces to the next tab
/// stop; control characters show as `·`.
pub fn cells(text: &str, column: usize) -> Vec<(usize, String, usize)> {
    let mut cells = Vec::new();
    let (mut column, mut index) = (column, 0);
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
