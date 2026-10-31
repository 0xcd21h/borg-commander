//! What the monitor asks the app for, and what comes back.

use super::{
    IntegrityReport, MemoryCensus, MemoryRegion, MemorySearchMode, MemorySearchReport, MemorySnapshot, ProcessFiles,
    ProcessKey, SystemSample,
};
use serde::{Deserialize, Serialize};

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
    Integrity {
        process: ProcessKey,
        admin: bool,
    },
}

impl ActivityRequest {
    /// Whether this reads what the process holds in memory, which macOS may keep from every
    /// program, rather than listing its mappings or files.
    pub fn reads_memory(&self) -> bool {
        matches!(
            self,
            Self::Search { snapshot: None, .. } | Self::Census { .. } | Self::Capture { .. } | Self::Integrity { .. }
        )
    }
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
            Self::Integrity { process, .. } => Some(Self::Integrity { process: *process, admin: true }),
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
    Integrity(IntegrityReport),
}
