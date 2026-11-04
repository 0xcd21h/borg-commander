//! How varied bytes are, in bits per byte: near 8 for compressed, encrypted or packed data,
//! near 4.5 for text, and 0 for one byte repeated.

use crate::{Source, executable::code_ranges};
use serde::{Deserialize, Serialize};
use std::{io, ops::Range};

/// The bytes judged together: a page of memory, or a block of a file.
pub const BLOCK: u64 = 4096;
/// Blocks a profile holds at most, so that a source of any size takes the same memory; a
/// larger source is sampled evenly.
const PROFILE_BLOCKS: u64 = 65_536;
/// Blocks of a program's code judged at most, so a large program is read 4 MiB at most.
const CODE_BLOCKS: u64 = 1024;
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
        let stride = stride(total, PROFILE_BLOCKS);
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

/// Every how many blocks one is judged, so that at most `most` of `blocks` are read and the rest
/// stand for the ones they follow.
pub fn stride(blocks: u64, most: u64) -> u64 {
    blocks.div_ceil(most.max(1)).max(1)
}

/// What the blocks judged in a region of memory or in a program's code add up to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tally {
    pub blocks: u32,
    pub high: u32,
    pub highest: Entropy,
    /// Where the first block with high entropy starts.
    pub first_high: Option<u64>,
}

impl Tally {
    /// Counts the block at `at`, judged `entropy`.
    pub fn add(&mut self, at: u64, entropy: Entropy) {
        self.blocks = self.blocks.saturating_add(1);
        self.highest = self.highest.max(entropy);
        if entropy.high() {
            self.high = self.high.saturating_add(1);
            self.first_high = Some(self.first_high.map_or(at, |first| first.min(at)));
        }
    }

    /// The share of the blocks judged with high entropy.
    pub fn share(&self) -> f64 {
        f64::from(self.high) / f64::from(self.blocks.max(1))
    }
}

/// The blocks judged across `ranges`, at most `most` of them, spread evenly over all the blocks
/// the ranges hold together. A block ends where its range does.
pub fn sampled(ranges: &[Range<u64>], most: u64) -> impl Iterator<Item = Range<u64>> + '_ {
    let blocks = |range: &Range<u64>| range.end.saturating_sub(range.start).div_ceil(BLOCK);
    let stride = stride(ranges.iter().map(blocks).sum(), most);
    let mut before = 0u64;
    ranges.iter().flat_map(move |range| {
        let first = before.next_multiple_of(stride) - before;
        before += blocks(range);
        let range = range.clone();
        (first..blocks(&range)).step_by(usize::try_from(stride).unwrap_or(usize::MAX)).map(move |index| {
            let start = range.start + index * BLOCK;
            start..(start + BLOCK).min(range.end)
        })
    })
}

/// The entropy of the code in the program `source`, sampled evenly, or `None` when it is no
/// program `code_ranges` knows or holds no code. Stopping when `go_on` says so is an error of
/// kind `Interrupted`.
pub fn code_tally(source: &dyn Source, go_on: &dyn Fn() -> bool) -> io::Result<Option<Tally>> {
    let Some(ranges) = code_ranges(source)?.filter(|ranges| !ranges.is_empty()) else { return Ok(None) };
    let mut tally = Tally::default();
    let mut block = vec![0; BLOCK as usize];
    for range in sampled(&ranges, CODE_BLOCKS) {
        if !go_on() || source.is_cancelled() {
            return Err(io::ErrorKind::Interrupted.into());
        }
        let wanted = usize::try_from(range.end - range.start).unwrap_or(block.len()).min(block.len());
        let read = source.read_at(range.start, &mut block[..wanted])?;
        tally.add(range.start, Entropy::of(&block[..read]));
    }
    Ok(Some(tally))
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
