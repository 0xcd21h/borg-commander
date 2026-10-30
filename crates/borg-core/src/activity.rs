use crate::{AppState, Effect};
use borg_i18n::tr;
use crossterm::event::{KeyCode, KeyEvent};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    time::{Duration, Instant},
};

pub const HISTORY: usize = 60;
pub const SNAPSHOT_LIMIT: usize = 64 * 1024;
pub const PROCESS_LIMIT: usize = 8192;
const REFRESH: Duration = Duration::from_secs(1);
const PREVIEW_DELAY: Duration = Duration::from_millis(120);
/// Zero rows in a row that fold into one.
const ZERO_ROWS: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProcessKey {
    pub pid: u32,
    pub start: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessInfo {
    pub key: ProcessKey,
    pub parent: Option<u32>,
    pub name: String,
    pub executable: Option<PathBuf>,
    pub cpu: Option<u32>,
    pub resident: Option<u64>,
    pub virtual_bytes: Option<u64>,
    pub read: Option<u64>,
    pub written: Option<u64>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemSample {
    pub processes: Vec<ProcessInfo>,
    pub total: u64,
    pub used: u64,
    pub swap_total: u64,
    pub swap_used: u64,
    pub cpu: Vec<u32>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryRegion {
    pub start: u64,
    pub end: u64,
    pub permissions: String,
    pub name: String,
    pub resident: Option<u64>,
    pub private: Option<u64>,
    pub shared: Option<u64>,
    pub swap: Option<u64>,
    /// Pages this process has written (macOS reports them; Linux reports private pages instead).
    #[serde(default)]
    pub dirty: Option<u64>,
    /// The file mapped into the region, when there is one.
    #[serde(default)]
    pub path: Option<String>,
}
impl MemoryRegion {
    pub fn readable(&self) -> bool {
        self.permissions.starts_with('r') && self.end > self.start
    }
    pub fn mapped(&self) -> u64 {
        self.end.saturating_sub(self.start)
    }
    /// Memory charged to this process alone: written (or private) pages plus swapped pages.
    /// Shared library pages that other processes also use are not part of it.
    pub fn footprint(&self) -> Option<u64> {
        match (self.dirty.or(self.private), self.swap) {
            (None, None) => None,
            (own, swap) => Some(own.unwrap_or(0).saturating_add(swap.unwrap_or(0))),
        }
    }
    /// The region type without the mapped file, for example `__TEXT` or `Stack · thread 2`.
    pub fn kind(&self) -> &str {
        match &self.path {
            Some(path) => self.name.strip_suffix(path.as_str()).map_or(&self.name, |k| k.trim_end_matches(" · ")),
            None => &self.name,
        }
    }
    pub fn group(&self, program: Option<&str>) -> MemoryGroup {
        let name = self.name.to_ascii_lowercase();
        let library = |path: &str| {
            let file = path.rsplit('/').next().unwrap_or(path);
            file.ends_with(".so") || file.contains(".so.") || file.ends_with(".dylib") || path.contains(".framework/")
        };
        if name.contains("guard")
            || name.contains("reserved")
            || !self.permissions.chars().any(|c| matches!(c, 'r' | 'w' | 'x'))
        {
            MemoryGroup::Reserved
        } else if let Some(path) = &self.path {
            if program == Some(path.as_str()) {
                MemoryGroup::Program
            } else if path.starts_with("/dev/shm/") || name.contains("shared memory") {
                MemoryGroup::Shared
            } else if self.kind().starts_with("__") || library(path) {
                MemoryGroup::Libraries
            } else {
                MemoryGroup::Files
            }
        } else if name.contains("stack") {
            MemoryGroup::Stacks
        } else if name.contains("malloc")
            || name.contains("heap")
            || name.starts_with("vm_allocate")
            || application_tag(&name)
            || (name.is_empty() && self.permissions.contains('w'))
        {
            MemoryGroup::Heap
        } else if name.contains("__") {
            MemoryGroup::Libraries
        } else if name.contains("shared") || self.permissions.ends_with('s') {
            MemoryGroup::Shared
        } else {
            MemoryGroup::Other
        }
    }
}
/// macOS reserves memory tags 240–255 for applications' own allocators, such as the
/// PartitionAlloc and V8 heaps of Chromium-based browsers.
fn application_tag(name: &str) -> bool {
    const APPLICATION_TAGS: std::ops::RangeInclusive<u8> = 240..=255;
    name.strip_prefix("memory tag ")
        .and_then(|rest| rest.split_whitespace().next()?.parse().ok())
        .is_some_and(|tag| APPLICATION_TAGS.contains(&tag))
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemoryGroup {
    Heap,
    Stacks,
    Program,
    Libraries,
    Files,
    Shared,
    Other,
    Reserved,
}
impl MemoryGroup {
    pub const ALL: [Self; 8] = [
        Self::Heap,
        Self::Stacks,
        Self::Program,
        Self::Libraries,
        Self::Files,
        Self::Shared,
        Self::Other,
        Self::Reserved,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Heap => tr!(memory_group_heap),
            Self::Stacks => tr!(memory_group_stacks),
            Self::Program => tr!(memory_group_program),
            Self::Libraries => tr!(memory_group_libraries),
            Self::Files => tr!(memory_group_files),
            Self::Shared => tr!(memory_group_shared),
            Self::Other => tr!(memory_group_other),
            Self::Reserved => tr!(memory_group_reserved),
        }
    }
    pub fn about(self) -> &'static str {
        match self {
            Self::Heap => tr!(memory_about_heap),
            Self::Stacks => tr!(memory_about_stacks),
            Self::Program => tr!(memory_about_program),
            Self::Libraries => tr!(memory_about_libraries),
            Self::Files => tr!(memory_about_files),
            Self::Shared => tr!(memory_about_shared),
            Self::Other => tr!(memory_about_other),
            Self::Reserved => tr!(memory_about_reserved),
        }
    }
    /// Groups whose regions are listed per mapped file rather than one by one.
    pub fn by_file(self) -> bool {
        matches!(self, Self::Libraries | Self::Files)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionRow {
    Group(MemoryGroup),
    /// A mapped file, identified by the index of its first region.
    File(usize),
    Region(usize),
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MemoryTotals {
    pub regions: usize,
    pub mapped: u64,
    pub resident: Option<u64>,
    pub footprint: Option<u64>,
}
impl MemoryTotals {
    fn add(&mut self, region: &MemoryRegion) {
        let sum = |total: Option<u64>, value: Option<u64>| match (total, value) {
            (None, None) => None,
            (a, b) => Some(a.unwrap_or(0).saturating_add(b.unwrap_or(0))),
        };
        self.regions += 1;
        self.mapped = self.mapped.saturating_add(region.mapped());
        self.resident = sum(self.resident, region.resident);
        self.footprint = sum(self.footprint, region.footprint());
    }
    pub fn of<'a>(regions: impl IntoIterator<Item = &'a MemoryRegion>) -> Self {
        let mut totals = Self::default();
        regions.into_iter().for_each(|r| totals.add(r));
        totals
    }
    fn rank(&self) -> (std::cmp::Reverse<u64>, std::cmp::Reverse<u64>) {
        (std::cmp::Reverse(self.footprint.unwrap_or(0)), std::cmp::Reverse(self.resident.unwrap_or(0)))
    }
}
/// A row inside a group, its totals, and the indices of its regions.
type Member = (RegionRow, MemoryTotals, Vec<usize>);
#[derive(Debug, Clone)]
struct GroupEntry {
    group: MemoryGroup,
    totals: MemoryTotals,
    members: Vec<Member>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegionItem {
    pub row: RegionRow,
    pub depth: usize,
    pub totals: MemoryTotals,
    /// Mapped files in a group listed per file.
    pub files: usize,
    pub expanded: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessFile {
    pub descriptor: String,
    pub access: String,
    pub kind: String,
    pub name: String,
    pub size: Option<u64>,
    pub offset: Option<u64>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessFiles {
    pub entries: Vec<ProcessFile>,
    pub skipped: usize,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemorySnapshot {
    pub process: ProcessKey,
    pub address: u64,
    pub bytes: Vec<u8>,
    pub captured: u64,
}
/// What a page of memory holds, as a census of a region finds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PageClass {
    /// The read failed.
    Unread,
    /// Never touched, or swapped out: not read, so the census changes nothing.
    Absent,
    Zero,
    /// One byte repeated, as padding or a fill pattern.
    Filler,
    Text,
    /// UTF-16 text: printable bytes with zeros between them.
    Wide,
    /// Many 8-byte words that point into the process's own memory.
    Pointers,
    /// In an executable mapping.
    Code,
    /// Bytes as varied as compressed or encrypted data.
    Dense,
    Data,
}
impl PageClass {
    pub const ALL: [Self; 10] = [
        Self::Pointers,
        Self::Text,
        Self::Wide,
        Self::Code,
        Self::Dense,
        Self::Data,
        Self::Filler,
        Self::Zero,
        Self::Absent,
        Self::Unread,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Unread => tr!(page_unread),
            Self::Absent => tr!(page_absent),
            Self::Zero => tr!(page_zero),
            Self::Filler => tr!(page_filler),
            Self::Text => tr!(page_text),
            Self::Wide => tr!(page_wide),
            Self::Pointers => tr!(page_pointers),
            Self::Code => tr!(page_code),
            Self::Dense => tr!(page_dense),
            Self::Data => tr!(page_data),
        }
    }
}
/// A region's pages by class. A large region is sampled: each entry stands for `stride` pages
/// and was judged by the first of them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryCensus {
    pub process: ProcessKey,
    pub start: u64,
    pub end: u64,
    pub page: u64,
    pub stride: u64,
    pub pages: Vec<PageClass>,
    /// Whether pages never touched or swapped out were told apart, rather than read as zeros.
    pub residency: bool,
}
impl MemoryCensus {
    pub fn address(&self, entry: usize) -> u64 {
        self.start.saturating_add(entry as u64 * self.stride * self.page).min(self.end.saturating_sub(1))
    }
    pub fn share(&self, class: PageClass) -> f32 {
        self.pages.iter().filter(|page| **page == class).count() as f32 / self.pages.len().max(1) as f32
    }
}
/// How the page map lays `entries` out in a pane of `width` × `height` cells: columns, rows,
/// and how many entries each cell stands for.
pub fn census_grid(entries: usize, width: usize, height: usize) -> (usize, usize, usize) {
    let columns = width.max(1);
    let cells = columns * height.max(1);
    let span = entries.div_ceil(cells).max(1);
    (columns, entries.div_ceil(span).div_ceil(columns), span)
}
/// Rows the page map's legend takes above the grid.
pub const CENSUS_LEGEND: usize = 3;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemorySearchMode {
    Text,
    Hex,
    Unicode,
    /// Every string of printable characters, or those containing the query.
    Strings,
    /// Strings that look like an indicator, such as a URL, a command line or a credential.
    Indicators,
}
impl MemorySearchMode {
    pub const ALL: [Self; 5] = [Self::Text, Self::Hex, Self::Unicode, Self::Strings, Self::Indicators];
    /// Whether the mode lists strings, which needs no query.
    pub fn strings(self) -> bool {
        matches!(self, Self::Strings | Self::Indicators)
    }
}
/// What a string in memory looks like it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Indicator {
    Url,
    Ip,
    Email,
    Path,
    Command,
    /// A private key, an access token, or a password assignment.
    Secret,
    /// A long run of base64.
    Encoded,
}
impl Indicator {
    pub fn label(self) -> &'static str {
        match self {
            Self::Url => tr!(indicator_url),
            Self::Ip => tr!(indicator_ip),
            Self::Email => tr!(indicator_email),
            Self::Path => tr!(indicator_path),
            Self::Command => tr!(indicator_command),
            Self::Secret => tr!(indicator_secret),
            Self::Encoded => tr!(indicator_encoded),
        }
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryHit {
    pub address: u64,
    pub length: usize,
    pub encoding: String,
    pub region: MemoryRegion,
    /// For a string, its first characters.
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub indicator: Option<Indicator>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemorySearchReport {
    pub process: ProcessKey,
    pub hits: Vec<MemoryHit>,
    pub scanned: u64,
    pub total: u64,
    pub skipped: u64,
    pub next: Option<u64>,
    pub cancelled: bool,
}
#[derive(Debug, Clone)]
pub struct MemorySearchForm {
    pub input: crate::LineInput,
    pub mode: MemorySearchMode,
    pub match_case: bool,
    pub snapshot_only: bool,
    pub selected: usize,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActivityRequest {
    Search {
        process: ProcessKey,
        query: String,
        mode: MemorySearchMode,
        match_case: bool,
        snapshot: Option<MemorySnapshot>,
        start: Option<u64>,
        admin: bool,
    },
    Sample,
    Regions {
        process: ProcessKey,
        admin: bool,
    },
    Files {
        process: ProcessKey,
        admin: bool,
    },
    Census {
        process: ProcessKey,
        region: MemoryRegion,
        /// The readable mappings, for telling pointers from other numbers.
        #[serde(default)]
        mapped: Vec<(u64, u64)>,
        admin: bool,
    },
    Capture {
        process: ProcessKey,
        region: MemoryRegion,
        admin: bool,
        /// When the window at the start is all zeros, read on to the first bytes of data.
        #[serde(default)]
        seek: bool,
    },
}
impl ActivityRequest {
    pub fn elevated(&self) -> Option<Self> {
        match self {
            Self::Sample => None,
            Self::Search { process, query, mode, match_case, snapshot, start, .. } => Some(Self::Search {
                process: *process,
                query: query.clone(),
                mode: *mode,
                match_case: *match_case,
                snapshot: snapshot.clone(),
                start: *start,
                admin: true,
            }),
            Self::Regions { process, .. } => Some(Self::Regions { process: *process, admin: true }),
            Self::Files { process, .. } => Some(Self::Files { process: *process, admin: true }),
            Self::Census { process, region, .. } => {
                Some(Self::Census { process: *process, region: region.clone(), mapped: Vec::new(), admin: true })
            }
            Self::Capture { process, region, seek, .. } => {
                Some(Self::Capture { process: *process, region: region.clone(), admin: true, seek: *seek })
            }
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActivityData {
    Search(MemorySearchReport),
    Sample(SystemSample),
    Regions(Vec<MemoryRegion>),
    Files(ProcessFiles),
    Snapshot(MemorySnapshot),
    Census(MemoryCensus),
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
    pub search_origin: ActivityPage,
    preview_due: Option<Instant>,
    pub search_form: Option<MemorySearchForm>,
    pub search_report: Option<MemorySearchReport>,
    pub search_cursor: usize,
    pub search_progress: Option<(u64, u64, usize)>,
    pub last_search: Option<ActivityRequest>,
    pub from_search: bool,
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
    pub tops: [usize; 4],
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
            search_origin: ActivityPage::Processes,
            preview_due: None,
            search_form: None,
            search_report: None,
            search_cursor: 0,
            search_progress: None,
            last_search: None,
            from_search: false,
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
            tops: [0; 4],
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
    pub fn regions(&self) -> &[MemoryRegion] {
        &self.regions
    }
    pub fn grouped(&self) -> bool {
        self.grouped
    }
    /// Totals of the whole memory map.
    pub fn region_totals(&self) -> MemoryTotals {
        self.totals
    }
    pub fn group_totals(&self, group: MemoryGroup) -> MemoryTotals {
        self.groups.iter().find(|g| g.group == group).map_or_else(MemoryTotals::default, |g| g.totals)
    }
    /// Replaces the memory map and ranks it once; moving through it reuses the result.
    pub fn set_regions(&mut self, regions: Vec<MemoryRegion>) {
        self.regions = regions;
        self.program = self.process().and_then(|p| p.executable.as_ref()).map(|p| p.to_string_lossy().into_owned());
        self.totals = MemoryTotals::of(&self.regions);
        self.groups = self.rank_groups();
        self.refresh_tree();
    }
    pub(crate) fn set_grouped(&mut self, grouped: bool) {
        self.grouped = grouped;
        self.refresh_tree();
    }
    pub fn region_group(&self, region: &MemoryRegion) -> MemoryGroup {
        region.group(self.program.as_deref())
    }
    /// Groups ordered by footprint (reservations last), each with its members: mapped files
    /// for groups listed per file, regions otherwise, ordered by footprint.
    fn rank_groups(&self) -> Vec<GroupEntry> {
        let mut buckets: HashMap<MemoryGroup, Vec<usize>> = HashMap::new();
        for (i, region) in self.regions.iter().enumerate() {
            buckets.entry(self.region_group(region)).or_default().push(i);
        }
        let totals = |indices: &[usize]| MemoryTotals::of(indices.iter().map(|&i| &self.regions[i]));
        let mut groups: Vec<_> = MemoryGroup::ALL
            .into_iter()
            .filter_map(|group| {
                let indices = buckets.remove(&group)?;
                let mut members: Vec<Member> = if group.by_file() {
                    let mut files: Vec<(usize, Vec<usize>)> = Vec::new();
                    let mut positions: HashMap<&str, usize> = HashMap::new();
                    for &i in &indices {
                        let at = *positions.entry(self.file_key(i)).or_insert_with(|| {
                            files.push((i, Vec::new()));
                            files.len() - 1
                        });
                        files[at].1.push(i);
                    }
                    files
                        .into_iter()
                        .map(|(first, members)| (RegionRow::File(first), totals(&members), members))
                        .collect()
                } else {
                    indices.iter().map(|&i| (RegionRow::Region(i), totals(&[i]), vec![i])).collect()
                };
                members.sort_by(|a, b| a.1.rank().cmp(&b.1.rank()).then_with(|| self.member_order(a.0, b.0)));
                Some(GroupEntry { group, totals: totals(&indices), members })
            })
            .collect();
        groups.sort_by_key(|g| (g.group == MemoryGroup::Reserved, g.totals.rank()));
        groups
    }
    fn member_order(&self, a: RegionRow, b: RegionRow) -> std::cmp::Ordering {
        match (a, b) {
            (RegionRow::File(a), RegionRow::File(b)) => self.file_name(a).cmp(self.file_name(b)),
            (RegionRow::Region(a), RegionRow::Region(b)) => self.regions[a].start.cmp(&self.regions[b].start),
            _ => std::cmp::Ordering::Equal,
        }
    }
    /// The members of a group, largest footprint first.
    pub fn group_members(&self, group: MemoryGroup) -> impl Iterator<Item = (RegionRow, MemoryTotals)> + '_ {
        self.groups.iter().filter(move |g| g.group == group).flat_map(|g| g.members.iter().map(|m| (m.0, m.1)))
    }
    fn refresh_tree(&mut self) {
        let item = |row, depth, totals, files, expanded| RegionItem { row, depth, totals, files, expanded };
        let single = |i: usize| MemoryTotals::of([&self.regions[i]]);
        if !self.grouped {
            self.visible =
                (0..self.regions.len()).map(|i| item(RegionRow::Region(i), 0, single(i), 0, false)).collect();
            return;
        }
        let mut items = Vec::new();
        for entry in &self.groups {
            let open = self.expanded_groups.contains(&entry.group);
            let files = if entry.group.by_file() { entry.members.len() } else { 0 };
            items.push(item(RegionRow::Group(entry.group), 0, entry.totals, files, open));
            if !open {
                continue;
            }
            for (row, totals, regions) in &entry.members {
                let file_open =
                    matches!(row, RegionRow::File(first) if self.expanded_files.contains(self.file_key(*first)));
                items.push(item(*row, 1, *totals, 0, file_open));
                if file_open {
                    items.extend(regions.iter().map(|&i| item(RegionRow::Region(i), 2, single(i), 0, false)));
                }
            }
        }
        self.visible = items;
    }
    /// The region list as shown: groups, open groups' members and open files' regions, or
    /// every region in address order when ungrouped.
    pub fn region_tree(&self) -> &[RegionItem] {
        &self.visible
    }
    fn file_key(&self, first: usize) -> &str {
        self.regions.get(first).map_or("", |r| r.path.as_deref().unwrap_or(&r.name))
    }
    /// The short name of a mapped file: its last path component.
    pub fn file_name(&self, first: usize) -> &str {
        let key = self.file_key(first);
        key.rsplit('/').find(|part| !part.is_empty()).unwrap_or(key)
    }
    fn file_entry(&self, first: usize) -> Option<(MemoryGroup, &Member)> {
        self.groups.iter().find_map(|g| g.members.iter().find(|m| m.0 == RegionRow::File(first)).map(|m| (g.group, m)))
    }
    /// The group listing a mapped file.
    pub fn file_group(&self, first: usize) -> Option<MemoryGroup> {
        self.file_entry(first).map(|(group, _)| group)
    }
    /// The regions mapped from the same file as the region at `first`, in address order.
    pub fn file_regions(&self, first: usize) -> Vec<&MemoryRegion> {
        self.file_entry(first).map(|(_, m)| m.2.iter().map(|&i| &self.regions[i]).collect()).unwrap_or_default()
    }
    pub fn region_rows(&self) -> Vec<RegionRow> {
        self.visible.iter().map(|item| item.row).collect()
    }
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
            _ => return,
        };
        self.tops[index] = top;
    }
    pub fn process(&self) -> Option<&ProcessInfo> {
        self.sample.processes.iter().find(|p| Some(p.key) == self.selected)
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
            self.tops[1..].fill(0);
            self.snapshot = None;
            self.previous = None;
            self.search_form = None;
            self.search_report = None;
            self.last_search = None;
            self.search_progress = None;
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
    fn request(&mut self, request: ActivityRequest) -> Vec<Effect> {
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
    /// Whether a process is open for inspection: its memory map, open files, or matches.
    pub fn inspecting(&self) -> bool {
        self.list != ActivityPage::Processes
    }
    pub fn focused_page(&self) -> ActivityPage {
        if self.preview_focused { self.page } else { self.list }
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
    pub fn preview_waiting(&self) -> bool {
        self.preview_due.is_some()
            || self.pending.as_ref().is_some_and(|(_, request)| {
                matches!(
                    request,
                    ActivityRequest::Regions { .. } | ActivityRequest::Capture { .. } | ActivityRequest::Census { .. }
                )
            })
    }
    fn capture_request(&self) -> Option<ActivityRequest> {
        let process = self.selected?;
        let (region, seek) = if self.list == ActivityPage::Search {
            let hit = self.search_report.as_ref()?.hits.get(self.search_cursor)?;
            let start = hit.address.saturating_sub(32).max(hit.region.start);
            let end = start.saturating_add(SNAPSHOT_LIMIT as u64).min(hit.region.end);
            (MemoryRegion { start, end, ..hit.region.clone() }, false)
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
        if self.list == ActivityPage::Search {
            self.search_report.as_ref()?.hits.get(self.search_cursor).map(|hit| &hit.region)
        } else {
            self.selected_region()
        }
    }
    /// Captures the shown window again, so D compares the same addresses; the selection's start
    /// before anything was captured.
    fn recapture_request(&self) -> Option<ActivityRequest> {
        if self.map {
            return self.capture_request();
        }
        self.snapshot
            .as_ref()
            .and_then(|shown| self.capture_at(shown.address, false))
            .or_else(|| self.capture_request())
    }
    /// Captures the shown region from `start`, for another window of it or the same one again.
    fn capture_at(&self, start: u64, seek: bool) -> Option<ActivityRequest> {
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
    fn data_block(&self, columns: usize, forward: bool) -> Option<usize> {
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
            ActivityPage::Snapshot => {
                let end = self.snapshot_rows(self.snapshot_columns(width)).len().saturating_sub(page_rows.max(1));
                self.offset = self.offset.saturating_add_signed(delta).min(end);
            }
            _ => {}
        }
        self.keep_visible(self.page, page_rows);
    }
}
pub(crate) fn open(state: &mut AppState) -> Vec<Effect> {
    if !state.settings.preferences.system_plugin {
        state.notice = Some(tr!(activity_disabled).into());
        return Vec::new();
    }
    state.overlay = None;
    crate::docker::close(state);
    state.activity.side = Some(state.active);
    state.activity.preview_focused = false;
    state.activity.paused = false;
    state.activity.page = ActivityPage::Memory;
    state.activity.list = ActivityPage::Processes;
    state.activity.preview_due = None;
    state.activity.request(ActivityRequest::Sample)
}
pub(crate) fn wake_at(state: &AppState) -> Option<Instant> {
    if state.activity.side.is_none() || state.view != crate::View::Panels || state.overlay.is_some() {
        return None;
    }
    state
        .activity
        .preview_due
        .into_iter()
        .chain((!state.activity.paused).then_some(state.activity.next).flatten())
        .min()
}
pub(crate) fn sync(state: &mut AppState, now: Instant) -> Vec<Effect> {
    if state.activity.side.is_none() || state.view != crate::View::Panels {
        if state.activity.side.is_none() {
            state.activity.preview_due = None;
        } else if state.activity.pending.as_ref().is_some_and(|(_, r)| matches!(r, ActivityRequest::Capture { .. })) {
            state.activity.preview_due = Some(now);
        }
        state.activity.pending = None;
        state.activity.next = None;
        return Vec::new();
    }
    if state.overlay.is_some() {
        return Vec::new();
    }
    let layout = state.layout(now);
    let side = state.activity.side.unwrap();
    let rect = layout.panels[side.other().index()];
    let list_rect = layout.panels[side.index()];
    let activity = &mut state.activity;
    if !list_rect.is_empty() {
        activity.keep_visible(
            activity.list,
            usize::from(list_rect.height.saturating_sub(if activity.list == ActivityPage::Search { 7 } else { 6 })),
        );
    }
    if activity.preview_due.is_some_and(|due| due <= now) {
        activity.preview_due = None;
        if activity.list == ActivityPage::Search
            && let Some(ActivityRequest::Search { snapshot: Some(snapshot), .. }) = &activity.last_search
        {
            if let Some(hit) = activity.search_report.as_ref().and_then(|r| r.hits.get(activity.search_cursor)) {
                let columns = activity.snapshot_columns(usize::from(rect.width.saturating_sub(2)));
                let at = hit.address.saturating_sub(snapshot.address) as usize;
                activity.snapshot = Some(snapshot.clone());
                let rows = activity.snapshot_rows(columns);
                let row = rows.iter().rposition(|row| row.start() <= at).unwrap_or(0);
                activity.offset = row.saturating_sub(1);
            }
        } else if let Some(request) = activity.capture_request() {
            return activity.request(request);
        }
    }
    if !activity.paused
        && activity.pending.is_none()
        && activity.preview_due.is_none()
        && activity.next.is_none_or(|next| now >= next)
    {
        return activity.request(ActivityRequest::Sample);
    }
    Vec::new()
}
pub(crate) fn receive(state: &mut AppState, id: u64, result: Result<ActivityData, String>, now: Instant) {
    let activity = &mut state.activity;
    let Some((pending, request)) = activity.pending.clone() else { return };
    if pending != id || activity.side.is_none() {
        return;
    }
    activity.pending = None;
    activity.next = Some(now + REFRESH);
    match result {
        Ok(ActivityData::Search(mut report)) => {
            if matches!(&request, ActivityRequest::Search { start: Some(_), .. })
                && let Some(previous) = &activity.search_report
            {
                report.scanned = report.scanned.saturating_add(previous.scanned).min(report.total);
                report.skipped = report.skipped.saturating_add(previous.skipped);
            }
            activity.retry = (report.skipped > 0).then(|| request.clone());
            activity.search_report = Some(report);
            activity.search_cursor = 0;
            activity.page = ActivityPage::Snapshot;
            activity.list = ActivityPage::Search;
            activity.preview_focused = false;
            activity.search_progress = None;
            activity.last_search = Some(request);
            let retry = activity.retry.clone();
            activity.schedule_preview(now);
            activity.retry = retry;
        }
        Ok(ActivityData::Sample(mut sample)) => {
            sample.processes.truncate(PROCESS_LIMIT);
            let keys: HashSet<_> = sample.processes.iter().map(|p| p.key).collect();
            activity.histories.retain(|key, _| keys.contains(key));
            for process in &sample.processes {
                if let Some(bytes) = process.resident {
                    let h = activity.histories.entry(process.key).or_insert_with(|| MemoryHistory {
                        values: Vec::new(),
                        first: bytes,
                        peak: bytes,
                    });
                    h.peak = h.peak.max(bytes);
                    h.values.push(bytes);
                    if h.values.len() > HISTORY {
                        h.values.remove(0);
                    }
                }
            }
            activity.sample = sample;
            activity.rebuild();
        }
        Ok(ActivityData::Regions(regions)) => {
            let selected = activity.selected_row_key();
            activity.set_regions(regions);
            activity.region_cursor = selected.and_then(|key| activity.find_row(&key)).unwrap_or(0);
            activity.retry = None;
            activity.schedule_preview(now);
        }
        Ok(ActivityData::Files(files)) => {
            let selected = activity.files.get(activity.file_cursor).map(|f| (f.descriptor.clone(), f.name.clone()));
            activity.file_cursor = selected
                .and_then(|key| files.entries.iter().position(|f| (f.descriptor.clone(), f.name.clone()) == key))
                .unwrap_or(0);
            activity.files = files.entries;
            activity.files_skipped = files.skipped;
            activity.files_loaded = true;
            activity.retry = None;
        }
        Ok(ActivityData::Census(census)) => {
            activity.census_cursor = activity.census_cursor.min(census.pages.len().saturating_sub(1));
            activity.census = Some(census);
            activity.retry = None;
        }
        Ok(ActivityData::Snapshot(snapshot)) => {
            activity.previous = activity
                .snapshot
                .take()
                .filter(|old| old.process == snapshot.process && old.address == snapshot.address);
            activity.snapshot = Some(snapshot);
            activity.page = ActivityPage::Snapshot;
            activity.offset = 0;
            activity.retry = None;
        }
        Err(error) => {
            activity.search_progress = None;
            activity.error = Some(error);
            if !matches!(request, ActivityRequest::Sample) {
                activity.retry = Some(request);
            }
        }
    }
}
pub(crate) fn on_key(state: &mut AppState, key: &KeyEvent) -> Vec<Effect> {
    let side = state.activity.side.unwrap_or(state.active);
    let rect =
        state.layout(state.epoch).panels[if state.activity.preview_focused { side.other() } else { side }.index()];
    let activity = &mut state.activity;
    if let Some(mut form) = activity.search_form.take() {
        let mut submit = false;
        match key.code {
            KeyCode::Esc => {
                activity.error = None;
                return Vec::new();
            }
            KeyCode::Tab | KeyCode::Down => form.selected = (form.selected + 1) % 4,
            KeyCode::BackTab | KeyCode::Up => form.selected = (form.selected + 3) % 4,
            KeyCode::Enter => submit = true,
            KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if form.selected == 1 => {
                let modes = MemorySearchMode::ALL;
                let at = modes.iter().position(|&mode| mode == form.mode).unwrap_or(0);
                let step = if key.code == KeyCode::Left { modes.len() - 1 } else { 1 };
                form.mode = modes[(at + step) % modes.len()];
            }
            KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if form.selected == 2 => {
                form.match_case = !form.match_case
            }
            KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if form.selected == 3 => {
                form.snapshot_only = !form.snapshot_only
            }
            _ if form.selected == 0 => {
                form.input.apply(key);
            }
            _ => {}
        }
        if submit {
            let valid = (form.mode.strings() || !form.input.text().is_empty())
                && form.input.text().len() <= 512
                && (form.mode != MemorySearchMode::Hex || {
                    let value = form.input.text().split_whitespace().collect::<String>();
                    !value.is_empty() && value.len() % 2 == 0 && value.chars().all(|c| c.is_ascii_hexdigit())
                });
            if !valid {
                activity.error = Some(tr!(memory_query_invalid).into());
            } else if let Some(process) = activity.selected {
                let snapshot =
                    if form.snapshot_only { activity.snapshot.clone().filter(|s| s.process == process) } else { None };
                if form.snapshot_only && snapshot.is_none() {
                    activity.error = Some(tr!(memory_no_snapshot).into());
                } else {
                    activity.search_progress = Some((0, 0, 0));
                    return activity.request(ActivityRequest::Search {
                        process,
                        query: form.input.text().to_owned(),
                        mode: form.mode,
                        match_case: form.match_case,
                        snapshot,
                        start: None,
                        admin: false,
                    });
                }
            }
        }
        activity.search_form = Some(form);
        return Vec::new();
    }
    // Typing edits the filter; the arrows and paging keys still move through the matches.
    let moving = matches!(
        key.code,
        KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown | KeyCode::Home | KeyCode::End
    );
    if activity.filtering && !moving {
        match key.code {
            KeyCode::Enter => activity.filtering = false,
            KeyCode::Esc => {
                activity.filtering = false;
                activity.filter.clear();
            }
            KeyCode::Backspace => {
                activity.filter.pop();
            }
            KeyCode::Char(c) => activity.filter.push(c),
            _ => {}
        }
        activity.rebuild();
        return Vec::new();
    }

    let rows = usize::from(rect.height.saturating_sub(match activity.page {
        ActivityPage::Snapshot => 8,
        ActivityPage::Search => 7,
        _ => 6,
    }))
    .max(1);
    let width = usize::from(rect.width.saturating_sub(2));
    if activity.page == ActivityPage::Snapshot
        && activity.map
        && let Some(effects) = map_key(activity, key.code, rows, width)
    {
        return effects;
    }
    match key.code {
        KeyCode::Down => activity.move_by(1, rows, width),
        KeyCode::Up => activity.move_by(-1, rows, width),
        KeyCode::PageDown => activity.move_by(rows as isize, rows, width),
        KeyCode::PageUp => activity.move_by(-(rows as isize), rows, width),
        KeyCode::Home => activity.move_by(isize::MIN, rows, width),
        KeyCode::End => activity.move_by(isize::MAX, rows, width),
        KeyCode::Char('/') if activity.page == ActivityPage::Processes => activity.filtering = true,
        KeyCode::Char('/')
            if !matches!(activity.page, ActivityPage::Processes | ActivityPage::Files | ActivityPage::FileDetails) =>
        {
            activity.search_form = Some(MemorySearchForm {
                input: crate::LineInput::default(),
                mode: MemorySearchMode::Text,
                match_case: true,
                snapshot_only: false,
                selected: 0,
            });
        }
        KeyCode::Char('c')
            if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL)
                && activity.pending.as_ref().is_some_and(|(_, r)| matches!(r, ActivityRequest::Search { .. })) =>
        {
            return vec![Effect::CancelMemorySearch];
        }
        KeyCode::Char('n' | 'N') if activity.page == ActivityPage::Search && activity.pending.is_none() => {
            if let (Some(report), Some(ActivityRequest::Search { process, query, mode, match_case, snapshot, .. })) =
                (&activity.search_report, &activity.last_search)
                && let Some(start) = report.next
            {
                return activity.request(ActivityRequest::Search {
                    process: *process,
                    query: query.clone(),
                    mode: *mode,
                    match_case: *match_case,
                    snapshot: snapshot.clone(),
                    start: Some(start),
                    admin: false,
                });
            }
        }
        KeyCode::Char(' ') => {
            activity.paused = !activity.paused;
            if !activity.paused && activity.pending.is_none() {
                return activity.request(ActivityRequest::Sample);
            }
        }
        KeyCode::Char('a' | 'A') if activity.pending.is_none() => {
            if let Some(request) = activity.retry.as_ref().and_then(ActivityRequest::elevated) {
                return activity.request(request);
            }
        }
        KeyCode::Char('r' | 'R') if activity.pending.is_none() => {
            if activity.page == ActivityPage::Snapshot {
                if let Some(request) = activity.recapture_request() {
                    return activity.request(request);
                }
            } else if activity.page == ActivityPage::Regions {
                if let Some(process) = activity.selected {
                    return activity.request(ActivityRequest::Regions { process, admin: false });
                }
            } else if matches!(activity.page, ActivityPage::Files | ActivityPage::FileDetails) {
                if let Some(process) = activity.selected {
                    return activity.request(ActivityRequest::Files { process, admin: false });
                }
            } else {
                return activity.request(ActivityRequest::Sample);
            }
        }
        KeyCode::Char('h' | 'H') if activity.page == ActivityPage::Snapshot => {
            activity.text = !activity.text;
            activity.offset = 0;
        }
        KeyCode::Char('d' | 'D') if activity.page == ActivityPage::Snapshot => activity.diff = !activity.diff,
        KeyCode::Char(c @ (']' | '[')) if activity.page == ActivityPage::Snapshot => {
            if let Some(row) = activity.data_block(activity.snapshot_columns(width), c == ']') {
                activity.offset = 0;
                activity.move_by(row as isize, rows, width);
            }
        }
        KeyCode::Char(c @ ('>' | '<')) if activity.page == ActivityPage::Snapshot && activity.pending.is_none() => {
            let start = activity.snapshot.as_ref().zip(activity.shown_region()).and_then(|(shown, region)| match c {
                '>' => Some(shown.address.saturating_add(shown.bytes.len() as u64)),
                _ => (shown.address > region.start)
                    .then(|| shown.address.saturating_sub(SNAPSHOT_LIMIT as u64).max(region.start)),
            });
            if let Some(request) = start.and_then(|start| activity.capture_at(start, c == '>')) {
                return activity.request(request);
            }
        }
        KeyCode::Char('e' | 'E') if activity.page == ActivityPage::Snapshot => {
            if let Some(snapshot) = &activity.snapshot {
                let path = state.panels[state.active.index()]
                    .dir()
                    .join(format!("memory-{}-{:x}-{}.bin", snapshot.process.pid, snapshot.address, snapshot.captured));
                return vec![Effect::ExportMemory { path, bytes: snapshot.bytes.clone() }];
            }
        }
        KeyCode::Char(c) if activity.page == ActivityPage::Processes => {
            let sort = match c.to_ascii_lowercase() {
                'm' => Some(ActivitySort::Memory),
                'c' => Some(ActivitySort::Cpu),
                'g' => Some(ActivitySort::Growth),
                'n' => Some(ActivitySort::Name),
                'p' => Some(ActivitySort::Pid),
                'i' => Some(ActivitySort::Read),
                'o' => Some(ActivitySort::Write),
                't' => {
                    activity.tree = !activity.tree;
                    None
                }
                _ => None,
            };
            if let Some(sort) = sort {
                if activity.sort == sort {
                    activity.reverse = !activity.reverse;
                } else {
                    activity.sort = sort;
                    activity.reverse = sort != ActivitySort::Name && sort != ActivitySort::Pid;
                }
            }
            activity.rebuild();
        }
        _ => {}
    }
    Vec::new()
}

/// Moves over the page map, or opens the page under the cursor in bytes with Enter.
fn map_key(activity: &mut Activity, code: KeyCode, rows: usize, width: usize) -> Option<Vec<Effect>> {
    let census = activity.census.as_ref()?;
    if code == KeyCode::Enter {
        let address = census.address(activity.census_cursor);
        let start = address - address % census.page.max(1);
        activity.map = false;
        return Some(activity.capture_at(start, false).map(|request| activity.request(request)).unwrap_or_default());
    }
    let height = rows.saturating_sub(CENSUS_LEGEND);
    let (columns, _, span) = census_grid(census.pages.len(), width, height);
    let cell = (activity.census_cursor / span) as isize;
    let last = (census.pages.len().saturating_sub(1) / span) as isize;
    let step = match code {
        KeyCode::Left => -1,
        KeyCode::Right => 1,
        KeyCode::Up => -(columns as isize),
        KeyCode::Down => columns as isize,
        KeyCode::PageUp => -((columns * height) as isize),
        KeyCode::PageDown => (columns * height) as isize,
        KeyCode::Home => -cell,
        KeyCode::End => last - cell,
        _ => return None,
    };
    activity.census_cursor = ((cell + step).clamp(0, last) as usize) * span;
    Some(Vec::new())
}

pub(crate) fn route_key(state: &mut AppState, key: &KeyEvent, now: Instant) -> Vec<Effect> {
    if key.modifiers == crossterm::event::KeyModifiers::ALT.union(crossterm::event::KeyModifiers::SHIFT)
        && matches!(key.code, KeyCode::Left | KeyCode::Right)
    {
        return crate::actions::perform(
            state,
            crate::Action::NudgeSplit(if key.code == KeyCode::Right {
                crate::keymap::SPLIT_STEP
            } else {
                -crate::keymap::SPLIT_STEP
            }),
            now,
        );
    }

    if key.code == KeyCode::Char('w') && key.modifiers == crossterm::event::KeyModifiers::ALT {
        state.activity.expanded = !state.activity.expanded;
        return Vec::new();
    }
    if state.activity.search_form.is_some() {
        return on_key(state, key);
    }
    if !state.activity.filtering && key.modifiers.is_empty() {
        match key.code {
            KeyCode::Char('1') => {
                if state.activity.selected.is_none() {
                    return Vec::new();
                }
                state.activity.preview_focused = false;
                if state.activity.pending.as_ref().is_some_and(|(_, r)| !matches!(r, ActivityRequest::Sample)) {
                    state.activity.pending = None;
                    state.activity.next = None;
                }
                if state.activity.regions.is_empty() {
                    return enter_map(state);
                }
                state.activity.list = ActivityPage::Regions;
                state.activity.page = ActivityPage::Snapshot;
                state.activity.schedule_preview(now);
                return Vec::new();
            }
            KeyCode::Char('2') => {
                let activity = &mut state.activity;
                let Some(process) = activity.selected else {
                    return Vec::new();
                };
                activity.list = ActivityPage::Files;
                activity.page = ActivityPage::FileDetails;
                activity.preview_focused = false;
                activity.preview_due = None;
                if !activity.files_loaded {
                    return activity.request(ActivityRequest::Files { process, admin: false });
                }
                if activity.pending.as_ref().is_some_and(|(_, r)| !matches!(r, ActivityRequest::Sample)) {
                    activity.pending = None;
                    activity.next = None;
                }
                return Vec::new();
            }
            _ => {}
        }
    }
    if key.code == KeyCode::Char('f') && key.modifiers == crossterm::event::KeyModifiers::CONTROL {
        if state.activity.list == ActivityPage::Files {
            return Vec::new();
        }
        state.activity.preview_focused = true;
        if state.activity.list != ActivityPage::Search {
            state.activity.search_origin = state.activity.list;
        }
        if state.activity.page == ActivityPage::Processes {
            state.activity.page = ActivityPage::Memory;
        }
        return on_key(state, &KeyCode::Char('/').into());
    }

    if key.code == KeyCode::Char('o') && key.modifiers == crossterm::event::KeyModifiers::CONTROL {
        return crate::actions::perform(state, crate::Action::ToggleShell, now);
    }
    if key.code == KeyCode::F(9) {
        return crate::actions::perform(state, crate::Action::OpenMenuBar, now);
    }
    if key.code == KeyCode::F(1) {
        return crate::actions::perform(state, crate::Action::About, now);
    }
    if key.code == KeyCode::F(10) {
        return crate::actions::perform(state, crate::Action::Quit, now);
    }
    if key.code == KeyCode::Char('m') && key.modifiers == crossterm::event::KeyModifiers::ALT {
        state.activity.side = None;
        return Vec::new();
    }
    if key.code == KeyCode::Tab {
        state.activity.filtering = false;
        state.activity.preview_focused = !state.activity.preview_focused;
        return Vec::new();
    }
    if key.code == KeyCode::Esc {
        let activity = &mut state.activity;
        activity.error = None;
        activity.retry = None;
        if activity.preview_focused {
            activity.preview_focused = false;
            return Vec::new();
        }
        if activity.list == ActivityPage::Processes {
            // Esc stops and clears a filter before it closes the monitor.
            if activity.filtering || !activity.filter.is_empty() {
                activity.filtering = false;
                activity.filter.clear();
                activity.rebuild();
            } else {
                activity.side = None;
            }
        } else {
            if activity.pending.as_ref().is_some_and(|(_, r)| !matches!(r, ActivityRequest::Sample)) {
                activity.pending = None;
                activity.next = None;
            }
            activity.preview_due = None;
            activity.search_progress = None;
            activity.list =
                if activity.list == ActivityPage::Search { activity.search_origin } else { ActivityPage::Processes };
            activity.page =
                if activity.list == ActivityPage::Processes { ActivityPage::Memory } else { ActivityPage::Snapshot };
            if activity.list == ActivityPage::Regions {
                activity.schedule_preview(now);
            }
        }
        return Vec::new();
    }
    if key.code == KeyCode::Enter && !state.activity.filtering {
        if state.activity.list == ActivityPage::Processes {
            return enter_map(state);
        }
        if state.activity.list == ActivityPage::Regions
            && (state.activity.selected_group().is_some() || state.activity.selected_file().is_some())
        {
            state.activity.preview_focused = false;
            toggle(&mut state.activity, true, now);
            return Vec::new();
        }
        if state.activity.preview_focused && state.activity.map && state.activity.census.is_some() {
            return on_key(state, key);
        }
        state.activity.preview_focused = true;
        if state.activity.preview_due.is_some() {
            state.activity.preview_due = Some(now);
        }
        return Vec::new();
    }
    if state.activity.list == ActivityPage::Regions && key.code == KeyCode::Char('m') && key.modifiers.is_empty() {
        state.activity.map = !state.activity.map;
        state.activity.schedule_preview(now);
        return Vec::new();
    }
    if !state.activity.preview_focused && state.activity.list == ActivityPage::Regions {
        match key.code {
            KeyCode::Char('g' | 'G') => {
                let address = state.activity.selected_region().map(|r| r.start);
                state.activity.set_grouped(!state.activity.grouped);
                state.activity.region_cursor =
                    address.and_then(|start| state.activity.find_row(&RowKey::Region(start))).unwrap_or(0);
                state.activity.tops[1] = 0;
                state.activity.schedule_preview(now);
                return Vec::new();
            }
            KeyCode::Left | KeyCode::Right => {
                toggle(&mut state.activity, key.code == KeyCode::Right, now);
                return Vec::new();
            }
            _ => {}
        }
    }
    if matches!(key.code, KeyCode::Char('r' | 'R'))
        && state.activity.preview_focused
        && state.activity.page == ActivityPage::Snapshot
    {
        if let Some(request) = state.activity.recapture_request() {
            return state.activity.request(request);
        }
        return Vec::new();
    }
    let page = state.activity.page;
    let context = state.activity.focused_page();
    let cursor =
        if context == ActivityPage::Regions { state.activity.region_cursor } else { state.activity.search_cursor };
    state.activity.page = context;
    let effects = on_key(state, key);
    state.activity.page = page;
    if state.activity.search_form.is_some() {
        if state.activity.list != ActivityPage::Search {
            state.activity.search_origin = state.activity.list;
        }
        state.activity.preview_focused = true;
    }
    if (context == ActivityPage::Regions && cursor != state.activity.region_cursor)
        || (context == ActivityPage::Search && cursor != state.activity.search_cursor)
    {
        state.activity.schedule_preview(now);
    }
    effects
}

/// Right opens the selected group or file and steps into it; Left closes the selected
/// group or file, or the one around the selected row, and selects it.
fn toggle(activity: &mut Activity, expand: bool, now: Instant) {
    if !activity.grouped {
        return;
    }
    let items = activity.region_tree();
    let Some(item) = items.get(activity.region_cursor) else { return };
    let parent = |depth: usize| items[..activity.region_cursor].iter().rposition(|i| i.depth < depth);
    let target = if expand || item.expanded { Some(activity.region_cursor) } else { parent(item.depth) };
    let Some((target, row)) = target.map(|t| (t, items[t].row)).filter(|(_, row)| !matches!(row, RegionRow::Region(_)))
    else {
        return;
    };
    let open = |activity: &mut Activity, row: RegionRow, open: bool| match row {
        RegionRow::Group(group) if open => _ = activity.expanded_groups.insert(group),
        RegionRow::Group(group) => _ = activity.expanded_groups.remove(&group),
        RegionRow::File(first) => {
            let key = activity.file_key(first).to_owned();
            if open {
                activity.expanded_files.insert(key)
            } else {
                activity.expanded_files.remove(&key)
            };
        }
        RegionRow::Region(_) => {}
    };
    open(activity, row, expand);
    activity.refresh_tree();
    activity.region_cursor = (target + usize::from(expand)).min(activity.visible.len().saturating_sub(1));
    activity.schedule_preview(now);
}

fn enter_map(state: &mut AppState) -> Vec<Effect> {
    let activity = &mut state.activity;
    let Some(process) = activity.selected else {
        return Vec::new();
    };
    activity.list = ActivityPage::Regions;
    activity.page = ActivityPage::Snapshot;
    activity.preview_focused = false;
    activity.search_origin = ActivityPage::Regions;
    activity.snapshot = None;
    activity.previous = None;
    activity.offset = 0;
    activity.from_search = false;
    activity.retry = None;
    activity.request(ActivityRequest::Regions { process, admin: false })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Appearance, Msg, Settings, Startup, ThemeChoice, update};
    fn state() -> (AppState, Instant) {
        let now = Instant::now();
        let (mut state, _) = AppState::new(
            Startup::at("/work".into()),
            120,
            32,
            Settings::default(),
            Appearance::new(vec![ThemeChoice { id: "midnight".into(), name: "Midnight".into() }], 0),
            now,
        );
        state.settings.preferences.git_plugin = false;
        (state, now)
    }
    fn process(pid: u32, start: u64, ram: u64) -> ProcessInfo {
        ProcessInfo {
            key: ProcessKey { pid, start },
            parent: None,
            name: format!("worker-{pid}"),
            executable: None,
            cpu: Some(100),
            resident: Some(ram),
            virtual_bytes: Some(ram * 2),
            read: Some(0),
            written: Some(0),
        }
    }
    fn sample(state: &mut AppState, processes: Vec<ProcessInfo>, now: Instant) {
        state.activity.request(ActivityRequest::Sample);
        let id = state.activity.pending.as_ref().unwrap().0;
        receive(state, id, Ok(ActivityData::Sample(SystemSample { processes, ..Default::default() })), now);
    }
    fn key(state: &mut AppState, code: KeyCode) -> Vec<Effect> {
        route_key(state, &code.into(), state.epoch)
    }
    #[test]
    fn samples_preserve_identity_selection_and_bound_history_without_inheriting_reused_pids() {
        let (mut state, now) = state();
        open(&mut state);
        sample(&mut state, vec![process(1, 10, 100), process(2, 10, 200)], now);
        assert_eq!(state.activity.selected, Some(ProcessKey { pid: 2, start: 10 }));
        key(&mut state, KeyCode::Down);
        for index in 0..80 {
            sample(&mut state, vec![process(1, 10, 100 + index), process(2, 10, 200)], now);
        }
        let key = ProcessKey { pid: 1, start: 10 };
        assert_eq!(state.activity.selected, Some(key));
        assert_eq!(state.activity.histories[&key].values.len(), HISTORY);
        assert_eq!(state.activity.histories[&key].peak, 179);
        assert_eq!(state.activity.growth(key), 79);
        sample(&mut state, vec![process(1, 20, 500)], now);
        assert!(!state.activity.histories.contains_key(&key));
        assert_eq!(state.activity.selected, Some(ProcessKey { pid: 1, start: 20 }));
        assert_eq!(state.activity.growth(ProcessKey { pid: 1, start: 20 }), 0);
    }
    #[test]
    fn inspection_denial_keeps_monitoring_and_only_an_explicit_retry_requests_admin() {
        let (mut state, now) = state();
        open(&mut state);
        sample(&mut state, vec![process(2, 10, 200)], now);
        let effects = key(&mut state, KeyCode::Enter);
        assert!(matches!(
            &effects[..],
            [Effect::Activity { request: ActivityRequest::Regions { admin: false, .. }, .. }]
        ));
        let id = state.activity.pending.as_ref().unwrap().0;
        receive(&mut state, id, Err("Process protected".into()), now);
        sample(&mut state, vec![process(2, 10, 210)], now);
        assert_eq!(state.activity.error.as_deref(), Some("Process protected"));
        assert!(state.activity.retry.is_some());
        let effects = key(&mut state, KeyCode::Char('a'));
        assert!(matches!(
            &effects[..],
            [Effect::Activity { request: ActivityRequest::Regions { admin: true, .. }, .. }]
        ));
        key(&mut state, KeyCode::Esc);
        assert_eq!(state.activity.page, ActivityPage::Memory);
        assert!(state.activity.retry.is_none());
    }
    #[test]
    fn closing_pausing_and_disabling_leave_no_monitor_timer_or_stale_data_updates() {
        let (mut state, now) = state();
        open(&mut state);
        let id = state.activity.pending.as_ref().unwrap().0;
        receive(&mut state, id + 1, Ok(ActivityData::Sample(SystemSample::default())), now);
        assert!(state.activity.pending.is_some());
        receive(&mut state, id, Ok(ActivityData::Sample(SystemSample::default())), now);
        assert_eq!(wake_at(&state), Some(now + REFRESH));
        key(&mut state, KeyCode::Char(' '));
        assert!(wake_at(&state).is_none());
        key(&mut state, KeyCode::Esc);
        sync(&mut state, now);
        assert!(state.activity.pending.is_none());
        assert!(wake_at(&state).is_none());
        receive(&mut state, id, Ok(ActivityData::Sample(SystemSample { total: 999, ..Default::default() })), now);
        assert_eq!(state.activity.sample.total, 0);
        state.settings.preferences.system_plugin = false;
        assert!(open(&mut state).is_empty());
        assert!(state.overlay.is_none());
    }
    #[test]
    fn tree_filter_handles_cycles_and_keeps_only_matching_processes() {
        let (mut state, now) = state();
        open(&mut state);
        let mut a = process(1, 10, 100);
        let mut b = process(2, 10, 200);
        a.parent = Some(2);
        b.parent = Some(1);
        sample(&mut state, vec![a, b], now);
        key(&mut state, KeyCode::Char('t'));
        assert_eq!(state.activity.rows.len(), 2);
        key(&mut state, KeyCode::Char('/'));
        key(&mut state, KeyCode::Char('2'));
        key(&mut state, KeyCode::Enter);
        assert_eq!(state.activity.rows.len(), 1);
        assert_eq!(state.activity.selected.unwrap().pid, 2);
        let effects = update(&mut state, Msg::Paste("rm -rf /".into()), now);
        assert!(!effects.iter().any(|e| matches!(e, Effect::Write(_))));
    }
    #[test]
    fn strings_modes_search_without_a_query_and_the_mode_cycles_both_ways() {
        let (mut state, now) = state();
        open(&mut state);
        sample(&mut state, vec![process(2, 10, 200)], now);
        let send = |state: &mut AppState, code| update(state, Msg::Key(KeyEvent::from(code)), now);
        send(&mut state, KeyCode::Tab);
        send(&mut state, KeyCode::Char('/'));
        send(&mut state, KeyCode::Enter);
        assert!(state.activity.error.is_some(), "a text search needs a query");
        send(&mut state, KeyCode::Tab);
        let mode = |state: &AppState| state.activity.search_form.as_ref().unwrap().mode;
        send(&mut state, KeyCode::Left);
        assert_eq!(mode(&state), MemorySearchMode::Indicators);
        send(&mut state, KeyCode::Left);
        assert_eq!(mode(&state), MemorySearchMode::Strings);
        send(&mut state, KeyCode::Right);
        assert_eq!(mode(&state), MemorySearchMode::Indicators);
        let effects = send(&mut state, KeyCode::Enter);
        assert!(
            effects.iter().any(|e| matches!(e, Effect::Activity { request: ActivityRequest::Search { mode: MemorySearchMode::Indicators, query, .. }, .. } if query.is_empty())),
            "{effects:?}"
        );
    }
    #[test]
    fn snapshots_compare_only_the_same_process_region_and_export_to_the_file_panel() {
        let (mut state, now) = state();
        open(&mut state);
        sample(&mut state, vec![process(2, 10, 200)], now);
        let process = state.activity.selected.unwrap();
        let region = MemoryRegion {
            start: 4096,
            end: 8192,
            permissions: "r--".into(),
            name: "heap".into(),
            resident: None,
            private: None,
            shared: None,
            swap: None,
            ..Default::default()
        };
        let request = ActivityRequest::Capture { process, region, admin: false, seek: false };
        for value in [1, 2] {
            state.activity.request(request.clone());
            let id = state.activity.pending.as_ref().unwrap().0;
            receive(
                &mut state,
                id,
                Ok(ActivityData::Snapshot(MemorySnapshot {
                    process,
                    address: 4096,
                    bytes: vec![value; 64],
                    captured: value.into(),
                })),
                now,
            );
        }
        assert_eq!(state.activity.previous.as_ref().unwrap().bytes[0], 1);
        assert_eq!(state.activity.snapshot.as_ref().unwrap().bytes[0], 2);
        state.activity.list = ActivityPage::Regions;
        state.activity.preview_focused = true;
        key(&mut state, KeyCode::Char('d'));
        assert!(state.activity.diff);
        assert!(
            matches!(&key(&mut state,KeyCode::Char('e'))[..],[Effect::ExportMemory {path,bytes}] if path.starts_with("/work") && bytes==&vec![2;64])
        );
        let pending = state.activity.request(request);
        assert_eq!(pending.len(), 1);
        let id = state.activity.pending.as_ref().unwrap().0;
        receive(
            &mut state,
            id,
            Ok(ActivityData::Snapshot(MemorySnapshot {
                process: ProcessKey { start: 11, ..process },
                address: 4096,
                bytes: vec![3],
                captured: 3,
            })),
            now,
        );
        assert!(state.activity.previous.is_none());
    }
    #[test]
    fn pane_focus_search_paste_and_snapshot_hits_never_reach_the_shell_or_live_memory() {
        let (mut state, now) = state();
        open(&mut state);
        sample(&mut state, vec![process(2, 10, 200)], now);
        let send = |state: &mut AppState, code| update(state, Msg::Key(KeyEvent::from(code)), now);
        send(&mut state, KeyCode::Tab);
        assert!(state.activity.preview_focused);
        assert_eq!(state.activity.page, ActivityPage::Memory);
        let process = state.activity.selected.unwrap();
        let snapshot = MemorySnapshot { process, address: 4096, bytes: b"--Borg--".to_vec(), captured: 1 };
        state.activity.snapshot = Some(snapshot.clone());
        send(&mut state, KeyCode::Char('/'));
        assert!(state.activity.search_form.is_some());
        let effects = update(&mut state, Msg::Paste("Borg".into()), now);
        assert!(!effects.iter().any(|e| matches!(e, Effect::Write(_))));
        send(&mut state, KeyCode::Tab);
        assert!(state.activity.preview_focused);
        assert_eq!(state.activity.search_form.as_ref().unwrap().selected, 1);
        state.activity.search_form.as_mut().unwrap().snapshot_only = true;
        let effects = send(&mut state, KeyCode::Enter);
        assert!(effects.iter().any(|e| matches!(e, Effect::Activity { request: ActivityRequest::Search { snapshot: Some(_), query, .. }, .. } if query == "Borg")));
        let id = state.activity.pending.as_ref().unwrap().0;
        update(&mut state, Msg::MemorySearchProgress { id: id + 1, scanned: 99, total: 100, hits: 2 }, now);
        assert_eq!(state.activity.search_progress, Some((0, 0, 0)));
        let region = MemoryRegion {
            start: 4096,
            end: 4104,
            permissions: "r--".into(),
            name: "Snapshot".into(),
            resident: None,
            private: None,
            shared: None,
            swap: None,
            ..Default::default()
        };
        receive(
            &mut state,
            id,
            Ok(ActivityData::Search(MemorySearchReport {
                process,
                hits: vec![MemoryHit {
                    address: 4098,
                    length: 4,
                    encoding: "UTF-8".into(),
                    region,
                    ..Default::default()
                }],
                scanned: 8,
                total: 8,
                skipped: 0,
                next: None,
                cancelled: false,
            })),
            now,
        );
        let effects = send(&mut state, KeyCode::Enter);
        assert!(
            !effects.iter().any(|e| matches!(e, Effect::Activity { request: ActivityRequest::Capture { .. }, .. }))
        );
        assert_eq!(state.activity.page, ActivityPage::Snapshot);
        assert_eq!(state.activity.snapshot, Some(snapshot));
        send(&mut state, KeyCode::Esc);
        assert_eq!(state.activity.list, ActivityPage::Search);
        assert!(!state.activity.preview_focused);
        assert!(crate::actions::perform(&mut state, crate::Action::Delete, now).is_empty());
        assert!(state.overlay.is_none());
        send(&mut state, KeyCode::Esc);
        assert_eq!(state.activity.list, ActivityPage::Processes);
        send(&mut state, KeyCode::Esc);
        assert!(state.activity.side.is_none());
    }

    #[test]
    fn partial_search_offers_admin_retry_and_cancellation_keeps_results() {
        let (mut state, now) = state();
        open(&mut state);
        sample(&mut state, vec![process(2, 10, 200)], now);
        state.activity.preview_focused = true;
        state.activity.page = ActivityPage::Memory;
        key(&mut state, KeyCode::Char('/'));
        state.activity.search_form.as_mut().unwrap().input = crate::LineInput::new("Borg");
        key(&mut state, KeyCode::Enter);
        let id = state.activity.pending.as_ref().unwrap().0;
        let process = state.activity.selected.unwrap();
        receive(
            &mut state,
            id,
            Ok(ActivityData::Search(MemorySearchReport {
                process,
                hits: vec![],
                scanned: 128,
                total: 8192,
                skipped: 4096,
                next: Some(8192),
                cancelled: true,
            })),
            now,
        );
        assert!(state.activity.retry.is_some());
        let effects = key(&mut state, KeyCode::Char('n'));
        assert!(matches!(
            &effects[..],
            [Effect::Activity { request: ActivityRequest::Search { start: Some(8192), .. }, .. }]
        ));
        let effects = on_key(&mut state, &KeyEvent::new(KeyCode::Char('c'), crossterm::event::KeyModifiers::CONTROL));
        assert!(matches!(&effects[..], [Effect::CancelMemorySearch]));
        assert!(state.activity.search_report.is_some());
    }
    #[test]
    fn finishing_filter_during_initial_sample_keeps_focus_and_selects_when_data_arrives() {
        let (mut state, now) = state();
        open(&mut state);
        for code in [KeyCode::Char('/'), KeyCode::Char('2'), KeyCode::Enter] {
            route_key(&mut state, &code.into(), now);
        }
        assert!(!state.activity.filtering);
        assert!(!state.activity.preview_focused);
        route_key(&mut state, &KeyCode::Enter.into(), now);
        sample(&mut state, vec![process(2, 10, 200)], now);
        assert!(state.activity.process().is_some());
        route_key(&mut state, &KeyCode::Enter.into(), now);
        assert_eq!(state.activity.list, ActivityPage::Regions);
        route_key(&mut state, &KeyCode::Char('/').into(), now);
        assert!(state.activity.search_form.is_some());
    }
    #[test]
    fn changing_process_cancels_its_inspection_and_clears_previous_memory() {
        let (mut state, now) = state();
        open(&mut state);
        sample(&mut state, vec![process(2, 10, 200), process(3, 10, 100)], now);
        let process = state.activity.selected.unwrap();
        let request = ActivityRequest::Regions { process, admin: false };
        state.activity.request(request.clone());
        let id = state.activity.pending.as_ref().unwrap().0;
        state.activity.error = Some("Access denied".into());
        state.activity.retry = Some(request);
        state.activity.snapshot = Some(MemorySnapshot { process, address: 100, bytes: vec![1], captured: 1 });
        route_key(&mut state, &KeyCode::Down.into(), now);
        assert_eq!(state.activity.selected.unwrap().pid, 3);
        assert!(state.activity.pending.is_none());
        assert!(state.activity.snapshot.is_none());
        assert!(state.activity.error.is_none() && state.activity.retry.is_none());
        receive(&mut state, id, Err("Late reply".into()), now);
        assert!(state.activity.error.is_none());
    }
    #[test]
    fn maps_drill_into_the_list_pane_and_debounce_bounded_previews_without_changing_focus() {
        let (mut state, now) = state();
        open(&mut state);
        state.activity.set_grouped(false);
        sample(&mut state, vec![process(2, 10, 200)], now);
        state.activity.paused = true;
        let selected = state.activity.selected;
        let effects = key(&mut state, KeyCode::Enter);
        assert!(matches!(&effects[..], [Effect::Activity { request: ActivityRequest::Regions { .. }, .. }]));
        let region = |start, permissions: &str| MemoryRegion {
            start,
            end: start + 1_000_000,
            permissions: permissions.into(),
            name: format!("region-{start}"),
            resident: None,
            private: None,
            shared: None,
            swap: None,
            ..Default::default()
        };
        let id = state.activity.pending.as_ref().unwrap().0;
        receive(
            &mut state,
            id,
            Ok(ActivityData::Regions(vec![region(0x1000, "r--"), region(0x200000, "---"), region(0x400000, "rw-")])),
            now,
        );
        assert_eq!(state.activity.list, ActivityPage::Regions);
        assert_eq!(state.activity.page, ActivityPage::Snapshot);
        assert!(!state.activity.preview_focused);
        assert!(sync(&mut state, now + PREVIEW_DELAY / 2).is_empty());
        let effects = sync(&mut state, now + PREVIEW_DELAY);
        assert!(
            matches!(&effects[..], [Effect::Activity { request: ActivityRequest::Capture { region, admin: false, .. }, .. }] if region.start == 0x1000)
        );
        let stale = state.activity.pending.as_ref().unwrap().0;
        let later = now + PREVIEW_DELAY * 2;
        route_key(&mut state, &KeyCode::Down.into(), later);
        assert!(state.activity.snapshot.is_none());
        assert!(sync(&mut state, later + PREVIEW_DELAY).is_empty());
        assert!(state.activity.pending.is_none());
        route_key(&mut state, &KeyCode::Down.into(), later + PREVIEW_DELAY);
        let latest = later + PREVIEW_DELAY * 2;
        let effects = sync(&mut state, latest);
        assert!(
            matches!(&effects[..], [Effect::Activity { request: ActivityRequest::Capture { region, .. }, .. }] if region.start == 0x400000)
        );
        let current = state.activity.pending.as_ref().unwrap().0;
        let snapshot = |address| MemorySnapshot {
            process: selected.unwrap(),
            address,
            bytes: b"Borg memory".to_vec(),
            captured: 1,
        };
        receive(&mut state, stale, Ok(ActivityData::Snapshot(snapshot(0x1000))), latest);
        assert!(state.activity.snapshot.is_none());
        receive(&mut state, current, Ok(ActivityData::Snapshot(snapshot(0x400000))), latest);
        assert!(!state.activity.preview_focused);
        assert_eq!(state.activity.list, ActivityPage::Regions);
        route_key(&mut state, &KeyCode::Tab.into(), latest);
        assert!(state.activity.preview_focused);
        route_key(&mut state, &KeyCode::Down.into(), latest);
        assert_eq!(state.activity.region_cursor, 2);
        route_key(&mut state, &KeyCode::Esc.into(), latest);
        assert_eq!(state.activity.list, ActivityPage::Regions);
        route_key(&mut state, &KeyCode::Esc.into(), latest);
        assert_eq!(state.activity.list, ActivityPage::Processes);
        assert_eq!(state.activity.selected, selected);
        assert_eq!(state.activity.region_cursor, 2);
    }
    #[test]
    fn zero_rows_fold_unless_the_compared_capture_or_the_selected_match_differs_there() {
        use SnapshotRow::{Bytes, Zeros};
        let (mut state, _) = state();
        let activity = &mut state.activity;
        let process = ProcessKey { pid: 2, start: 10 };
        let mut bytes = vec![0u8; 16 * 6];
        bytes[0] = 1;
        bytes[16 * 5] = 2;
        let snapshot = MemorySnapshot { process, address: 0x1000, bytes: bytes.clone(), captured: 1 };
        activity.snapshot = Some(snapshot.clone());
        assert_eq!(activity.snapshot_rows(16), [Bytes(0), Zeros { start: 16, len: 64 }, Bytes(80)]);
        let mut old = bytes;
        old[40] = 9;
        activity.previous = Some(MemorySnapshot { bytes: old, ..snapshot });
        activity.diff = true;
        assert_eq!(
            activity.snapshot_rows(16),
            [Bytes(0), Bytes(16), Bytes(32), Zeros { start: 48, len: 32 }, Bytes(80)]
        );
        activity.diff = false;
        activity.from_search = true;
        let region = MemoryRegion { start: 0x1000, end: 0x2000, permissions: "rw-".into(), ..Default::default() };
        let hit = MemoryHit { address: 0x1000 + 50, length: 2, encoding: "hex".into(), region, ..Default::default() };
        activity.search_report = Some(MemorySearchReport {
            process,
            hits: vec![hit],
            scanned: 0,
            total: 0,
            skipped: 0,
            next: None,
            cancelled: false,
        });
        assert_eq!(
            activity.snapshot_rows(16),
            [Bytes(0), Zeros { start: 16, len: 32 }, Bytes(48), Bytes(64), Bytes(80)]
        );
    }
    #[test]
    fn windows_move_through_a_region_and_recapture_reads_the_shown_window_again() {
        let (mut state, now) = state();
        open(&mut state);
        state.activity.set_grouped(false);
        sample(&mut state, vec![process(2, 10, 200)], now);
        state.activity.paused = true;
        key(&mut state, KeyCode::Enter);
        let id = state.activity.pending.as_ref().unwrap().0;
        let start = 0x10000;
        let window = SNAPSHOT_LIMIT as u64;
        let region = MemoryRegion { start, end: start + 4 * window, permissions: "rw-".into(), ..Default::default() };
        receive(&mut state, id, Ok(ActivityData::Regions(vec![region])), now);
        let capture = |effects: &[Effect]| match effects {
            [Effect::Activity { request: ActivityRequest::Capture { region, seek, .. }, .. }] => {
                Some((region.start, *seek))
            }
            _ => None,
        };
        assert_eq!(capture(&sync(&mut state, now + PREVIEW_DELAY)), Some((start, true)));
        let id = state.activity.pending.as_ref().unwrap().0;
        let shown = start + window;
        let process = state.activity.selected.unwrap();
        let bytes = vec![1; SNAPSHOT_LIMIT];
        receive(
            &mut state,
            id,
            Ok(ActivityData::Snapshot(MemorySnapshot { process, address: shown, bytes, captured: 1 })),
            now,
        );
        key(&mut state, KeyCode::Tab);
        assert!(state.activity.preview_focused);
        assert_eq!(capture(&key(&mut state, KeyCode::Char('>'))), Some((shown + window, true)));
        state.activity.pending = None;
        assert_eq!(capture(&key(&mut state, KeyCode::Char('<'))), Some((start, false)));
        state.activity.pending = None;
        assert_eq!(capture(&key(&mut state, KeyCode::Char('r'))), Some((shown, false)));
    }
    #[test]
    fn the_page_map_takes_a_census_of_the_region_and_opens_the_page_under_its_cursor() {
        const PAGE: u64 = 4096;
        let (mut state, now) = state();
        open(&mut state);
        state.activity.set_grouped(false);
        sample(&mut state, vec![process(2, 10, 200)], now);
        state.activity.paused = true;
        key(&mut state, KeyCode::Enter);
        let id = state.activity.pending.as_ref().unwrap().0;
        let start = 0x10000;
        let heap = MemoryRegion { start, end: start + 64 * PAGE, permissions: "rw-".into(), ..Default::default() };
        let guard =
            MemoryRegion { start: heap.end, end: heap.end + PAGE, permissions: "---".into(), ..Default::default() };
        let code = MemoryRegion { start: 0x400000, end: 0x401000, permissions: "r-x".into(), ..Default::default() };
        receive(&mut state, id, Ok(ActivityData::Regions(vec![heap.clone(), guard, code.clone()])), now);
        key(&mut state, KeyCode::Char('m'));
        let effects = sync(&mut state, now + PREVIEW_DELAY);
        let [Effect::Activity { id, request: ActivityRequest::Census { region, mapped, admin: false, .. } }] =
            &effects[..]
        else {
            panic!("expected a census, got {effects:?}");
        };
        assert_eq!(region.start, start);
        assert_eq!(*mapped, [(heap.start, heap.end), (code.start, code.end)], "only readable mappings count");
        let process = state.activity.selected.unwrap();
        let pages = [vec![PageClass::Zero; 32], vec![PageClass::Text; 32]].concat();
        let census = MemoryCensus { process, start, end: heap.end, page: PAGE, stride: 1, pages, residency: true };
        receive(&mut state, *id, Ok(ActivityData::Census(census)), now);
        assert!(state.activity.census.is_some() && !state.activity.preview_waiting());
        key(&mut state, KeyCode::Tab);
        assert!(state.activity.preview_focused);
        key(&mut state, KeyCode::Right);
        assert_eq!(state.activity.census_cursor, 1);
        key(&mut state, KeyCode::End);
        assert_eq!(state.activity.census_cursor, 63);
        let effects = key(&mut state, KeyCode::Enter);
        assert!(
            matches!(&effects[..], [Effect::Activity { request: ActivityRequest::Capture { region, seek: false, .. }, .. }] if region.start == start + 63 * PAGE),
            "{effects:?}"
        );
        assert!(!state.activity.map, "the page opens in bytes");
        key(&mut state, KeyCode::Char('m'));
        assert!(state.activity.map && state.activity.census.is_none(), "M brings the map back for a fresh census");
    }
    #[test]
    fn scrolling_up_keeps_the_viewport_until_the_cursor_reaches_its_top() {
        let (mut state, now) = state();
        open(&mut state);
        sample(&mut state, (1..=100).map(|pid| process(pid, 1, 100)).collect(), now);
        for page in [ActivityPage::Processes, ActivityPage::Regions, ActivityPage::Search, ActivityPage::Files] {
            state.activity.page = page;
            state.activity.set_grouped(false);
            let region = MemoryRegion {
                start: 0,
                end: 1,
                permissions: "r--".into(),
                name: "heap".into(),
                resident: Some(1),
                private: None,
                shared: None,
                swap: None,
                ..Default::default()
            };
            state.activity.set_regions(vec![region.clone(); 100]);
            state.activity.files = (0..100)
                .map(|i| ProcessFile {
                    descriptor: i.to_string(),
                    access: "r".into(),
                    kind: "REG".into(),
                    name: "file".into(),
                    size: None,
                    offset: None,
                })
                .collect();
            state.activity.search_report = Some(MemorySearchReport {
                process: state.activity.selected.unwrap(),
                hits: vec![
                    MemoryHit { address: 0, length: 1, encoding: "UTF-8".into(), region, ..Default::default() };
                    100
                ],
                scanned: 100,
                total: 100,
                skipped: 0,
                next: None,
                cancelled: false,
            });
            state.activity.move_by(isize::MAX, 10, 60);
            assert_eq!(state.activity.list_start(page, 10), 90);
            for _ in 0..9 {
                state.activity.move_by(-1, 10, 60);
                assert_eq!(state.activity.list_start(page, 10), 90);
            }
            state.activity.move_by(-1, 10, 60);
            assert_eq!(state.activity.list_start(page, 10), 89);
        }
    }
    #[test]
    fn groups_expand_without_reading_aggregate_ranges_and_tabs_keep_the_process() {
        let (mut state, now) = state();
        open(&mut state);
        sample(&mut state, vec![process(2, 10, 200)], now);
        let process = state.activity.selected.unwrap();
        key(&mut state, KeyCode::Enter);
        let id = state.activity.pending.as_ref().unwrap().0;
        let region = MemoryRegion {
            start: 0x1000,
            end: 0x2000,
            permissions: "rw-".into(),
            name: "MALLOC_SMALL".into(),
            resident: Some(1024),
            private: None,
            shared: None,
            swap: None,
            ..Default::default()
        };
        receive(
            &mut state,
            id,
            Ok(ActivityData::Regions(vec![
                region.clone(),
                MemoryRegion {
                    start: 0x3000,
                    end: 0x4000,
                    permissions: "---".into(),
                    name: "MALLOC guard".into(),
                    ..region
                },
            ])),
            now,
        );
        assert_eq!(
            state.activity.region_rows(),
            vec![RegionRow::Group(MemoryGroup::Heap), RegionRow::Group(MemoryGroup::Reserved)]
        );
        assert!(state.activity.capture_request().is_none());
        key(&mut state, KeyCode::Enter);
        assert_eq!(state.activity.selected_region().unwrap().start, 0x1000);
        assert!(state.activity.capture_request().is_some());
        key(&mut state, KeyCode::Left);
        assert_eq!(state.activity.selected_group(), Some(MemoryGroup::Heap));
        assert!(state.activity.capture_request().is_none());
        let effects = key(&mut state, KeyCode::Char('2'));
        assert!(
            matches!(&effects[..], [Effect::Activity { request: ActivityRequest::Files { process: p, admin: false }, .. }] if *p == process)
        );
        let id = state.activity.pending.as_ref().unwrap().0;
        receive(&mut state, id, Ok(ActivityData::Files(ProcessFiles { entries: vec![], skipped: 1 })), now);
        assert_eq!(state.activity.list, ActivityPage::Files);
        assert_eq!(state.activity.page, ActivityPage::FileDetails);
        key(&mut state, KeyCode::Char('1'));
        assert_eq!(state.activity.list, ActivityPage::Regions);
        assert_eq!(state.activity.selected_group(), Some(MemoryGroup::Heap));
        assert_eq!(state.activity.selected, Some(process));
        assert!(key(&mut state, KeyCode::Char('2')).is_empty());
        assert_eq!(state.activity.files_skipped, 1);
    }
    #[test]
    fn regions_group_by_purpose_rank_by_footprint_and_collapse_library_segments_per_file() {
        let (mut state, now) = state();
        open(&mut state);
        let mut program = process(2, 10, 200);
        program.executable = Some("/app/borg".into());
        sample(&mut state, vec![program], now);
        key(&mut state, KeyCode::Enter);
        let region = |start: u64, name: &str, path: Option<&str>, dirty: u64| MemoryRegion {
            start,
            end: start + 0x4000,
            permissions: "r-x".into(),
            name: path.map_or(name.into(), |p| format!("{name} · {p}")),
            resident: Some(0x4000),
            dirty: Some(dirty),
            swap: Some(0),
            path: path.map(Into::into),
            ..Default::default()
        };
        let regions = vec![
            region(0x1000, "__TEXT", Some("/app/borg"), 0),
            region(0x10000, "__TEXT", Some("/usr/lib/libc.dylib"), 0),
            region(0x20000, "__DATA", Some("/usr/lib/libc.dylib"), 64),
            region(0x30000, "__TEXT", Some("/usr/lib/libz.dylib"), 128),
            MemoryRegion { permissions: "rw-".into(), ..region(0x40000, "MALLOC_SMALL", None, 4096) },
            MemoryRegion { permissions: "---".into(), ..region(0x50000, "STACK GUARD", None, 0) },
        ];
        let id = state.activity.pending.as_ref().unwrap().0;
        receive(&mut state, id, Ok(ActivityData::Regions(regions.clone())), now);
        let groups: Vec<_> = state.activity.region_tree().iter().map(|item| item.row).collect();
        assert_eq!(
            groups,
            [MemoryGroup::Heap, MemoryGroup::Libraries, MemoryGroup::Program, MemoryGroup::Reserved]
                .map(RegionRow::Group)
        );
        assert_eq!(MemoryTotals::of(&state.activity.regions).footprint, Some(4096 + 64 + 128));
        let libraries = state.activity.region_tree()[1].clone();
        assert_eq!((libraries.files, libraries.totals.regions, libraries.totals.footprint), (2, 3, Some(192)));
        key(&mut state, KeyCode::Down);
        key(&mut state, KeyCode::Right);
        assert_eq!(state.activity.selected_file().map(|f| state.activity.file_name(f)), Some("libz.dylib"));
        key(&mut state, KeyCode::Down);
        key(&mut state, KeyCode::Enter);
        assert_eq!(state.activity.selected_region().unwrap().kind(), "__TEXT");
        assert_eq!(state.activity.region_tree()[state.activity.region_cursor].depth, 2);
        key(&mut state, KeyCode::Down);
        assert_eq!(state.activity.selected_region().unwrap().kind(), "__DATA");
        let id = state
            .activity
            .request(ActivityRequest::Regions { process: state.activity.selected.unwrap(), admin: false });
        let id = match &id[..] {
            [Effect::Activity { id, .. }] => *id,
            _ => unreachable!(),
        };
        receive(&mut state, id, Ok(ActivityData::Regions(regions)), now);
        assert_eq!(state.activity.selected_region().unwrap().start, 0x20000);
        key(&mut state, KeyCode::Left);
        assert_eq!(state.activity.selected_file().map(|f| state.activity.file_name(f)), Some("libc.dylib"));
        assert!(state.activity.capture_request().is_none());
        key(&mut state, KeyCode::Left);
        assert_eq!(state.activity.selected_group(), Some(MemoryGroup::Libraries));
        assert_eq!(state.activity.region_rows().len(), 4);
        key(&mut state, KeyCode::Char('g'));
        assert_eq!(state.activity.region_rows().len(), 6);
        let tagged = |name: &str| MemoryRegion { name: name.into(), permissions: "rw-".into(), ..Default::default() };
        assert_eq!(tagged("Memory Tag 253").group(None), MemoryGroup::Heap);
        assert_eq!(tagged("Memory Tag 22").group(None), MemoryGroup::Other);
    }
    #[test]
    fn the_process_filter_types_moves_keeps_and_clears() {
        let (mut state, now) = state();
        open(&mut state);
        sample(&mut state, vec![process(21, 1, 300), process(22, 1, 200), process(3, 1, 100)], now);
        for code in [KeyCode::Char('/'), KeyCode::Char('2')] {
            key(&mut state, code);
        }
        assert_eq!(state.activity.rows.len(), 2);
        key(&mut state, KeyCode::Down);
        assert!(state.activity.filtering, "arrows move without leaving the filter");
        assert_eq!(state.activity.selected.unwrap().pid, 22);
        key(&mut state, KeyCode::Enter);
        assert!(!state.activity.filtering && state.activity.filter == "2");
        key(&mut state, KeyCode::Esc);
        assert!(state.activity.filter.is_empty() && state.activity.side.is_some(), "Esc clears before closing");
        assert_eq!(state.activity.rows.len(), 3);
        for code in [KeyCode::Char('/'), KeyCode::Char('3'), KeyCode::Esc] {
            key(&mut state, code);
        }
        assert!(!state.activity.filtering && state.activity.filter.is_empty());
        key(&mut state, KeyCode::Esc);
        assert!(state.activity.side.is_none());
    }
}
