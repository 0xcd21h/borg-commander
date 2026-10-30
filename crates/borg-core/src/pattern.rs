use std::borrow::Cow;

use borg_vfs::Entry;
use globset::{Glob, GlobBuilder, GlobSet, GlobSetBuilder};
use unicode_normalization::{IsNormalized, UnicodeNormalization, is_nfc_quick};

const DIR_SUFFIX: char = '/';

#[derive(Debug, thiserror::Error)]
pub enum PatternError {
    #[error("empty pattern")]
    Empty,
    #[error(transparent)]
    Glob(#[from] globset::Error),
}

#[derive(Debug, Clone)]
pub struct Pattern {
    any: GlobSet,
    dirs: GlobSet,
}

/// `text` composed as keyboards type it, so a name the file system keeps decomposed (`й` as `и`
/// and a combining breve, as macOS often does) compares equal to what was typed.
pub fn composed(text: &str) -> Cow<'_, str> {
    if text.is_ascii() || is_nfc_quick(text.chars()) == IsNormalized::Yes {
        Cow::Borrowed(text)
    } else {
        Cow::Owned(text.nfc().collect())
    }
}

impl Pattern {
    pub fn parse(text: &str) -> Result<Self, PatternError> {
        // Globs fold only ASCII case, so patterns and names other than ASCII compare in lower case.
        let text = composed(text).to_lowercase();
        let tokens: Vec<&str> = text.split_whitespace().collect();
        if tokens.is_empty() {
            return Err(PatternError::Empty);
        }

        let mut any = GlobSetBuilder::new();
        let mut dirs = GlobSetBuilder::new();
        for token in tokens {
            match token.strip_suffix(DIR_SUFFIX) {
                Some(dir) => dirs.add(glob(dir)?),
                None => any.add(glob(token)?),
            };
        }
        Ok(Self { any: any.build()?, dirs: dirs.build()? })
    }

    pub fn matches(&self, entry: &Entry) -> bool {
        self.matches_name(&entry.name, entry.is_dir())
    }

    pub fn matches_name(&self, name: &str, dir: bool) -> bool {
        let name = if name.is_ascii() { Cow::Borrowed(name) } else { Cow::Owned(composed(name).to_lowercase()) };
        self.any.is_match(name.as_ref()) || (dir && self.dirs.is_match(name.as_ref()))
    }
}

fn glob(text: &str) -> Result<Glob, globset::Error> {
    GlobBuilder::new(text).case_insensitive(true).literal_separator(true).backslash_escape(true).build()
}

#[cfg(test)]
mod tests {
    use borg_vfs::EntryKind;

    use super::*;

    fn entry(name: &str, kind: EntryKind) -> Entry {
        Entry {
            source: None,
            missing: false,
            usage: None,
            name: name.into(),
            kind,
            size: 0,
            modified: None,
            executable: false,
            permissions: 0o644,
        }
    }

    #[test]
    fn matches_globs_case_insensitively() {
        let pattern = Pattern::parse("*.RS  *.{toml,md}").unwrap_or_else(|e| panic!("{e}"));
        assert!(pattern.matches(&entry("main.rs", EntryKind::File)));
        assert!(pattern.matches(&entry("Cargo.toml", EntryKind::File)));
        assert!(pattern.matches(&entry("README.MD", EntryKind::File)));
        assert!(!pattern.matches(&entry("build.sh", EntryKind::File)));
    }

    #[test]
    fn matches_names_beyond_ascii_whatever_their_case_or_composition() {
        let pattern = Pattern::parse("*регламент*").unwrap_or_else(|e| panic!("{e}"));
        assert!(pattern.matches(&entry("Регламент передачи ключей.pdf", EntryKind::File)));
        assert!(pattern.matches(&entry("РЕГЛАМЕНТ.txt", EntryKind::File)));
        let decomposed = "Регламент передачи ключеи\u{306}.pdf";
        assert!(Pattern::parse("*КЛЮЧЕЙ*").unwrap().matches(&entry(decomposed, EntryKind::File)));
        assert!(Pattern::parse("Café*").unwrap().matches(&entry("cafe\u{301} notes.md", EntryKind::File)));
        assert!(!pattern.matches(&entry("regulations.pdf", EntryKind::File)));
    }

    #[test]
    fn trailing_slash_limits_to_directories() {
        let pattern = Pattern::parse("t*/").unwrap_or_else(|e| panic!("{e}"));
        assert!(pattern.matches(&entry("target", EntryKind::Dir)));
        assert!(!pattern.matches(&entry("todo.txt", EntryKind::File)));
    }

    #[test]
    fn rejects_empty_and_invalid_patterns() {
        assert!(matches!(Pattern::parse("   "), Err(PatternError::Empty)));
        assert!(matches!(Pattern::parse("[a-"), Err(PatternError::Glob(_))));
    }
}
