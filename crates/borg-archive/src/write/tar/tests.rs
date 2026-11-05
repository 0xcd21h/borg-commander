use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use borg_i18n::tr;
use borg_jobs::{Control, Totals};

use super::{BLOCK, Exact, verify};
use crate::{Compression, Error, meter::Meter};

const DATA: &[u8] = b"hello archive\n";
const SAVE: &[u8] = b"alpha, first save";
const SAVE_PAST_A_BLOCK: usize = 2000;
/// How far into the second entry's data a cut archive ends.
const CUT_INTO_DATA: usize = 100;
/// The first entry's header and data, and the second entry's header.
const BLOCKS_BEFORE_SECOND_DATA: usize = 3;
const MODE: u32 = 0o644;

fn entry(builder: &mut tar::Builder<Vec<u8>>, path: &str, data: &[u8], size: usize) {
    let mut header = tar::Header::new_gnu();
    header.set_size(size as u64);
    header.set_mode(MODE);
    builder.append_data(&mut header, path, data).unwrap();
}

fn two_entries() -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    entry(&mut builder, "a.txt", DATA, DATA.len());
    entry(&mut builder, "b.txt", DATA, DATA.len());
    builder.into_inner().unwrap()
}

fn gzip(data: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(data).unwrap();
    encoder.finish().unwrap()
}

fn verified(bytes: &[u8], compression: Option<Compression>, written: usize) -> Result<(), Error> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("check.tar");
    fs::write(&path, bytes).unwrap();
    let control = Control::default();
    let mut emit = |_| {};
    let meter = Meter::new(&control, &mut emit, Totals::default(), PathBuf::new());
    verify(&mut fs::File::open(&path).unwrap(), compression, written, &meter)
}

fn damaged(result: Result<(), Error>) -> bool {
    matches!(result, Err(Error::Damaged(_)))
}

#[test]
fn a_tar_reads_back_when_it_holds_every_entry_written_whole() {
    assert!(verified(&two_entries(), None, 2).is_ok());
    assert!(verified(&gzip(&two_entries()), Some(Compression::Gzip), 2).is_ok());
}

#[test]
fn a_tar_missing_an_entry_or_ending_inside_one_does_not_read_back() {
    let whole = two_entries();
    assert!(damaged(verified(&whole, None, 3)));
    assert!(damaged(verified(&gzip(&whole), Some(Compression::Gzip), 3)));
    let second_data = BLOCKS_BEFORE_SECOND_DATA * BLOCK as usize;
    let cut = &whole[..second_data + CUT_INTO_DATA];
    assert!(damaged(verified(cut, None, 2)));
    assert!(damaged(verified(&gzip(cut), Some(Compression::Gzip), 2)));
}

#[test]
fn an_entry_holding_more_data_than_its_header_says_does_not_read_back() {
    let mut builder = tar::Builder::new(Vec::new());
    entry(&mut builder, "a.txt", &[b'y'; SAVE_PAST_A_BLOCK], SAVE.len());
    entry(&mut builder, "b.txt", DATA, DATA.len());
    let spilled = builder.into_inner().unwrap();
    assert!(damaged(verified(&spilled, None, 2)));
    assert!(damaged(verified(&gzip(&spilled), Some(Compression::Gzip), 2)));
}

#[test]
fn a_file_read_for_an_entry_stops_at_its_size_and_fails_when_it_ends_sooner() {
    let source = Path::new("a.txt");
    let mut read = Vec::new();
    Exact { inner: SAVE, left: DATA.len() as u64, source }.read_to_end(&mut read).unwrap();
    assert_eq!(read, &SAVE[..DATA.len()]);

    let error = Exact { inner: DATA, left: SAVE.len() as u64, source }.read_to_end(&mut Vec::new()).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
    assert_eq!(error.to_string(), tr!(archive_source_shrank, path = source.display()));
}
