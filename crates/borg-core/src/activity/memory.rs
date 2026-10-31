//! A process's memory: its regions, how they group, and what its pages hold.

use super::ProcessKey;
use borg_i18n::tr;
use serde::{Deserialize, Serialize};

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
    /// Where in the file the region starts, when the system says (Linux).
    #[serde(default)]
    pub offset: Option<u64>,
}

impl MemoryRegion {
    pub fn readable(&self) -> bool {
        self.permissions.starts_with('r') && self.end > self.start
    }
    pub fn mapped(&self) -> u64 {
        self.end.saturating_sub(self.start)
    }
    /// Memory the process fills itself: no file backs it, or the process may write to it. Pages
    /// it never wrote, which macOS reports, hold only zeros or what the system put there, such
    /// as the shared libraries' symbol tables, so a region without any is not its own.
    pub fn own(&self) -> bool {
        let written = self.dirty != Some(0) || self.swap.is_some_and(|swap| swap > 0);
        written && (self.path.is_none() || self.permissions.as_bytes().get(1) == Some(&b'w'))
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
pub(super) fn application_tag(name: &str) -> bool {
    pub(super) const APPLICATION_TAGS: std::ops::RangeInclusive<u8> = 240..=255;
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
    pub(super) fn add(&mut self, region: &MemoryRegion) {
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
    pub(super) fn rank(&self) -> (std::cmp::Reverse<u64>, std::cmp::Reverse<u64>) {
        (std::cmp::Reverse(self.footprint.unwrap_or(0)), std::cmp::Reverse(self.resident.unwrap_or(0)))
    }
}

/// A row inside a group, its totals, and the indices of its regions.
pub(super) type Member = (RegionRow, MemoryTotals, Vec<usize>);

#[derive(Debug, Clone)]
pub(super) struct GroupEntry {
    pub(super) group: MemoryGroup,
    pub(super) totals: MemoryTotals,
    pub(super) members: Vec<Member>,
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
