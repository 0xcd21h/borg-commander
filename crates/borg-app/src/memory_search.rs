use crate::memory_strings::{Found, Known};
use anyhow::{Result, bail};
use borg_core::{
    Indicator, MemoryHit, MemoryRegion, MemorySearchMode, MemorySearchReport, ProcessKey, STRING_PLACES, StringsTally,
    rank_strings,
};
use borg_i18n::tr;
use std::{
    collections::{HashMap, HashSet},
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
/// Different strings a search lists, and how many more that look like indicators; beyond
/// them it only counts.
const DISTINCT: usize = 100_000;
const INDICATORS: usize = 50_000;
/// Different strings a search remembers having looked at, listed or not.
const SEEN: usize = 2_000_000;
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
/// The matches in `data`, read from `base`.
fn find(query: &Query, data: &[u8], base: u64) -> Result<Vec<MemoryHit>> {
    let hits = matches(data, base, &query.text, query.mode, query.match_case)?;
    Ok(hits
        .into_iter()
        .map(|(address, length, encoding)| MemoryHit { address, length, encoding, ..Default::default() })
        .collect())
}

/// What a search knows of a string it has seen: listed as a result, left out by the mode or the
/// query, or wanted but beyond the results it keeps track of.
#[derive(Clone, Copy)]
enum Seen {
    Listed(u32),
    Unwanted,
    Uncounted,
}

/// The strings a search found so far, each listed once with its copies counted. A string is
/// looked at once, when first seen; its copies only add up.
struct Strings<'a> {
    query: &'a Query,
    needle: String,
    known: &'a Known,
    hits: Vec<MemoryHit>,
    seen: HashMap<u64, Seen>,
    found: u64,
    /// Different wanted strings beyond those listed.
    unlisted: u64,
}

impl<'a> Strings<'a> {
    fn new(query: &'a Query, known: &'a Known) -> Self {
        let needle = if query.match_case { query.text.clone() } else { query.text.to_lowercase() };
        Self { query, needle, known, hits: Vec::new(), seen: HashMap::new(), found: 0, unlisted: 0 }
    }

    fn add(&mut self, found: Found, address: u64, region: &MemoryRegion, index: u32) {
        let key = crate::memory_strings::hash(&found.text);
        match self.seen.get(&key).copied() {
            Some(Seen::Listed(i)) => {
                self.found += 1;
                let kept = &mut self.hits[i as usize];
                kept.copies += 1;
                if kept.places.len() < STRING_PLACES {
                    kept.places.push((address, index));
                }
                return;
            }
            Some(Seen::Unwanted) => return,
            Some(Seen::Uncounted) => {
                self.found += 1;
                return;
            }
            None => {}
        }
        let text = &found.text;
        let contains = self.needle.is_empty()
            || if self.query.match_case {
                text.contains(&self.needle)
            } else {
                text.to_lowercase().contains(&self.needle)
            };
        let indicator = contains.then(|| crate::memory_strings::indicator(text)).flatten();
        let wanted = contains
            && match self.query.mode {
                MemorySearchMode::Strings => indicator.is_some() || !borg_core::junk_string(text),
                _ => indicator.is_some(),
            };
        let room = self.hits.len() < DISTINCT || (indicator.is_some() && self.hits.len() < DISTINCT + INDICATORS);
        let seen = match (wanted, room) {
            (false, _) => Seen::Unwanted,
            (true, false) => Seen::Uncounted,
            (true, true) => Seen::Listed(self.hits.len() as u32),
        };
        if self.seen.len() < SEEN || matches!(seen, Seen::Listed(_)) {
            self.seen.insert(key, seen);
        }
        if wanted {
            self.found += 1;
        }
        match seen {
            Seen::Listed(_) => self.hits.push(MemoryHit {
                address,
                length: found.length,
                encoding: found.encoding.into(),
                region: region.clone(),
                text: Some(text.chars().take(SHOWN).collect()),
                indicator,
                copies: 1,
                places: vec![(address, index)],
                in_file: self.known.holds(key),
            }),
            Seen::Uncounted => self.unlisted += 1,
            Seen::Unwanted => {}
        }
    }

