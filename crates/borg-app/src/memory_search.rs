use anyhow::{Result, bail};
use borg_core::{MemoryHit, MemoryRegion, MemorySearchMode, MemorySearchReport, ProcessKey};
use borg_i18n::tr;
use std::{
    collections::HashSet,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;

const CHUNK: usize = 1024 * 1024;
const OVERLAP: usize = 8192;
const HITS: usize = 2000;
const BUDGET: Duration = Duration::from_secs(60);
const PAGE: usize = 4096;
const MAX_QUERY: usize = 512;
/// Characters of a string the list shows.
const SHOWN: usize = 160;
#[derive(Clone, Copy)]
enum Encoding {
    Utf8,
    Utf16(bool),
    Utf32(bool),
}
impl Encoding {
    fn label(self) -> &'static str {
        match self {
            Self::Utf8 => "UTF-8",
            Self::Utf16(false) => "UTF-16LE",
            Self::Utf16(true) => "UTF-16BE",
            Self::Utf32(false) => "UTF-32LE",
            Self::Utf32(true) => "UTF-32BE",
        }
    }
    fn encode(self, text: &str) -> Vec<u8> {
        match self {
            Self::Utf8 => text.as_bytes().to_vec(),
            Self::Utf16(be) => {
                text.encode_utf16().flat_map(|v| if be { v.to_be_bytes() } else { v.to_le_bytes() }).collect()
            }
            Self::Utf32(be) => text
                .chars()
                .flat_map(|v| if be { (v as u32).to_be_bytes() } else { (v as u32).to_le_bytes() })
                .collect(),
        }
    }
}
const ENCODINGS: [Encoding; 5] =
    [Encoding::Utf8, Encoding::Utf16(false), Encoding::Utf16(true), Encoding::Utf32(false), Encoding::Utf32(true)];
