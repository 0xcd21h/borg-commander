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
    /// The container whose filesystem the panel shows, if any.
    pub mount: Option<crate::Mount>,
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
            mount: None,
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
        (!self.is_results() && self.dir() == self.destination()).then_some(self.git.repository.as_ref()).flatten()
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

    pub fn navigate(&mut self, dir: PathBuf, generation: u64, focus: Option<String>) {
        self.results = None;
        let refresh = self.is_reloading_into(&dir);
        self.incoming = Some(Incoming { dir, generation, focus: focus.map(Anchor::Name), refresh });
    }

    pub fn reload(&mut self, generation: u64) {
        // A folder still loading keeps the entry it was opened to select.
        let focus = match &self.incoming {
            Some(incoming) => incoming.focus.clone(),
            None => self.anchor(),
        };
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

    fn replace_entries(&mut self) {
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

    pub fn point_at_path(&mut self, path: PathBuf) {
        self.focus = None;
        self.restore(Some(Anchor::Path(path)));
    }

    pub fn point_at(&mut self, name: &str) {
        self.focus = None;
        self.restore(Some(Anchor::Name(name.to_owned())));
    }

    pub fn toggle_mark(&mut self) {
        if let Some(Row::Group { label, .. }) = self.hovered() {
            let label = label.to_owned();
            let Some(results) = &self.results else { return };
            let paths: Vec<_> = self
                .entries
                .iter()
                .filter(|entry| results.matches(entry) && results.group_label(entry) == label)
                .map(|entry| entry.identity().to_path_buf())
                .collect();
            let clear = paths.iter().all(|path| self.marks.contains(path));
            for path in paths {
                if clear {
                    self.marks.remove(&path);
                } else {
                    self.marks.insert(path);
                }
            }
            return;
        }
        let Some(name) = self.hovered_entry().map(|entry| entry.identity().to_path_buf()) else { return };
        if !self.marks.remove(&name) {
            self.marks.insert(name);
        }
    }

    pub fn mark_matching(&mut self, pattern: &Pattern, mark: bool) -> usize {
        self.mark_where(|entry| pattern.matches(entry), mark)
    }

    pub fn mark_where(&mut self, accept: impl Fn(&Entry) -> bool, mark: bool) -> usize {
        let names: Vec<PathBuf> =
            self.visible_entries().filter(|entry| accept(entry)).map(|entry| entry.identity().to_path_buf()).collect();
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
        self.marks = self
            .visible_entries()
            .map(|e| e.identity().to_path_buf())
            .filter(|name| !self.marks.contains(name))
            .collect();
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
        let present: HashSet<&Path> = self.entries.iter().map(Entry::identity).collect();
        self.marks.retain(|name| present.contains(name.as_path()));
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
        // A container's root still has `..`, which returns to the host folder.
        usize::from(!self.is_results() && (self.dir.parent().is_some() || self.mount.is_some()))
    }

    fn anchor(&self) -> Option<Anchor> {
        self.hovered().map(|row| match row {
            Row::Parent => Anchor::Parent,
            Row::Entry(entry) => Anchor::Path(entry.identity().to_path_buf()),
            Row::Group { label, .. } => Anchor::Name(label.to_owned()),
        })
    }

    fn locate(&self, anchor: &Anchor) -> Option<usize> {
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

    pub fn is_results(&self) -> bool {
        self.results.is_some()
    }

    pub fn visible_entries(&self) -> impl Iterator<Item = &Entry> {
        let visible = self.results.as_ref().filter(|r| r.tree.is_some()).map(|r| {
            r.rows
                .iter()
                .filter_map(|row| match row {
                    crate::ResultRow::Entry(i) => Some(*i),
                    _ => None,
                })
                .collect::<HashSet<_>>()
        });
        self.entries
            .iter()
            .enumerate()
            .filter(move |(index, entry)| {
                visible
                    .as_ref()
                    .map_or_else(|| self.results.as_ref().is_none_or(|r| r.matches(entry)), |set| set.contains(index))
            })
            .map(|(_, entry)| entry)
    }

    pub fn open_results(&mut self, source: crate::ResultSource, entries: Vec<Entry>, generation: u64) {
        self.incoming = None;
        self.pending.clear();
        self.replacing = false;
        self.entries = entries;
        self.generation = generation;
        self.results = Some(crate::Results::new(source));
        self.listing = Listing::Ready;
        self.mode = ListingMode::Full;
        self.cursor = 0;
        self.scroll = 0;
        self.focus = None;
        self.marks.clear();
        self.set_sort(self.sort);
    }

    pub fn rebuild_results(&mut self) {
        if let Some(results) = &mut self.results {
            results.rebuild(&self.entries);
        }
    }

    pub fn filter_results(&mut self, filter: String) {
        let anchor = self.anchor();
        if let Some(results) = &mut self.results {
            results.filter = filter;
        }
        self.rebuild_results();
        self.restore(anchor);
    }

    pub fn group_results(&mut self, group: crate::ResultGroup) {
        if self.is_tree() {
            return;
        }
        let anchor = self.anchor();
        if let Some(results) = &mut self.results {
            results.group = group;
            results.collapsed.clear();
        }
        self.rebuild_results();
        self.restore(anchor);
    }

    pub fn toggle_group(&mut self) {
        let Some(Row::Group { label, .. }) = self.hovered() else { return };
        let label = label.to_owned();
        if let Some(results) = &mut self.results
            && !results.collapsed.remove(&label)
        {
            results.collapsed.insert(label);
        }
        self.rebuild_results();
        self.clamp();
    }

    pub fn is_tree(&self) -> bool {
        self.results.as_ref().is_some_and(|r| r.tree.is_some())
    }

    pub fn is_size_view(&self) -> bool {
        matches!(self.results.as_ref().map(|r| &r.source),
            Some(crate::ResultSource::Search { request, .. }) if matches!(request.options.kind,
                crate::SearchKind::FolderTree | crate::SearchKind::FolderTotals))
    }

    pub fn toggle_tree_node(&mut self) {
        let Some(path) = self.hovered_entry().map(|e| e.identity().to_path_buf()) else { return };
        if let Some(tree) = self.results.as_mut().and_then(|r| r.tree.as_mut())
            && tree.branches.contains(&path)
            && !tree.open.remove(&path)
        {
            tree.open.insert(path);
        }
        self.rebuild_results();
        self.clamp();
    }

    pub fn tree_step(&mut self, expand: bool) {
        let Some(path) = self.hovered_entry().map(|e| e.identity().to_path_buf()) else { return };
        let Some(tree) = self.results.as_mut().and_then(|r| r.tree.as_mut()) else { return };
        let anchor = path.clone();
        if expand {
            if !tree.branches.contains(&path) {
                return;
            }
            if !tree.open.insert(path.clone()) {
                if let Some(index) = (self.cursor + 1..self.len()).find(
                    |&i| matches!(self.row(i),Some(Row::Entry(next)) if next.identity().parent()==Some(path.as_path())),
                ) {
                    self.move_by(index as isize - self.cursor as isize);
                }
                return;
            }
        } else if !tree.open.remove(&path) {
            if let Some(parent) = path.parent()
                && let Some(index) =
                    (0..self.len()).find(|&i| matches!(self.row(i), Some(Row::Entry(e)) if e.identity() == parent))
            {
                self.cursor = index;
                self.clamp();
            }
            return;
        }
        self.rebuild_results();
        self.restore(Some(Anchor::Path(anchor)));
    }

    pub fn cancel_refresh(&mut self) {
        if self.replacing {
            self.pending.clear();
            self.replacing = false;
        }
    }

    pub fn remove_selected(&mut self) {
        let selected: HashSet<_> = self.selection().into_iter().map(|entry| entry.identity().to_path_buf()).collect();
        self.entries.retain(|entry| !selected.contains(entry.identity()));
        self.prune_marks();
        self.rebuild_results();
        self.clamp();
    }

    pub fn append_results(&mut self, entries: Vec<Entry>) {
        let mut present: HashSet<_> = self.entries.iter().map(|entry| entry.identity().to_path_buf()).collect();
        self.entries.extend(entries.into_iter().filter(|entry| present.insert(entry.identity().to_path_buf())));
        self.set_sort(self.sort);
    }

    pub fn replace_collection(&mut self, entries: Vec<Entry>) {
        let anchor = self.anchor();
        self.entries = entries;
        self.prune_marks();
        self.entries.sort_by(|a, b| order(a, b, self.sort));
        self.rebuild_results();
        self.restore(anchor);
    }

    pub fn begin_search(&mut self, generation: u64) {
        self.generation = generation;
        self.pending.clear();
        self.replacing = false;
        self.listing = Listing::Loading;
    }

    pub fn finish_results_edit(&mut self, generation: u64) {
        self.generation = generation;
        self.pending.clear();
        self.replacing = false;
        self.listing = Listing::Ready;
    }

    pub fn begin_results_refresh(&mut self, generation: u64) {
        self.cache_observations();
        if let Some(tree) = self.results.as_mut().and_then(|r| r.tree.as_mut()) {
            tree.loaded = HashSet::from([tree.root.clone()]);
            tree.pending.clear();
        }
        self.generation = generation;
        self.pending.clear();
        self.replacing = true;
        self.listing = Listing::Loading;
    }

    pub(crate) fn measure_sizes(&mut self, totals: &std::collections::HashMap<PathBuf, (u64, borg_vfs::FolderUsage)>) {
        if self.is_results() {
            return;
        }
        let pending_dir = self.incoming.as_ref().map_or(&self.dir, |incoming| &incoming.dir);
        if !totals
            .keys()
            .any(|path| path.parent() == Some(self.dir.as_path()) || path.parent() == Some(pending_dir.as_path()))
        {
            return;
        }
        let mut changed = false;
        for entry in &mut self.entries {
            if entry.kind == borg_vfs::EntryKind::Dir
                && let Some((bytes, usage)) = totals.get(&entry.path(&self.dir))
            {
                entry.size = *bytes;
                entry.usage = Some(usage.clone());
                changed = true;
            }
        }
        let pending_dir = self.incoming.as_ref().map_or(&self.dir, |incoming| &incoming.dir);
        for entry in &mut self.pending {
            if entry.kind == borg_vfs::EntryKind::Dir
                && let Some((bytes, usage)) = totals.get(&entry.path(pending_dir))
            {
                entry.size = *bytes;
                entry.usage = Some(usage.clone());
            }
        }
        if changed && matches!(self.sort.key, SortKey::Size | SortKey::Files) {
            self.set_sort(self.sort);
        }
    }

    pub fn cache_observations(&mut self) {
        if !self.is_size_view() {
            return;
        }
        for entry in &mut self.entries {
            if let Some(usage) = &mut entry.usage {
                usage.cached = true;
            }
        }
        if self.listing == Listing::Ready
            && let Some(crate::Results { source: crate::ResultSource::Search { progress, .. }, .. }) = &mut self.results
        {
            progress.cached = true;
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
            source: None,
            missing: false,
            usage: None,
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
