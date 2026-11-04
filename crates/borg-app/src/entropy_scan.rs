//! The entropy scan of one process: which of its regions to read, the shared caches every
//! process maps alike and so are left out, and the pages sampled from each region.

use crate::integrity::{KERNEL_PAGES, untouched};
use anyhow::Result;
use borg_core::{EntropyHit, EntropyScope, HitKind, MemoryRegion, ProcessKey};
use borg_view::entropy::{BLOCK, Entropy, Tally, stride};
use std::{collections::HashSet, sync::Mutex};

const PAGE: u64 = BLOCK;
/// Pages sampled from one region at most, and from one process: a process with many regions
/// gets fewer from each, but never fewer than `REGION_FLOOR`.
const REGION_PAGES: u64 = 1024;
const PROCESS_PAGES: u64 = 32_768;
const REGION_FLOOR: u64 = 16;

const MH_MAGIC: u32 = 0xfeed_face;
const MH_MAGIC_64: u32 = 0xfeed_facf;
const MACH_FLAGS: usize = 24;
/// The flag dyld sets in the header of the images its shared cache holds.
const MH_DYLIB_IN_CACHE: u32 = 0x8000_0000;

/// A file mapped unchanged, which every process that maps it holds alike: its path, the region
/// type, where in the file it starts and its size.
pub type SharedFile = (String, String, u64, u64);

/// What reading one process found.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Found {
    pub hits: Vec<EntropyHit>,
    pub pages: u64,
    /// Pages that could not be read; a process none of whose pages could be read is unreadable.
    pub failed: u64,
}

/// Whether the region is part of a cache every process maps alike: the dyld shared cache and
/// what vmmap calls its unused parts on macOS, and the pages the kernel maps on Linux.
pub fn shared_cache(region: &MemoryRegion) -> bool {
    region.name.to_ascii_lowercase().contains("shared cache")
        || region.kind().starts_with("unused ")
        || KERNEL_PAGES.contains(&region.name.as_str())
}

/// The libraries the dyld shared cache holds, by path: the header at the start of their `__TEXT`
/// says so.
pub fn cached_images(regions: &[MemoryRegion], read: impl Fn(u64, usize) -> Result<Vec<u8>>) -> HashSet<String> {
    let word = |bytes: &[u8], at: usize| Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?));
    regions
        .iter()
        .filter(|region| region.readable() && region.kind().starts_with("__TEXT"))
        .filter_map(|region| {
            let path = region.path.as_ref()?;
            let head = read(region.start, MACH_FLAGS + 4).ok()?;
            let image = matches!(word(&head, 0)?, MH_MAGIC | MH_MAGIC_64);
            (image && word(&head, MACH_FLAGS)? & MH_DYLIB_IN_CACHE != 0).then(|| path.clone())
        })
        .collect()
}

/// The file a region maps unchanged, so the scan reads it in the first process only. A file
/// region the process wrote to, or may write to, is its own.
pub fn shared_file(region: &MemoryRegion) -> Option<SharedFile> {
    let path = region.path.as_ref()?;
    let written = region.dirty.is_some_and(|dirty| dirty > 0) || region.swap.is_some_and(|swap| swap > 0);
    let writable = region.permissions.as_bytes().get(1) == Some(&b'w');
    (!written && !writable)
        .then(|| (path.clone(), region.kind().to_owned(), region.offset.unwrap_or(0), region.mapped()))
}

/// Up to `most` pages spread evenly over `region`. `resident` says whether a page is in memory,
/// when the system says: the scan skips pages that are not, so it neither faults in pages the
/// process never touched nor brings back swapped ones. Returns how many pages could not be read.
pub fn sample(
    region: &MemoryRegion,
    most: u64,
    read: impl Fn(u64, usize) -> Result<Vec<u8>>,
    resident: impl Fn(u64) -> Option<bool>,
    go_on: impl Fn() -> bool,
) -> (Tally, u64) {
    let start = region.start - region.start % PAGE;
    let pages = region.end.saturating_sub(start).div_ceil(PAGE);
    let stride = stride(pages, most);
    let (mut tally, mut failed) = (Tally::default(), 0);
    for index in (0..pages.div_ceil(stride)).map(|entry| entry * stride) {
        if !go_on() {
            break;
        }
        let address = start + index * PAGE;
        if resident(address) == Some(false) {
            continue;
        }
        let len = PAGE.min(region.end - address) as usize;
        match read(address, len) {
            Ok(bytes) => tally.add(address, Entropy::of(&bytes)),
            Err(_) => failed += 1,
        }
    }
    (tally, failed)
}

/// Samples the regions of one process that a scan in `scope` reads, leaving out shared caches,
/// regions the process never touched, and files another process already had read. A region
/// with a dense page is a hit.
#[allow(clippy::too_many_arguments)]
pub fn scan_process(
    process: ProcessKey,
    name: &str,
    scope: EntropyScope,
    regions: &[MemoryRegion],
    read: impl Fn(u64, usize) -> Result<Vec<u8>>,
    resident: impl Fn(u64) -> Option<bool>,
    seen: &Mutex<HashSet<SharedFile>>,
    go_on: impl Fn() -> bool,
) -> Found {
    let cached = cached_images(regions, &read);
    let chosen: Vec<_> = regions
        .iter()
        .filter(|region| scope.reads(region) && !shared_cache(region) && !untouched(region.resident, region.swap))
        .filter(|region| region.path.as_ref().is_none_or(|path| !cached.contains(path)))
        .filter(|region| shared_file(region).is_none_or(|file| seen.lock().map_or(true, |mut seen| seen.insert(file))))
        .collect();
    let most = (PROCESS_PAGES / chosen.len().max(1) as u64).clamp(REGION_FLOOR, REGION_PAGES);
    let mut found = Found::default();
    for region in chosen {
        if !go_on() {
            break;
        }
        let (tally, failed) = sample(region, most, &read, &resident, &go_on);
        found.pages += u64::from(tally.blocks);
        found.failed += failed;
        if tally.high > 0 {
            let kind = HitKind::of(region);
            found.hits.push(EntropyHit { process, name: name.into(), region: region.clone(), kind, tally });
        }
    }
    found
}

#[cfg(test)]
mod tests;
