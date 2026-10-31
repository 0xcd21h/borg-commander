use super::*;
use crate::memory_strings::Known;
use anyhow::bail;
use borg_core::{Indicator, MemoryRegion, MemorySearchMode, MemorySearchReport, ProcessKey};
use std::{
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};
fn region(start: u64, end: u64) -> MemoryRegion {
    MemoryRegion { start, end, permissions: "r--".into(), name: "test".into(), ..Default::default() }
}
#[test]
fn text_finds_utf8_utf16_and_utf32_with_original_byte_addresses() {
    for encoding in ENCODINGS {
        let mut bytes = vec![0xff; 7];
        bytes.extend(encoding.encode("Borg"));
        bytes.extend([0xff; 8]);
        let found = matches(&bytes, 0x1000, "Borg", MemorySearchMode::Text, true).unwrap();
        assert!(found.iter().any(|(at, len, label)| *at == 0x1007
            && *len == encoding.encode("Borg").len()
            && label == encoding.label()));
    }
}
#[test]
fn normalization_matches_styled_letters_combining_sequences_and_wide_encodings() {
    for encoding in ENCODINGS {
        let mut bytes = vec![0xff; 3];
        bytes.extend(encoding.encode("𝐁𝐨𝐫𝐠"));
        let found = matches(&bytes, 100, "Borg", MemorySearchMode::Unicode, true).unwrap();
        assert!(found.iter().any(|(at, _, _)| *at == 103));
        let found = matches(&encoding.encode("cafe\u{301}"), 0, "café", MemorySearchMode::Unicode, true).unwrap();
        assert!(found.iter().any(|(at, _, _)| *at == 0));
    }
    assert!(!matches(b"BORg", 0, "borg", MemorySearchMode::Text, false).unwrap().is_empty());
}
#[test]
fn hex_matches_binary_and_invalid_patterns_are_errors() {
    assert_eq!(matches(&[0, 0xff, 0, 0x42], 50, "00 ff 00", MemorySearchMode::Hex, true).unwrap()[0].0, 50);
    for query in ["0", "xyz", "", "🐱"] {
        assert!(validate(query, MemorySearchMode::Hex).is_err());
    }
}
fn borg() -> Query {
    Query { text: "Borg".into(), mode: MemorySearchMode::Text, match_case: true }
}
#[test]
fn scans_cross_chunk_boundaries_and_reports_unreadable_pages() {
    let mut data = vec![0; CHUNK + 32];
    data[CHUNK - 2..CHUNK + 2].copy_from_slice(b"Borg");
    let cancelled = Arc::new(AtomicBool::new(false));
    let report = scan(
        Target {
            process: ProcessKey { pid: 1, start: 1 },
            regions: vec![region(0x1000, 0x1000 + data.len() as u64)],
            known: &Known::default(),
        },
        &borg(),
        None,
        &cancelled,
        |address, count| Ok(data[(address - 0x1000) as usize..(address - 0x1000) as usize + count].to_vec()),
        |_, _, _| {},
    )
    .unwrap();
    assert_eq!(report.hits.len(), 1);
    assert_eq!(report.hits[0].address, 0x1000 + CHUNK as u64 - 2);
    assert_eq!(report.scanned, data.len() as u64);
    assert!(report.next.is_none());
    let mut data = vec![0; PAGE * 4];
    data[PAGE * 2 - 2..PAGE * 2 + 2].copy_from_slice(b"Borg");
    let report = scan(
        Target {
            process: ProcessKey { pid: 1, start: 1 },
            regions: vec![region(0, data.len() as u64)],
            known: &Known::default(),
        },
        &borg(),
        None,
        &cancelled,
        |address, count| {
            if address < PAGE as u64 || count > PAGE {
                bail!("unreadable");
            }
            Ok(data[address as usize..address as usize + count].to_vec())
        },
        |_, _, _| {},
    )
    .unwrap();
    assert_eq!(report.skipped, PAGE as u64);
    assert_eq!(report.hits[0].address, (PAGE * 2 - 2) as u64);
}
fn strings(data: &[u8], mode: MemorySearchMode, text: &str, known: &Known, start: Option<u64>) -> MemorySearchReport {
    let query = Query { text: text.into(), mode, match_case: false };
    scan(
        Target { process: ProcessKey { pid: 1, start: 1 }, regions: vec![region(0, data.len() as u64)], known },
        &query,
        start,
        &Arc::new(AtomicBool::new(false)),
        |address, count| Ok(data[address as usize..address as usize + count].to_vec()),
        |_, _, _| {},
    )
    .unwrap()
}
#[test]
fn strings_span_reads_once_and_indicators_keep_only_what_looks_like_one() {
    let mut data = vec![0; 2 * CHUNK + 64];
    let url = b"https://updates.example.com/check";
    data[CHUNK - 10..CHUNK - 10 + url.len()].copy_from_slice(url);
    data[100..112].copy_from_slice(b"plain words!");
    let long = 2 * CHUNK - 15_000..2 * CHUNK + 30;
    for (i, byte) in data[long.clone()].iter_mut().enumerate() {
        *byte = b"abcdefg"[i % 7];
    }
    let none = Known::default();
    let report = strings(&data, MemorySearchMode::Strings, "", &none, None);
    let mut found: Vec<_> = report.hits.iter().map(|hit| (hit.address, hit.length)).collect();
    found.sort();
    // A run too long to carry over is listed up to the end of the read, and the rest after it.
    assert_eq!(
        found,
        [
            (100, 12),
            (CHUNK as u64 - 10, url.len()),
            (long.start as u64, 2 * CHUNK - long.start),
            (2 * CHUNK as u64, long.end - 2 * CHUNK)
        ],
    );
    assert_eq!(report.hits[0].text.as_deref(), Some("https://updates.example.com/check"), "indicators lead");
    assert_eq!(report.strings.as_ref().map(|t| t.kinds.clone()), Some(vec![(Indicator::Url, 1)]));
    let report = strings(&data, MemorySearchMode::Indicators, "", &none, None);
    assert_eq!(report.hits.iter().map(|hit| hit.indicator).collect::<Vec<_>>(), [Some(Indicator::Url)]);
    let report = strings(&data, MemorySearchMode::Strings, "PLAIN", &none, None);
    assert_eq!(report.hits.len(), 1, "the query keeps strings containing it, ignoring case");
    assert!(strings(&data, MemorySearchMode::Strings, "", &none, Some(200)).hits.iter().all(|hit| hit.address >= 200));
}
#[test]
fn strings_are_listed_once_with_their_copies_and_whether_a_file_holds_them() {
    let mut data = vec![0; 4096];
    for (at, text) in [(16, "token=hunter22"), (64, "reading the config"), (512, "token=hunter22"), (900, "@@@@@@@@@@")]
    {
        data[at..at + text.len()].copy_from_slice(text.as_bytes());
    }
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("program");
    std::fs::write(&file, b"\0\0reading the config\0\0").unwrap();
    let known = Known::read([file], Instant::now() + Duration::from_secs(5));
    let report = strings(&data, MemorySearchMode::Strings, "", &known, None);
    let rows: Vec<_> = report.hits.iter().map(|hit| (hit.text.as_deref().unwrap(), hit.copies, hit.in_file)).collect();
    // Filler such as a run of one character is left out.
    assert_eq!(rows, [("token=hunter22", 2, Some(false)), ("reading the config", 1, Some(true))]);
    assert_eq!(report.hits[0].places, [(16, 0), (512, 0)]);
    let tally = report.strings.unwrap();
    assert_eq!((tally.found, tally.distinct, tally.memory_only, tally.files), (3, 2, 1, 1));
    assert_eq!(tally.regions.len(), 1, "only the regions the results name");
}
#[test]
fn cancellation_preserves_a_continuation_without_reading_more_memory() {
    let cancelled = Arc::new(AtomicBool::new(true));
    let mut reads = 0;
    let report = scan(
        Target { process: ProcessKey { pid: 1, start: 1 }, regions: vec![region(100, 1000)], known: &Known::default() },
        &borg(),
        None,
        &cancelled,
        |_, _| {
            reads += 1;
            Ok(vec![])
        },
        |_, _, _| {},
    )
    .unwrap();
    assert_eq!(reads, 0);
    assert!(report.cancelled);
    assert_eq!(report.next, Some(100));
}