    /// The results, best first, with the tally and only the regions their places name.
    fn finish(mut self, report: &mut MemorySearchReport, regions: &[MemoryRegion]) {
        let mut tally = StringsTally {
            found: self.found,
            distinct: self.hits.len() as u64 + self.unlisted,
            memory_only: self.hits.iter().filter(|hit| hit.in_file == Some(false)).count() as u64,
            files: self.known.files,
            ..Default::default()
        };
        for kind in Indicator::RANKED {
            let count = self.hits.iter().filter(|hit| hit.indicator == Some(kind)).count() as u64;
            if count > 0 {
                tally.kinds.push((kind, count));
            }
        }
        rank_strings(&mut self.hits);
        self.hits.truncate(HITS);
        let mut renumbered = HashMap::new();
        for hit in &mut self.hits {
            for (_, region) in &mut hit.places {
                let next = renumbered.len() as u32;
                *region = *renumbered.entry(*region).or_insert_with(|| {
                    tally.regions.push(regions[*region as usize].clone());
                    next
                });
            }
        }
        report.hits = self.hits;
        report.strings = Some(tally);
    }
}
/// What a memory search looks for: the text, how to read it, and whether case matters.
pub struct Query {
    pub text: String,
    pub mode: MemorySearchMode,
    pub match_case: bool,
}
/// The memory a search reads: a process's regions and, for strings, what its files hold, which
/// tells the strings only memory holds.
pub struct Target<'a> {
    pub process: ProcessKey,
    pub regions: Vec<MemoryRegion>,
    pub known: &'a Known,
}
pub fn scan(
    Target { process, regions, known }: Target,
    query: &Query,
    start: Option<u64>,
    cancelled: &Arc<AtomicBool>,
    mut read: impl FnMut(u64, usize) -> Result<Vec<u8>>,
    mut progress: impl FnMut(u64, u64, usize),
) -> Result<MemorySearchReport> {
    validate(&query.text, query.mode)?;
    let regions: Vec<_> = regions.into_iter().filter(MemoryRegion::readable).collect();
    let total = regions.iter().fold(0u64, |n, r| n.saturating_add(r.end - r.start));
    let mut report = MemorySearchReport {
        process,
        hits: Vec::new(),
        scanned: 0,
        total,
        skipped: 0,
        next: None,
        cancelled: false,
        strings: None,
    };
    // Strings are listed once each, ranked when the search ends; matches are listed as found.
    let mut strings = query.mode.strings().then(|| Strings::new(query, known));
    let mut seen = HashSet::new();
    let began = Instant::now();
    let mut last = Instant::now();
    'regions: for (index, region) in regions.iter().enumerate() {
        let mut address = region.start.max(start.unwrap_or(region.start));
        if address >= region.end {
            continue;
        }
        let mut carry = Vec::new();
        while address < region.end {
            if last.elapsed() >= Duration::from_millis(150) {
                progress(report.scanned, total, strings.as_ref().map_or(report.hits.len(), |s| s.hits.len()));
                last = Instant::now();
            }
            if cancelled.load(Ordering::Relaxed) || began.elapsed() >= BUDGET {
                report.next = Some(address.saturating_sub(carry.len() as u64).max(region.start));
                report.cancelled = cancelled.load(Ordering::Relaxed);
                break 'regions;
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
            if let Some(listed) = &mut strings {
                for found in crate::memory_strings::find(&carry, fresh, OVERLAP, last) {
                    let at = base + found.offset as u64;
                    if at + found.length as u64 > address {
                        listed.add(found, at, region, index as u32);
                    }
                }
            } else {
                for hit in find(query, &carry, base)? {
                    let (at, length) = (hit.address, hit.length);
                    if at + length as u64 > address && seen.insert((at, length)) {
                        report.hits.push(MemoryHit { region: region.clone(), ..hit });
                    }
                    if report.hits.len() >= HITS {
                        report.scanned =
                            report.scanned.saturating_add((at + 1).saturating_sub(address).min(read_len as u64));
                        report.next = Some(at + 1);
                        return Ok(report);
                    }
                }
            }
            address += read_len as u64;
            report.scanned = report.scanned.saturating_add(read_len as u64);
            if carry.len() > OVERLAP {
                carry.drain(..carry.len() - OVERLAP);
            }
        }
    }
    if let Some(strings) = strings {
        strings.finish(&mut report, &regions);
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
    fn strings(
        data: &[u8],
        mode: MemorySearchMode,
        text: &str,
        known: &Known,
        start: Option<u64>,
    ) -> MemorySearchReport {
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
        assert!(
            strings(&data, MemorySearchMode::Strings, "", &none, Some(200)).hits.iter().all(|hit| hit.address >= 200)
        );
    }
    #[test]
    fn strings_are_listed_once_with_their_copies_and_whether_a_file_holds_them() {
        let mut data = vec![0; 4096];
        for (at, text) in
            [(16, "token=hunter22"), (64, "reading the config"), (512, "token=hunter22"), (900, "@@@@@@@@@@")]
        {
            data[at..at + text.len()].copy_from_slice(text.as_bytes());
        }
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("program");
        std::fs::write(&file, b"\0\0reading the config\0\0").unwrap();
        let known = Known::read([file], Instant::now() + Duration::from_secs(5));
        let report = strings(&data, MemorySearchMode::Strings, "", &known, None);
        let rows: Vec<_> =
            report.hits.iter().map(|hit| (hit.text.as_deref().unwrap(), hit.copies, hit.in_file)).collect();
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
            Target {
                process: ProcessKey { pid: 1, start: 1 },
                regions: vec![region(100, 1000)],
                known: &Known::default(),
            },
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
