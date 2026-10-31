//! Listing a folder into a panel: navigating, reloading, taking entries in batches, sorting them, and keeping the cursor on the same entry.

use super::{
    Anchor, EAGER_MERGE_LIMIT, EXTENSION_SEPARATOR, INITIAL_GENERATION, Incoming, Listing, ListingMode,
    MERGE_GROWTH_DIVISOR, Panel, Row, Sort, SortKey,
};
use borg_vfs::Entry;
use std::{
    cmp::Ordering,
    collections::HashSet,
    path::{Path, PathBuf},
};

impl Panel {
    pub fn navigate(&mut self, dir: PathBuf, generation: u64, focus: Option<String>) {
        self.results = None;
        let refresh = self.is_reloading_into(&dir);
        self.incoming = Some(Incoming { dir, generation, focus: focus.map(Anchor::Name), refresh });
    }

    pub fn reload(&mut self, generation: u64) {
        // A folder still loading keeps the entry it was opened to select; otherwise the
        // refresh selects whatever is hovered when it arrives, after any move made meanwhile.
        let focus = self.incoming.as_ref().and_then(|incoming| incoming.focus.clone());
        let dir = self.destination().to_path_buf();
        let refresh = self.is_reloading_into(&dir);
        self.incoming = Some(Incoming { dir, generation, focus, refresh });
    }

    pub fn receive(&mut self, generation: u64, batch: Vec<Entry>, show_hidden: bool) {
        if self.incoming.as_ref().is_some_and(|incoming| incoming.generation == generation) {
            self.commit();
        }
        if generation == self.generation {
            let results = self.is_results();
            self.pending.extend(batch.into_iter().filter(|entry| results || show_hidden || !entry.is_hidden()));
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
                if error.is_some() {
                    self.cancel_refresh();
                }
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
        if self.is_tree() && !self.pending.is_empty() {
            let anchor = self.focus.clone().or_else(|| self.anchor());
            let mut by_path: std::collections::HashMap<PathBuf, Entry> =
                std::mem::take(&mut self.entries).into_iter().map(|e| (e.identity().to_path_buf(), e)).collect();
            for entry in self.pending.drain(..) {
                by_path.insert(entry.identity().to_path_buf(), entry);
            }
            self.entries = by_path.into_values().collect();
            self.entries.sort_by(|a, b| order(a, b, self.sort));
            self.rebuild_results();
            self.restore(anchor);
            return;
        }
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
            self.rebuild_results();
            self.restore(anchor);
        }
        if !loading {
            self.prune_marks();
        }
    }

    pub(super) fn replace_entries(&mut self) {
        let anchor = self.focus.clone().or_else(|| self.anchor());
        let sort = self.sort;
        let mut fresh = std::mem::take(&mut self.pending);
        if self.is_tree() {
            let mut by_path = std::collections::HashMap::new();
            for entry in fresh {
                by_path.insert(entry.identity().to_path_buf(), entry);
            }
            fresh = by_path.into_values().collect();
        }
        fresh.sort_by(|a, b| order(a, b, sort));
        self.entries = fresh;
        self.replacing = false;
        self.rebuild_results();
        if self.is_tree() && anchor.as_ref().is_some_and(|anchor| self.locate(anchor).is_none()) {
            self.focus = anchor.clone();
        }
        self.restore(anchor);
        if !self.is_tree() {
            self.prune_marks();
        }
    }

    pub fn set_sort(&mut self, sort: Sort) {
        let anchor = self.focus.clone().or_else(|| self.anchor());
        self.sort = sort;
        self.entries.sort_by(|a, b| order(a, b, sort));
        self.rebuild_results();
        self.restore(anchor);
    }

    pub fn set_mode(&mut self, mode: ListingMode) {
        self.mode = if self.is_results() { ListingMode::Full } else { mode };
        self.clamp();
    }

