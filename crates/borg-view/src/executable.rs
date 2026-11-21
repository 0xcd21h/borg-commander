//! Where a program file keeps its code: the parts of it the loader maps executable, in ELF,
//! Mach-O (thin or universal) and PE files.

use crate::Source;
use std::{io, ops::Range};

/// Bytes read from the start to tell the format.
const HEAD: usize = 64;
/// Bytes of program headers, load commands or section headers read at most, far more than a
/// real program has.
const TABLE_LIMIT: u64 = 1024 * 1024;
/// Slices a universal binary has at most. A Java class starts the same way, with a version of
/// 45 or more where the slice count would be.
const SLICE_LIMIT: u32 = 16;

const ELF_MAGIC: &[u8] = b"\x7fELF";
const ELF_WIDE: u8 = 2;
const ELF_NARROW: u8 = 1;
const ELF_LITTLE: u8 = 1;
const ELF_BIG: u8 = 2;
const ET_EXEC: u16 = 2;
const ET_DYN: u16 = 3;
const PT_LOAD: u32 = 1;
const PF_X: u32 = 1;
const ELF_HEADER_WIDE: usize = 64;
const ELF_HEADER_NARROW: usize = 52;
const PROGRAM_HEADER_WIDE: usize = 56;
const PROGRAM_HEADER_NARROW: usize = 32;

const MH_MAGIC: u32 = 0xfeed_face;
const MH_MAGIC_64: u32 = 0xfeed_facf;
const MH_CIGAM: u32 = 0xcefa_edfe;
const MH_CIGAM_64: u32 = 0xcffa_edfe;
const MH_OBJECT: u32 = 1;
const MACH_HEADER: usize = 28;
const MACH_HEADER_64: usize = 32;
const LC_SEGMENT: u32 = 0x1;
const LC_SEGMENT_64: u32 = 0x19;
const LOAD_COMMAND: usize = 8;
const VM_PROT_EXECUTE: u32 = 0x4;
const FAT_MAGIC: u32 = 0xcafe_babe;
const FAT_MAGIC_64: u32 = 0xcafe_babf;
const FAT_HEADER: usize = 8;
const FAT_ARCH: usize = 20;
const FAT_ARCH_64: usize = 32;

const MZ: &[u8] = b"MZ";
const PE_POINTER: usize = 0x3c;
const PE_SIGNATURE: &[u8] = b"PE\0\0";
const COFF_HEADER: usize = 24;
const PE_SECTION: usize = 40;
const IMAGE_SCN_CNT_CODE: u32 = 0x20;
const IMAGE_SCN_MEM_EXECUTE: u32 = 0x2000_0000;

/// The byte order of a format's numbers.
#[derive(Clone, Copy)]
struct Order {
    little: bool,
}

impl Order {
    fn bytes<const N: usize>(bytes: &[u8], at: usize) -> Option<[u8; N]> {
        bytes.get(at..at.checked_add(N)?)?.try_into().ok()
    }
    fn u16(self, bytes: &[u8], at: usize) -> Option<u16> {
        let raw = Self::bytes(bytes, at)?;
        Some(if self.little { u16::from_le_bytes(raw) } else { u16::from_be_bytes(raw) })
    }
    fn u32(self, bytes: &[u8], at: usize) -> Option<u32> {
        let raw = Self::bytes(bytes, at)?;
        Some(if self.little { u32::from_le_bytes(raw) } else { u32::from_be_bytes(raw) })
    }
    fn u64(self, bytes: &[u8], at: usize) -> Option<u64> {
        let raw = Self::bytes(bytes, at)?;
        Some(if self.little { u64::from_le_bytes(raw) } else { u64::from_be_bytes(raw) })
    }
    /// A 32-bit number, or a 64-bit one when `wide`.
    fn word(self, bytes: &[u8], at: usize, wide: bool) -> Option<u64> {
        if wide { self.u64(bytes, at) } else { self.u32(bytes, at).map(u64::from) }
    }
}

const LITTLE: Order = Order { little: true };
const BIG: Order = Order { little: false };

