use std::{ops::Add, path::PathBuf, time::Duration};

use borg_drives::{DriveKind, Remember};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Spec {
    Copy { sources: Vec<PathBuf>, target: PathBuf, optimize_hdd: bool },
    Move { sources: Vec<PathBuf>, target: PathBuf },
    Delete { sources: Vec<PathBuf> },
}

impl Spec {
    pub fn sources(&self) -> &[PathBuf] {
        match self {
            Self::Copy { sources, .. } | Self::Move { sources, .. } | Self::Delete { sources } => sources,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Totals {
    pub files: u64,
    pub bytes: u64,
}

impl Add for Totals {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self { files: self.files + other.files, bytes: self.bytes + other.bytes }
    }
}

impl Totals {
    pub(crate) fn file(bytes: u64) -> Self {
        Self { files: 1, bytes }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Progress {
    pub done: Totals,
    pub total: Totals,
    pub current: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ask {
    Conflict {
        source: PathBuf,
        target: PathBuf,
        sizes: (u64, u64),
        newer: bool,
    },
    Failure {
        path: PathBuf,
        message: String,
    },
    /// An archive that unpacks to far more than it takes on disk, or to very many files.
    Expand {
        archive: PathBuf,
        packed: u64,
        unpacked: u64,
        files: u64,
    },
    /// Less free space where the job writes than it is about to write.
    Room {
        target: PathBuf,
        needed: u64,
        free: u64,
    },
    /// Work that would take long, by what Borg knows of the drives it reads and writes.
    Slow {
        /// What the work is on: the archive it changes, or the folder it writes into.
        path: PathBuf,
        /// Whether an answer may name `path` itself, as it may an archive.
        file: bool,
        bytes: u64,
        estimate: Duration,
        /// The kind of the drive that holds the work up.
        kind: DriveKind,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Overwrite,
    OverwriteAll,
    Skip,
    SkipAll,
    KeepBoth,
    Retry,
    Proceed,
    /// Go ahead, and do not ask about such work again.
    Always(Remember),
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Done { done: Totals, skipped: u64 },
    Cancelled { done: Totals },
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Scanning(Totals),
    Progress(Progress),
    Asking(Ask),
    Finished(Outcome),
}
