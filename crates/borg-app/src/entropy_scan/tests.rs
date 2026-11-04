use super::*;
use anyhow::bail;
use std::cell::Cell;

const PROCESS: ProcessKey = ProcessKey { pid: 7, start: 1 };

fn region(name: &str, path: Option<&str>, start: u64, pages: u64, permissions: &str) -> MemoryRegion {
    MemoryRegion {
        start,
        end: start + pages * PAGE,
        permissions: permissions.into(),
        name: path.map_or_else(|| name.into(), |path| format!("{name} · {path}")),
        path: path.map(String::from),
        dirty: Some(0),
        ..Default::default()
    }
}

fn noise(len: usize, mut seed: u64) -> Vec<u8> {
    (0..len)
        .map(|_| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed as u8
        })
        .collect()
}

/// Memory that holds random bytes from `dense` on and zeros below it, with a cached Mach-O image
/// at `cached`.
fn memory(dense: u64, cached: u64) -> impl Fn(u64, usize) -> Result<Vec<u8>> {
    move |address, len| {
        if address == cached {
            let mut head = vec![0; len];
            head[..4].copy_from_slice(&MH_MAGIC_64.to_le_bytes());
            head[MACH_FLAGS..MACH_FLAGS + 4].copy_from_slice(&(MH_DYLIB_IN_CACHE | 0x85).to_le_bytes());
            return Ok(head);
        }
        Ok(if address >= dense { noise(len, address | 1) } else { vec![0; len] })
    }
}

fn scan(scope: EntropyScope, regions: &[MemoryRegion], seen: &Mutex<HashSet<SharedFile>>) -> Found {
    scan_process(PROCESS, "tool", scope, regions, memory(0x10_0000, 0x5000), |_| None, seen, || true)
}

#[test]
fn code_scans_read_executable_regions_and_memory_scans_read_the_rest_too() {
    let packed = region("__TEXT", Some("/opt/tool"), 0x10_0000, 8, "r-x");
    let plain = region("__TEXT", Some("/opt/plain"), 0x1000, 2, "r-x");
    let heap = MemoryRegion { dirty: Some(4096), ..region("MALLOC_SMALL", None, 0x20_0000, 4, "rw-") };
    let regions = [plain, packed.clone(), heap.clone()];
    let found = scan(EntropyScope::Code, &regions, &Mutex::default());
    assert_eq!(found.hits.len(), 1);
    let hit = &found.hits[0];
    assert_eq!((hit.kind, &hit.region, hit.tally.high, hit.tally.blocks), (HitKind::Code, &packed, 8, 8));
    assert!(hit.packed() && hit.tally.first_high == Some(packed.start));
    assert_eq!((found.pages, found.failed), (10, 0));

    let found = scan(EntropyScope::Memory, &regions, &Mutex::default());
    let kinds: Vec<_> = found.hits.iter().map(|hit| (hit.kind, hit.region.start)).collect();
    assert_eq!(kinds, [(HitKind::Code, packed.start), (HitKind::Own, heap.start)]);
}

#[test]
fn shared_caches_are_left_out() {
    let cached = region("__TEXT", Some("/usr/lib/system/libcached.dylib"), 0x5000, 1, "r-x");
    let cached_data = region("__DATA_CONST", Some("/usr/lib/system/libcached.dylib"), 0x10_1000, 1, "r--");
    let linkedit = region("dyld shared cache combined __LINKEDIT", None, 0x10_2000, 1, "r--");
    let unused = region("unused __DATA", None, 0x10_3000, 1, "r--");
    let vdso = MemoryRegion { name: "[vdso]".into(), ..region("", None, 0x10_4000, 1, "r-x") };
    let own = region("__TEXT", Some("/opt/tool"), 0x10_5000, 1, "r-x");
    let regions = [cached, cached_data, linkedit, unused, vdso, own.clone()];
    let found = scan(EntropyScope::Memory, &regions, &Mutex::default());
    let read: Vec<_> = found.hits.iter().map(|hit| hit.region.start).collect();
    assert_eq!(read, [own.start], "only the program's own image");
}

#[test]
fn a_file_mapped_unchanged_is_read_in_the_first_process_only() {
    let library = region("r-x", Some("/usr/lib/libz.so.1"), 0x10_0000, 2, "r-x");
    let seen = Mutex::default();
    assert_eq!(scan(EntropyScope::Code, std::slice::from_ref(&library), &seen).hits.len(), 1);
    assert!(scan(EntropyScope::Code, std::slice::from_ref(&library), &seen).hits.is_empty(), "read once");
    let patched = MemoryRegion { dirty: Some(4096), ..library };
    assert_eq!(scan(EntropyScope::Code, &[patched], &seen).hits.len(), 1, "a changed copy is the process's own");
}

#[test]
fn untouched_and_absent_pages_are_not_read_and_large_regions_are_sampled() {
    let reads = Cell::new(0);
    let read = |address: u64, len: usize| {
        reads.set(reads.get() + 1);
        Ok(noise(len, address | 1))
    };
    let untouched = MemoryRegion { resident: Some(0), ..region("MALLOC_LARGE", None, 0, 64, "rw-") };
    let large = region("MALLOC_LARGE", None, 0x100_0000, 4096, "rw-");
    let half = |address: u64| Some((address / PAGE).is_multiple_of(8));
    let found =
        scan_process(PROCESS, "tool", EntropyScope::Memory, &[untouched, large], read, half, &Mutex::default(), || {
            true
        });
    assert_eq!(reads.get(), 512, "1,024 pages sampled four apart, half of them in memory");
    assert_eq!(found.pages, 512);

    let (tally, failed) = sample(&region("x", None, 0, 3, "r--"), 1024, |_, _| bail!("refused"), |_| None, || true);
    assert_eq!((tally.blocks, failed), (0, 3), "a process whose pages all fail is unreadable");
    let (tally, _) = sample(&region("x", None, 0, 3, "r--"), 1024, read, |_| None, || false);
    assert_eq!(tally.blocks, 0, "a stopped scan reads nothing more");
}
