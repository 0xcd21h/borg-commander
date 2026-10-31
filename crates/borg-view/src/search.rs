//! Finding text in a document of any size: what to look for, its matches in one line for
//! highlighting, and the next or previous match in a whole source, read a chunk at a time.
//! A match never spans a line break.

use std::{io, ops::Range};

use regex::bytes::{Regex, RegexBuilder};

use crate::Source;

/// How much is read at a time.
const CHUNK: u64 = 1024 * 1024;
/// A line longer than a chunk is read whole up to this length; longer lines are searched in
/// parts, and a match across two parts is missed.
const LONG_LINE: u64 = 16 * 1024 * 1024;
/// Bytes read before where a search starts, so a word boundary or a line start there is seen.
const CONTEXT: u64 = 4;
const NEWLINE: u8 = b'\n';
/// Counting stops here, as a count past it says little more.
pub const MAX_COUNT: usize = 10_000;

/// What to look for, and how.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Query {
    pub text: String,
    /// Match case exactly; otherwise a query without capitals ignores case.
    pub case: bool,
    /// Match whole words only.
    pub words: bool,
    /// The text is a regular expression.
    pub regex: bool,
}

impl Query {
    /// The search the query asks for: `None` for no text, or why a regular expression is not one.
    pub fn compile(&self) -> Result<Option<Search>, String> {
        if self.text.is_empty() {
            return Ok(None);
        }
        let mut pattern = if self.regex { format!("(?:{})", self.text) } else { regex::escape(&self.text) };
        if self.words {
            let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
            let (first, last) = (self.text.chars().next(), self.text.chars().next_back());
            if self.regex || word(first) {
                pattern.insert_str(0, r"\b");
            }
            if self.regex || word(last) {
                pattern.push_str(r"\b");
            }
        }
        RegexBuilder::new(&pattern)
            .case_insensitive(!self.case && !capitals(&self.text, self.regex))
            .multi_line(true)
            .crlf(true)
            .build()
            .map(|regex| Some(Search { regex }))
            .map_err(|error| reason(&error))
    }
}

/// Whether `text` has a capital letter, leaving out a regular expression's escapes such as
/// `\S` and `\p{Lu}`.
fn capitals(text: &str, regex: bool) -> bool {
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if regex && c == '\\' {
            if matches!(chars.next(), Some('p' | 'P')) && chars.clone().next() == Some('{') {
                chars.by_ref().take_while(|c| *c != '}').for_each(drop);
            }
        } else if c.is_uppercase() {
            return true;
        }
    }
    false
}

/// The last line of a regular expression's error, which says what is wrong.
fn reason(error: &regex::Error) -> String {
    let text = error.to_string();
    let line = text.lines().rev().find(|line| !line.trim().is_empty()).unwrap_or(&text);
    line.trim().strip_prefix("error: ").unwrap_or(line.trim()).to_owned()
}

/// Where a match stands among all of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Count {
    /// Counted from 1; `None` when it lies past `MAX_COUNT`.
    pub index: Option<usize>,
    pub total: usize,
    /// More matches than `MAX_COUNT`.
    pub more: bool,
}

#[derive(Debug, Clone)]
pub struct Search {
    regex: Regex,
}

impl Search {
    /// The matches in `line`, given without its break, none empty or overlapping.
    pub fn in_line(&self, line: &[u8]) -> Vec<Range<usize>> {
        self.regex.find_iter(line).filter(|found| !found.is_empty()).map(|found| found.range()).collect()
    }

    /// `with` for the match at `range` of `line`: with a regular expression's groups, such as
    /// `$1`, filled in.
    pub fn replacement(&self, line: &[u8], range: Range<usize>, with: &str) -> String {
        let Some(captures) = self.regex.captures_at(&line[..range.end], range.start) else { return with.to_owned() };
        let mut out = Vec::new();
        captures.expand(with.as_bytes(), &mut out);
        String::from_utf8_lossy(&out).into_owned()
    }

    /// The first match starting at `from` or after it. `go_on` hears how far the search got
    /// between chunks and stops it by answering false.
    pub fn next(&self, source: &dyn Source, from: u64, go_on: &dyn Fn(u64) -> bool) -> io::Result<Option<Range<u64>>> {
        let mut found = None;
        self.each(source, from, source.len(), go_on, &mut |range| {
            found = Some(range);
            false
        })?;
        Ok(found)
    }

    /// The last match starting before `before`, searching back a chunk at a time.
    pub fn previous(
        &self,
        source: &dyn Source,
        before: u64,
        go_on: &dyn Fn(u64) -> bool,
    ) -> io::Result<Option<Range<u64>>> {
        let before = before.min(source.len());
        // A match starting before `before` may end after it, so the line it is on is searched whole.
        let mut end = line_end(source, before)?;
        let mut limit = before;
        while limit > 0 {
            if !go_on(limit) {
                return Err(stopped());
            }
            let start = line_start_before(source, limit.saturating_sub(CHUNK), limit)?;
            let mut last = None;
            self.each(source, start, end, &|_| true, &mut |range| {
                let keep = range.start < limit;
                if keep {
                    last = Some(range);
                }
                keep
            })?;
            if last.is_some() {
                return Ok(last);
            }
            (end, limit) = (start, start);
        }
        Ok(None)
    }

