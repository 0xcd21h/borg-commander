use std::{
    borrow::Cow,
    collections::BTreeSet,
    ops::{Range, RangeInclusive},
};

use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};

use crate::pattern::composed;

const ONE_TYPO_FROM: usize = 4;
const TWO_TYPOS_FROM: usize = 9;
const WHOLE_WORD_FREE_FROM: usize = 6;
const PART_OF_WORD_COST: u8 = 1;
const SHUFFLED_FROM: usize = 4;
const SCATTERED_FROM: usize = 4;
const SCATTERED_COVER_PERCENT: usize = 50;
const PERCENT: usize = 100;
const UNREACHABLE: u8 = u8::MAX / 2;
const SWAP_COST: u8 = 1;
const LETTER_COST: u8 = 1;
const LETTER_BITS: u32 = 26;
const OTHER_BIT: u64 = 1 << 36;
const DECIMAL: u32 = 10;
const STEM_SEPARATOR: char = '.';
const WITHOUT_TYPOS: [RangeInclusive<char>; 10] = [
    '\u{1100}'..='\u{11FF}',
    '\u{2E80}'..='\u{2FDF}',
    '\u{3040}'..='\u{30FF}',
    '\u{3130}'..='\u{318F}',
    '\u{31F0}'..='\u{31FF}',
    '\u{3400}'..='\u{4DBF}',
    '\u{4E00}'..='\u{9FFF}',
    '\u{AC00}'..='\u{D7AF}',
    '\u{F900}'..='\u{FAFF}',
    '\u{20000}'..='\u{3134F}',
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tier {
    Exact,
    Prefix,
    WordStart,
    Inside,
    Typo,
    Shuffled,
    Scattered,
}

/// How well a typed word matches a name, best first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MatchRank {
    pub tier: Tier,
    pub edits: u8,
    pub partial: bool,
}

impl MatchRank {
    pub const EXACT: Self = Self::of(Tier::Exact);

    const fn of(tier: Tier) -> Self {
        Self { tier, edits: 0, partial: false }
    }

    pub fn is_close(self) -> bool {
        self.tier >= Tier::Typo
    }
}

pub type Spans = Vec<Range<usize>>;

/// A typed word, ready to be looked for in names exactly and, when allowed, closely: with
/// typos, with its digits in another order, or with letters left out.
#[derive(Debug, Clone)]
pub struct Needle {
    chars: Vec<char>,
    text: String,
    fold: bool,
    close: Option<Close>,
}

#[derive(Debug, Clone)]
struct Close {
    letters: Vec<char>,
    mask: u64,
    leads: u64,
    digits: bool,
    parts: Vec<Close>,
}

struct Name {
    letters: Vec<char>,
    starts: Vec<bool>,
    origin: Vec<Range<usize>>,
    mask: u64,
}

