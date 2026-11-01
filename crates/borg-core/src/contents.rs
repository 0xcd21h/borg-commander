//! Looking inside a file for a search's contents query: whether the file is text worth reading,
//! and the first line that matches, cut to a preview around the match.

use std::{io, ops::Range};

use borg_view::{Search, Source};

/// Files larger than this are left unread.
pub const CONTENTS_SIZE_LIMIT: u64 = 64 * 1024 * 1024;
/// How much of a file's start tells text from binary: a NUL byte there means binary.
const BINARY_PROBE: u64 = 8 * 1024;
/// How much of the matching line a result keeps before the match, and in all.
const PREVIEW_BEFORE: u64 = 64;
const PREVIEW_LIMIT: u64 = 256;
/// How much is read at a time while counting lines.
const CHUNK: u64 = 64 * 1024;
const NEWLINE: u8 = b'\n';
const CARRIAGE_RETURN: u8 = b'\r';
const UTF8_CONTINUATION_MASK: u8 = 0b1100_0000;
const UTF8_CONTINUATION: u8 = 0b1000_0000;

/// The first match in a file: its line, where it starts, and the line around it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineMatch {
    /// Counted from 1.
    pub line: u64,
    /// The match's first byte in the file.
    pub offset: u64,
    pub before: String,
    pub found: String,
    pub after: String,
}

/// What reading a file for a contents query came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scan {
    Matched(LineMatch),
    NoMatch,
    Binary,
    TooLarge,
}

/// Looks for `search` in `source`, unless it is binary or over `CONTENTS_SIZE_LIMIT`. `go_on`
/// hears how far the read got and stops it by answering false.
pub fn scan(source: &dyn Source, search: &Search, go_on: &dyn Fn(u64) -> bool) -> io::Result<Scan> {
    if source.len() > CONTENTS_SIZE_LIMIT {
        return Ok(Scan::TooLarge);
    }
    if read(source, 0, BINARY_PROBE.min(source.len()))?.contains(&0) {
        return Ok(Scan::Binary);
    }
    match search.next(source, 0, go_on)? {
        Some(found) => line_match(source, found, go_on).map(Scan::Matched),
        None => Ok(Scan::NoMatch),
    }
}

fn line_match(source: &dyn Source, found: Range<u64>, go_on: &dyn Fn(u64) -> bool) -> io::Result<LineMatch> {
    let (line, line_start) = line_of(source, found.start, go_on)?;
    let start = line_start.max(found.start.saturating_sub(PREVIEW_BEFORE));
    let mut bytes = read(source, start, (start + PREVIEW_LIMIT).min(source.len()))?;
    let at = usize::try_from(found.start - start).unwrap_or(usize::MAX).min(bytes.len());
    let cut = match bytes[at..].iter().position(|&b| b == NEWLINE) {
        Some(end) => {
            bytes.truncate(at + end);
            false
        }
        None => start + (bytes.len() as u64) < source.len(),
    };
    if bytes.last() == Some(&CARRIAGE_RETURN) {
        bytes.pop();
    }
    let end = usize::try_from(found.end - start).unwrap_or(usize::MAX).clamp(at, bytes.len());
    let skip = if start > line_start { bytes[..at].iter().take_while(|&&b| continues(b)).count() } else { 0 };
    let after = if cut { whole_chars(&bytes[end..]) } else { &bytes[end..] };
    Ok(LineMatch {
        line,
        offset: found.start,
        before: shown(&bytes[skip..at]),
        found: shown(&bytes[at..end]),
        after: shown(after),
    })
}

/// The line `at` is on, counted from 1, and where that line starts.
fn line_of(source: &dyn Source, at: u64, go_on: &dyn Fn(u64) -> bool) -> io::Result<(u64, u64)> {
    let (mut line, mut start, mut offset) = (1, 0, 0);
    while offset < at {
        if !go_on(offset) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "search stopped"));
        }
        let bytes = read(source, offset, (offset + CHUNK).min(at))?;
        if bytes.is_empty() {
            break;
        }
        for (index, _) in bytes.iter().enumerate().filter(|(_, b)| **b == NEWLINE) {
            line += 1;
            start = offset + index as u64 + 1;
        }
        offset += bytes.len() as u64;
    }
    Ok((line, start))
}

fn continues(byte: u8) -> bool {
    byte & UTF8_CONTINUATION_MASK == UTF8_CONTINUATION
}

/// `bytes` without a character cut short at their end.
fn whole_chars(bytes: &[u8]) -> &[u8] {
    match std::str::from_utf8(bytes) {
        Err(error) if error.error_len().is_none() => &bytes[..error.valid_up_to()],
        _ => bytes,
    }
}

/// Text for a terminal cell: control characters such as tabs become spaces.
fn shown(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).chars().map(|c| if c.is_control() { ' ' } else { c }).collect()
}

fn read(source: &dyn Source, start: u64, end: u64) -> io::Result<Vec<u8>> {
    let mut buf = vec![0; usize::try_from(end.saturating_sub(start)).unwrap_or(usize::MAX)];
    let read = source.read_at(start, &mut buf)?;
    buf.truncate(read);
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use borg_view::Query;

    fn search(text: &str) -> Search {
        Query { text: text.into(), ..Default::default() }.compile().unwrap().unwrap()
    }

    fn scanned(bytes: &[u8], text: &str) -> Scan {
        scan(&bytes.to_vec(), &search(text), &|_| true).unwrap()
    }

    fn matched(bytes: &[u8], text: &str) -> LineMatch {
        match scanned(bytes, text) {
            Scan::Matched(found) => found,
            other => panic!("no match: {other:?}"),
        }
    }

    struct Huge;

    impl Source for Huge {
        fn len(&self) -> u64 {
            CONTENTS_SIZE_LIMIT + 1
        }
        fn read_at(&self, _: u64, _: &mut [u8]) -> io::Result<usize> {
            panic!("a file over the limit is not read")
        }
    }

    #[test]
    fn the_first_match_gives_its_line_number_offset_and_the_line_around_it() {
        let text = b"fn main() {\r\n\tlet x = 1;\r\n\tprintln!(\"todo: x\");\r\n}\r\n// TODO later\n";
        let found = matched(text, "todo");
        assert_eq!(found.line, 3);
        assert_eq!(found.offset, 37);
        assert_eq!(
            (found.before.as_str(), found.found.as_str(), found.after.as_str()),
            (" println!(\"", "todo", ": x\");")
        );
    }

    #[test]
    fn binary_files_large_files_and_misses_are_told_apart() {
        assert_eq!(scanned(b"\x7fELF\0\0todo", "todo"), Scan::Binary);
        assert_eq!(scanned(b"nothing here\n", "todo"), Scan::NoMatch);
        assert_eq!(scan(&Huge, &search("todo"), &|_| true).unwrap(), Scan::TooLarge);
        let stopped = scan(&b"a\nb\ntodo\n".to_vec(), &search("todo"), &|_| false).unwrap_err();
        assert_eq!(stopped.kind(), io::ErrorKind::Interrupted);
    }

    #[test]
    fn a_long_line_keeps_a_preview_around_the_match_in_whole_characters() {
        let mut line = format!("{}x", "é".repeat(200)).into_bytes();
        line.extend_from_slice(b"needle");
        line.extend("🦀".repeat(400).into_bytes());
        line.push(NEWLINE);
        let found = matched(&line, "needle");
        assert_eq!(found.line, 1);
        assert_eq!(found.offset, 401);
        assert_eq!(found.before, format!("{}x", "é".repeat(31)));
        assert_eq!(found.found, "needle");
        assert_eq!(found.after, "🦀".repeat(46));
    }
}
