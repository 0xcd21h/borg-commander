//! The path typed at the cursor: the folder it points into, the part of a name typed there, and
//! the keys that finish a subfolder's name the way the shell reads it.

use std::path::{Component, Path, PathBuf};

use borg_term::TypedLine;

use super::QUOTES;

const SEPARATOR: char = '/';
const HOME: &str = "~";
const HOME_PREFIX: &str = "~/";
const ROOT: &str = "/";
const HIDDEN: char = '.';
const ESCAPE: char = '\\';
const DOUBLE_QUOTE: char = '"';
const SINGLE_QUOTE: char = '\'';
/// What ends a word on the command line besides blanks.
const WORD_BREAKS: [char; 7] = [';', '&', '|', '<', '>', '(', ')'];
/// What a bare word escapes so the shell reads it as part of a name.
const SPECIAL: &str = " \t\\'\"$`!&;|<>()*?[]{}#";
/// What a word in double quotes escapes.
const SPECIAL_IN_DOUBLE_QUOTES: &str = "\\\"$`";
/// A single quote inside single quotes: close them, an escaped quote, open them again.
const SINGLE_QUOTE_INSIDE: &str = "'\\''";
/// The key that erases the character before the cursor.
const ERASE: u8 = 0x7f;
const CURRENT: &str = ".";
const PARENT: &str = "..";

/// The word the cursor ends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Word {
    /// The word as the shell reads it, without quotes and escapes.
    pub(crate) text: String,
    /// The characters typed after the word's last `/`, quotes and escapes included.
    typed_name: usize,
    /// The quote open where the name after the last `/` starts.
    name_quote: Option<char>,
    /// The quote open at the cursor.
    quote: Option<char>,
}

impl Word {
    /// Whether the word is a path to a folder: it holds a `/` or is the home folder.
    pub(crate) fn is_path(&self) -> bool {
        self.text.contains(SEPARATOR) || self.text == HOME
    }
}

/// The word before the cursor, or `None` while the cursor is inside a word.
pub(crate) fn word_at_cursor(line: &TypedLine) -> Option<Word> {
    if line.after_cursor().starts_with(|c: char| !c.is_whitespace()) {
        return None;
    }
    let fresh = Word { text: String::new(), typed_name: 0, name_quote: None, quote: None };
    let mut word = fresh.clone();
    let mut escaped = false;
    for c in line.before_cursor().chars() {
        word.typed_name += 1;
        if escaped {
            escaped = false;
            if word.quote == Some(DOUBLE_QUOTE) && !SPECIAL_IN_DOUBLE_QUOTES.contains(c) {
                word.text.push(ESCAPE);
            }
            word.text.push(c);
        } else if word.quote == Some(c) {
            word.quote = None;
        } else if c == ESCAPE && word.quote != Some(SINGLE_QUOTE) {
            escaped = true;
        } else if word.quote.is_some() {
            word.text.push(c);
        } else if QUOTES.contains(&c) {
            word.quote = Some(c);
        } else if c.is_whitespace() || WORD_BREAKS.contains(&c) {
            word = fresh.clone();
        } else {
            word.text.push(c);
        }
        if c == SEPARATOR {
            word.typed_name = 0;
            word.name_quote = word.quote;
        }
    }
    Some(word)
}

/// The folder a word points into and the part of a subfolder's name typed there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Target {
    pub(crate) dir: PathBuf,
    pub(crate) name: String,
    /// The word names `dir` itself without the `/` after it.
    inside: bool,
}

/// Where `word` points: a relative word starts at `base` and `~` is `home`. A word that names a
/// subfolder `holds` knows of points inside it.
pub(crate) fn target(
    word: &str,
    base: &Path,
    home: Option<&Path>,
    holds: impl Fn(&Path, &str) -> bool,
) -> Option<Target> {
    let (dir, name) = match word.rsplit_once(SEPARATOR) {
        Some((dir, name)) => (resolve(if dir.is_empty() { ROOT } else { dir }, base, home)?, name),
        None if word == HOME => (home?.to_path_buf(), CURRENT),
        None => (base.to_path_buf(), word),
    };
    let inside = name == CURRENT || name == PARENT || (!name.is_empty() && holds(&dir, name));
    Some(if inside {
        Target { dir: normal(&dir.join(name)), name: String::new(), inside: true }
    } else {
        Target { dir, name: name.to_owned(), inside: false }
    })
}

fn resolve(text: &str, base: &Path, home: Option<&Path>) -> Option<PathBuf> {
    let path = if text == HOME {
        home?.to_path_buf()
    } else if let Some(rest) = text.strip_prefix(HOME_PREFIX) {
        home?.join(rest)
    } else if text.starts_with(HOME) {
        return None;
    } else {
        base.join(text)
    };
    Some(normal(&path))
}

/// `path` without `.` and with each `..` taking off the folder before it, as `cd` reads it.
fn normal(path: &Path) -> PathBuf {
    let mut normal = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normal.pop();
            }
            other => normal.push(other),
        }
    }
    normal
}

/// The names in `names` that hold `typed`, ignoring case: those that start with it first, each
/// group in alphabetical order. Hidden folders show once `typed` starts with a `.`, or when
/// `hidden` asks for them.
pub(crate) fn matching<'a>(names: &'a [String], typed: &str, hidden: bool) -> Vec<&'a str> {
    let typed = typed.to_lowercase();
    let shown = hidden || typed.starts_with(HIDDEN);
    let mut found: Vec<(bool, String, &str)> = names
        .iter()
        .filter(|name| shown || !name.starts_with(HIDDEN))
        .filter_map(|name| {
            let lower = name.to_lowercase();
            lower.contains(&typed).then(|| (!lower.starts_with(&typed), lower, name.as_str()))
        })
        .collect();
    found.sort();
    found.into_iter().map(|(_, _, name)| name).collect()
}

/// The keys that make `word` end in the subfolder `name` and a `/`: what was typed of a name is
/// erased first, and the name is quoted the way the word is.
pub(crate) fn keys(word: &Word, target: &Target, name: &str) -> Vec<u8> {
    let (erase, quote, lead) =
        if target.inside { (0, word.quote, ROOT) } else { (word.typed_name, word.name_quote, "") };
    let mut keys = vec![ERASE; erase];
    keys.extend(format!("{lead}{}{SEPARATOR}", quoted(name, quote)).into_bytes());
    keys
}

fn quoted(name: &str, quote: Option<char>) -> String {
    match quote {
        Some(SINGLE_QUOTE) => name.replace(SINGLE_QUOTE, SINGLE_QUOTE_INSIDE),
        Some(_) => escaped(name, SPECIAL_IN_DOUBLE_QUOTES),
        None => escaped(name, SPECIAL),
    }
}

fn escaped(name: &str, special: &str) -> String {
    let mut text = String::with_capacity(name.len());
    for c in name.chars() {
        if special.contains(c) {
            text.push(ESCAPE);
        }
        text.push(c);
    }
    text
}

#[cfg(test)]
mod tests;