/// Up to `len` bytes of `source` from `offset`, fewer at its end.
fn read(source: &dyn Source, offset: u64, len: u64) -> io::Result<Vec<u8>> {
    let mut bytes = vec![0; usize::try_from(len.min(TABLE_LIMIT)).unwrap_or_default()];
    let read = source.read_at(offset, &mut bytes)?;
    bytes.truncate(read);
    Ok(bytes)
}

/// The byte ranges of `source` that hold code, sorted and merged, or `None` when it is no
/// program this knows. A program with no code gives none.
pub fn code_ranges(source: &dyn Source) -> io::Result<Option<Vec<Range<u64>>>> {
    let head = read(source, 0, HEAD as u64)?;
    let ranges = if head.starts_with(ELF_MAGIC) {
        elf(source, &head)?
    } else if head.starts_with(MZ) {
        pe(source, &head)?
    } else if matches!(BIG.u32(&head, 0), Some(FAT_MAGIC | FAT_MAGIC_64)) {
        universal(source, &head)?
    } else {
        mach_o(source, 0)?
    };
    Ok(ranges.map(|ranges| tidy(ranges, source.len())))
}

/// `ranges` within `len` bytes, without empty ones, sorted, with overlaps merged.
fn tidy(mut ranges: Vec<Range<u64>>, len: u64) -> Vec<Range<u64>> {
    ranges.iter_mut().for_each(|range| *range = range.start.min(len)..range.end.min(len));
    ranges.retain(|range| range.start < range.end);
    ranges.sort_by_key(|range| range.start);
    let mut merged: Vec<Range<u64>> = Vec::with_capacity(ranges.len());
    for range in ranges {
        match merged.last_mut() {
            Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
            _ => merged.push(range),
        }
    }
    merged
}

/// The loadable segments an ELF program or library maps executable.
fn elf(source: &dyn Source, head: &[u8]) -> io::Result<Option<Vec<Range<u64>>>> {
    let wide = match head.get(4) {
        Some(&ELF_WIDE) => true,
        Some(&ELF_NARROW) => false,
        _ => return Ok(None),
    };
    let order = match head.get(5) {
        Some(&ELF_LITTLE) => LITTLE,
        Some(&ELF_BIG) => BIG,
        _ => return Ok(None),
    };
    let header = read(source, 0, (if wide { ELF_HEADER_WIDE } else { ELF_HEADER_NARROW }) as u64)?;
    let (table, size, count, entry) = if wide {
        (order.u64(&header, 0x20), order.u16(&header, 0x36), order.u16(&header, 0x38), PROGRAM_HEADER_WIDE)
    } else {
        (
            order.u32(&header, 0x1c).map(u64::from),
            order.u16(&header, 0x2a),
            order.u16(&header, 0x2c),
            PROGRAM_HEADER_NARROW,
        )
    };
    let kind = order.u16(&header, 0x10);
    let (Some(table), Some(size), Some(count), Some(ET_EXEC | ET_DYN)) = (table, size, count, kind) else {
        return Ok(None);
    };
    let size = usize::from(size);
    if size < entry {
        return Ok(None);
    }
    let headers = read(source, table, (size * usize::from(count)) as u64)?;
    let ranges = headers
        .chunks_exact(size)
        .filter_map(|program| {
            let (flags, offset, length) = if wide {
                (order.u32(program, 4)?, order.u64(program, 8)?, order.u64(program, 32)?)
            } else {
                (order.u32(program, 24)?, order.word(program, 4, false)?, order.word(program, 16, false)?)
            };
            (order.u32(program, 0)? == PT_LOAD && flags & PF_X != 0).then(|| offset..offset.saturating_add(length))
        })
        .collect();
    Ok(Some(ranges))
}