    pub(super) fn commit(&mut self) {
        let Some(incoming) = self.incoming.take() else { return };
        self.generation = incoming.generation;
        if incoming.focus.is_some() || !incoming.refresh {
            self.focus = incoming.focus;
        }
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

    pub(super) fn is_reloading_into(&self, dir: &Path) -> bool {
        dir == self.dir && self.generation != INITIAL_GENERATION
    }

    pub(super) fn prune_marks(&mut self) {
        if self.marks.is_empty() {
            return;
        }
        let present: HashSet<&Path> = self.entries.iter().map(Entry::identity).collect();
        self.marks.retain(|name| present.contains(name.as_path()));
    }

    pub(super) fn restore(&mut self, anchor: Option<Anchor>) {
        if let Some(index) = anchor.as_ref().and_then(|anchor| self.locate(anchor)) {
            self.cursor = index;
            if self.focus.as_ref() == anchor.as_ref() {
                self.focus = None;
            }
        }
        self.clamp();
    }

    pub(super) fn parent_rows(&self) -> usize {
        // A container's root still has `..`, which returns to the host folder.
        usize::from(
            !self.is_results() && (self.dir.parent().is_some() || self.mount.is_some() || self.remote_mount.is_some()),
        )
    }

    pub(super) fn anchor(&self) -> Option<Anchor> {
        self.hovered().map(|row| match row {
            Row::Parent => Anchor::Parent,
            Row::Entry(entry) => Anchor::Path(entry.identity().to_path_buf()),
            Row::Group { label, .. } => Anchor::Name(label.to_owned()),
        })
    }

    pub(super) fn locate(&self, anchor: &Anchor) -> Option<usize> {
        if self.is_results() {
            return (0..self.len()).find(|index| match (self.row(*index), anchor) {
                (Some(Row::Entry(entry)), Anchor::Path(path)) => entry.identity() == path,
                (Some(Row::Group { label, .. }), Anchor::Name(name)) => label == name,
                _ => false,
            });
        }
        match anchor {
            Anchor::Path(path) => self
                .entries
                .iter()
                .position(|entry| entry.identity() == path || entry.path(&self.dir) == *path)
                .map(|i| i + self.parent_rows()),
            Anchor::Parent => (self.parent_rows() > 0).then_some(0),
            Anchor::Name(name) => {
                self.entries.iter().position(|entry| &entry.name == name).map(|i| i + self.parent_rows())
            }
        }
    }
}

pub fn order(a: &Entry, b: &Entry, sort: Sort) -> Ordering {
    let group = b.is_dir().cmp(&a.is_dir());
    let measured = if matches!(sort.key, SortKey::Size | SortKey::Files) && a.is_dir() && b.is_dir() {
        b.usage.is_some().cmp(&a.usage.is_some())
    } else {
        Ordering::Equal
    };
    let within = match sort.key {
        SortKey::Name => Ordering::Equal,
        SortKey::Extension => folded_cmp(extension(&a.name), extension(&b.name)),
        SortKey::Size => a.size.cmp(&b.size),
        SortKey::Modified => a.modified.cmp(&b.modified),
        SortKey::Files => a.usage.as_ref().map(|u| u.files).cmp(&b.usage.as_ref().map(|u| u.files)),
        SortKey::Source => a.source.as_ref().and_then(|p| p.parent()).cmp(&b.source.as_ref().and_then(|p| p.parent())),
    }
    .then_with(|| folded_cmp(&a.name, &b.name))
    .then_with(|| a.name.cmp(&b.name))
    .then_with(|| a.identity().cmp(b.identity()));
    group.then(measured).then(if sort.reverse { within.reverse() } else { within })
}

pub(super) fn extension(name: &str) -> &str {
    name.rsplit_once(EXTENSION_SEPARATOR).filter(|(stem, _)| !stem.is_empty()).map_or("", |(_, ext)| ext)
}

pub(super) fn folded_cmp(a: &str, b: &str) -> Ordering {
    if a.is_ascii() && b.is_ascii() {
        a.bytes().map(|c| c.to_ascii_lowercase()).cmp(b.bytes().map(|c| c.to_ascii_lowercase()))
    } else {
        a.chars().flat_map(char::to_lowercase).cmp(b.chars().flat_map(char::to_lowercase))
    }
}

pub(super) fn merge(left: Vec<Entry>, right: Vec<Entry>, sort: Sort) -> Vec<Entry> {
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
