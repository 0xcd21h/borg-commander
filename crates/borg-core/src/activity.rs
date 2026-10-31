use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    time::{Duration, Instant},
};
mod process;
pub use process::{MemoryProtection, ProcessInfo, ProcessKey, SystemSample};
mod memory;
pub use memory::{
    CENSUS_LEGEND, MemoryCensus, MemoryGroup, MemoryRegion, MemorySnapshot, MemoryTotals, PageClass, ProcessFile,
    ProcessFiles, RegionItem, RegionRow, census_grid,
};
use memory::{GroupEntry, Member};
mod integrity;
use integrity::check_integrity;
pub use integrity::{FINDING_NOTE, Finding, FindingKind, IntegrityReport, Severity};
mod strings;
use strings::merge_strings;
pub use strings::{
    Indicator, MemoryHit, MemorySearchForm, MemorySearchMode, MemorySearchReport, STRING_PLACES, StringsTally,
    junk_string, rank_strings, string_score,
};
mod request;
pub use request::{ActivityData, ActivityRequest};
mod keys;
pub(crate) use keys::{on_key, paste, route_key};
mod refresh;
pub(crate) use refresh::{open, receive, receive_file_preview, sync, sync_file_preview, wake_at};
mod capture;
mod tree;

pub const HISTORY: usize = 60;
pub const SNAPSHOT_LIMIT: usize = 64 * 1024;
pub const PROCESS_LIMIT: usize = 8192;
const REFRESH: Duration = Duration::from_secs(1);
const PREVIEW_DELAY: Duration = Duration::from_millis(120);
/// Zero rows in a row that fold into one.
const ZERO_ROWS: usize = 2;

/// Rows the selected open file's details take above its preview.
pub const FILE_PREVIEW_TOP: u16 = 5;
/// Rows a pane's frame, header and footer take from a list or preview of each page.
pub fn chrome_rows(page: ActivityPage) -> u16 {
    match page {
        ActivityPage::Snapshot => 8,
        ActivityPage::Search => 8,
        ActivityPage::Integrity => 7 + FINDING_NOTE,
        _ => 6,
    }
}

