use borg_i18n::tr;
use std::{
    borrow::Cow,
    path::{Component, Path},
    time::{Duration, SystemTime},
};

use borg_vfs::{Entry, EntryKind};

use crate::Pattern;

const GLOB_CHARS: [char; 3] = ['*', '?', '['];
const OR: &str = "|";
const HOUR: u64 = 60 * 60;
const DAY: u64 = 24 * HOUR;

/// The recursive search's query: words that must all match, `|` between alternatives,
/// quoted phrases, globs, paths, and filters such as `ext:rs`, `size:>10M` or `mod:<7d`.
/// A word in lowercase ignores case; one with an uppercase letter matches it exactly.
#[derive(Debug, Clone)]
pub struct SearchQuery {
    alternatives: Vec<Vec<Condition>>,
    /// `hidden:yes` or `hidden:no`, over the search window's setting.
    pub hidden: Option<bool>,
    /// `excluded:yes` also searches artifact folders and ignored files; `excluded:no` skips them.
    pub excluded: Option<bool>,
}

/// What a query looks at: an entry from a listing, or a row of the search index.
pub trait Candidate {
    fn name(&self) -> Cow<'_, str>;
    fn kind(&self) -> EntryKind;
    fn size(&self) -> u64;
    fn modified(&self) -> Option<SystemTime>;
    /// The full path, built only for the terms that read it.
    fn path(&self) -> Cow<'_, Path>;
}

impl Candidate for (&Entry, &Path) {
    fn name(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.0.name)
    }
    fn kind(&self) -> EntryKind {
        self.0.kind
    }
    fn size(&self) -> u64 {
        self.0.size
    }
    fn modified(&self) -> Option<SystemTime> {
        self.0.modified
    }
    fn path(&self) -> Cow<'_, Path> {
        Cow::Borrowed(self.1)
    }
}

#[derive(Debug, Clone)]
struct Condition {
    negated: bool,
    term: Term,
}

#[derive(Debug, Clone)]
enum Term {
    Name(Text),
    Path(Text),
    Glob(Pattern),
    Ext(Vec<String>),
    Kind(Kind),
    Size { min: u64, max: u64 },
    Age { min: Duration, max: Duration },
    In(Vec<Text>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    File,
    Dir,
    Link,
}

#[derive(Debug, Clone)]
struct Text {
    text: String,
    fold: bool,
}

impl Text {
    fn new(text: &str) -> Self {
        let fold = !text.chars().any(char::is_uppercase);
        Self { text: if fold { text.to_lowercase() } else { text.into() }, fold }
    }

    fn found_in(&self, haystack: &str) -> bool {
        if !self.fold {
            return haystack.contains(&self.text);
        }
        // Most names are ASCII: compare without lowercasing a copy of each.
        if haystack.is_ascii() && self.text.is_ascii() {
            let needle = self.text.as_bytes();
            return needle.is_empty()
                || haystack.as_bytes().windows(needle.len()).any(|window| window.eq_ignore_ascii_case(needle));
        }
        haystack.to_lowercase().contains(&self.text)
    }

    fn is(&self, value: &str) -> bool {
        if self.fold { value.to_lowercase() == self.text } else { value == self.text }
    }
}

/// Why a query could not be read, with the word it stopped at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryError {
    pub word: String,
    pub problem: Problem,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Problem {
    Quote,
    Size,
    Age,
    Kind,
    Switch,
    Glob,
    Alternative,
}

impl QueryError {
    pub fn message(&self) -> String {
        let word = &self.word;
        match self.problem {
            Problem::Quote => tr!(query_error_quote, word = word),
            Problem::Size => tr!(query_error_size, word = word),
            Problem::Age => tr!(query_error_age, word = word),
            Problem::Kind => tr!(query_error_kind, word = word),
            Problem::Switch => tr!(query_error_switch, word = word),
            Problem::Glob => tr!(query_error_glob, word = word),
            Problem::Alternative => tr!(query_error_alternative, word = word),
        }
    }
}

struct Word {
    text: String,
    /// Some of it was in quotes, so glob characters in it are literal.
    quoted: bool,
    /// It began with a quote: a literal name, never a filter or a negation.
    literal: bool,
}

fn words(text: &str) -> Result<Vec<Word>, QueryError> {
    let mut words = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
            continue;
        }
        let mut word = String::new();
        let mut quoted = false;
        let literal = c == '"';
        while let Some(&c) = chars.peek() {
            if c.is_whitespace() {
                break;
            }
            chars.next();
            if c != '"' {
                word.push(c);
                continue;
            }
            quoted = true;
            let mut closed = false;
            for c in chars.by_ref() {
                if c == '"' {
                    closed = true;
                    break;
                }
                word.push(c);
            }
            if !closed {
                return Err(QueryError { word: format!("\"{word}"), problem: Problem::Quote });
            }
        }
        words.push(Word { text: word, quoted, literal });
    }
    Ok(words)
}

