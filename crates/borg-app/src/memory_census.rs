//! The page map of a region: what each page holds, read without suspending the process.

use anyhow::Result;
use borg_core::{CensusPage, MemoryCensus, MemoryRegion, PageClass, ProcessKey};
use borg_view::entropy::{BLOCK, Entropy, stride};

const PAGE: u64 = BLOCK;
/// Entries a census reports at most; a larger region is sampled evenly.
const ENTRIES: u64 = 16_384;
const TEXT_SHARE: f32 = 0.85;
const WIDE_SHARE: f32 = 0.7;
const POINTER_SHARE: f32 = 0.25;
const WORD: usize = 8;

/// What `page` holds, and how varied its bytes are.
pub fn judge(page: &[u8], executable: bool, mapped: &[(u64, u64)]) -> CensusPage {
    let entropy = Entropy::of(page);
    CensusPage { class: classify(page, entropy, executable, mapped), entropy: Some(entropy) }
}

/// What `page`, of the given `entropy`, holds. `mapped` lists the process's readable mappings,
/// sorted by start, so words that point into them count as pointers.
fn classify(page: &[u8], entropy: Entropy, executable: bool, mapped: &[(u64, u64)]) -> PageClass {
    let Some(&first) = page.first() else { return PageClass::Unread };
    if page.iter().all(|&byte| byte == 0) {
        return PageClass::Zero;
    }
    if page.iter().all(|&byte| byte == first) {
        return PageClass::Filler;
    }
    if executable {
        return PageClass::Code;
    }
    let printable = |byte: u8| byte.is_ascii_graphic() || matches!(byte, b' ' | b'\t' | b'\n' | b'\r');
    let share = |count: usize, of: usize| count as f32 / of.max(1) as f32;
    if share(page.iter().filter(|&&byte| printable(byte)).count(), page.len()) >= TEXT_SHARE {
        return PageClass::Text;
    }
    let wide = page.as_chunks::<2>().0.iter().filter(|[byte, high]| printable(*byte) && *high == 0).count();
    if share(wide, page.len() / 2) >= WIDE_SHARE {
        return PageClass::Wide;
    }
    let words = page.as_chunks::<WORD>().0.iter().map(|word| u64::from_le_bytes(*word));
    let pointers = words.filter(|&word| word != 0 && points_into(word, mapped)).count();
    if share(pointers, page.len() / WORD) >= POINTER_SHARE {
        return PageClass::Pointers;
    }
    if entropy.high() {
        return PageClass::Dense;
    }
    PageClass::Data
}

fn points_into(address: u64, mapped: &[(u64, u64)]) -> bool {
    let after = mapped.partition_point(|(start, _)| *start <= address);
    after > 0 && address < mapped[after - 1].1
}

/// Classifies the region's pages, or an even sample of them when there are more than `ENTRIES`.
/// `resident` tells whether a page is in memory, when the system says: pages that are not are
/// left unread, so the census neither faults in untouched pages nor brings back swapped ones.
pub fn census(
    process: ProcessKey,
    region: &MemoryRegion,
    mapped: &[(u64, u64)],
    read: impl Fn(u64, usize) -> Result<Vec<u8>>,
    resident: impl Fn(u64) -> Option<bool>,
) -> MemoryCensus {
    let start = region.start - region.start % PAGE;
    let total = region.end.saturating_sub(start).div_ceil(PAGE);
    let stride = stride(total, ENTRIES);
    let executable = region.permissions.contains('x');
    let mut residency = false;
    let pages = (0..total.div_ceil(stride))
        .map(|entry| {
            let address = start + entry * stride * PAGE;
            match resident(address) {
                Some(false) => {
                    residency = true;
                    return CensusPage::unread(PageClass::Absent);
                }
                Some(true) => residency = true,
                None => {}
            }
            read(address, PAGE as usize)
                .map_or(CensusPage::unread(PageClass::Unread), |page| judge(&page, executable, mapped))
        })
        .collect();
    MemoryCensus { process, start, end: region.end, page: PAGE, stride, pages, residency }
}

/// Whether pages are in memory, from Linux's page map, which whoever may read the process's
/// memory may read too.
#[cfg(target_os = "linux")]
pub struct Residency(Option<std::fs::File>);

#[cfg(target_os = "linux")]
impl Residency {
    pub const PRESENT: u64 = 1 << 63;
    pub const SWAPPED: u64 = 1 << 62;
    /// The page is the file's own, not a private copy of it.
    pub const FILE: u64 = 1 << 61;

