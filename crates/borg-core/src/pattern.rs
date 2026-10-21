use borg_vfs::Entry;
use globset::{Glob, GlobBuilder, GlobSet, GlobSetBuilder};

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

impl Pattern {
    pub fn parse(text: &str) -> Result<Self, PatternError> {
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
        self.any.is_match(&entry.name) || (entry.is_dir() && self.dirs.is_match(&entry.name))
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