impl SearchQuery {
    /// An empty query, or `*`, matches everything.
    pub fn parse(text: &str) -> Result<Self, QueryError> {
        let mut query = Self { alternatives: vec![Vec::new()], hidden: None, excluded: None };
        let mut last = String::new();
        for word in words(text)? {
            if !word.literal && word.text == OR {
                if query.alternatives.last().is_some_and(Vec::is_empty) {
                    return Err(QueryError { word: OR.into(), problem: Problem::Alternative });
                }
                query.alternatives.push(Vec::new());
                continue;
            }
            last.clone_from(&word.text);
            let (negated, body) = match word.text.strip_prefix('-') {
                Some(rest) if !rest.is_empty() && !word.literal => (true, rest),
                _ => (false, word.text.as_str()),
            };
            let error = |problem| QueryError { word: word.text.clone(), problem };
            let term = if word.literal {
                Some(Term::Name(Text::new(body)))
            } else {
                match body.split_once(':') {
                    Some(("hidden", value)) => {
                        query.hidden = Some(switch(value).ok_or_else(|| error(Problem::Switch))? != negated);
                        None
                    }
                    Some(("excluded", value)) => {
                        query.excluded = Some(switch(value).ok_or_else(|| error(Problem::Switch))? != negated);
                        None
                    }
                    Some(("ext", value)) => Some(Term::Ext(
                        value
                            .split(',')
                            .map(|ext| ext.trim_start_matches('.').to_lowercase())
                            .filter(|ext| !ext.is_empty())
                            .collect(),
                    )),
                    Some(("type", value)) => Some(Term::Kind(kind(value).ok_or_else(|| error(Problem::Kind))?)),
                    Some(("size", value)) => {
                        let (min, max) =
                            bounds(value, crate::discovery::parse_size).ok_or_else(|| error(Problem::Size))?;
                        Some(Term::Size { min, max })
                    }
                    Some(("mod", value)) => {
                        let (min, max) = bounds(value, age).ok_or_else(|| error(Problem::Age))?;
                        Some(Term::Age { min: Duration::from_secs(min), max: Duration::from_secs(max) })
                    }
                    Some(("in", value)) if !value.is_empty() => {
                        Some(Term::In(value.split('/').filter(|part| !part.is_empty()).map(Text::new).collect()))
                    }
                    _ => Some(plain(body, word.quoted).map_err(|()| error(Problem::Glob))?),
                }
            };
            if let (Some(term), Some(all)) = (term, query.alternatives.last_mut()) {
                all.push(Condition { negated, term });
            }
        }
        if query.alternatives.len() > 1 && query.alternatives.last().is_some_and(Vec::is_empty) {
            return Err(QueryError { word: last, problem: Problem::Alternative });
        }
        Ok(query)
    }

    /// Whether matching reads sizes or modification times, which a directory listing does not
    /// give without reading each entry's metadata.
    pub fn needs_metadata(&self) -> bool {
        self.alternatives.iter().flatten().any(|c| matches!(c.term, Term::Size { .. } | Term::Age { .. }))
    }

    pub fn accepts(&self, candidate: &impl Candidate, now: SystemTime) -> bool {
        self.alternatives.iter().any(|all| all.iter().all(|c| c.term.matches(candidate, now) != c.negated))
    }

    /// The first word to look for, for ranking: names that are it, then those starting with it.
    pub fn primary(&self) -> Option<&str> {
        self.alternatives.first()?.iter().find_map(|c| match &c.term {
            Term::Name(text) if !c.negated => Some(text.text.as_str()),
            _ => None,
        })
    }
}

/// A word without a filter: a glob, a path when it has a `/` inside, or a name fragment.
fn plain(text: &str, quoted: bool) -> Result<Term, ()> {
    if text == "*" {
        return Ok(Term::Glob(Pattern::parse("*").map_err(|_| ())?));
    }
    if !quoted && text.contains(GLOB_CHARS) {
        return Pattern::parse(text).map(Term::Glob).map_err(|_| ());
    }
    let inner = text.trim_end_matches('/');
    if inner.contains('/') {
        return Ok(Term::Path(Text::new(text)));
    }
    if inner.len() < text.len() && !quoted {
        return Pattern::parse(&format!("*{inner}*/")).map(Term::Glob).map_err(|_| ());
    }
    Ok(Term::Name(Text::new(text)))
}