    pub fn open(pid: u32) -> Self {
        Self(std::fs::File::open(format!("/proc/{pid}/pagemap")).ok())
    }

    pub fn resident(&self, address: u64) -> Option<bool> {
        Some(self.entries(address, 1)?[0] & Self::PRESENT != 0)
    }

    /// The page map entries of `count` pages from `address`.
    pub fn entries(&self, address: u64, count: usize) -> Option<Vec<u64>> {
        use std::os::unix::fs::FileExt;
        let mut bytes = vec![0u8; count * 8];
        self.0.as_ref()?.read_exact_at(&mut bytes, address / PAGE * 8).ok()?;
        Some(bytes.as_chunks::<8>().0.iter().map(|entry| u64::from_le_bytes(*entry)).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(fill: impl FnMut(usize) -> u8) -> Vec<u8> {
        (0..PAGE as usize).map(fill).collect()
    }

    fn random() -> Vec<u8> {
        let mut noise = 0x2545_f491_4f6c_dd1du64;
        page(|_| {
            noise ^= noise << 13;
            noise ^= noise >> 7;
            noise ^= noise << 17;
            noise as u8
        })
    }

    #[test]
    fn pages_are_told_apart_by_what_they_hold() {
        let mapped = [(0x1000, 0x9000), (0x7f00_0000_0000, 0x7f00_0010_0000)];
        let kind = |bytes: &[u8], executable: bool| judge(bytes, executable, &mapped).class;
        assert_eq!(kind(&page(|_| 0), false), PageClass::Zero);
        assert_eq!(kind(&page(|_| 0xcc), false), PageClass::Filler);
        assert_eq!(kind(&page(|i| b"hello world\n"[i % 12]), false), PageClass::Text);
        assert_eq!(kind(&page(|i| if i % 2 == 0 { b'A' + (i % 26) as u8 } else { 0 }), false), PageClass::Wide);
        let pointers: Vec<u8> = (0..PAGE / 8).flat_map(|i| (0x7f00_0000_0000u64 + i * 16).to_le_bytes()).collect();
        assert_eq!(kind(&pointers, false), PageClass::Pointers);
        assert_eq!(kind(&random(), false), PageClass::Dense);
        assert_eq!(kind(&random(), true), PageClass::Code);
        assert_eq!(kind(&page(|i| (i % 7) as u8), false), PageClass::Data);
    }

    #[test]
    fn every_page_read_carries_its_entropy_so_packed_code_stands_out() {
        let packed = judge(&random(), true, &[]);
        assert!(packed.class == PageClass::Code && packed.high(), "{packed:?}");
        let opcodes = [0x48, 0x89, 0xe5, 0xc3, 0x90, 0x55, 0x8b, 0x45, 0xe8, 0x0f, 0x1f, 0x44, 0x00, 0x83, 0xec, 0x10];
        let code: Vec<u8> = random().iter().map(|&byte| opcodes[usize::from(byte) % opcodes.len()]).collect();
        let unpacked = judge(&code, true, &[]);
        assert!(unpacked.class == PageClass::Code && !unpacked.high(), "{unpacked:?}");
        assert_eq!(judge(&page(|_| 0), false, &[]).entropy.map(Entropy::bits), Some(0.0));
        assert!(!CensusPage::unread(PageClass::Absent).high());
    }

    #[test]
    fn large_regions_are_sampled_and_absent_pages_are_not_read() {
        let process = ProcessKey { pid: 1, start: 1 };
        let region =
            MemoryRegion { start: 0, end: PAGE * ENTRIES * 3, permissions: "rw-".into(), ..Default::default() };
        let reads = std::cell::Cell::new(0);
        let read = |_: u64, len: usize| {
            reads.set(reads.get() + 1);
            Ok(vec![0; len])
        };
        let census = census(process, &region, &[], read, |address| Some(address % (2 * PAGE * 3) == 0));
        assert_eq!((census.stride, census.pages.len()), (3, ENTRIES as usize));
        assert_eq!(reads.get(), ENTRIES as usize / 2, "only resident pages are read");
        assert!(
            census.residency && census.pages[1].class == PageClass::Absent && census.pages[0].class == PageClass::Zero
        );
        assert_eq!((census.pages[1].entropy, census.highest()), (None, census.pages[0].entropy));
        assert_eq!(census.address(1), PAGE * 3);
    }
}
