use std::path::{Path, PathBuf};

/// Searches remembered.
pub const HISTORY_LIMIT: usize = 200;

/// A search as typed, and the folder it ran in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PastSearch {
    pub query: String,
    pub root: PathBuf,
}

/// Recent searches, newest first, each query once.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchHistory {
    entries: Vec<PastSearch>,
}

impl SearchHistory {
    pub fn new(mut entries: Vec<PastSearch>) -> Self {
        entries.truncate(HISTORY_LIMIT);
        Self { entries }
    }

    pub fn entries(&self) -> &[PastSearch] {
        &self.entries
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Remembers `query`, moving an earlier use of it to the front; false when nothing changed.
    pub(crate) fn record(&mut self, query: &str, root: &Path) -> bool {
        let query = query.trim();
        let entry = PastSearch { query: query.into(), root: root.into() };
        if query.is_empty() || query == "*" || self.entries.first() == Some(&entry) {
            return false;
        }
        self.entries.retain(|past| past.query != query);
        self.entries.insert(0, entry);
        self.entries.truncate(HISTORY_LIMIT);
        true
    }

    /// The searches to recall in a search below `root`: those made there first, newest first.
    pub fn recent(&self, root: &Path) -> Vec<&PastSearch> {
        let (here, elsewhere): (Vec<_>, Vec<_>) = self.entries.iter().partition(|entry| entry.root == root);
        here.into_iter().chain(elsewhere).collect()
    }

    #[cfg(test)]
    fn recall(&self, root: &Path) -> Vec<&str> {
        self.recent(root).into_iter().map(|entry| entry.query.as_str()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queries_are_kept_once_newest_first_and_recalled_for_the_folder_first() {
        let mut history = SearchHistory::default();
        history.record("*.rs", Path::new("/work"));
        history.record("notes", Path::new("/home"));
        history.record("*", Path::new("/home"));
        history.record(" *.rs ", Path::new("/work"));
        history.record("ext:md", Path::new("/home"));
        assert_eq!(history.entries().len(), 3);
        assert_eq!(history.recall(Path::new("/home")), ["ext:md", "notes", "*.rs"]);
        assert_eq!(history.recall(Path::new("/work")), ["*.rs", "ext:md", "notes"]);
        for n in 0..HISTORY_LIMIT + 5 {
            history.record(&n.to_string(), Path::new("/"));
        }
        assert_eq!(history.entries().len(), HISTORY_LIMIT);
    }
}
