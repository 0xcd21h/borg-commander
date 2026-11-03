use super::*;
use std::{
    cell::Cell,
    sync::atomic::{AtomicU64, Ordering},
};

/// Bytes that look random, the same each run.
fn noise(len: usize) -> Vec<u8> {
    let mut state = 0x2545_f491_4f6c_dd1du64;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        })
        .collect()
}

fn text(len: usize) -> Vec<u8> {
    b"the quick brown fox jumps over the lazy dog\n".iter().copied().cycle().take(len).collect()
}

#[test]
fn entropy_is_the_bits_each_byte_carries() {
    assert_eq!(Entropy::of(&[]).bits(), 0.0);
    assert_eq!(Entropy::of(&[7; 4096]).bits(), 0.0);
    assert_eq!(Entropy::of(b"abab").bits(), 1.0);
    let every_value: Vec<u8> = (0..=255).collect();
    assert_eq!(Entropy::of(&every_value).bits(), 8.0);
    let text = Entropy::of(&text(4096));
    assert!((3.5..5.0).contains(&text.bits()), "{text:?}");
    let random = Entropy::of(&noise(4096));
    assert!(random.bits() > 7.9, "{random:?}");
}

#[test]
fn bands_say_what_entropy_usually_means() {
    assert_eq!(Entropy::of(&[0; 64]).band(), Band::Low);
    assert_eq!(Entropy::of(&text(4096)).band(), Band::Typical);
    assert_eq!(Entropy::of(&noise(4096)).band(), Band::High);
    assert!(Entropy::of(&noise(4096)).high() && !Entropy::of(&text(4096)).high());
    assert!(Entropy::HIGH.high() && Entropy::HIGH.bits() == 7.2);
}

#[test]
fn a_profile_judges_each_block_and_finds_where_high_runs_start() {
    let block = BLOCK as usize;
    let file = [text(block), noise(2 * block), text(block), noise(block), text(block / 2)].concat();
    let profile = Profile::read(&file, &|| true).unwrap().unwrap();
    assert_eq!((profile.len, profile.stride, profile.blocks.len()), (file.len() as u64, 1, 6));
    let high: Vec<bool> = profile.blocks.iter().map(|entropy| entropy.high()).collect();
    assert_eq!(high, [false, true, true, false, true, false]);
    assert_eq!(profile.at(BLOCK + 5), Some(profile.blocks[1]));
    assert_eq!(profile.at(BLOCK * 99), None);
    assert_eq!(profile.highest(0..BLOCK), Some(profile.blocks[0]));
    assert!(profile.highest(0..BLOCK * 2).is_some_and(Entropy::high));
    assert_eq!(profile.highest(10..11), Some(profile.blocks[0]), "a range inside a block takes the block");
    assert!((profile.high_share() - 0.5).abs() < f64::EPSILON);

    assert_eq!(profile.next_high(0), Some(BLOCK));
    assert_eq!(profile.next_high(BLOCK), Some(4 * BLOCK), "the run the page is in is passed over");
    assert_eq!(profile.next_high(2 * BLOCK), Some(4 * BLOCK));
    assert_eq!(profile.next_high(4 * BLOCK), None);
    assert_eq!(profile.previous_high(5 * BLOCK), Some(4 * BLOCK));
    assert_eq!(profile.previous_high(4 * BLOCK), Some(BLOCK));
    assert_eq!(profile.previous_high(2 * BLOCK), Some(BLOCK), "back to the start of the run the page is in");
    assert_eq!(profile.previous_high(BLOCK), None);
}

#[test]
fn a_large_source_is_sampled_evenly_in_bounded_memory() {
    struct Huge(AtomicU64);
    impl Source for Huge {
        fn len(&self) -> u64 {
            PROFILE_BLOCKS * BLOCK * 3 + 1
        }
        fn read_at(&self, _: u64, buf: &mut [u8]) -> io::Result<usize> {
            self.0.fetch_add(1, Ordering::Relaxed);
            buf.fill(0);
            Ok(buf.len())
        }
    }
    let huge = Huge(AtomicU64::new(0));
    let profile = Profile::read(&huge, &|| true).unwrap().unwrap();
    assert_eq!(profile.stride, 4);
    assert_eq!(profile.blocks.len(), (PROFILE_BLOCKS * 3 + 1).div_ceil(4) as usize);
    assert_eq!(huge.0.load(Ordering::Relaxed), profile.blocks.len() as u64, "one block read for each entry");
    assert_eq!(profile.at(PROFILE_BLOCKS * BLOCK * 3), profile.blocks.last().copied());
}

