//! Searching a process's memory, and ranking the strings it holds.

use super::{MemoryRegion, ProcessKey};
use borg_i18n::tr;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemorySearchMode {
    Text,
    Hex,
    Unicode,
    /// Every string of printable characters, or those containing the query.
    Strings,
    /// Strings that look like an indicator, such as a URL, a command line or a credential.
    Indicators,
}

impl MemorySearchMode {
    pub const ALL: [Self; 5] = [Self::Text, Self::Hex, Self::Unicode, Self::Strings, Self::Indicators];
    /// Whether the mode lists strings, which needs no query.
    pub fn strings(self) -> bool {
        matches!(self, Self::Strings | Self::Indicators)
    }
}

/// What a string in memory looks like it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Indicator {
    Url,
    Ip,
    Email,
    Path,
    Command,
    /// A private key, an access token, or a password assignment.
    Secret,
    /// A long run of base64.
    Encoded,
}

impl Indicator {
    pub fn label(self) -> &'static str {
        match self {
            Self::Url => tr!(indicator_url),
            Self::Ip => tr!(indicator_ip),
            Self::Email => tr!(indicator_email),
            Self::Path => tr!(indicator_path),
            Self::Command => tr!(indicator_command),
            Self::Secret => tr!(indicator_secret),
            Self::Encoded => tr!(indicator_encoded),
        }
    }
    /// Kinds in the order strings list them: what most often means trouble first.
    pub const RANKED: [Self; 7] =
        [Self::Secret, Self::Command, Self::Url, Self::Ip, Self::Email, Self::Path, Self::Encoded];
    pub(super) fn rank(self) -> usize {
        Self::RANKED.iter().position(|&kind| kind == self).unwrap_or(Self::RANKED.len())
    }
}