fn switch(value: &str) -> Option<bool> {
    match value {
        "yes" | "y" | "true" | "on" => Some(true),
        "no" | "n" | "false" | "off" => Some(false),
        _ => None,
    }
}

fn kind(value: &str) -> Option<Kind> {
    match value {
        "file" | "f" => Some(Kind::File),
        "dir" | "d" | "folder" => Some(Kind::Dir),
        "link" | "l" | "symlink" => Some(Kind::Link),
        _ => None,
    }
}

/// `>N`, `>=N`, `<N`, `<=N`, `A..B`, or exactly `N`, as inclusive bounds.
fn bounds(value: &str, number: impl Fn(&str) -> Option<u64>) -> Option<(u64, u64)> {
    let parse = |text: &str| number(text).filter(|_| !text.trim().is_empty());
    if let Some((low, high)) = value.split_once("..") {
        let (low, high) = (parse(low)?, parse(high)?);
        return (low <= high).then_some((low, high));
    }
    if let Some(rest) = value.strip_prefix(">=") {
        return Some((parse(rest)?, u64::MAX));
    }
    if let Some(rest) = value.strip_prefix("<=") {
        return Some((0, parse(rest)?));
    }
    if let Some(rest) = value.strip_prefix('>') {
        return Some((parse(rest)?.checked_add(1)?, u64::MAX));
    }
    if let Some(rest) = value.strip_prefix('<') {
        return Some((0, parse(rest)?.checked_sub(1)?));
    }
    let exact = parse(value)?;
    Some((exact, exact))
}

/// An age such as `30min`, `12h`, `7d`, `2w`, `6mo` or `1y`, in seconds.
fn age(text: &str) -> Option<u64> {
    let text = text.trim();
    let split = text.find(|c: char| !c.is_ascii_digit())?;
    let number: u64 = text[..split].parse().ok()?;
    let unit = match &text[split..] {
        "min" => 60,
        "h" => HOUR,
        "d" => DAY,
        "w" => 7 * DAY,
        "mo" => 30 * DAY,
        "y" => 365 * DAY,
        _ => return None,
    };
    number.checked_mul(unit)
}

