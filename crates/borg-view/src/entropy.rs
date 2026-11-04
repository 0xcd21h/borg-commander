//! How varied bytes are, in bits per byte: near 8 for compressed, encrypted or packed data,
//! near 4.5 for text, and 0 for one byte repeated.

use crate::Source;
use serde::{Deserialize, Serialize};
use std::{io, ops::Range};

/// The bytes judged together: a page of memory, or a block of a file.
pub const BLOCK: u64 = 4096;
/// Blocks a profile holds at most, so that a source of any size takes the same memory; a
/// larger source is sampled evenly.
const PROFILE_BLOCKS: u64 = 65_536;
const BYTE_VALUES: usize = 256;
const HUNDREDTHS: f64 = 100.0;
/// Below this, bytes repeat a lot, as in sparse tables, padding and fill patterns.
const LOW_BELOW: Entropy = Entropy(400);
/// From this on, bytes look compressed, encrypted or packed.
const HIGH_FROM: Entropy = Entropy(720);

/// Shannon entropy in hundredths of a bit per byte, so that it compares and travels exactly.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Entropy(u16);

/// What an entropy usually says about the bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Band {
    /// Sparse tables, padding and fill patterns.
    Low,
    /// Text, code and structured data.
    Typical,
    /// Compressed, encrypted or packed: media, archives and keys look the same.
    High,
}

impl Entropy {
    /// Where high entropy starts.
    pub const HIGH: Self = HIGH_FROM;
    /// Where low entropy ends.
    pub const LOW: Self = LOW_BELOW;

    pub fn of(bytes: &[u8]) -> Self {
        let mut counts = [0u32; BYTE_VALUES];
        bytes.iter().for_each(|&byte| counts[usize::from(byte)] += 1);
        let total = bytes.len() as f64;
        let bits: f64 = counts
            .iter()
            .filter(|&&count| count > 0)
            .map(|&count| f64::from(count) / total)
            .map(|share| -share * share.log2())
            .sum();
        Self((bits * HUNDREDTHS).round() as u16)
    }

    pub fn bits(self) -> f64 {
        f64::from(self.0) / HUNDREDTHS
    }

    pub fn band(self) -> Band {
        if self >= HIGH_FROM {
            Band::High
        } else if self < LOW_BELOW {
            Band::Low
        } else {
            Band::Typical
        }
    }

    pub fn high(self) -> bool {
        self.band() == Band::High
    }
}

/// The entropy of a source's blocks, or of every `stride`th block when it has more than
/// `PROFILE_BLOCKS`: each entry stands for `stride` blocks and was judged by the first of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    pub len: u64,
    pub stride: u64,
    pub blocks: Vec<Entropy>,
}

impl Profile {
    /// Reads `source` a block at a time, or gives `None` once `go_on` says to stop.
    pub fn read(source: &dyn Source, go_on: &dyn Fn() -> bool) -> io::Result<Option<Self>> {
        let len = source.len();
        let total = len.div_ceil(BLOCK);
        let stride = total.div_ceil(PROFILE_BLOCKS).max(1);
        let mut block = vec![0; BLOCK as usize];
        let mut blocks = Vec::with_capacity(total.div_ceil(stride) as usize);
        for entry in 0..total.div_ceil(stride) {
            if !go_on() || source.is_cancelled() {
                return Ok(None);
            }
            let read = source.read_at(entry * stride * BLOCK, &mut block)?;
            blocks.push(Entropy::of(&block[..read]));
        }
        Ok(Some(Self { len, stride, blocks }))
    }

    /// The entropy of the block `offset` is in, as its entry judged it.
    pub fn at(&self, offset: u64) -> Option<Entropy> {
        self.blocks.get(self.entry(offset)).copied()
    }

    /// The highest entropy of the entries in `range`, or of the one it starts in.
    pub fn highest(&self, range: Range<u64>) -> Option<Entropy> {
        let first = self.entry(range.start);
        let last = self.entry(range.end.saturating_sub(1)).max(first);
        self.blocks.get(first..=last.min(self.blocks.len().saturating_sub(1)))?.iter().copied().max()
    }

    /// Where the next run of high-entropy blocks after the block at `offset` starts.
    pub fn next_high(&self, offset: u64) -> Option<u64> {
        let here = self.entry(offset);
        (here + 1..self.blocks.len()).find(|&entry| self.starts_high(entry)).map(|entry| self.offset(entry))
    }

    /// Where the closest run of high-entropy blocks that starts before the block at `offset`
    /// starts.
    pub fn previous_high(&self, offset: u64) -> Option<u64> {
        let here = self.entry(offset).min(self.blocks.len());
        (0..here).rev().find(|&entry| self.starts_high(entry)).map(|entry| self.offset(entry))
    }

    /// The share of entries with high entropy.
    pub fn high_share(&self) -> f64 {
        self.blocks.iter().filter(|entropy| entropy.high()).count() as f64 / self.blocks.len().max(1) as f64
    }

    fn starts_high(&self, entry: usize) -> bool {
        self.blocks[entry].high() && entry.checked_sub(1).is_none_or(|before| !self.blocks[before].high())
    }

    fn offset(&self, entry: usize) -> u64 {
        entry as u64 * self.stride * BLOCK
    }

    fn entry(&self, offset: u64) -> usize {
        usize::try_from(offset / BLOCK / self.stride.max(1)).unwrap_or(usize::MAX)
    }
}

/// The bytes the `index`th of `cells` cells stands for, when a source of `len` bytes is laid
/// along them evenly.
pub fn strip_cell(len: u64, cells: usize, index: usize) -> Range<u64> {
    let at = |index: usize| (u128::from(len) * index as u128 / cells.max(1) as u128) as u64;
    at(index)..at(index + 1).max(at(index) + 1).min(len.max(1))
}

/// The cell of `cells` that the byte at `offset` of a source of `len` bytes falls in.
pub fn strip_cell_of(len: u64, cells: usize, offset: u64) -> usize {
    let cell = u128::from(offset) * cells as u128 / u128::from(len.max(1));
    (cell as usize).min(cells.saturating_sub(1))
}

#[cfg(test)]
mod tests;