#[test]
fn reading_stops_when_asked() {
    let file = noise(BLOCK as usize * 4);
    let left = Cell::new(2u8);
    let go_on = || {
        left.set(left.get().saturating_sub(1));
        left.get() > 0
    };
    assert_eq!(Profile::read(&file, &go_on).unwrap(), None);
}

#[test]
fn strip_cells_share_the_source_evenly_and_find_their_bytes() {
    assert_eq!(strip_cell(100, 4, 0), 0..25);
    assert_eq!(strip_cell(100, 4, 3), 75..100);
    assert_eq!(strip_cell(3, 6, 0), 0..1, "a short source still gives each cell a byte");
    assert_eq!(strip_cell(3, 6, 5), 2..3);
    assert_eq!(strip_cell_of(100, 4, 0), 0);
    assert_eq!(strip_cell_of(100, 4, 74), 2);
    assert_eq!(strip_cell_of(100, 4, 99), 3);
    assert_eq!(strip_cell_of(100, 4, 100), 3);
    assert_eq!(strip_cell_of(u64::MAX, 40, u64::MAX - 1), 39);
}

#[test]
fn a_stride_keeps_reads_to_the_most_asked() {
    assert_eq!(stride(0, 8), 1);
    assert_eq!(stride(8, 8), 1);
    assert_eq!(stride(9, 8), 2);
    assert_eq!(stride(80, 8), 10);
    assert_eq!(stride(5, 0), 5, "no limit reads one block");
}

#[test]
fn a_tally_counts_high_blocks_and_where_the_first_one_is() {
    let mut tally = Tally::default();
    assert_eq!(tally.share(), 0.0);
    tally.add(BLOCK * 3, Entropy::of(&noise(BLOCK as usize)));
    tally.add(BLOCK, Entropy::of(&text(BLOCK as usize)));
    tally.add(BLOCK * 2, Entropy::of(&noise(BLOCK as usize)));
    tally.add(0, Entropy::of(&[0; 16]));
    assert_eq!((tally.blocks, tally.high, tally.first_high), (4, 2, Some(BLOCK * 2)));
    assert!(tally.highest.high() && (tally.share() - 0.5).abs() < f64::EPSILON);
}

#[test]
fn sampling_spreads_over_all_ranges_and_ends_blocks_with_them() {
    let all: Vec<_> = sampled(&[0..BLOCK * 2 + 10, BLOCK * 10..BLOCK * 11], 100).collect();
    assert_eq!(all, [0..BLOCK, BLOCK..BLOCK * 2, BLOCK * 2..BLOCK * 2 + 10, BLOCK * 10..BLOCK * 11]);
    let ranges = [0..BLOCK * 5, BLOCK * 100..BLOCK * 105];
    let starts: Vec<_> = sampled(&ranges, 4).map(|block| block.start / BLOCK).collect();
    assert_eq!(starts, [0, 3, 101, 104], "every third of the ten blocks, counted across both ranges");
    assert_eq!(sampled(&[], 4).count(), 0);
}

#[test]
fn a_programs_code_is_judged_and_its_first_high_block_found() {
    let block = BLOCK as usize;
    let mut file = [text(block), text(block), noise(block), noise(block), text(block)].concat();
    file[..4].copy_from_slice(b"\x7fELF");
    file[4..7].copy_from_slice(&[2, 1, 1]);
    file[0x10..0x12].copy_from_slice(&2u16.to_le_bytes());
    file[0x20..0x28].copy_from_slice(&64u64.to_le_bytes());
    file[0x36..0x38].copy_from_slice(&56u16.to_le_bytes());
    file[0x38..0x3a].copy_from_slice(&1u16.to_le_bytes());
    file[64..68].copy_from_slice(&1u32.to_le_bytes());
    file[68..72].copy_from_slice(&5u32.to_le_bytes());
    file[72..80].copy_from_slice(&BLOCK.to_le_bytes());
    file[96..104].copy_from_slice(&(BLOCK * 3).to_le_bytes());
    let tally = code_tally(&file, &|| true).unwrap().unwrap();
    assert_eq!((tally.blocks, tally.high, tally.first_high), (3, 2, Some(BLOCK * 2)));
    assert_eq!(code_tally(&text(block), &|| true).unwrap(), None, "text is no program");
    let stopped = code_tally(&file, &|| false).unwrap_err();
    assert_eq!(stopped.kind(), io::ErrorKind::Interrupted);
}
