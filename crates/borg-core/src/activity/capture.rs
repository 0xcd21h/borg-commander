//! What the monitor asks to read next: memory to capture, files to preview, snapshots to lay out.

use super::{
    Activity, ActivityPage, ActivityRequest, Finding, MemoryRegion, MemorySearchReport, PREVIEW_DELAY, SNAPSHOT_LIMIT,
    SnapshotRow, ZERO_ROWS,
};
use crate::Effect;
use std::{path::PathBuf, time::Instant};

impl Activity {
    pub(super) fn request(&mut self, request: ActivityRequest) -> Vec<Effect> {
        if !matches!(request, ActivityRequest::Sample) {
            self.preview_due = None;
        }
        self.next_id += 1;
        self.pending = Some((self.next_id, request.clone()));
        self.next = None;
        if request != ActivityRequest::Sample {
            self.error = None;
        }
        vec![Effect::Activity { id: self.next_id, request }]
    }

    /// The selected open file, when it is a regular file whose contents can be previewed.
    pub fn previewable_file(&self) -> Option<PathBuf> {
        let file = self.files.get(self.file_cursor).filter(|_| self.list == ActivityPage::Files)?;
        let path = PathBuf::from(&file.name);
        (file.kind == "REG" && path.is_absolute()).then_some(path)
    }

    /// Whether a preview of the selected open file is loading.
    pub fn file_preview_loading(&self) -> bool {
        self.file_previewing.is_some()
    }

    /// Whether a process is open for inspection: its memory map, open files, or matches.
    pub fn inspecting(&self) -> bool {
        self.list != ActivityPage::Processes
    }

    pub fn focused_page(&self) -> ActivityPage {
        if self.preview_focused { self.page } else { self.list }
    }

    /// The cursor of a list page.
    pub fn list_cursor(&self, page: ActivityPage) -> usize {
        match page {
            ActivityPage::Processes => self.cursor,
            ActivityPage::Search => self.search_cursor,
            ActivityPage::Integrity => self.finding_cursor,
            ActivityPage::Files => self.file_cursor,
            _ => self.region_cursor,
        }
    }

    pub(crate) fn schedule_preview(&mut self, now: Instant) {
        if self
            .pending
            .as_ref()
            .is_some_and(|(_, r)| matches!(r, ActivityRequest::Capture { .. } | ActivityRequest::Census { .. }))
        {
            self.pending = None;
            self.next = None;
        }
        self.snapshot = None;
        self.previous = None;
        self.census = None;
        self.census_cursor = 0;
        self.offset = 0;
        self.error = None;
        self.retry = None;
        self.from_search = self.list == ActivityPage::Search;
        self.preview_due = self.capture_request().is_some().then_some(now + PREVIEW_DELAY);
    }

    /// Moves the selected string to its next copy in memory; false when it has no other.
    pub(super) fn next_copy(&mut self) -> bool {
        let Some(MemorySearchReport { hits, strings: Some(tally), .. }) = self.search_report.as_mut() else {
            return false;
        };
        let Some(hit) = hits.get_mut(self.search_cursor).filter(|hit| hit.places.len() > 1) else { return false };
        hit.places.rotate_left(1);
        let (address, region) = hit.places[0];
        let Some(region) = tally.regions.get(region as usize) else { return false };
        hit.address = address;
        hit.region = region.clone();
        true
    }

    pub fn preview_waiting(&self) -> bool {
        self.preview_due.is_some()
            || self.pending.as_ref().is_some_and(|(_, request)| {
                matches!(
                    request,
                    ActivityRequest::Regions { .. } | ActivityRequest::Capture { .. } | ActivityRequest::Census { .. }
                )
            })
    }

    pub(super) fn capture_request(&self) -> Option<ActivityRequest> {
        let process = self.selected?;
        let around = |address: u64, region: &MemoryRegion| {
            let start = address.saturating_sub(32).max(region.start);
            let end = start.saturating_add(SNAPSHOT_LIMIT as u64).min(region.end);
            (MemoryRegion { start, end, ..region.clone() }, false)
        };
        let (region, seek) = if self.list == ActivityPage::Search {
            let hit = self.search_report.as_ref()?.hits.get(self.search_cursor)?;
            around(hit.address, &hit.region)
        } else if self.list == ActivityPage::Integrity {
            let finding = self.selected_finding()?;
            around(finding.address?, finding.region.as_ref()?)
        } else {
            (self.selected_region()?.clone(), true)
        };
        if self.map && self.list == ActivityPage::Regions {
            let mapped = self.regions.iter().filter(|r| r.readable()).map(|r| (r.start, r.end)).collect();
            return region.readable().then_some(ActivityRequest::Census { process, region, mapped, admin: false });
        }
        region.readable().then_some(ActivityRequest::Capture { process, region, admin: false, seek })
    }

    /// The region the shown capture comes from: the selected one, or the search match's.
    pub fn shown_region(&self) -> Option<&MemoryRegion> {
        match self.list {
            ActivityPage::Search => self.search_report.as_ref()?.hits.get(self.search_cursor).map(|hit| &hit.region),
            ActivityPage::Integrity => self.selected_finding()?.region.as_ref(),
            _ => self.selected_region(),
        }
    }

    pub fn selected_finding(&self) -> Option<&Finding> {
        self.integrity.as_ref()?.findings.get(self.finding_cursor)
    }

