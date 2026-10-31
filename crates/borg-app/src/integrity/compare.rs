//! Comparing the code in memory with the file it was mapped from, page by page, within the time and size limits.

use super::{
    Access, CHUNK, COMPARE_LIMIT, PAGE, Run, SCAN_LIMIT, SHOWN, Stretch, executable, hex, label, size, untouched,
};
use borg_core::{Finding, FindingKind, IntegrityReport, MemoryRegion, Severity};
use borg_i18n::tr;

/// Of a Mach-O image in the dyld shared cache, which has no file of its own.
pub(super) const MH_DYLIB_IN_CACHE: u32 = 0x8000_0000;

/// Where in its file the Mach-O image whose header starts `region` starts, unless the image is
/// in the shared cache.
pub(super) fn macho_slice(
    access: &impl Access,
    path: &str,
    region: &MemoryRegion,
    report: &mut IntegrityReport,
) -> Option<u64> {
    let Ok(header) = access.read(region.start, 32) else {
        report.unreadable += size(region);
        return None;
    };
    let word = |bytes: &[u8], at: usize| Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?));
    if !matches!(word(&header, 0)?, 0xfeed_facf | 0xfeed_face) || word(&header, 24)? & MH_DYLIB_IN_CACHE != 0 {
        return None;
    }
    let head = access.read_file(path, 0, PAGE as usize)?;
    slice_offset(&head, &header[..24], |offset| access.read_file(path, offset, 24))
}

/// Where the image whose header begins with `key` starts in a file that begins with `head`:
/// at the start of a thin file, or at the slice of a universal one that matches.
pub(super) fn slice_offset(head: &[u8], key: &[u8], read: impl Fn(u64) -> Option<Vec<u8>>) -> Option<u64> {
    if head.starts_with(key) {
        return Some(0);
    }
    let word = |at: usize| Some(u32::from_be_bytes(head.get(at..at + 4)?.try_into().ok()?));
    let wide = match word(0)? {
        0xcafe_babe => false,
        0xcafe_babf => true,
        _ => return None,
    };
    let entry = if wide { 32 } else { 20 };
    (0..word(4)? as usize).take(64).find_map(|i| {
        let at = 8 + i * entry + 8;
        let offset =
            if wide { u64::from_be_bytes(head.get(at..at + 8)?.try_into().ok()?) } else { u64::from(word(at)?) };
        (read(offset)?.as_slice() == key).then_some(offset)
    })
}

/// The first byte where a page of memory differs from the file, which reads as zeros past its
/// end.
pub(super) fn difference(memory: &[u8], disk: &[u8]) -> Option<usize> {
    memory.iter().enumerate().position(|(i, &byte)| disk.get(i).copied().unwrap_or(0) != byte)
}

/// Compares one stretch of code with its file. Only modified pages can differ, so where the
/// system says which those are, only they are read: Linux's page tables tell private copies
/// and swapped pages from the file's own, and macOS counts the region's dirty pages.
pub(super) fn compare<P: FnMut(u64, u64, usize)>(run: &mut Run<P>, access: &impl Access, stretch: &Stretch) {
    let region = stretch.region;
    if untouched(region.dirty, region.swap) {
        run.report.compared += size(region);
        return run.tick();
    }
    let (mut differing, mut first) = (0u64, None);
    let mut at = region.start;
    while at < region.end {
        if run.stopped() || run.compare_read >= COMPARE_LIMIT {
            run.report.complete = false;
            run.report.uncompared += region.end - at;
            break;
        }
        let len = (region.end - at).min(CHUNK);
        let count = len.div_ceil(PAGE) as usize;
        let wanted: Vec<bool> = access.pages(at, count).map_or_else(
            || vec![true; count],
            |pages| pages.iter().map(|p| p.swapped || (p.present && !p.file)).collect(),
        );
        if wanted.contains(&true) {
            let offset = stretch.offset + (at - region.start);
            let Some(disk) = access.read_file(stretch.path, offset, len as usize) else {
                run.report.uncompared += region.end - at;
                break;
            };
            let Ok(memory) = access.read(at, len as usize) else {
                run.report.unreadable += len;
                run.report.uncompared += len;
                at += len;
                continue;
            };
            run.compare_read += len;
            for (i, page) in memory.chunks(PAGE as usize).enumerate().filter(|(i, _)| wanted[*i]) {
                let from = i * PAGE as usize;
                let disk = disk.get(from..(from + page.len()).min(disk.len())).unwrap_or_default();
                if let Some(byte) = difference(page, disk) {
                    differing += 1;
                    first.get_or_insert_with(|| {
                        let shown = |bytes: &[u8]| {
                            bytes.get(byte..(byte + SHOWN).min(bytes.len())).unwrap_or_default().to_vec()
                        };
                        (from + byte, offset + (from + byte) as u64, shown(page), shown(disk))
                    });
                }
            }
        }
        run.report.compared += len;
        at += len;
        run.tick();
    }
    if let Some((at, offset, memory, disk)) = first {
        let detail = tr!(
            detail_code_changed,
            pages = differing,
            total = size(region).div_ceil(PAGE),
            file = stretch.path,
            offset = offset,
            memory = hex(&memory),
            disk = hex(&disk),
        );
        run.add(Finding {
            kind: FindingKind::CodeChanged,
            severity: Severity::Alert,
            detail,
            region: Some(region.clone()),
            address: Some(region.start + at as u64),
            length: memory.len().max(1),
        });
    }
}