fn fold_char(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

fn typos(letters: usize) -> u8 {
    match letters {
        count if count >= TWO_TYPOS_FROM => 2,
        count if count >= ONE_TYPO_FROM => 1,
        _ => 0,
    }
}

fn bit(c: char) -> u64 {
    let c = c.to_ascii_lowercase();
    match c {
        'a'..='z' => 1 << (c as u32 - 'a' as u32),
        '0'..='9' => 1 << (LETTER_BITS + c as u32 - '0' as u32),
        _ => OTHER_BIT,
    }
}

fn missing(wanted: u64, present: u64) -> u8 {
    (wanted & !present).count_ones() as u8
}

fn base(c: char) -> char {
    if c.is_ascii() { c } else { std::iter::once(c).nfd().next().unwrap_or(c) }
}

fn starts_word(previous: Option<char>, c: char, next: Option<char>) -> bool {
    previous.is_none_or(|p| {
        p.is_ascii_digit() != c.is_ascii_digit()
            || (p.is_lowercase() && c.is_uppercase())
            || (p.is_uppercase() && c.is_uppercase() && next.is_some_and(char::is_lowercase))
    })
}

fn push_bare(c: char, fold: bool, into: &mut Vec<char>) {
    if c.is_ascii() {
        into.push(if fold { c.to_ascii_lowercase() } else { c });
        return;
    }
    for plain in std::iter::once(c).nfd().filter(|c| !is_combining_mark(*c)) {
        if fold {
            into.extend(plain.to_lowercase());
        } else {
            into.push(plain);
        }
    }
}

fn marked(c: char) -> bool {
    !c.is_ascii() && is_combining_mark(c)
}

/// The letters and digits in `name`, and those that start one of its words, as bits.
fn masks(name: &str) -> (u64, u64) {
    let mut chars = name.chars().filter(|c| !marked(*c)).peekable();
    let (mut all, mut initials, mut previous) = (0, 0, None);
    while let Some(c) = chars.next() {
        if !c.is_alphanumeric() {
            previous = None;
            continue;
        }
        let found = bit(base(c));
        all |= found;
        if starts_word(previous, c, chars.peek().copied()) {
            initials |= found;
        }
        previous = Some(c);
    }
    (all, initials)
}

fn joined(mut spans: Spans) -> Spans {
    spans.sort_by_key(|span| span.start);
    let mut out: Spans = Vec::with_capacity(spans.len());
    for span in spans {
        match out.last_mut() {
            Some(last) if span.start <= last.end => last.end = last.end.max(span.end),
            _ => out.push(span),
        }
    }
    out
}

impl Needle {
    /// `text` as typed: in lowercase it ignores case, with a capital it matches case. Close
    /// matching needs `close`, and names in scripts without spelling, such as Chinese, Japanese
    /// and Korean, only match exactly.
    pub fn new(text: &str, close: bool) -> Self {
        let text = composed(text);
        let fold = !text.chars().any(char::is_uppercase);
        let chars: Vec<char> = text.chars().map(|c| if fold { fold_char(c) } else { c }).collect();
        let close = close.then(|| Close::new(&chars, fold)).flatten();
        Self { text: chars.iter().collect(), chars, fold, close }
    }

    /// Whether the word could match a name closely.
    pub fn can_be_close(&self) -> bool {
        self.close.is_some()
    }

    /// Whether `name` holds the word as typed.
    pub fn found_in(&self, name: &str) -> bool {
        if name.is_ascii() && self.text.is_ascii() {
            let needle = self.text.as_bytes();
            return needle.is_empty()
                || name
                    .as_bytes()
                    .windows(needle.len())
                    .any(|window| if self.fold { window.eq_ignore_ascii_case(needle) } else { window == needle });
        }
        self.exact(name).is_some()
    }

    /// Whether `name` holds the word closely, though not as typed.
    pub fn close_to(&self, name: &str) -> bool {
        let Some(close) = &self.close else { return false };
        if close.parts.is_empty() {
            if close.digits && close.letters.len() < SHUFFLED_FROM {
                return false;
            }
            let (all, initials) = masks(name);
            let absent = missing(close.mask, all);
            let refused = if close.digits {
                absent > 0
            } else {
                initials & close.leads == 0 || absent > typos(close.letters.len())
            };
            if refused {
                return false;
            }
        }
        close.find(&Name::new(name, self.fold)).is_some()
    }

    pub fn rank(&self, name: &str, close: bool) -> Option<MatchRank> {
        self.locate(name, close).map(|(rank, _)| rank)
    }

    /// How well the word matches `name`, and the bytes of `name` it matched.
    pub fn locate(&self, name: &str, close: bool) -> Option<(MatchRank, Spans)> {
        self.exact(name).or_else(|| if close { self.close.as_ref()?.find(&Name::new(name, self.fold)) } else { None })
    }

    fn exact(&self, name: &str) -> Option<(MatchRank, Spans)> {
        let text = composed(name);
        let chars: Vec<(usize, char)> = text.char_indices().collect();
        let width = self.chars.len();
        if width == 0 {
            return Some((MatchRank::EXACT, Vec::new()));
        }
        let normal = |c: char| if self.fold { fold_char(c) } else { c };
        let fits = |at: usize| self.chars.iter().zip(&chars[at..]).all(|(want, (_, have))| normal(*have) == *want);
        let found: Vec<usize> = (0..=chars.len().checked_sub(width)?).filter(|at| fits(*at)).collect();
        let first = *found.first()?;
        let stem = chars.iter().rposition(|(_, c)| *c == STEM_SEPARATOR).filter(|at| *at > 0).unwrap_or(chars.len());
        let starts = |at: usize| {
            let previous = at.checked_sub(1).map(|before| chars[before].1).filter(|c| c.is_alphanumeric());
            chars[at].1.is_alphanumeric() && starts_word(previous, chars[at].1, chars.get(at + 1).map(|(_, c)| *c))
        };
        let (tier, at) = match found.iter().copied().find(|at| starts(*at)) {
            _ if first == 0 && (width == chars.len() || width == stem) => (Tier::Exact, 0),
            _ if first == 0 => (Tier::Prefix, 0),
            Some(at) => (Tier::WordStart, at),
            None => (Tier::Inside, first),
        };
        let end = chars.get(at + width).map_or(text.len(), |(byte, _)| *byte);
        let spans = matches!(text, Cow::Borrowed(_)).then(|| chars[at].0..end).into_iter().collect();
        Some((MatchRank::of(tier), spans))
    }
}

impl Close {
    fn new(chars: &[char], fold: bool) -> Option<Self> {
        if chars.iter().any(|c| WITHOUT_TYPOS.iter().any(|range| range.contains(c))) {
            return None;
        }
        let mut letters = Vec::with_capacity(chars.len());
        for c in chars.iter().copied().filter(|c| c.is_alphanumeric()) {
            push_bare(c, fold, &mut letters);
        }
        (!letters.is_empty()).then(|| Self::of(letters, true))
    }

    fn of(letters: Vec<char>, split: bool) -> Self {
        let mask = letters.iter().fold(0, |mask, c| mask | bit(*c));
        let leads = letters.iter().take(2).fold(0, |mask, c| mask | bit(*c));
        let digits = letters.iter().all(char::is_ascii_digit);
        let mut parts = Vec::new();
        if split {
            let mut rest = letters.as_slice();
            while let Some(first) = rest.first() {
                let end = rest.iter().position(|c| c.is_ascii_digit() != first.is_ascii_digit()).unwrap_or(rest.len());
                parts.push(rest[..end].to_vec());
                rest = &rest[end..];
            }
        }
        let parts =
            if parts.len() > 1 { parts.into_iter().map(|part| Self::of(part, false)).collect() } else { Vec::new() };
        Self { letters, mask, leads, digits, parts }
    }

    fn find(&self, name: &Name) -> Option<(MatchRank, Spans)> {
        if !self.parts.is_empty() {
            return if self.letters.len() >= ONE_TYPO_FROM { self.by_parts(name) } else { None };
        }
        if self.digits {
            return if self.letters.len() >= SHUFFLED_FROM { self.shuffled(name) } else { None };
        }
        let absent = missing(self.mask, name.mask);
        let budget = typos(self.letters.len());
        if absent <= budget
            && let Some(found) = self.typo(name, budget)
        {
            return Some(found);
        }
        if self.letters.len() >= SCATTERED_FROM && absent == 0 {
            return self.scattered(name);
        }
        None
    }

    fn by_parts(&self, name: &Name) -> Option<(MatchRank, Spans)> {
        let mut worst = MatchRank::of(Tier::Typo);
        let mut spans = Vec::new();
        for part in &self.parts {
            let (rank, found) = part.within(name).or_else(|| part.find(name))?;
            worst = worst.max(rank);
            spans.extend(found);
        }
        Some((worst, joined(spans)))
    }

    fn within(&self, name: &Name) -> Option<(MatchRank, Spans)> {
        let width = self.letters.len();
        let at = (0..=name.letters.len().checked_sub(width)?)
            .find(|at| name.starts[*at] && name.letters[*at..*at + width] == *self.letters)?;
        Some((MatchRank::of(Tier::Typo), vec![name.span(at, width)]))
    }

    fn shuffled(&self, name: &Name) -> Option<(MatchRank, Spans)> {
        let count = |digits: &[char]| {
            let mut counts = [0u8; DECIMAL as usize];
            for digit in digits.iter().filter_map(|c| c.to_digit(DECIMAL)) {
                counts[digit as usize] = counts[digit as usize].saturating_add(1);
            }
            counts
        };
        let wanted = count(&self.letters);
        let mut at = 0;
        while at < name.letters.len() {
            if !name.letters[at].is_ascii_digit() {
                at += 1;
                continue;
            }
            let end = (at + 1..name.letters.len())
                .find(|i| !name.letters[*i].is_ascii_digit() || name.starts[*i])
                .unwrap_or(name.letters.len());
            if end - at == self.letters.len() && count(&name.letters[at..end]) == wanted {
                return Some((MatchRank::of(Tier::Shuffled), vec![name.span(at, end - at)]));
            }
            at = end;
        }
        None
    }

    fn typo(&self, name: &Name, budget: u8) -> Option<(MatchRank, Spans)> {
        let typed = &self.letters;
        let width = typed.len();
        let cost = |c: char| if c.is_ascii_digit() { UNREACHABLE } else { LETTER_COST };
        let room = width + usize::from(budget);
        let columns = room + 1;
        let mut rows = vec![UNREACHABLE; (width + 1) * columns];
        let mut best: Option<(u8, bool, usize, usize)> = None;
        for start in (0..name.letters.len()).filter(|start| name.starts[*start]) {
            let text = &name.letters[start..];
            let swapped = width > 1 && text.len() > 1 && text[0] == typed[1] && text[1] == typed[0];
            if text[0] != typed[0] && !swapped {
                continue;
            }
            let span = text.len().min(room);
            rows.fill(UNREACHABLE);
            rows[0] = 0;
            for j in 1..=span {
                rows[j] = rows[j - 1].saturating_add(cost(text[j - 1])).min(UNREACHABLE);
            }
            for i in 1..=width {
                let row = i * columns;
                rows[row] = rows[row - columns].saturating_add(cost(typed[i - 1])).min(UNREACHABLE);
                let mut lowest = rows[row];
                for j in 1..=span {
                    let (a, b) = (typed[i - 1], text[j - 1]);
                    let change = if a == b {
                        0
                    } else if a.is_ascii_digit() || b.is_ascii_digit() {
                        UNREACHABLE
                    } else {
                        LETTER_COST
                    };
                    let mut cell = rows[row - columns + j - 1]
                        .saturating_add(change)
                        .min(rows[row - columns + j].saturating_add(cost(a)))
                        .min(rows[row + j - 1].saturating_add(cost(b)));
                    if i > 1 && j > 1 && a == text[j - 2] && typed[i - 2] == b && a != b {
                        cell = cell.min(rows[row - 2 * columns + j - 2].saturating_add(SWAP_COST));
                    }
                    rows[row + j] = cell.min(UNREACHABLE);
                    lowest = lowest.min(rows[row + j]);
                }
                if lowest > budget {
                    break;
                }
            }
            for j in 1..=span {
                let partial = !name.ends_word(start + j);
                let edits = rows[width * columns + j];
                let charge = if partial && width < WHOLE_WORD_FREE_FROM { PART_OF_WORD_COST } else { 0 };
                if edits.saturating_add(charge) > budget {
                    continue;
                }
                if best.is_none_or(|(e, p, _, _)| (edits, partial) < (e, p)) {
                    best = Some((edits, partial, start, j));
                }
            }
        }
        let (edits, partial, start, length) = best?;
        Some((MatchRank { tier: Tier::Typo, edits, partial }, vec![name.span(start, length)]))
    }

    fn scattered(&self, name: &Name) -> Option<(MatchRank, Spans)> {
        let typed = &self.letters;
        let text = &name.letters;
        let words = name.words();
        let mut from: Vec<Vec<Option<usize>>> = vec![vec![None; text.len()]; typed.len()];
        let mut reached = vec![vec![false; text.len()]; typed.len()];
        for at in 0..text.len() {
            reached[0][at] = name.starts[at] && text[at] == typed[0];
        }
        for i in 1..typed.len() {
            let mut last = None;
            for at in 0..text.len() {
                if let Some(before) = last
                    && text[at] == typed[i]
                    && (name.starts[at] || words[before] == words[at])
                {
                    reached[i][at] = true;
                    from[i][at] = Some(before);
                }
                if reached[i - 1][at] {
                    last = Some(at);
                }
            }
        }
        let mut at = (0..text.len()).find(|at| reached[typed.len() - 1][*at])?;
        let mut spans = Vec::with_capacity(typed.len());
        let mut touched = BTreeSet::new();
        for i in (0..typed.len()).rev() {
            spans.push(name.span(at, 1));
            touched.insert(words[at]);
            if i > 0 {
                at = from[i][at]?;
            }
        }
        let covered: usize = touched.iter().map(|word| name.word_length(*word)).sum();
        if typed.len() * PERCENT < covered * SCATTERED_COVER_PERCENT {
            return None;
        }
        Some((MatchRank::of(Tier::Scattered), joined(spans)))
    }
}

impl Name {
    fn new(name: &str, fold: bool) -> Self {
        let chars: Vec<(usize, char)> = name.char_indices().collect();
        let mut letters = Vec::with_capacity(chars.len());
        let mut starts = Vec::with_capacity(chars.len());
        let mut origin: Vec<Range<usize>> = Vec::with_capacity(chars.len());
        let mut previous: Option<char> = None;
        for (i, &(at, c)) in chars.iter().enumerate() {
            if marked(c) {
                if previous.is_some()
                    && let Some(last) = origin.last_mut()
                {
                    last.end = at + c.len_utf8();
                }
                continue;
            }
            if !c.is_alphanumeric() {
                previous = None;
                continue;
            }
            let next = chars[i + 1..].iter().map(|(_, c)| *c).find(|c| !marked(*c));
            let start = starts_word(previous, c, next);
            let before = letters.len();
            push_bare(c, fold, &mut letters);
            for added in before..letters.len() {
                starts.push(start && added == before);
                origin.push(at..at + c.len_utf8());
            }
            previous = Some(c);
        }
        let mask = letters.iter().fold(0, |mask, c| mask | bit(*c));
        Self { letters, starts, origin, mask }
    }

    fn ends_word(&self, at: usize) -> bool {
        at == self.letters.len() || self.starts[at]
    }

    fn span(&self, at: usize, length: usize) -> Range<usize> {
        self.origin[at].start..self.origin[at + length - 1].end
    }

    fn words(&self) -> Vec<usize> {
        let mut word = 0;
        (0..self.letters.len())
            .map(|at| {
                if self.starts[at] {
                    word = at;
                }
                word
            })
            .collect()
    }

    fn word_length(&self, start: usize) -> usize {
        (start + 1..self.letters.len()).find(|at| self.starts[*at]).unwrap_or(self.letters.len()) - start
    }
}

#[cfg(test)]
mod tests;