pub fn validate(query: &str, mode: MemorySearchMode) -> Result<()> {
    if (query.is_empty() && !mode.strings()) || query.len() > MAX_QUERY {
        bail!(tr!(memory_query_length));
    }
    if mode == MemorySearchMode::Hex {
        hex(query)?;
    }
    Ok(())
}
fn hex(query: &str) -> Result<Vec<u8>> {
    let value = query.split_whitespace().collect::<String>();
    if value.is_empty() || value.len() % 2 != 0 || !value.is_ascii() {
        bail!(tr!(memory_hex_invalid));
    }
    (0..value.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&value[i..i + 2], 16).map_err(|_| anyhow::anyhow!(tr!(memory_hex_invalid))))
        .collect()
}
pub fn matches(
    data: &[u8],
    base: u64,
    query: &str,
    mode: MemorySearchMode,
    case: bool,
) -> Result<Vec<(u64, usize, String)>> {
    validate(query, mode)?;
    let mut hits = Vec::new();
    if mode == MemorySearchMode::Hex {
        let needle = hex(query)?;
        for offset in memchr::memmem::find_iter(data, &needle).take(HITS) {
            hits.push((base + offset as u64, needle.len(), tr!(memory_hex_encoding).into()));
        }
    } else if mode == MemorySearchMode::Text && case {
        for encoding in ENCODINGS {
            let needle = encoding.encode(query);
            for offset in memchr::memmem::find_iter(data, &needle).take(HITS) {
                hits.push((base + offset as u64, needle.len(), encoding.label().into()));
            }
        }
    } else {
        let needle = normal(query, mode == MemorySearchMode::Unicode, case);
        for encoding in ENCODINGS {
            let alignments = match encoding {
                Encoding::Utf8 => 1,
                Encoding::Utf16(_) => 2,
                Encoding::Utf32(_) => 4,
            };
            for alignment in 0..alignments {
                let units = decode(data, encoding, alignment);
                let mut text = String::new();
                let mut offsets = Vec::new();
                for (ch, start, end) in units {
                    let at = text.len();
                    text.push(ch);
                    offsets.push((at, start, end));
                }
                let mut transformed = String::new();
                let mut positions = Vec::new();
                for (at, grapheme) in text.grapheme_indices(true) {
                    let first = offsets.partition_point(|&(offset, _, _)| offset < at);
                    let last =
                        offsets.partition_point(|&(offset, _, _)| offset < at + grapheme.len()).saturating_sub(1);
                    let Some(&(_, start, _)) = offsets.get(first) else { continue };
                    let end = offsets[last].2;
                    for ch in normal(grapheme, mode == MemorySearchMode::Unicode, case).chars() {
                        positions.push((transformed.len(), start, end));
                        transformed.push(ch);
                    }
                }
                for at in memchr::memmem::find_iter(transformed.as_bytes(), needle.as_bytes()).take(HITS) {
                    let first = positions.partition_point(|&(offset, _, _)| offset < at);
                    let last =
                        positions.partition_point(|&(offset, _, _)| offset < at + needle.len()).saturating_sub(1);
                    if let Some(&(_, start, _)) = positions.get(first) {
                        hits.push((base + start as u64, positions[last].2 - start, encoding.label().into()));
                    }
                }
            }
        }
    }
    hits.sort_by_key(|h| (h.0, h.1));
    hits.dedup_by_key(|h| (h.0, h.1));
    hits.truncate(HITS);
    Ok(hits)
}
fn normal(text: &str, normalize: bool, case: bool) -> String {
    let text = if normalize { text.nfkc().collect::<String>() } else { text.to_owned() };
    if case { text } else { text.chars().flat_map(char::to_lowercase).collect() }
}
fn decode(data: &[u8], encoding: Encoding, alignment: usize) -> Vec<(char, usize, usize)> {
    let mut out = Vec::new();
    let mut i = alignment;
    while i < data.len() {
        let (ch, n) = match encoding {
            Encoding::Utf8 => {
                let length = match data[i] {
                    0..=0x7f => 1,
                    0xc2..=0xdf => 2,
                    0xe0..=0xef => 3,
                    0xf0..=0xf4 => 4,
                    _ => 1,
                };
                match data.get(i..i + length).and_then(|s| std::str::from_utf8(s).ok()).and_then(|s| s.chars().next()) {
                    Some(c) => (c, length),
                    None => ('\u{fffd}', 1),
                }
            }
            Encoding::Utf16(be) => {
                let Some(bytes) = data.get(i..i + 2) else { break };
                let u = if be {
                    u16::from_be_bytes([bytes[0], bytes[1]])
                } else {
                    u16::from_le_bytes([bytes[0], bytes[1]])
                };
                if (0xd800..=0xdbff).contains(&u)
                    && let Some(bytes) = data.get(i + 2..i + 4)
                {
                    let v = if be {
                        u16::from_be_bytes([bytes[0], bytes[1]])
                    } else {
                        u16::from_le_bytes([bytes[0], bytes[1]])
                    };
                    if (0xdc00..=0xdfff).contains(&v) {
                        (
                            char::from_u32(0x10000 + ((u32::from(u) - 0xd800) << 10) + (u32::from(v) - 0xdc00))
                                .unwrap_or('\u{fffd}'),
                            4,
                        )
                    } else {
                        ('\u{fffd}', 2)
                    }
                } else {
                    (char::from_u32(u32::from(u)).unwrap_or('\u{fffd}'), 2)
                }
            }
            Encoding::Utf32(be) => {
                let Some(bytes) = data.get(i..i + 4) else { break };
                let bytes: [u8; 4] = bytes.try_into().unwrap();
                (
                    char::from_u32(if be { u32::from_be_bytes(bytes) } else { u32::from_le_bytes(bytes) })
                        .unwrap_or('\u{fffd}'),
                    4,
                )
            }
        };
        out.push((ch, i, i + n));
        i += n;
    }
    out
}
/// The hits in `data`, read from `base`, whose first `fresh` bytes were searched before; `last`
/// when the region ends with it. Strings modes keep those containing the query, if any.
fn find(query: &Query, data: &[u8], base: u64, fresh: usize, last: bool) -> Result<Vec<MemoryHit>> {
    if !query.mode.strings() {
        let hits = matches(data, base, &query.text, query.mode, query.match_case)?;
        return Ok(hits
            .into_iter()
            .map(|(address, length, encoding)| MemoryHit { address, length, encoding, ..Default::default() })
            .collect());
    }
    let needle = if query.match_case { query.text.clone() } else { query.text.to_lowercase() };
    let wanted = |text: &str| {
        needle.is_empty()
            || if query.match_case { text.contains(&needle) } else { text.to_lowercase().contains(&needle) }
    };
    Ok(crate::memory_strings::find(data, fresh, OVERLAP, last)
        .into_iter()
        .filter(|found| wanted(&found.text))
        .filter_map(|found| {
            let indicator = crate::memory_strings::indicator(&found.text);
            (query.mode == MemorySearchMode::Strings || indicator.is_some()).then(|| MemoryHit {
                address: base + found.offset as u64,
                length: found.length,
                encoding: found.encoding.into(),
                text: Some(found.text.chars().take(SHOWN).collect()),
                indicator,
                ..Default::default()
            })
        })
        .collect())
}
/// What a memory search looks for: the text, how to read it, and whether case matters.
pub struct Query {
    pub text: String,
    pub mode: MemorySearchMode,
    pub match_case: bool,
}
pub fn scan(
    process: ProcessKey,
    regions: Vec<MemoryRegion>,
    query: &Query,
    start: Option<u64>,
    cancelled: &Arc<AtomicBool>,
    mut read: impl FnMut(u64, usize) -> Result<Vec<u8>>,
    mut progress: impl FnMut(u64, u64, usize),
) -> Result<MemorySearchReport> {
    validate(&query.text, query.mode)?;
    let total = regions.iter().filter(|r| r.readable()).fold(0u64, |n, r| n.saturating_add(r.end - r.start));
    let mut report =
        MemorySearchReport { process, hits: Vec::new(), scanned: 0, total, skipped: 0, next: None, cancelled: false };
    let mut seen = HashSet::new();
    let began = Instant::now();
    let mut last = Instant::now();
    for region in regions.into_iter().filter(|r| r.readable()) {
        let mut address = region.start.max(start.unwrap_or(region.start));
        if address >= region.end {
            continue;
        }
        let mut carry = Vec::new();
        while address < region.end {
            if last.elapsed() >= Duration::from_millis(150) {
                progress(report.scanned, total, report.hits.len());
                last = Instant::now();
            }
            if cancelled.load(Ordering::Relaxed) || began.elapsed() >= BUDGET {
                report.next = Some(address.saturating_sub(carry.len() as u64).max(region.start));
                report.cancelled = cancelled.load(Ordering::Relaxed);
                return Ok(report);
            }
            let count = (region.end - address).min(CHUNK as u64) as usize;
            let data = read(address, count).or_else(|_| read(address, count.min(PAGE)));
            let data = match data {
                Ok(bytes) if !bytes.is_empty() && bytes.len() <= count => bytes,
                _ => {
                    let skipped = count.min(PAGE);
                    report.skipped = report.skipped.saturating_add(skipped as u64);
                    address += skipped as u64;
                    carry.clear();
                    continue;
                }
            };
            let base = address - carry.len() as u64;
            let read_len = data.len();
            let fresh = carry.len();
            carry.extend(data);
            let last = address + read_len as u64 >= region.end;
            for hit in find(query, &carry, base, fresh, last)? {
                let (at, length) = (hit.address, hit.length);
                if at + length as u64 > address && seen.insert((at, length)) {
                    report.hits.push(MemoryHit { region: region.clone(), ..hit });
                }
                if report.hits.len() >= HITS {
                    // A string is listed whole, so the search goes on after it rather than inside it.
                    let resume = if query.mode.strings() { at + length as u64 } else { at + 1 };
                    report.scanned = report.scanned.saturating_add(resume.saturating_sub(address).min(read_len as u64));
                    report.next = Some(resume);
                    return Ok(report);
                }
            }
            address += read_len as u64;
            report.scanned = report.scanned.saturating_add(read_len as u64);
            if carry.len() > OVERLAP {
                carry.drain(..carry.len() - OVERLAP);
            }
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
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
            ProcessKey { pid: 1, start: 1 },
            vec![region(0x1000, 0x1000 + data.len() as u64)],
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
            ProcessKey { pid: 1, start: 1 },
            vec![region(0, data.len() as u64)],
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
    #[test]
    fn strings_span_reads_once_and_indicators_keep_only_what_looks_like_one() {
        let mut data = vec![0; 2 * CHUNK + 64];
        let url = b"https://updates.example.com/check";
        data[CHUNK - 10..CHUNK - 10 + url.len()].copy_from_slice(url);
        data[100..112].copy_from_slice(b"plain words!");
        let long = 2 * CHUNK - 15_000..2 * CHUNK + 30;
        data[long.clone()].fill(b'x');
        let strings = |mode, text: &str, start| {
            let query = Query { text: text.into(), mode, match_case: false };
            scan(
                ProcessKey { pid: 1, start: 1 },
                vec![region(0, data.len() as u64)],
                &query,
                start,
                &Arc::new(AtomicBool::new(false)),
                |address, count| Ok(data[address as usize..address as usize + count].to_vec()),
                |_, _, _| {},
            )
            .unwrap()
        };
        let report = strings(MemorySearchMode::Strings, "", None);
        let found: Vec<_> = report.hits.iter().map(|hit| (hit.address, hit.length)).collect();
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
        assert_eq!(report.hits[1].text.as_deref(), Some("https://updates.example.com/check"));
        let report = strings(MemorySearchMode::Indicators, "", None);
        assert_eq!(report.hits.iter().map(|hit| hit.indicator).collect::<Vec<_>>(), [Some(borg_core::Indicator::Url)]);
        let report = strings(MemorySearchMode::Strings, "PLAIN", None);
        assert_eq!(report.hits.len(), 1, "the query keeps strings containing it, ignoring case");
        assert!(strings(MemorySearchMode::Strings, "", Some(200)).hits.iter().all(|hit| hit.address >= 200));
    }
    #[test]
    fn cancellation_preserves_a_continuation_without_reading_more_memory() {
        let cancelled = Arc::new(AtomicBool::new(true));
        let mut reads = 0;
        let report = scan(
            ProcessKey { pid: 1, start: 1 },
            vec![region(100, 1000)],
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
}