/// The executable format whose header starts `page`.
pub(super) fn image_format(page: &[u8]) -> Option<&'static str> {
    let word = |at: usize| Some(u32::from_le_bytes(page.get(at..at + 4)?.try_into().ok()?));
    match page.get(..4)? {
        [0x7f, b'E', b'L', b'F'] => {
            let kind = u16::from_le_bytes(page.get(16..18)?.try_into().ok()?);
            (matches!(page.get(4..6)?, [1 | 2, 1 | 2]) && matches!(kind, 2 | 3)).then_some("ELF")
        }
        [0xcf | 0xce, 0xfa, 0xed, 0xfe] => {
            // An executable, library, bundle or dyld, with a sane number of load commands,
            // and not one of the shared cache's, which every process maps.
            let plausible = matches!(word(12)?, 2 | 6 | 7 | 8) && (1..=1024).contains(&word(16)?);
            (plausible && word(24)? & MH_DYLIB_IN_CACHE == 0).then_some("Mach-O")
        }
        [b'M', b'Z', ..] => {
            let at = word(0x3c)? as usize;
            (page.get(at..at + 4)? == b"PE\0\0").then_some("PE")
        }
        _ => None,
    }
}

/// Searches memory no file backs for the headers of executable images, which a loader places
/// at the start of a page. `next_executable` says whether code follows right after it, as an
/// image's code follows its header.
pub(super) fn scan<P: FnMut(u64, u64, usize)>(
    run: &mut Run<P>,
    access: &impl Access,
    region: &MemoryRegion,
    next_executable: bool,
) {
    if untouched(region.resident, region.swap) {
        run.report.scanned += size(region);
        return run.tick();
    }
    let mut at = region.start.next_multiple_of(PAGE);
    while at < region.end {
        if run.stopped() || run.scan_read >= SCAN_LIMIT {
            run.report.complete = false;
            return;
        }
        let len = (region.end - at).min(CHUNK);
        let count = len.div_ceil(PAGE) as usize;
        let wanted: Vec<bool> = access
            .pages(at, count)
            .map_or_else(|| vec![true; count], |pages| pages.iter().map(|p| p.present || p.swapped).collect());
        if wanted.contains(&true) {
            let Ok(data) = access.read(at, len as usize) else {
                run.report.unreadable += len;
                at += len;
                continue;
            };
            run.scan_read += len;
            for (i, page) in data.chunks(PAGE as usize).enumerate().filter(|(i, _)| wanted[*i]) {
                if let Some(format) = image_format(page) {
                    let alert = executable(region) || next_executable;
                    run.add(Finding {
                        kind: FindingKind::HiddenImage,
                        severity: if alert { Severity::Alert } else { Severity::Warning },
                        detail: tr!(detail_hidden_image, format = format, region = label(region)),
                        region: Some(region.clone()),
                        address: Some(at + i as u64 * PAGE),
                        length: 4,
                    });
                }
            }
        }
        run.report.scanned += len;
        at += len;
        run.tick();
    }
}