/// How many places of a string a result keeps, so its copies can be visited in turn.
pub const STRING_PLACES: usize = 16;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryHit {
    pub address: u64,
    pub length: usize,
    pub encoding: String,
    pub region: MemoryRegion,
    /// For a string, its first characters.
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub indicator: Option<Indicator>,
    /// For a string, how many times memory holds it.
    #[serde(default)]
    pub copies: u64,
    /// For a string, where its first copies are: an address and the index of its region in
    /// the report's strings tally.
    #[serde(default)]
    pub places: Vec<(u64, u32)>,
    /// For a string, whether a file mapped into the process holds it too; `None` when no file
    /// could be compared.
    #[serde(default)]
    pub in_file: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemorySearchReport {
    pub process: ProcessKey,
    pub hits: Vec<MemoryHit>,
    pub scanned: u64,
    pub total: u64,
    pub skipped: u64,
    pub next: Option<u64>,
    pub cancelled: bool,
    /// What a search for strings found in all, beyond the results it lists.
    #[serde(default)]
    pub strings: Option<StringsTally>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StringsTally {
    /// Strings found, every copy counted.
    pub found: u64,
    /// Different strings found.
    pub distinct: u64,
    /// Different strings of each kind that looks like an indicator.
    pub kinds: Vec<(Indicator, u64)>,
    /// Different strings no compared file holds.
    pub memory_only: u64,
    /// Files mapped into the process whose strings were compared.
    pub files: u64,
    /// The regions the results' places name.
    pub regions: Vec<MemoryRegion>,
}

/// How much a string reads like words, from 0 to 100: the share of its characters in words of
/// three or more letters with a vowel, where a word is lower case, Capitalized or in capitals,
/// and a capital starts a new word inside camelCase.
pub fn string_score(text: &str) -> u8 {
    let total = text.chars().count().max(1);
    let mut wordy = text.chars().filter(|&c| c == ' ').count();
    let mut word = String::new();
    let mut close = |word: &mut String| {
        let rest = word.chars().skip(1);
        let cased = word.chars().all(|c| c.is_ascii_uppercase()) || rest.clone().all(|c| c.is_ascii_lowercase());
        if word.len() >= 3 && cased && word.chars().any(|c| "aeiouyAEIOUY".contains(c)) {
            wordy += word.len();
        }
        word.clear();
    };
    let mut previous = ' ';
    for c in text.chars() {
        if !c.is_ascii_alphabetic() || (c.is_ascii_uppercase() && previous.is_ascii_lowercase()) {
            close(&mut word);
        }
        if c.is_ascii_alphabetic() {
            word.push(c);
        }
        previous = c;
    }
    close(&mut word);
    (wordy * 100 / total).min(100) as u8
}

/// Whether a string is filler that happens to be printable: one character most of the way,
/// hardly any different characters, or mostly punctuation.
pub fn junk_string(text: &str) -> bool {
    // Counted by character; anything beyond ASCII shares the last count.
    let mut counts = [0usize; 129];
    let (mut total, mut letters) = (0, 0);
    for c in text.chars() {
        counts[(c as usize).min(128)] += 1;
        total += 1;
        letters += usize::from(c.is_ascii_alphanumeric());
    }
    let distinct = counts.iter().filter(|&&count| count > 0).count();
    let most = counts.iter().copied().max().unwrap_or(0);
    distinct < 4 || most * 2 > total || letters * 2 < total
}

/// Orders strings for reading: those that look like an indicator first, the ones only memory
/// holds ahead of those a file holds too, then strings that read like words, longer first.
pub fn rank_strings(hits: &mut [MemoryHit]) {
    hits.sort_by_cached_key(|hit| {
        let text = hit.text.as_deref().unwrap_or_default();
        (
            hit.indicator.is_none(),
            hit.in_file == Some(true),
            hit.indicator.map_or(0, Indicator::rank),
            std::cmp::Reverse(string_score(text) / 10),
            std::cmp::Reverse(text.chars().count().min(80)),
            hit.address,
        )
    });
}

/// Adds the strings a continued search found to those listed before: the same string counts
/// its copies together, and the tally adds up.
pub(super) fn merge_strings(previous: &MemorySearchReport, report: &mut MemorySearchReport) {
    let (Some(old), Some(new)) = (&previous.strings, report.strings.as_mut()) else { return };
    let shift = old.regions.len() as u32;
    let mut hits = previous.hits.clone();
    let mut index: HashMap<String, usize> =
        hits.iter().enumerate().filter_map(|(i, hit)| Some((hit.text.clone()?, i))).collect();
    let mut repeated = 0;
    for mut hit in std::mem::take(&mut report.hits) {
        hit.places.iter_mut().for_each(|(_, region)| *region += shift);
        match hit.text.as_ref().and_then(|text| index.get(text)) {
            Some(&i) => {
                repeated += 1;
                let kept = &mut hits[i];
                kept.copies += hit.copies;
                let room = STRING_PLACES.saturating_sub(kept.places.len());
                kept.places.extend(hit.places.into_iter().take(room));
                kept.in_file = kept.in_file.max(hit.in_file);
            }
            None => {
                if let Some(text) = hit.text.clone() {
                    index.insert(text, hits.len());
                }
                hits.push(hit);
            }
        }
    }
    rank_strings(&mut hits);
    report.hits = hits;
    new.found += old.found;
    new.distinct = (new.distinct + old.distinct).saturating_sub(repeated);
    new.memory_only += old.memory_only;
    new.files = new.files.max(old.files);
    for &(kind, count) in &old.kinds {
        match new.kinds.iter_mut().find(|(k, _)| *k == kind) {
            Some((_, total)) => *total += count,
            None => new.kinds.push((kind, count)),
        }
    }
    new.kinds.sort_by_key(|(kind, _)| kind.rank());
    new.regions = old.regions.iter().cloned().chain(std::mem::take(&mut new.regions)).collect();
}

#[derive(Debug, Clone)]
pub struct MemorySearchForm {
    pub input: crate::LineInput,
    pub mode: MemorySearchMode,
    pub match_case: bool,
    pub snapshot_only: bool,
    pub selected: usize,
}
