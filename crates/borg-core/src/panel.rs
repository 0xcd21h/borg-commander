use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use borg_vfs::Entry;

mod cursor;
mod listing;
mod results;

pub use listing::order;

const EAGER_MERGE_LIMIT: usize = 4096;
const MERGE_GROWTH_DIVISOR: usize = 2;
const BRIEF_COLUMNS: usize = 2;
const EXTENSION_SEPARATOR: char = '.';
const INITIAL_GENERATION: u64 = 0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seek {
    Here,
    Next,
    Previous,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row<'a> {
    Parent,
    Entry(&'a Entry),
    Group { label: &'a str, count: usize, collapsed: bool },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Listing {
    Loading,
    Ready,
    Failed(String),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ListingMode {
    #[default]
    Full,
    Brief,
}

impl ListingMode {
    pub fn columns(self) -> usize {
        match self {
            Self::Full => 1,
            Self::Brief => BRIEF_COLUMNS,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SortKey {
    #[default]
    Name,
    Extension,
    Size,
    Modified,
    Source,
    Files,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Sort {
    pub key: SortKey,
    pub reverse: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Anchor {
    Parent,
    Name(String),
    Path(PathBuf),
}

#[derive(Debug, Clone)]
struct Incoming {
    dir: PathBuf,
    generation: u64,
    focus: Option<Anchor>,
    refresh: bool,
}

#[derive(Debug, Clone)]
pub struct Panel {
    dir: PathBuf,
    generation: u64,
    entries: Vec<Entry>,
    pending: Vec<Entry>,
    cursor: usize,
    scroll: usize,
    rows: usize,
    mode: ListingMode,
    sort: Sort,
    marks: HashSet<PathBuf>,
    pub results: Option<crate::Results>,
    listing: Listing,
    incoming: Option<Incoming>,
    focus: Option<Anchor>,
    replacing: bool,
    pub git: crate::GitPanel,
    pub backup: crate::BackupPanel,
    /// The container whose filesystem the panel shows, if any.
    pub mount: Option<crate::Mount>,
    /// A paired Borg session whose filesystem this panel shows.
    pub remote_mount: Option<RemoteMount>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteMount {
    pub machine: String,
    pub session: String,
    pub name: String,
    /// The local folder to return to when leaving the remote root.
    pub host: PathBuf,
}

impl Panel {
    pub fn new(dir: PathBuf, rows: usize, mode: ListingMode, sort: Sort) -> Self {
        Self {
            dir,
            generation: INITIAL_GENERATION,
            entries: Vec::new(),
            pending: Vec::new(),
            cursor: 0,
            scroll: 0,
            rows,
            mode,
            sort,
            marks: HashSet::new(),
            results: None,
            listing: Listing::Loading,
            incoming: None,
            focus: None,
            replacing: false,
            git: crate::GitPanel::default(),
            backup: crate::BackupPanel::default(),
            mount: None,
            remote_mount: None,
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn destination(&self) -> &Path {
        self.incoming.as_ref().map_or(&self.dir, |incoming| &incoming.dir)
    }

    pub fn listing(&self) -> &Listing {
        &self.listing
    }

    /// Includes the wait for a first batch while the previous folder remains visible.
    pub fn loading(&self) -> bool {
        self.incoming.is_some() || self.listing == Listing::Loading
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn generation(&self) -> u64 {
        self.incoming.as_ref().map_or(self.generation, |incoming| incoming.generation)
    }

    pub fn git_repository(&self) -> Option<&borg_git::Repository> {
        (!self.is_results() && self.remote_mount.is_none() && self.dir() == self.destination())
            .then_some(self.git.repository.as_ref())
            .flatten()
    }

    pub fn mode(&self) -> ListingMode {
        self.mode
    }

    pub fn sort(&self) -> Sort {
        self.sort
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn scroll(&self) -> usize {
        self.scroll
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    pub fn capacity(&self) -> usize {
        self.rows.max(1) * self.mode.columns()
    }

    pub fn received(&self) -> usize {
        if self.replacing { self.entries.len() } else { self.entries.len() + self.pending.len() }
    }

    pub fn len(&self) -> usize {
        self.results.as_ref().map_or(self.entries.len() + self.parent_rows(), |results| results.rows.len())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn row(&self, index: usize) -> Option<Row<'_>> {
        if let Some(results) = &self.results {
            return match results.rows.get(index)? {
                crate::ResultRow::Entry(index) => self.entries.get(*index).map(Row::Entry),
                crate::ResultRow::Group { label, count } => {
                    Some(Row::Group { label, count: *count, collapsed: results.collapsed.contains(label) })
                }
            };
        }
        match index.checked_sub(self.parent_rows()) {
            None => Some(Row::Parent),
            Some(i) => self.entries.get(i).map(Row::Entry),
        }
    }

    pub fn hovered(&self) -> Option<Row<'_>> {
        self.row(self.cursor)
    }

    pub fn hovered_entry(&self) -> Option<&Entry> {
        match self.hovered()? {
            Row::Entry(entry) => Some(entry),
            Row::Parent | Row::Group { .. } => None,
        }
    }

    pub fn visible(&self) -> impl Iterator<Item = (usize, Row<'_>)> {
        (self.scroll..self.len().min(self.scroll + self.capacity())).filter_map(|i| self.row(i).map(|row| (i, row)))
    }

    pub fn is_marked(&self, entry: &Entry) -> bool {
        self.marks.contains(entry.identity())
    }

    pub fn marked(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter().filter(|entry| self.marks.contains(entry.identity()))
    }

    pub fn marked_count(&self) -> usize {
        self.marks.len()
    }

    pub fn selection(&self) -> Vec<&Entry> {
        if self.marks.is_empty() { self.hovered_entry().into_iter().collect() } else { self.marked().collect() }
    }

    fn clamp(&mut self) {
        self.cursor = self.cursor.min(self.len().saturating_sub(1));
        let capacity = self.capacity();
        let column = self.rows.max(1);
        let page_step = match self.mode {
            ListingMode::Full => 1,
            ListingMode::Brief => column,
        };
        if self.cursor < self.scroll {
            self.scroll = self.cursor - self.cursor % page_step;
        } else if self.cursor >= self.scroll + capacity {
            self.scroll = (self.cursor + 1 - capacity).div_ceil(page_step) * page_step;
        }
        let last_scroll = match self.mode {
            ListingMode::Full => self.len().saturating_sub(capacity),
            ListingMode::Brief => self.len().saturating_sub(1) / column * column,
        };
        self.scroll = self.scroll.min(last_scroll);
    }
}

#[cfg(test)]
mod tests;