    /// Hands `found` each match from `from` on, in order, while it answers true.
    pub fn each_match(
        &self,
        source: &dyn Source,
        from: u64,
        go_on: &dyn Fn(u64) -> bool,
        found: &mut dyn FnMut(Range<u64>) -> bool,
    ) -> io::Result<()> {
        self.each(source, from, source.len(), go_on, found)
    }

    /// Where the match at `at` stands among all matches in `source`, counting up to `MAX_COUNT`.
    pub fn count(&self, source: &dyn Source, at: u64, go_on: &dyn Fn(u64) -> bool) -> io::Result<Count> {
        let (mut total, mut before) = (0, 0);
        self.each(source, 0, source.len(), go_on, &mut |range| {
            total += 1;
            before += usize::from(range.start < at);
            total < MAX_COUNT
        })?;
        let more = total >= MAX_COUNT;
        let index = (!more || before < total).then_some(before + 1);
        Ok(Count { index, total, more })
    }

    /// Hands `found` each match starting in `from..until` in order, while it answers true.
    /// `until` is the end of a line or of the source.
    fn each(
        &self,
        source: &dyn Source,
        from: u64,
        until: u64,
        go_on: &dyn Fn(u64) -> bool,
        found: &mut dyn FnMut(Range<u64>) -> bool,
    ) -> io::Result<()> {
        let mut start = from;
        while start < until {
            if !go_on(start) {
                return Err(stopped());
            }
            let base = start.saturating_sub(CONTEXT);
            let skip = (start - base) as usize;
            let mut bytes = read(source, base, (base + CHUNK).min(until))?;
            if bytes.len() <= skip {
                break;
            }
            let mut end = bytes.len();
            if base + (bytes.len() as u64) < until {
                // Whole lines only, so no match is cut short; a line longer than the chunk is read on.
                end = match bytes[skip..].iter().rposition(|&b| b == NEWLINE) {
                    Some(i) => skip + i + 1,
                    None => {
                        bytes = read(source, base, (base + LONG_LINE).min(until))?;
                        let whole = base + bytes.len() as u64 >= until;
                        let last = bytes[skip..].iter().rposition(|&b| b == NEWLINE).map(|i| skip + i + 1);
                        if whole { bytes.len() } else { last.unwrap_or(bytes.len()) }
                    }
                };
            }
            let haystack = &bytes[..end];
            let mut at = skip;
            while let Some(hit) = self.regex.find_at(haystack, at) {
                let mut range = hit.range();
                if let Some(newline) = haystack[range.clone()].iter().position(|&b| b == NEWLINE) {
                    // The match ran on past its line: look again in the line alone.
                    let line_end = range.start + newline;
                    match self.regex.find_at(&haystack[..line_end], range.start).filter(|hit| !hit.is_empty()) {
                        Some(hit) => range = hit.range(),
                        None => {
                            at = line_end + 1;
                            continue;
                        }
                    }
                }
                if range.is_empty() {
                    at = range.end + 1;
                    if at > haystack.len() {
                        break;
                    }
                    continue;
                }
                if !found(base + range.start as u64..base + range.end as u64) {
                    return Ok(());
                }
                at = range.end;
            }
            start = base + end as u64;
        }
        Ok(())
    }
}

fn stopped() -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, "search stopped")
}

fn read(source: &dyn Source, start: u64, end: u64) -> io::Result<Vec<u8>> {
    let mut buf = vec![0; usize::try_from(end.saturating_sub(start)).unwrap_or(usize::MAX)];
    let read = source.read_at(start, &mut buf)?;
    buf.truncate(read);
    Ok(buf)
}

/// The end of the line `offset` is on, past its break, looking no further than `LONG_LINE`.
fn line_end(source: &dyn Source, offset: u64) -> io::Result<u64> {
    let mut start = offset;
    let limit = (offset + LONG_LINE).min(source.len());
    while start < limit {
        let bytes = read(source, start, (start + CHUNK).min(limit))?;
        if let Some(i) = bytes.iter().position(|&b| b == NEWLINE) {
            return Ok(start + i as u64 + 1);
        }
        if bytes.is_empty() {
            break;
        }
        start += bytes.len() as u64;
    }
    Ok(limit)
}

/// The first line start in `from..before`, or `from` itself when it starts a line or no line
/// starts in there within `LONG_LINE` of it.
fn line_start_before(source: &dyn Source, from: u64, before: u64) -> io::Result<u64> {
    if from == 0 {
        return Ok(0);
    }
    let bytes = read(source, from - 1, before)?;
    if let Some(i) = bytes.iter().position(|&b| b == NEWLINE)
        && from + (i as u64) < before
    {
        return Ok(from + i as u64);
    }
    // A line longer than the chunk: go back to where it starts, as far as `LONG_LINE`.
    let floor = before.saturating_sub(LONG_LINE);
    let mut end = from;
    while end > floor {
        let start = end.saturating_sub(CHUNK).max(floor);
        let bytes = read(source, start, end)?;
        if let Some(i) = bytes.iter().rposition(|&b| b == NEWLINE) {
            return Ok(start + i as u64 + 1);
        }
        end = start;
    }
    Ok(if floor == 0 { 0 } else { from })
}

#[cfg(test)]
mod tests;
