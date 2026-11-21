//! Bytes as hex rows: where each row starts, its bytes in hex and the bytes that print as text,
//! with as many bytes to a row as fit the width.

use crate::{Mark, Paint, Run};

/// The bytes a row shows when nothing limits its width.
const HEX_WIDTH: usize = 16;
const OFFSET_DIGITS: usize = 4;
const HEX_GAP: &str = "  ";
/// Cells a byte takes in hex, with the space before the next.
const HEX_CELLS: usize = 3;
const HEX_DIGIT_BITS: u32 = 4;
const FEWEST_BYTES: usize = 1;
/// Bytes on a row, from the most to the fewest, for its rows to fit their width.
const ROW_BYTES: [usize; 7] = [64, 32, HEX_WIDTH, GROUP_BYTES, 4, 2, FEWEST_BYTES];
/// Bytes that stand together in hex, with a wider space before the next group.
const GROUP_BYTES: usize = 8;
const GROUP_GAP: &str = "  ";
/// What a byte that does not print shows as in the text column.
const UNPRINTED: char = '.';
const BYTE_GAP: &str = " ";

/// How a hex dump lays out its rows: the bytes on each, and the digits of their offsets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HexLayout {
    pub columns: usize,
    pub digits: usize,
}

impl HexLayout {
    /// `HEX_WIDTH` bytes a row, for a dump of `len` bytes.
    pub fn full(len: u64) -> Self {
        Self { columns: HEX_WIDTH, digits: offset_digits(len) }
    }

    /// The most bytes a row whose rows fit `width` cells, for a dump of `len` bytes.
    pub fn fitting(width: usize, len: u64) -> Self {
        let digits = offset_digits(len);
        let fits = |columns: usize| Self { columns, digits }.width() <= width;
        Self { columns: ROW_BYTES.into_iter().find(|&columns| fits(columns)).unwrap_or(FEWEST_BYTES), digits }
    }

    /// Cells a row takes: its offset, its bytes in hex and the bytes that print as text.
    pub fn width(self) -> usize {
        self.offset_width() + hex_cells(self.columns) + HEX_GAP.len() + self.columns
    }

    /// Cells a row's offset takes, with the space after it.
    pub fn offset_width(self) -> usize {
        self.digits + HEX_GAP.len()
    }

    /// `offset` as a row shows it.
    pub fn offset(self, offset: u64) -> String {
        format!("{offset:0digits$x}", digits = self.digits)
    }

    /// The rows a dump of `len` bytes takes.
    pub fn rows(self, len: u64) -> u64 {
        len.div_ceil(self.step())
    }

    /// The offset of the row that `offset` is on.
    pub fn row_start(self, offset: u64) -> u64 {
        offset - offset % self.step()
    }

    /// The bytes from one row's start to the next.
    pub fn step(self) -> u64 {
        self.columns as u64
    }

    /// `data` as the rows of a dump laid out this way.
    pub fn lines(self, data: &[u8]) -> Vec<String> {
        data.chunks(self.columns).zip((0..).step_by(self.columns)).map(|(row, at)| self.line(at, row)).collect()
    }

    /// The row of `bytes`, found `offset` bytes into the dump.
    pub fn line(self, offset: u64, bytes: &[u8]) -> String {
        self.runs(offset, bytes, &|_| Mark::None).iter().map(|run| run.text.as_str()).collect()
    }

    /// The row of `bytes`, found `offset` bytes into the dump, as runs: the offset and zero
    /// bytes faint, and each byte marked in both columns as `mark` says for its offset.
    pub fn runs(self, offset: u64, bytes: &[u8], mark: &dyn Fn(u64) -> Mark) -> Vec<Run> {
        let looks: Vec<(Paint, Mark)> =
            bytes.iter().zip(offset..).map(|(&byte, at)| (paint_of(byte), mark(at))).collect();
        let mut runs = Vec::new();
        push(&mut runs, &self.offset(offset), (faint(), Mark::None));
        push(&mut runs, HEX_GAP, plain());
        for (index, (byte, &look)) in bytes.iter().zip(&looks).enumerate() {
            if let Some(&before) = index.checked_sub(1).and_then(|before| looks.get(before)) {
                push(&mut runs, gap_before(index), between(before, look));
            }
            push(&mut runs, &format!("{byte:02x}"), look);
        }
        push(&mut runs, &" ".repeat(hex_cells(self.columns).saturating_sub(hex_cells(bytes.len()))), plain());
        push(&mut runs, HEX_GAP, plain());
        for (&byte, &look) in bytes.iter().zip(&looks) {
            push(&mut runs, printed(byte).encode_utf8(&mut [0; 4]), look);
        }
        runs
    }
}

/// Cells `count` bytes take in hex, with the spaces between them.
fn hex_cells(count: usize) -> usize {
    let groups = count.saturating_sub(1) / GROUP_BYTES;
    (count * HEX_CELLS).saturating_sub(BYTE_GAP.len()) + groups * (GROUP_GAP.len() - BYTE_GAP.len())
}

/// The space before the byte at `index` of a row, wider where a group starts.
fn gap_before(index: usize) -> &'static str {
    if index.is_multiple_of(GROUP_BYTES) { GROUP_GAP } else { BYTE_GAP }
}

/// Hex digits for the offsets of a dump of `len` bytes, at least `OFFSET_DIGITS`.
fn offset_digits(len: u64) -> usize {
    let bits = u64::BITS - len.saturating_sub(1).leading_zeros();
    (bits.div_ceil(HEX_DIGIT_BITS) as usize).max(OFFSET_DIGITS)
}

fn printed(byte: u8) -> char {
    if byte.is_ascii_graphic() || byte == b' ' { char::from(byte) } else { UNPRINTED }
}

fn paint_of(byte: u8) -> Paint {
    if byte == 0 { faint() } else { Paint::default() }
}

fn faint() -> Paint {
    Paint { dim: true, ..Paint::default() }
}

fn plain() -> (Paint, Mark) {
    (Paint::default(), Mark::None)
}

/// The look of the space between two bytes: marked when both are, so a match reads as one.
fn between(before: (Paint, Mark), after: (Paint, Mark)) -> (Paint, Mark) {
    if before.1 == after.1 { (Paint::default(), after.1) } else { plain() }
}

fn push(runs: &mut Vec<Run>, text: &str, (paint, mark): (Paint, Mark)) {
    match runs.last_mut().filter(|last| last.paint == paint && last.mark == mark) {
        Some(last) => last.text.push_str(text),
        None if text.is_empty() => {}
        None => runs.push(Run { text: text.to_owned(), paint, mark }),
    }
}

#[cfg(test)]
mod tests;
