use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    time::{Duration, Instant},
};
mod process;
pub use process::{MemoryProtection, ProcessInfo, ProcessKey, SystemSample};
mod memory;
pub use memory::{
    CENSUS_ENTROPY, CENSUS_KINDS, CENSUS_LEGEND, CensusPage, MemoryCensus, MemoryGroup, MemoryRegion, MemorySnapshot,
    MemoryTotals, PageClass, ProcessFile, ProcessFiles, RegionItem, RegionRow, census_grid,
};
use memory::{GroupEntry, Member};
mod integrity;
use integrity::check_integrity;
pub use integrity::{FINDING_NOTE, Finding, FindingKind, IntegrityReport, Severity};
mod rules;
use rules::scan_with_rules;
pub use rules::{FindingsReport, RulesReport, evidence_text, problem_finding, rule_finding, rule_source};
mod strings;
use strings::merge_strings;
pub use strings::{
    Indicator, MemoryHit, MemorySearchForm, MemorySearchMode, MemorySearchReport, STRING_PLACES, StringsTally,
    junk_string, rank_strings, string_score,
};
mod request;
pub use request::{ActivityData, ActivityRequest};
mod scan;
pub use scan::{ScanChecks, ScanSubject, scan_ask};
mod keys;
pub(crate) use keys::{on_key, panels_action, paste, route_key};
mod refresh;
pub(crate) use refresh::{open, receive, receive_file_preview, sync, sync_file_preview, wake_at};
mod capture;
mod control;
mod cpu;
pub use control::{
    CONTROL_CHOICES, ControlChoice, ControlDialog, ControlOutcome, ControlReport, DEFAULT_NICE, LEAST_NICE, NICEST,
    PRIORITY_STEP, ProcessControl, ProcessSignal,
};
pub(crate) use control::{on_key as on_control_key, open as open_control};
pub use cpu::CpuAverage;
mod entropy;
mod family;
pub use family::{Branch, Kin, ListedProcess, Relative};
mod tree;
pub use entropy::{
    EntropyChoice, EntropyDialog, EntropyHit, EntropyReport, EntropyScan, EntropyScope, HIT_LIMIT, HitKind,
};
pub(crate) use entropy::{on_dialog_key as on_entropy_key, open as open_entropy, receive as receive_entropy};

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
        ActivityPage::Findings => 7 + FINDING_NOTE,
        _ => crate::screens::CHROME_ROWS,
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
    /// What the integrity check or rules found, in the list pane.
    Findings,
    Files,
    FileDetails,
    /// The hits of the entropy scan of every process, in the list pane.
    Entropy,
    /// The selected hit's details, in the preview pane.
    EntropyDetails,
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
    /// The list that search matches and findings go back to with Esc.
    pub results_origin: ActivityPage,
    preview_due: Option<Instant>,
    pub search_form: Option<MemorySearchForm>,
    /// What the memory search form held when it closed, for the next time it opens.
    pub(crate) search_draft: Option<MemorySearchForm>,
    pub search_report: Option<MemorySearchReport>,
    pub search_cursor: usize,
    pub search_progress: Option<(u64, u64, usize)>,
    pub last_search: Option<ActivityRequest>,
    pub from_search: bool,
    pub findings: Option<FindingsReport>,
    pub finding_cursor: usize,
    pub sample: SystemSample,
    pub histories: HashMap<ProcessKey, MemoryHistory>,
    /// Each process's CPU use averaged over the last few seconds, which the list shows and
    /// sorts by.
    cpu: HashMap<ProcessKey, CpuAverage>,
    /// The whole system's CPU use, averaged the same way.
    system_cpu: Option<CpuAverage>,
    pub selected: Option<ProcessKey>,
    pub rows: Vec<ListedProcess>,
    pub cursor: usize,
    pub page: ActivityPage,
    pub sort: ActivitySort,
    pub reverse: bool,
    pub tree: bool,
    /// The processes whose branches the tree keeps folded.
    folded: HashSet<ProcessKey>,
    pub filter: String,
    pub filtering: bool,
    /// The list leaves out processes that used no CPU in the last few seconds.
    pub hide_idle: bool,
    /// How many processes the list leaves out as idle.
    idle_hidden: usize,
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
    pub tops: [usize; 6],
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
    /// A process to select once a sample lists it, and whether to offer its signals then.
    revealing: Option<(u32, bool)>,
    /// The monitor was opened from the network screen, and Esc goes back to it.
    pub from_network: bool,
    /// The entropy scan of every process, which keeps running while the monitor shows
    /// something else.
    pub entropy: Option<EntropyScan>,
    /// The memory map was opened from a hit of the entropy scan, and Esc goes back to the hits.
    pub from_entropy: bool,
    landing: Option<entropy::Landing>,
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
            search_draft: None,
            search_report: None,
            search_cursor: 0,
            search_progress: None,
            last_search: None,
            from_search: false,
            findings: None,
            finding_cursor: 0,
            sample: Default::default(),
            histories: Default::default(),
            cpu: HashMap::new(),
            system_cpu: None,
            selected: None,
            rows: Vec::new(),
            cursor: 0,
            page: ActivityPage::Processes,
            sort: ActivitySort::Memory,
            reverse: true,
            tree: false,
            folded: HashSet::new(),
            filter: String::new(),
            filtering: false,
            hide_idle: false,
            idle_hidden: 0,
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
            tops: [0; 6],
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
            revealing: None,
            from_network: false,
            entropy: None,
            from_entropy: false,
            landing: None,
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
            ActivityPage::Findings => (self.finding_cursor, self.tops[4]),
            ActivityPage::Entropy => (self.entropy.as_ref().map_or(0, |scan| scan.cursor), self.tops[5]),
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
            ActivityPage::Findings => 4,
            ActivityPage::Entropy => 5,
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
    /// A process's CPU use averaged over the last few seconds, in hundredths of a percent of one
    /// core and rounded to the step the list shows, once a sample has measured it.
    pub fn cpu(&self, key: ProcessKey) -> Option<u32> {
        self.cpu.get(&key).map(CpuAverage::shown)
    }
    /// The whole system's CPU use, averaged like each process's, in hundredths of a percent.
    pub fn system_cpu(&self) -> Option<u32> {
        self.system_cpu.as_ref().map(CpuAverage::shown)
    }
    /// Whether a process used no CPU in the last few seconds. One not measured yet is not idle.
    pub fn idle(&self, key: ProcessKey) -> bool {
        self.cpu(key) == Some(0)
    }
    /// How many processes the list leaves out as idle.
    pub fn idle_hidden(&self) -> usize {
        self.idle_hidden
    }
    /// Averages each process's CPU and the system's into what the list shows, forgetting the
    /// processes that ended.
    pub(crate) fn average_cpu(&mut self, now: Instant) {
        let keys: HashSet<_> = self.sample.processes.iter().map(|p| p.key).collect();
        self.cpu.retain(|key, _| keys.contains(key));
        for process in &self.sample.processes {
            if let Some(cpu) = process.cpu {
                self.cpu.insert(process.key, CpuAverage::next(self.cpu.get(&process.key).copied(), cpu, now));
            }
        }
        let cores = self.sample.cpu.len();
        if cores > 0 {
            let total = self.sample.cpu.iter().map(|&core| u64::from(core)).sum::<u64>() / cores as u64;
            self.system_cpu = Some(CpuAverage::next(self.system_cpu, u32::try_from(total).unwrap_or(u32::MAX), now));
        }
    }
    /// The processes of `matched` the list keeps while idle ones are hidden: those that used CPU,
    /// and in the tree the processes they descend from, so the tree holds together.
    fn without_idle(&self, matched: Vec<usize>, tree: bool) -> Vec<usize> {
        if !self.hide_idle {
            return matched;
        }
        let processes = &self.sample.processes;
        let busy: Vec<usize> = matched.iter().copied().filter(|&index| !self.idle(processes[index].key)).collect();
        let mut kept: HashSet<usize> = busy.iter().copied().collect();
        if tree {
            let by_pid: HashMap<u32, usize> = processes.iter().enumerate().map(|(i, p)| (p.key.pid, i)).collect();
            for index in busy {
                let mut parent = processes[index].parent;
                while let Some(&up) = parent.and_then(|pid| by_pid.get(&pid)) {
                    if !kept.insert(up) {
                        break;
                    }
                    parent = processes[up].parent;
                }
            }
        }
        matched.into_iter().filter(|index| kept.contains(index)).collect()
    }
    pub fn rebuild(&mut self) {
        let filter = self.filter.to_lowercase();
        let tree = self.tree && filter.is_empty();
        let matched: Vec<_> = self
            .sample
            .processes
            .iter()
            .enumerate()
            .filter(|(_, p)| filter.is_empty() || format!("{} {}", p.name, p.key.pid).to_lowercase().contains(&filter))
            .map(|(i, _)| i)
            .collect();
        let matches = matched.len();
        let mut rows = self.without_idle(matched, tree);
        self.idle_hidden = matches - rows.len();
        rows.sort_by(|&a, &b| {
            let (a, b) = (&self.sample.processes[a], &self.sample.processes[b]);
            let order = match self.sort {
                ActivitySort::Memory => a.resident.cmp(&b.resident),
                ActivitySort::Cpu => self.cpu(a.key).cmp(&self.cpu(b.key)),
                ActivitySort::Growth => self.growth(a.key).cmp(&self.growth(b.key)),
                ActivitySort::Name => a.name.cmp(&b.name),
                ActivitySort::Pid => a.key.pid.cmp(&b.key.pid),
                ActivitySort::Read => a.read.cmp(&b.read),
                ActivitySort::Write => a.written.cmp(&b.written),
            };
            (if self.reverse { order.reverse() } else { order }).then(a.key.pid.cmp(&b.key.pid))
        });
        self.forget_ended_folds();
        self.rows = if tree {
            family::tree(&self.sample.processes, &rows, &self.folded)
        } else {
            rows.into_iter().map(|index| ListedProcess { index, depth: 0, branch: Branch::Leaf }).collect()
        };
        self.cursor = self
            .rows
            .iter()
            .position(|row| Some(self.sample.processes[row.index].key) == self.selected)
            .unwrap_or(self.cursor.min(self.rows.len().saturating_sub(1)));
        if self.list == ActivityPage::Processes || self.selected.is_none() {
            self.select();
        }
    }
    /// Whether the monitor is waiting for the next sample to select `pid`.
    pub fn reveals(&self, pid: u32) -> bool {
        self.revealing.is_some_and(|(waiting, _)| waiting == pid)
    }

    /// Selects the process with `pid` once the next sample lists it, clearing a filter and
    /// showing idle processes so neither hides it, and with `control` offers its signals and
    /// priority then.
    pub(crate) fn reveal(&mut self, pid: u32, control: bool) {
        self.filter.clear();
        self.filtering = false;
        self.hide_idle = false;
        self.revealing = Some((pid, control));
    }
    fn select_pid(&mut self, pid: u32) -> bool {
        let Some(cursor) = self.rows.iter().position(|row| self.sample.processes[row.index].key.pid == pid) else {
            return false;
        };
        self.cursor = cursor;
        self.select();
        true
    }
    fn select(&mut self) {
        let selected = self.rows.get(self.cursor).map(|row| self.sample.processes[row.index].key);
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
            self.tops[1..5].fill(0);
            self.snapshot = None;
            self.previous = None;
            self.search_form = None;
            self.search_report = None;
            self.last_search = None;
            self.search_progress = None;
            self.findings = None;
            self.finding_cursor = 0;
            self.preview_due = None;
            self.from_search = false;
            self.error = None;
            self.retry = None;
            self.from_entropy = false;
            self.landing = None;
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
