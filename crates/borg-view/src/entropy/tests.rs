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