    /// The bytes the preview marks: the selected match, or the selected finding's evidence.
    pub fn marked(&self) -> Option<std::ops::Range<u64>> {
        let (address, length) = match self.list {
            ActivityPage::Search if self.from_search => {
                let hit = self.search_report.as_ref()?.hits.get(self.search_cursor)?;
                (hit.address, hit.length)
            }
            ActivityPage::Integrity => {
                let finding = self.selected_finding()?;
                (finding.address?, finding.length)
            }
            _ => return None,
        };
        Some(address..address.saturating_add(length as u64))
    }

    /// Captures the shown window again, so D compares the same addresses; the selection's start
    /// before anything was captured.
    pub(super) fn recapture_request(&self) -> Option<ActivityRequest> {
        if self.map {
            return self.capture_request();
        }
        self.snapshot
            .as_ref()
            .and_then(|shown| self.capture_at(shown.address, false))
            .or_else(|| self.capture_request())
    }

    /// Captures the shown region from `start`, for another window of it or the same one again.
    pub(super) fn capture_at(&self, start: u64, seek: bool) -> Option<ActivityRequest> {
        let process = self.snapshot.as_ref().map(|shown| shown.process).or(self.census.as_ref().map(|c| c.process))?;
        let region = self.shown_region()?;
        (start >= region.start && start < region.end).then(|| ActivityRequest::Capture {
            process,
            region: MemoryRegion { start, ..region.clone() },
            admin: false,
            seek,
        })
    }

    /// The capture as rows of the view: runs of zero rows fold into one, except where the
    /// compared capture differs or the selected search match lies.
    pub fn snapshot_rows(&self, columns: usize) -> Vec<SnapshotRow> {
        let Some(snapshot) = &self.snapshot else { return Vec::new() };
        let bytes = &snapshot.bytes;
        let columns = columns.max(1);
        let old = self.previous.as_ref().filter(|_| self.diff).map(|old| old.bytes.as_slice());
        let hit = self
            .search_report
            .as_ref()
            .filter(|_| self.from_search)
            .and_then(|report| report.hits.get(self.search_cursor))
            .and_then(|hit| {
                let start = usize::try_from(hit.address.checked_sub(snapshot.address)?).ok()?;
                Some(start..start.saturating_add(hit.length))
            });
        let zero = |start: usize| {
            let end = start + columns;
            end <= bytes.len()
                && bytes[start..end].iter().all(|&b| b == 0)
                && old.is_none_or(|old| old.get(start..end).is_none_or(|o| o.iter().all(|&b| b == 0)))
                && hit.as_ref().is_none_or(|hit| hit.end <= start || hit.start >= end)
        };
        let mut rows = Vec::new();
        let mut start = 0;
        while start < bytes.len() {
            if !zero(start) {
                rows.push(SnapshotRow::Bytes(start));
                start += columns;
                continue;
            }
            let mut end = start;
            while zero(end) {
                end += columns;
            }
            if (end - start) / columns >= ZERO_ROWS {
                rows.push(SnapshotRow::Zeros { start, len: end - start });
            } else {
                rows.extend((start..end).step_by(columns).map(SnapshotRow::Bytes));
            }
            start = end;
        }
        rows
    }

    /// The row where the next block of data after the top row starts, or the previous one's.
    pub(super) fn data_block(&self, columns: usize, forward: bool) -> Option<usize> {
        let rows = self.snapshot_rows(columns);
        let mut starts = (0..rows.len()).filter(|&i| {
            matches!(rows[i], SnapshotRow::Bytes(_)) && (i == 0 || matches!(rows[i - 1], SnapshotRow::Zeros { .. }))
        });
        if forward { starts.find(|&i| i > self.offset) } else { starts.rfind(|&i| i < self.offset) }
    }

    pub fn snapshot_columns(&self, width: usize) -> usize {
        let room = width.saturating_sub(18).max(1);
        if self.text { room.min(64) } else { [16, 8, 4, 2, 1].into_iter().find(|n| n * 4 <= room).unwrap_or(1) }
    }

    pub fn move_by(&mut self, delta: isize, page_rows: usize, width: usize) {
        match self.page {
            ActivityPage::Processes => {
                self.cursor = self.cursor.saturating_add_signed(delta).min(self.rows.len().saturating_sub(1));
                self.select();
            }
            ActivityPage::Search => {
                self.search_cursor = self
                    .search_cursor
                    .saturating_add_signed(delta)
                    .min(self.search_report.as_ref().map_or(0, |r| r.hits.len().saturating_sub(1)))
            }
            ActivityPage::Regions => {
                self.region_cursor =
                    self.region_cursor.saturating_add_signed(delta).min(self.visible.len().saturating_sub(1))
            }
            ActivityPage::Files => {
                self.file_cursor = self.file_cursor.saturating_add_signed(delta).min(self.files.len().saturating_sub(1))
            }
            ActivityPage::Integrity => {
                let findings = self.integrity.as_ref().map_or(0, |report| report.findings.len());
                self.finding_cursor = self.finding_cursor.saturating_add_signed(delta).min(findings.saturating_sub(1))
            }
            ActivityPage::Snapshot => {
                let end = self.snapshot_rows(self.snapshot_columns(width)).len().saturating_sub(page_rows.max(1));
                self.offset = self.offset.saturating_add_signed(delta).min(end);
            }
            _ => {}
        }
        self.keep_visible(self.page, page_rows);
    }
}