impl Term {
    fn matches(&self, candidate: &impl Candidate, now: SystemTime) -> bool {
        let kind = candidate.kind();
        match self {
            Self::Name(text) => text.found_in(&candidate.name()),
            Self::Path(text) => text.found_in(&candidate.path().to_string_lossy()),
            Self::Glob(pattern) => pattern.matches_name(&candidate.name(), kind.is_dir()),
            Self::Ext(exts) => {
                let name = candidate.name();
                !kind.is_dir()
                    && exts.iter().any(|ext| {
                        name.len() > ext.len() + 1
                            && name.is_char_boundary(name.len() - ext.len() - 1)
                            && name[name.len() - ext.len()..].eq_ignore_ascii_case(ext)
                            && name.as_bytes()[name.len() - ext.len() - 1] == b'.'
                    })
            }
            Self::Kind(Kind::File) => kind == EntryKind::File,
            Self::Kind(Kind::Dir) => kind == EntryKind::Dir,
            Self::Kind(Kind::Link) => matches!(kind, EntryKind::Symlink { .. }),
            Self::Size { min, max } => !kind.is_dir() && (*min..=*max).contains(&candidate.size()),
            Self::Age { min, max } => candidate
                .modified()
                .and_then(|modified| now.duration_since(modified).ok())
                .is_some_and(|age| (*min..=*max).contains(&age)),
            Self::In(parts) => {
                let path = candidate.path();
                let folders: Vec<String> = path
                    .parent()
                    .into_iter()
                    .flat_map(Path::components)
                    .filter_map(|c| match c {
                        Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
                        _ => None,
                    })
                    .collect();
                !parts.is_empty()
                    && folders
                        .windows(parts.len())
                        .any(|window| window.iter().zip(parts).all(|(folder, part)| part.is(folder)))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const NOW: Duration = Duration::from_secs(1_800_000_000);

    fn entry(path: &str, kind: EntryKind, size: u64, age_days: u64) -> (Entry, PathBuf) {
        let path = PathBuf::from(path);
        let modified = SystemTime::UNIX_EPOCH + NOW - Duration::from_secs(age_days * DAY);
        let entry = Entry {
            source: Some(path.clone()),
            missing: false,
            usage: None,
            name: path.file_name().unwrap().to_string_lossy().into_owned(),
            kind,
            size,
            modified: Some(modified),
            executable: false,
            permissions: 0o644,
        };
        (entry, path)
    }

    fn matches(query: &str, path: &str, kind: EntryKind, size: u64, age_days: u64) -> bool {
        let (entry, path) = entry(path, kind, size, age_days);
        SearchQuery::parse(query).unwrap().accepts(&(&entry, path.as_path()), SystemTime::UNIX_EPOCH + NOW)
    }

    fn file(query: &str, path: &str) -> bool {
        matches(query, path, EntryKind::File, 100, 1)
    }

    #[test]
    fn words_must_all_match_and_bars_separate_alternatives() {
        assert!(file("main rs", "/src/main.rs"));
        assert!(!file("main toml", "/src/main.rs"));
        assert!(file("*.toml | *.rs", "/src/main.rs"));
        assert!(file("", "/src/main.rs") && file("*", "/src/main.rs"));
        assert!(file("\"my notes\"", "/home/my notes.txt") && !file("my notes", "/home/notes.txt"));
        assert!(file("\"-v\"", "/bin/x-v") && !file("-\"v\"", "/bin/v1"), "a leading quote keeps the dash");
        assert!(file("\"ext:rs\"", "/a ext:rs b") && file("\"*.rs\"", "/odd *.rs name"));
        assert!(file("in:\"My Folder\"", "/Users/me/My Folder/a.txt"));
    }

    #[test]
    fn lowercase_ignores_case_and_uppercase_matches_it() {
        assert!(file("readme", "/README.md"));
        assert!(file("README", "/README.md") && !file("README", "/readme.md"));
    }

    #[test]
    fn filters_cover_extensions_kinds_sizes_ages_and_folders() {
        assert!(file("ext:rs,toml", "/src/main.rs") && file("ext:.TOML", "/Cargo.toml"));
        assert!(file("ext:tar.gz", "/a.tar.gz") && file("ext:gz", "/a.tar.gz") && !file("ext:gz", "/.gz"));
        assert!(matches("type:dir", "/src", EntryKind::Dir, 0, 1) && !file("type:dir", "/src"));
        assert!(matches("size:>1M", "/big.iso", EntryKind::File, 2 << 20, 1));
        assert!(!matches("size:>1M", "/big", EntryKind::Dir, 2 << 20, 1), "folders have no size of their own");
        assert!(matches("size:0", "/empty", EntryKind::File, 0, 1) && file("size:1..200", "/a"));
        assert!(
            matches("mod:<7d", "/new", EntryKind::File, 1, 2) && !matches("mod:<7d", "/old", EntryKind::File, 1, 9)
        );
        assert!(matches("mod:>1y", "/ancient", EntryKind::File, 1, 400));
        assert!(file("in:src", "/work/src/lib/a.rs") && !file("in:src", "/work/source/a.rs"));
        assert!(file("in:src/lib", "/work/src/lib/a.rs") && !file("in:lib/src", "/work/src/lib/a.rs"));
        assert!(file("src/lib", "/work/src/lib/a.rs"), "a slash inside a word matches the path");
        assert!(matches("build/", "/work/build", EntryKind::Dir, 0, 1) && !file("build/", "/work/build"));
    }

    #[test]
    fn a_dash_negates_and_switches_set_hidden_and_excluded_searching() {
        assert!(!file("-test", "/src/test_main.rs") && file("-test", "/src/main.rs"));
        assert!(!file("rs -ext:rs", "/main.rs"));
        let query = SearchQuery::parse("hidden:yes excluded:no notes").unwrap();
        assert_eq!((query.hidden, query.excluded), (Some(true), Some(false)));
        assert!(!query.needs_metadata() && SearchQuery::parse("size:>1k").unwrap().needs_metadata());
        assert!(file("weird:colon", "/a weird:colon b"), "unknown prefixes are plain words");
    }

    #[test]
    fn mistakes_name_the_word_and_what_is_wrong() {
        let problem = |query: &str| SearchQuery::parse(query).map(|_| ()).map_err(|error| (error.word, error.problem));
        assert_eq!(problem("size:big"), Err(("size:big".into(), Problem::Size)));
        assert_eq!(problem("a mod:7"), Err(("mod:7".into(), Problem::Age)));
        assert_eq!(problem("type:pipe"), Err(("type:pipe".into(), Problem::Kind)));
        assert_eq!(problem("hidden:maybe"), Err(("hidden:maybe".into(), Problem::Switch)));
        assert_eq!(problem("\"open"), Err(("\"open".into(), Problem::Quote)));
        assert_eq!(problem("a |").map_err(|e| e.1), Err(Problem::Alternative));
        assert_eq!(problem("| a").map_err(|e| e.1), Err(Problem::Alternative));
        assert_eq!(problem("size:5..1").map_err(|e| e.1), Err(Problem::Size));
    }
}
