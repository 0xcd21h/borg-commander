//! Search results and collections in a panel: their filter, groups, tree and size view, and the edits and refreshes they take.

use super::{Anchor, Listing, ListingMode, Panel, Row, SortKey, order};
use borg_vfs::Entry;
use std::{collections::HashSet, path::PathBuf};

impl Panel {
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
}
