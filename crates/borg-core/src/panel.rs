use std::{
    cmp::Ordering,
    path::{Path, PathBuf},
};

use borg_vfs::Entry;

const EAGER_MERGE_LIMIT: usize = 4096;
const MERGE_GROWTH_DIVISOR: usize = 2;

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

#[derive(Debug, Clone, PartialEq, Eq)]
enum Anchor {
    Parent,
    Entry { name: String, dir: bool },
}

#[derive(Debug, Clone)]
struct Incoming {
    dir: PathBuf,
    generation: u64,
    focus: Option<Anchor>,
}

#[derive(Debug, Clone)]
pub struct Panel {
    dir: PathBuf,
    generation: u64,
    entries: Vec<Entry>,
    pending: Vec<Entry>,
    cursor: usize,
    scroll: usize,
    viewport: usize,
    listing: Listing,
    incoming: Option<Incoming>,
    focus: Option<Anchor>,
}

impl Panel {
    pub fn new(dir: PathBuf, generation: u64, viewport: usize) -> Self {
        Self {
            dir,
            generation,
            entries: Vec::new(),
            pending: Vec::new(),
            cursor: 0,
            scroll: 0,
            viewport,
            listing: Listing::Loading,
            incoming: None,
            focus: None,
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

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn scroll(&self) -> usize {
        self.scroll
    }

    pub fn received(&self) -> usize {
        self.entries.len() + self.pending.len()
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

    pub fn visible(&self) -> impl Iterator<Item = (usize, Row<'_>)> {
        (self.scroll..self.len().min(self.scroll + self.viewport)).filter_map(|i| self.row(i).map(|row| (i, row)))
    }

    pub fn navigate(&mut self, dir: PathBuf, generation: u64, focus_child: Option<String>) {
        let focus = focus_child.map(|name| Anchor::Entry { name, dir: true });
        self.incoming = Some(Incoming { dir, generation, focus });
    }

    pub fn reload(&mut self, generation: u64) {
        let focus = self.anchor();
        self.incoming = Some(Incoming { dir: self.destination().to_path_buf(), generation, focus });
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
        let small = self.entries.len() < EAGER_MERGE_LIMIT;
        let grown = self.pending.len() >= self.entries.len() / MERGE_GROWTH_DIVISOR;
        if self.pending.is_empty() || (loading && !small && !grown) {
            return;
        }

        let anchor = self.focus.clone().or_else(|| self.anchor());
        let mut fresh = std::mem::take(&mut self.pending);
        fresh.sort_by(order);
        self.entries = merge(std::mem::take(&mut self.entries), fresh);

        if let Some(index) = anchor.as_ref().and_then(|anchor| self.locate(anchor)) {
            self.cursor = index;
            if self.focus.as_ref() == anchor.as_ref() {
                self.focus = None;
            }
        }
        self.clamp();
    }

    pub fn move_by(&mut self, delta: isize) {
        self.focus = None;
        self.cursor = self.cursor.saturating_add_signed(delta);
        self.clamp();
    }

    pub fn page(&mut self, pages: isize) {
        let step = isize::try_from(self.viewport.saturating_sub(1).max(1)).unwrap_or(isize::MAX);
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

    pub fn set_viewport(&mut self, viewport: usize) {
        self.viewport = viewport;
        self.clamp();
    }

    fn commit(&mut self) {
        let Some(incoming) = self.incoming.take() else { return };
        self.dir = incoming.dir;
        self.generation = incoming.generation;
        self.focus = incoming.focus;
        self.entries.clear();
        self.pending.clear();
        self.cursor = 0;
        self.scroll = 0;
        self.listing = Listing::Loading;
    }

    fn parent_rows(&self) -> usize {
        usize::from(self.dir.parent().is_some())
    }

    fn anchor(&self) -> Option<Anchor> {
        self.hovered().map(|row| match row {
            Row::Parent => Anchor::Parent,
            Row::Entry(entry) => Anchor::Entry { name: entry.name.clone(), dir: entry.is_dir() },
        })
    }

    fn locate(&self, anchor: &Anchor) -> Option<usize> {
        match anchor {
            Anchor::Parent => (self.parent_rows() > 0).then_some(0),
            Anchor::Entry { name, dir } => self
                .entries
                .binary_search_by(|entry| compare(entry.is_dir(), &entry.name, *dir, name))
                .ok()
                .map(|i| i + self.parent_rows()),
        }
    }

    fn clamp(&mut self) {
        self.cursor = self.cursor.min(self.len().saturating_sub(1));
        let viewport = self.viewport.max(1);
        if self.cursor < self.scroll {
            self.scroll = self.cursor;
        } else if self.cursor >= self.scroll + viewport {
            self.scroll = self.cursor + 1 - viewport;
        }
        self.scroll = self.scroll.min(self.len().saturating_sub(viewport));
    }
}

fn order(a: &Entry, b: &Entry) -> Ordering {
    compare(a.is_dir(), &a.name, b.is_dir(), &b.name)
}

fn compare(a_dir: bool, a_name: &str, b_dir: bool, b_name: &str) -> Ordering {
    b_dir.cmp(&a_dir).then_with(|| folded_cmp(a_name, b_name)).then_with(|| a_name.cmp(b_name))
}

fn folded_cmp(a: &str, b: &str) -> Ordering {
    if a.is_ascii() && b.is_ascii() {
        a.bytes().map(|c| c.to_ascii_lowercase()).cmp(b.bytes().map(|c| c.to_ascii_lowercase()))
    } else {
        a.chars().flat_map(char::to_lowercase).cmp(b.chars().flat_map(char::to_lowercase))
    }
}

fn merge(left: Vec<Entry>, right: Vec<Entry>) -> Vec<Entry> {
    let mut out = Vec::with_capacity(left.len() + right.len());
    let mut left = left.into_iter().peekable();
    let mut right = right.into_iter().peekable();
    loop {
        let take_left = match (left.peek(), right.peek()) {
            (Some(l), Some(r)) => order(l, r) != Ordering::Greater,
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (None, None) => return out,
        };
        out.extend(if take_left { left.next() } else { right.next() });
    }
}

#[cfg(test)]
mod tests {
    use borg_vfs::EntryKind;

    use super::*;

    const VIEWPORT: usize = 3;

    fn entry(name: &str, kind: EntryKind) -> Entry {
        Entry { name: name.into(), kind, size: 0, modified: None, executable: false }
    }

    fn file(name: &str) -> Entry {
        entry(name, EntryKind::File)
    }

    fn dir(name: &str) -> Entry {
        entry(name, EntryKind::Dir)
    }

    fn names(panel: &Panel) -> Vec<&str> {
        panel.entries().iter().map(|e| e.name.as_str()).collect()
    }

    fn loaded(path: &str, batches: Vec<Vec<Entry>>) -> Panel {
        let mut panel = Panel::new(PathBuf::from("/"), 0, VIEWPORT);
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
    fn hides_dotfiles_unless_requested() {
        let panel = loaded("/x", vec![vec![file(".env"), file("main.rs")]]);
        assert_eq!(names(&panel), ["main.rs"]);
    }

    #[test]
    fn keeps_cursor_on_same_entry_while_batches_arrive() {
        let mut panel = Panel::new(PathBuf::from("/"), 0, VIEWPORT);
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
        let mut panel = Panel::new(PathBuf::from("/"), 0, VIEWPORT);
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
        assert_eq!(panel.scroll(), 10 + 1 - VIEWPORT);
        panel.to_start();
        assert_eq!((panel.cursor(), panel.scroll()), (0, 0));
        assert_eq!(panel.hovered(), Some(Row::Parent));
    }
}
