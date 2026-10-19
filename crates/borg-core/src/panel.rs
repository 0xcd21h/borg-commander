use std::{
    cmp::Ordering,
    collections::HashSet,
    path::{Path, PathBuf},
};

use borg_vfs::Entry;

use crate::Pattern;

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
    marks: HashSet<String>,
    listing: Listing,
    incoming: Option<Incoming>,
    focus: Option<Anchor>,
    replacing: bool,
    pub git: crate::GitPanel,
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
            listing: Listing::Loading,
            incoming: None,
            focus: None,
            replacing: false,
            git: crate::GitPanel::default(),
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

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn generation(&self) -> u64 {
        self.incoming.as_ref().map_or(self.generation, |incoming| incoming.generation)
    }

    pub fn git_repository(&self) -> Option<&borg_git::Repository> {
        (self.dir() == self.destination()).then_some(self.git.repository.as_ref()).flatten()
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
        self.entries.len() + self.parent_rows()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn row(&self, index: usize) -> Option<Row<'_>> {
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
            Row::Parent => None,
        }
    }

    pub fn visible(&self) -> impl Iterator<Item = (usize, Row<'_>)> {
        (self.scroll..self.len().min(self.scroll + self.capacity())).filter_map(|i| self.row(i).map(|row| (i, row)))
    }

    pub fn is_marked(&self, entry: &Entry) -> bool {
        self.marks.contains(&entry.name)
    }

    pub fn marked(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter().filter(|entry| self.marks.contains(&entry.name))
    }

    pub fn marked_count(&self) -> usize {
        self.marks.len()
    }

    pub fn selection(&self) -> Vec<&Entry> {
        if self.marks.is_empty() { self.hovered_entry().into_iter().collect() } else { self.marked().collect() }
    }

    pub fn navigate(&mut self, dir: PathBuf, generation: u64, focus: Option<String>) {
        let refresh = self.is_reloading_into(&dir);
        self.incoming = Some(Incoming { dir, generation, focus: focus.map(Anchor::Name), refresh });
    }

    pub fn reload(&mut self, generation: u64) {
        let focus = self.anchor();
        let dir = self.destination().to_path_buf();
        let refresh = self.is_reloading_into(&dir);
        self.incoming = Some(Incoming { dir, generation, focus, refresh });
    }

    pub fn receive(&mut self, generation: u64, batch: Vec<Entry>, show_hidden: bool) {
        if self.incoming.as_ref().is_some_and(|incoming| incoming.generation == generation) {
            self.commit();
        }
        if generation == self.generation {
            self.pending.extend(batch.into_iter().filter(|entry| show_hidden || !entry.is_hidden()));
        }
    }

    pub fn finish(&mut self, generation: u64, error: Option<String>) -> Option<String> {
        let incoming = self.incoming.as_ref().is_some_and(|incoming| incoming.generation == generation);
        match (incoming, error) {
            (true, Some(error)) => {
                self.incoming = None;
                Some(error)
            }
            (true, None) => {
                self.commit();
                self.listing = Listing::Ready;
                None
            }
            (false, error) if generation == self.generation => {
                self.listing = error.map_or(Listing::Ready, Listing::Failed);
                None
            }
            (false, _) => None,
        }
    }

    pub fn settle(&mut self) {
        let loading = self.listing == Listing::Loading;
        if self.replacing {
            if !loading {
                self.replace_entries();
            }
            return;
        }
        let small = self.entries.len() < EAGER_MERGE_LIMIT;
        let grown = self.pending.len() >= self.entries.len() / MERGE_GROWTH_DIVISOR;
        if self.pending.is_empty() && loading {
            return;
        }
        if !self.pending.is_empty() && (!loading || small || grown) {
            let anchor = self.focus.clone().or_else(|| self.anchor());
            let sort = self.sort;
            let mut fresh = std::mem::take(&mut self.pending);
            fresh.sort_by(|a, b| order(a, b, sort));
            self.entries = merge(std::mem::take(&mut self.entries), fresh, sort);
            self.restore(anchor);
        }
        if !loading {
            self.prune_marks();
        }
    }

    fn replace_entries(&mut self) {
        let anchor = self.focus.take().or_else(|| self.anchor());
        let sort = self.sort;
        let mut fresh = std::mem::take(&mut self.pending);
        fresh.sort_by(|a, b| order(a, b, sort));
        self.entries = fresh;
        self.replacing = false;
        self.restore(anchor);
        self.prune_marks();
    }

    pub fn set_sort(&mut self, sort: Sort) {
        let anchor = self.anchor();
        self.sort = sort;
        self.entries.sort_by(|a, b| order(a, b, sort));
        self.restore(anchor);
    }

    pub fn set_mode(&mut self, mode: ListingMode) {
        self.mode = mode;
        self.clamp();
    }

    pub fn set_rows(&mut self, rows: usize) {
        self.rows = rows;
        self.clamp();
    }

    pub fn move_by(&mut self, delta: isize) {
        self.focus = None;
        self.cursor = self.cursor.saturating_add_signed(delta);
        self.clamp();
    }

    pub fn move_column(&mut self, columns: isize) {
        let step = isize::try_from(self.rows.max(1)).unwrap_or(isize::MAX);
        self.move_by(step.saturating_mul(columns));
    }

    pub fn page(&mut self, pages: isize) {
        let step = isize::try_from(self.capacity().saturating_sub(1).max(1)).unwrap_or(isize::MAX);
        self.move_by(step.saturating_mul(pages));
    }

    pub fn to_start(&mut self) {
        self.focus = None;
        self.cursor = 0;
        self.clamp();
    }

    pub fn to_end(&mut self) {
        self.focus = None;
        self.cursor = self.len().saturating_sub(1);
        self.clamp();
    }

    pub fn seek(&mut self, step: Seek, accept: impl Fn(&Entry) -> bool) -> bool {
        let count = self.entries.len();
        if count == 0 {
            return false;
        }
        let offset = self.parent_rows();
        let here = self.cursor.checked_sub(offset);
        let (start, forward) = match (step, here) {
            (Seek::Here, Some(here)) => (here, true),
            (Seek::Next, Some(here)) => ((here + 1) % count, true),
            (Seek::Previous, Some(here)) => ((here + count - 1) % count, false),
            (Seek::Here | Seek::Next, None) => (0, true),
            (Seek::Previous, None) => (count - 1, false),
        };
        let order =
            (0..count).map(|step| if forward { (start + step) % count } else { (start + count - step) % count });
        let found = order.into_iter().find(|index| self.entries.get(*index).is_some_and(&accept));
        if let Some(index) = found {
            self.focus = None;
            self.cursor = index + offset;
            self.clamp();
        }
        found.is_some()
    }

    pub fn point_at(&mut self, name: &str) {
        self.focus = None;
        self.restore(Some(Anchor::Name(name.to_owned())));
    }

    pub fn toggle_mark(&mut self) {
        let Some(name) = self.hovered_entry().map(|entry| entry.name.clone()) else { return };
        if !self.marks.remove(&name) {
            self.marks.insert(name);
        }
    }

    pub fn mark_matching(&mut self, pattern: &Pattern, mark: bool) -> usize {
        self.mark_where(|entry| pattern.matches(entry), mark)
    }

    pub fn mark_where(&mut self, accept: impl Fn(&Entry) -> bool, mark: bool) -> usize {
        let names: Vec<String> =
            self.entries.iter().filter(|entry| accept(entry)).map(|entry| entry.name.clone()).collect();
        let count = names.len();
        names.into_iter().for_each(|name| {
            if mark {
                self.marks.insert(name);
            } else {
                self.marks.remove(&name);
            }
        });
        count
    }

    pub fn invert_marks(&mut self) {
        self.marks = self.entries.iter().map(|e| e.name.clone()).filter(|name| !self.marks.contains(name)).collect();
    }

    pub fn clear_marks(&mut self) {
        self.marks.clear();
    }

    fn commit(&mut self) {
        let Some(incoming) = self.incoming.take() else { return };
        self.generation = incoming.generation;
        self.focus = incoming.focus;
        self.replacing = incoming.refresh;
        self.pending.clear();
        self.listing = Listing::Loading;
        self.dir = incoming.dir;
        if !incoming.refresh {
            self.entries.clear();
            self.marks.clear();
            self.cursor = 0;
            self.scroll = 0;
        }
    }

    fn is_reloading_into(&self, dir: &Path) -> bool {
        dir == self.dir && self.generation != INITIAL_GENERATION
    }

    fn prune_marks(&mut self) {
        if self.marks.is_empty() {
            return;
        }
        let present: HashSet<&str> = self.entries.iter().map(|e| e.name.as_str()).collect();
        self.marks.retain(|name| present.contains(name.as_str()));
    }

    fn restore(&mut self, anchor: Option<Anchor>) {
        if let Some(index) = anchor.as_ref().and_then(|anchor| self.locate(anchor)) {
            self.cursor = index;
            if self.focus.as_ref() == anchor.as_ref() {
                self.focus = None;
            }
        }
        self.clamp();
    }

    fn parent_rows(&self) -> usize {
        usize::from(self.dir.parent().is_some())
    }

    fn anchor(&self) -> Option<Anchor> {
        self.hovered().map(|row| match row {
            Row::Parent => Anchor::Parent,
            Row::Entry(entry) => Anchor::Name(entry.name.clone()),
        })
    }

    fn locate(&self, anchor: &Anchor) -> Option<usize> {
        match anchor {
            Anchor::Parent => (self.parent_rows() > 0).then_some(0),
            Anchor::Name(name) => {
                self.entries.iter().position(|entry| &entry.name == name).map(|i| i + self.parent_rows())
            }
        }
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

pub fn order(a: &Entry, b: &Entry, sort: Sort) -> Ordering {
    let group = b.is_dir().cmp(&a.is_dir());
    let within = match sort.key {
        SortKey::Name => Ordering::Equal,
        SortKey::Extension => folded_cmp(extension(&a.name), extension(&b.name)),
        SortKey::Size => a.size.cmp(&b.size),
        SortKey::Modified => a.modified.cmp(&b.modified),
    }
    .then_with(|| folded_cmp(&a.name, &b.name))
    .then_with(|| a.name.cmp(&b.name));
    group.then(if sort.reverse { within.reverse() } else { within })
}

fn extension(name: &str) -> &str {
    name.rsplit_once(EXTENSION_SEPARATOR).filter(|(stem, _)| !stem.is_empty()).map_or("", |(_, ext)| ext)
}

fn folded_cmp(a: &str, b: &str) -> Ordering {
    if a.is_ascii() && b.is_ascii() {
        a.bytes().map(|c| c.to_ascii_lowercase()).cmp(b.bytes().map(|c| c.to_ascii_lowercase()))
    } else {
        a.chars().flat_map(char::to_lowercase).cmp(b.chars().flat_map(char::to_lowercase))
    }
}

fn merge(left: Vec<Entry>, right: Vec<Entry>, sort: Sort) -> Vec<Entry> {
    let mut out = Vec::with_capacity(left.len() + right.len());
    let mut left = left.into_iter().peekable();
    let mut right = right.into_iter().peekable();
    loop {
        let take_left = match (left.peek(), right.peek()) {
            (Some(l), Some(r)) => order(l, r, sort) != Ordering::Greater,
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (None, None) => return out,
        };
        out.extend(if take_left { left.next() } else { right.next() });
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use borg_vfs::EntryKind;

    use super::*;

    const ROWS: usize = 3;

    fn entry(name: &str, kind: EntryKind, size: u64) -> Entry {
        Entry {
            name: name.into(),
            kind,
            size,
            modified: Some(UNIX_EPOCH + Duration::from_secs(size)),
            executable: false,
            permissions: 0o644,
        }
    }

    fn file(name: &str) -> Entry {
        entry(name, EntryKind::File, 0)
    }

    fn sized(name: &str, size: u64) -> Entry {
        entry(name, EntryKind::File, size)
    }

    fn dir(name: &str) -> Entry {
        entry(name, EntryKind::Dir, 0)
    }

    fn names(panel: &Panel) -> Vec<&str> {
        panel.entries().iter().map(|e| e.name.as_str()).collect()
    }

    fn marked(panel: &Panel) -> Vec<&str> {
        panel.marked().map(|e| e.name.as_str()).collect()
    }

    fn loaded(path: &str, batches: Vec<Vec<Entry>>) -> Panel {
        let mut panel = Panel::new(PathBuf::from("/"), ROWS, ListingMode::Full, Sort::default());
        panel.navigate(PathBuf::from(path), 1, None);
        batches.into_iter().for_each(|batch| panel.receive(1, batch, false));
        panel.finish(1, None);
        panel.settle();
        panel
    }

    #[test]
    fn sorts_directories_first_case_insensitively() {
        let panel = loaded("/x", vec![vec![file("b"), dir("Zed"), file("A")], vec![dir("alpha")]]);
        assert_eq!(names(&panel), ["alpha", "Zed", "A", "b"]);
    }

    #[test]
    fn sorts_by_size_extension_and_reverse_keeping_dirs_first() {
        let mut panel = loaded("/x", vec![vec![sized("a.rs", 30), sized("b.md", 10), sized("c.toml", 20), dir("z")]]);
        panel.set_sort(Sort { key: SortKey::Size, reverse: false });
        assert_eq!(names(&panel), ["z", "b.md", "c.toml", "a.rs"]);
        panel.set_sort(Sort { key: SortKey::Extension, reverse: false });
        assert_eq!(names(&panel), ["z", "b.md", "a.rs", "c.toml"]);
        panel.set_sort(Sort { key: SortKey::Name, reverse: true });
        assert_eq!(names(&panel), ["z", "c.toml", "b.md", "a.rs"]);
    }

    #[test]
    fn resorting_keeps_cursor_on_same_file() {
        let mut panel = loaded("/x", vec![vec![sized("a", 30), sized("b", 10), sized("c", 20)]]);
        panel.move_by(1);
        assert_eq!(panel.hovered_entry().map(|e| e.name.as_str()), Some("a"));
        panel.set_sort(Sort { key: SortKey::Size, reverse: false });
        assert_eq!(panel.hovered_entry().map(|e| e.name.as_str()), Some("a"));
    }

    #[test]
    fn hides_dotfiles_unless_requested() {
        let panel = loaded("/x", vec![vec![file(".env"), file("main.rs")]]);
        assert_eq!(names(&panel), ["main.rs"]);
    }

    #[test]
    fn keeps_cursor_on_same_entry_while_batches_arrive() {
        let mut panel = Panel::new(PathBuf::from("/"), ROWS, ListingMode::Full, Sort::default());
        panel.navigate(PathBuf::from("/x"), 1, None);
        panel.receive(1, vec![file("m"), file("z")], false);
        panel.settle();
        panel.move_by(1);
        assert_eq!(panel.hovered(), Some(Row::Entry(&file("m"))));

        panel.receive(1, vec![file("a"), file("b")], false);
        panel.settle();
        assert_eq!(panel.hovered(), Some(Row::Entry(&file("m"))));
    }

    #[test]
    fn focuses_child_after_leaving_it() {
        let mut panel = loaded("/x/sub", vec![vec![file("f")]]);
        panel.navigate(PathBuf::from("/x"), 2, Some("sub".into()));
        panel.receive(2, vec![dir("a"), dir("sub"), dir("z")], false);
        panel.settle();
        assert_eq!(panel.dir(), Path::new("/x"));
        assert_eq!(panel.hovered(), Some(Row::Entry(&dir("sub"))));
    }

    #[test]
    fn keeps_previous_listing_until_destination_responds() {
        let mut panel = loaded("/x", vec![vec![file("f")]]);
        panel.navigate(PathBuf::from("/forbidden"), 2, None);
        assert_eq!(panel.dir(), Path::new("/x"));

        let error = panel.finish(2, Some("permission denied".into()));
        assert_eq!(error.as_deref(), Some("permission denied"));
        assert_eq!(panel.dir(), Path::new("/x"));
        assert_eq!(names(&panel), ["f"]);
    }

    #[test]
    fn ignores_stale_batches() {
        let mut panel = loaded("/x", vec![vec![file("f")]]);
        panel.receive(0, vec![file("ghost")], false);
        panel.settle();
        assert_eq!(names(&panel), ["f"]);
    }

    #[test]
    fn defers_merges_geometrically_while_loading() {
        let batch = |range: std::ops::Range<usize>| range.map(|i| file(&format!("f{i:06}"))).collect::<Vec<_>>();
        let mut panel = Panel::new(PathBuf::from("/"), ROWS, ListingMode::Full, Sort::default());
        panel.navigate(PathBuf::from("/x"), 1, None);
        panel.receive(1, batch(0..EAGER_MERGE_LIMIT), false);
        panel.settle();
        assert_eq!(panel.entries().len(), EAGER_MERGE_LIMIT);

        panel.receive(1, batch(EAGER_MERGE_LIMIT..EAGER_MERGE_LIMIT + 10), false);
        panel.settle();
        assert_eq!(panel.entries().len(), EAGER_MERGE_LIMIT);
        assert_eq!(panel.received(), EAGER_MERGE_LIMIT + 10);

        panel.finish(1, None);
        panel.settle();
        assert_eq!(panel.entries().len(), EAGER_MERGE_LIMIT + 10);
    }

    #[test]
    fn scrolls_to_keep_cursor_visible() {
        let mut panel = loaded("/x", vec![(0..10).map(|i| file(&format!("f{i}"))).collect()]);
        panel.to_end();
        assert_eq!(panel.cursor(), 10);
        assert_eq!(panel.scroll(), 10 + 1 - ROWS);
        panel.to_start();
        assert_eq!((panel.cursor(), panel.scroll()), (0, 0));
        assert_eq!(panel.hovered(), Some(Row::Parent));
    }

    #[test]
    fn brief_mode_scrolls_by_whole_columns() {
        let mut panel = loaded("/x", vec![(0..20).map(|i| file(&format!("f{i:02}"))).collect()]);
        panel.set_mode(ListingMode::Brief);
        assert_eq!(panel.capacity(), ROWS * BRIEF_COLUMNS);

        panel.move_column(1);
        assert_eq!(panel.cursor(), ROWS);
        panel.move_column(1);
        assert_eq!(panel.cursor(), ROWS * 2);
        assert_eq!(panel.scroll() % ROWS, 0);
        assert!(panel.cursor() < panel.scroll() + panel.capacity());
    }

    #[test]
    fn marks_toggle_match_and_invert() {
        let mut panel = loaded("/x", vec![vec![file("a.rs"), file("b.rs"), file("notes.md"), dir("src")]]);
        panel.move_by(2);
        panel.toggle_mark();
        assert_eq!(panel.marked_count(), 1);

        let rust = Pattern::parse("*.RS").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(panel.mark_matching(&rust, true), 2);
        assert_eq!(marked(&panel), ["a.rs", "b.rs"]);

        panel.invert_marks();
        assert_eq!(marked(&panel), ["src", "notes.md"]);
    }

    #[test]
    fn selection_falls_back_to_hovered_entry() {
        let mut panel = loaded("/x", vec![vec![file("a"), file("b")]]);
        assert!(panel.selection().is_empty());
        panel.move_by(2);
        assert_eq!(panel.selection().iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["b"]);
    }

    #[test]
    fn marks_survive_reload_but_not_navigation() {
        let mut panel = loaded("/x", vec![vec![file("a"), file("b")]]);
        panel.move_by(1);
        panel.toggle_mark();

        panel.reload(2);
        panel.receive(2, vec![file("a"), file("b")], false);
        panel.finish(2, None);
        panel.settle();
        assert_eq!(panel.marked_count(), 1);

        panel.navigate(PathBuf::from("/y"), 3, None);
        panel.finish(3, None);
        assert_eq!(panel.marked_count(), 0);
    }

    #[test]
    fn cursor_moves_to_next_entry_when_hovered_file_vanishes() {
        let mut panel = loaded("/x", vec![vec![file("a"), file("b"), file("c")]]);
        panel.move_by(2);
        assert_eq!(panel.hovered_entry().map(|e| e.name.as_str()), Some("b"));

        panel.reload(2);
        panel.receive(2, vec![file("a"), file("c")], false);
        panel.finish(2, None);
        panel.settle();
        assert_eq!(panel.hovered_entry().map(|e| e.name.as_str()), Some("c"));
    }

    #[test]
    fn reload_keeps_old_listing_until_new_one_is_complete() {
        let mut panel = loaded("/x", vec![vec![file("a"), file("b")]]);
        panel.move_by(2);
        panel.reload(2);
        panel.receive(2, vec![file("b")], false);
        panel.settle();
        assert_eq!(names(&panel), ["a", "b"]);
        assert_eq!(panel.hovered_entry().map(|e| e.name.as_str()), Some("b"));
        assert_eq!(panel.received(), 2);

        panel.receive(2, vec![file("c")], false);
        panel.finish(2, None);
        panel.settle();
        assert_eq!(names(&panel), ["b", "c"]);
        assert_eq!(panel.hovered_entry().map(|e| e.name.as_str()), Some("b"));
    }

    #[test]
    fn first_listing_streams_even_into_the_starting_directory() {
        let mut panel = Panel::new(PathBuf::from("/x"), ROWS, ListingMode::Full, Sort::default());
        panel.navigate(PathBuf::from("/x"), 1, None);
        panel.receive(1, vec![file("a")], false);
        panel.settle();
        assert_eq!(names(&panel), ["a"]);
    }

    #[test]
    fn marks_of_vanished_files_are_dropped_after_reload() {
        let mut panel = loaded("/x", vec![vec![file("a"), file("b")]]);
        panel.move_by(1);
        panel.toggle_mark();

        panel.reload(2);
        panel.receive(2, vec![file("b")], false);
        panel.finish(2, None);
        panel.settle();
        assert_eq!(panel.marked_count(), 0);
    }
}