/// The segments a Mach-O image starting at `base` maps executable. Object files are left out:
/// their one segment holds everything, debug data included.
fn mach_o(source: &dyn Source, base: u64) -> io::Result<Option<Vec<Range<u64>>>> {
    let head = read(source, base, MACH_HEADER_64 as u64)?;
    let (order, wide) = match LITTLE.u32(&head, 0) {
        Some(MH_MAGIC) => (LITTLE, false),
        Some(MH_MAGIC_64) => (LITTLE, true),
        Some(MH_CIGAM) => (BIG, false),
        Some(MH_CIGAM_64) => (BIG, true),
        _ => return Ok(None),
    };
    let (Some(kind), Some(count), Some(size)) = (order.u32(&head, 12), order.u32(&head, 16), order.u32(&head, 20))
    else {
        return Ok(None);
    };
    if kind == MH_OBJECT {
        return Ok(None);
    }
    let start = base + (if wide { MACH_HEADER_64 } else { MACH_HEADER }) as u64;
    let commands = read(source, start, u64::from(size))?;
    let mut ranges = Vec::new();
    let mut at = 0;
    for _ in 0..count {
        let (Some(command), Some(length)) = (order.u32(&commands, at), order.u32(&commands, at + 4)) else { break };
        let length = length as usize;
        if length < LOAD_COMMAND {
            break;
        }
        let segment = match command {
            LC_SEGMENT_64 => {
                order.u64(&commands, at + 40).zip(order.u64(&commands, at + 48)).zip(order.u32(&commands, at + 60))
            }
            LC_SEGMENT => order
                .word(&commands, at + 32, false)
                .zip(order.word(&commands, at + 36, false))
                .zip(order.u32(&commands, at + 44)),
            _ => None,
        };
        if let Some(((offset, length), protection)) = segment
            && protection & VM_PROT_EXECUTE != 0
        {
            ranges.push(base.saturating_add(offset)..base.saturating_add(offset).saturating_add(length));
        }
        at += length;
    }
    Ok(Some(ranges))
}

/// The code of every slice of a universal Mach-O binary.
fn universal(source: &dyn Source, head: &[u8]) -> io::Result<Option<Vec<Range<u64>>>> {
    let (Some(magic), Some(count)) = (BIG.u32(head, 0), BIG.u32(head, 4)) else { return Ok(None) };
    if count == 0 || count > SLICE_LIMIT {
        return Ok(None);
    }
    let wide = magic == FAT_MAGIC_64;
    let entry = if wide { FAT_ARCH_64 } else { FAT_ARCH };
    let table = read(source, FAT_HEADER as u64, (entry * count as usize) as u64)?;
    let mut ranges = Vec::new();
    for slice in table.chunks_exact(entry) {
        let Some(offset) = BIG.word(slice, 8, wide) else { continue };
        ranges.extend(mach_o(source, offset)?.into_iter().flatten());
    }
    Ok(Some(ranges))
}

/// The sections a PE program or library marks as code or executable.
fn pe(source: &dyn Source, head: &[u8]) -> io::Result<Option<Vec<Range<u64>>>> {
    let Some(at) = LITTLE.u32(head, PE_POINTER).map(u64::from) else { return Ok(None) };
    let header = read(source, at, COFF_HEADER as u64)?;
    if !header.starts_with(PE_SIGNATURE) {
        return Ok(None);
    }
    let (Some(count), Some(optional)) = (LITTLE.u16(&header, 6), LITTLE.u16(&header, 20)) else { return Ok(None) };
    let table = at + COFF_HEADER as u64 + u64::from(optional);
    let sections = read(source, table, (PE_SECTION * usize::from(count)) as u64)?;
    let ranges = sections
        .as_chunks::<PE_SECTION>()
        .0
        .iter()
        .filter_map(|section| {
            let (length, offset, flags) =
                (LITTLE.u32(section, 16)?, LITTLE.u32(section, 20)?, LITTLE.u32(section, 36)?);
            let code = flags & (IMAGE_SCN_CNT_CODE | IMAGE_SCN_MEM_EXECUTE) != 0;
            code.then(|| u64::from(offset)..u64::from(offset) + u64::from(length))
        })
        .collect();
    Ok(Some(ranges))
}

#[cfg(test)]
mod tests;
