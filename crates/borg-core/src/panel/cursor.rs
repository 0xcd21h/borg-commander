//! Moving a panel's cursor and marking its entries.

use super::{Anchor, Panel, Row, Seek};
use crate::Pattern;
use borg_vfs::Entry;
use std::path::PathBuf;

impl Panel {
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
}