/// One row of the hex view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotRow {
    /// The bytes from this offset of the capture.
    Bytes(usize),
    /// Whole rows of zeros from `start`, folded into one row.
    Zeros { start: usize, len: usize },
}
impl SnapshotRow {
    pub fn start(self) -> usize {
        match self {
            Self::Bytes(start) | Self::Zeros { start, .. } => start,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
enum RowKey {
    Group(MemoryGroup),
    File(String),
    Region(u64),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityPage {
    Processes,
    Memory,
    Regions,
    Snapshot,
    Search,
    /// Integrity findings, in the list pane.
    Integrity,
    Files,
    FileDetails,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivitySort {
    Memory,
    Cpu,
    Growth,
    Name,
    Pid,
    Read,
    Write,
}
#[derive(Debug, Clone)]
pub struct MemoryHistory {
    pub values: Vec<u64>,
    pub first: u64,
    pub peak: u64,
}
#[derive(Debug, Clone)]
pub struct Activity {
    pub expanded: bool,
    pub side: Option<crate::Side>,
    pub preview_focused: bool,
    pub list: ActivityPage,
    /// The list that search matches and integrity findings go back to with Esc.
    pub results_origin: ActivityPage,
    preview_due: Option<Instant>,
    pub search_form: Option<MemorySearchForm>,
    pub search_report: Option<MemorySearchReport>,
    pub search_cursor: usize,
    pub search_progress: Option<(u64, u64, usize)>,
    pub last_search: Option<ActivityRequest>,
    pub from_search: bool,
    pub integrity: Option<IntegrityReport>,
    pub finding_cursor: usize,
    pub sample: SystemSample,
    pub histories: HashMap<ProcessKey, MemoryHistory>,
    pub selected: Option<ProcessKey>,
    pub rows: Vec<(usize, usize)>,
    pub cursor: usize,
    pub page: ActivityPage,
    pub sort: ActivitySort,
    pub reverse: bool,
    pub tree: bool,
    pub filter: String,
    pub filtering: bool,
    pub paused: bool,
    regions: Vec<MemoryRegion>,
    pub region_cursor: usize,
    grouped: bool,
    expanded_groups: HashSet<MemoryGroup>,
    expanded_files: HashSet<String>,
    /// The inspected program's executable, which identifies its own mappings.
    program: Option<String>,
    /// Regions grouped and ranked once per memory map.
    groups: Vec<GroupEntry>,
    /// The rows shown for the current grouping and expansion.
    visible: Vec<RegionItem>,
    totals: MemoryTotals,
    pub files: Vec<ProcessFile>,
    pub files_skipped: usize,
    pub files_loaded: bool,
    pub file_cursor: usize,
    /// The selected open file's preview, when it is a regular file, and the one being read.
    pub file_preview: Option<crate::Peeked>,
    file_previewing: Option<PathBuf>,
    pub tops: [usize; 5],
    pub snapshot: Option<MemorySnapshot>,
    pub previous: Option<MemorySnapshot>,
    pub text: bool,
    pub diff: bool,
    pub offset: usize,
    /// The preview shows the selected region's page map instead of bytes.
    pub map: bool,
    pub census: Option<MemoryCensus>,
    pub census_cursor: usize,
    pub error: Option<String>,
    pub retry: Option<ActivityRequest>,
    pub pending: Option<(u64, ActivityRequest)>,
    next_id: u64,
    next: Option<Instant>,
}
impl Default for Activity {
    fn default() -> Self {
        Self {
            expanded: false,
            side: None,
            preview_focused: false,
            list: ActivityPage::Processes,
            results_origin: ActivityPage::Processes,
            preview_due: None,
            search_form: None,
            search_report: None,
            search_cursor: 0,
            search_progress: None,
            last_search: None,
            from_search: false,
            integrity: None,
            finding_cursor: 0,
            sample: Default::default(),
            histories: Default::default(),
            selected: None,
            rows: Vec::new(),
            cursor: 0,
            page: ActivityPage::Processes,
            sort: ActivitySort::Memory,
            reverse: true,
            tree: false,
            filter: String::new(),
            filtering: false,
            paused: false,
            regions: Vec::new(),
            region_cursor: 0,
            grouped: true,
            expanded_groups: HashSet::new(),
            expanded_files: HashSet::new(),
            program: None,
            groups: Vec::new(),
            visible: Vec::new(),
            totals: MemoryTotals::default(),
            files: Vec::new(),
            files_skipped: 0,
            files_loaded: false,
            file_cursor: 0,
            file_preview: None,
            file_previewing: None,
            tops: [0; 5],
            snapshot: None,
            previous: None,
            text: false,
            diff: false,
            offset: 0,
            map: false,
            census: None,
            census_cursor: 0,
            error: None,
            retry: None,
            pending: None,
            next_id: 0,
            next: None,
        }
    }
}
impl Activity {
    fn selected_row(&self) -> Option<RegionRow> {
        self.visible.get(self.region_cursor).map(|item| item.row)
    }
    pub fn selected_region(&self) -> Option<&MemoryRegion> {
        match self.selected_row()? {
            RegionRow::Region(i) => self.regions.get(i),
            _ => None,
        }
    }
    pub fn selected_group(&self) -> Option<MemoryGroup> {
        match self.selected_row()? {
            RegionRow::Group(g) => Some(g),
            _ => None,
        }
    }
    /// The selected mapped file, as the index of its first region.
    pub fn selected_file(&self) -> Option<usize> {
        match self.selected_row()? {
            RegionRow::File(first) => Some(first),
            _ => None,
        }
    }
    fn selected_row_key(&self) -> Option<RowKey> {
        Some(match self.selected_row()? {
            RegionRow::Group(group) => RowKey::Group(group),
            RegionRow::File(first) => RowKey::File(self.file_key(first).to_owned()),
            RegionRow::Region(i) => RowKey::Region(self.regions.get(i)?.start),
        })
    }
    fn find_row(&self, key: &RowKey) -> Option<usize> {
        self.visible.iter().map(|item| &item.row).position(|row| match (row, key) {
            (RegionRow::Group(group), RowKey::Group(wanted)) => group == wanted,
            (RegionRow::File(first), RowKey::File(wanted)) => self.file_key(*first) == wanted,
            (RegionRow::Region(i), RowKey::Region(start)) => self.regions[*i].start == *start,
            _ => false,
        })
    }
    pub fn list_start(&self, page: ActivityPage, height: usize) -> usize {
        let (cursor, top) = match page {
            ActivityPage::Processes => (self.cursor, self.tops[0]),
            ActivityPage::Regions => (self.region_cursor, self.tops[1]),
            ActivityPage::Search => (self.search_cursor, self.tops[2]),
            ActivityPage::Files => (self.file_cursor, self.tops[3]),
            ActivityPage::Integrity => (self.finding_cursor, self.tops[4]),
            _ => return 0,
        };
        top.min(cursor).max(cursor.saturating_sub(height.max(1) - 1))
    }
    pub(crate) fn keep_visible(&mut self, page: ActivityPage, height: usize) {
        let top = self.list_start(page, height);
        let index = match page {
            ActivityPage::Processes => 0,
            ActivityPage::Regions => 1,
            ActivityPage::Search => 2,
            ActivityPage::Files => 3,
            ActivityPage::Integrity => 4,
            _ => return,
        };
        self.tops[index] = top;
    }
    pub fn process(&self) -> Option<&ProcessInfo> {
        self.sample.processes.iter().find(|p| Some(p.key) == self.selected)
    }
    /// Why macOS keeps the inspected process's memory from every program, when it does.
    pub fn protection(&self) -> Option<MemoryProtection> {
        self.process().and_then(|p| p.protection)
    }
    pub fn growth(&self, key: ProcessKey) -> i128 {
        self.histories.get(&key).map_or(0, |h| i128::from(*h.values.last().unwrap_or(&h.first)) - i128::from(h.first))
    }
    pub fn rebuild(&mut self) {
        let filter = self.filter.to_lowercase();
        let mut rows: Vec<_> = self
            .sample
            .processes
            .iter()
            .enumerate()
            .filter(|(_, p)| filter.is_empty() || format!("{} {}", p.name, p.key.pid).to_lowercase().contains(&filter))
            .map(|(i, _)| i)
            .collect();
        rows.sort_by(|&a, &b| {
            let (a, b) = (&self.sample.processes[a], &self.sample.processes[b]);
            let order = match self.sort {
                ActivitySort::Memory => a.resident.cmp(&b.resident),
                ActivitySort::Cpu => a.cpu.cmp(&b.cpu),
                ActivitySort::Growth => self.growth(a.key).cmp(&self.growth(b.key)),
                ActivitySort::Name => a.name.cmp(&b.name),
                ActivitySort::Pid => a.key.pid.cmp(&b.key.pid),
                ActivitySort::Read => a.read.cmp(&b.read),
                ActivitySort::Write => a.written.cmp(&b.written),
            };
            (if self.reverse { order.reverse() } else { order }).then(a.key.pid.cmp(&b.key.pid))
        });
        self.rows = if self.tree && filter.is_empty() {
            let pids: HashSet<_> = rows.iter().map(|&i| self.sample.processes[i].key.pid).collect();
            let mut children: HashMap<u32, Vec<usize>> = HashMap::new();
            let mut roots = Vec::new();
            for &i in &rows {
                match self.sample.processes[i]
                    .parent
                    .filter(|pid| pids.contains(pid) && *pid != self.sample.processes[i].key.pid)
                {
                    Some(parent) => children.entry(parent).or_default().push(i),
                    None => roots.push(i),
                }
            }
            let mut seen = HashSet::new();
            let mut out = Vec::new();
            for root in roots.into_iter().chain(rows.iter().copied()) {
                let mut stack = vec![(root, 0)];
                while let Some((i, depth)) = stack.pop() {
                    if seen.insert(i) {
                        out.push((i, depth));
                        if let Some(children) = children.get(&self.sample.processes[i].key.pid) {
                            stack.extend(children.iter().rev().map(|&c| (c, depth + 1)));
                        }
                    }
                }
            }
            out
        } else {
            rows.into_iter().map(|i| (i, 0)).collect()
        };
        self.cursor = self
            .rows
            .iter()
            .position(|&(i, _)| Some(self.sample.processes[i].key) == self.selected)
            .unwrap_or(self.cursor.min(self.rows.len().saturating_sub(1)));
        if self.list == ActivityPage::Processes || self.selected.is_none() {
            self.select();
        }
    }
    fn select(&mut self) {
        let selected = self.rows.get(self.cursor).map(|&(i, _)| self.sample.processes[i].key);
        if self.selected != selected {
            self.region_cursor = 0;
            self.expanded_groups.clear();
            self.expanded_files.clear();
            self.set_regions(Vec::new());
            self.files.clear();
            self.files_skipped = 0;
            self.files_loaded = false;
            self.file_cursor = 0;
            self.file_preview = None;
            self.file_previewing = None;
            self.tops[1..].fill(0);
            self.snapshot = None;
            self.previous = None;
            self.search_form = None;
            self.search_report = None;
            self.last_search = None;
            self.search_progress = None;
            self.integrity = None;
            self.finding_cursor = 0;
            self.preview_due = None;
            self.from_search = false;
            self.error = None;
            self.retry = None;
            if self.pending.as_ref().is_some_and(|(_, r)| !matches!(r, ActivityRequest::Sample)) {
                self.pending = None;
                self.next = None;
            }
        }
        self.selected = selected;
    }
}

#[cfg(test)]
mod tests;
