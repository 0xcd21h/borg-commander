use std::{ops::Add, path::PathBuf};

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
    Conflict { source: PathBuf, target: PathBuf, sizes: (u64, u64), newer: bool },
    Failure { path: PathBuf, message: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Overwrite,
    OverwriteAll,
    Skip,
    SkipAll,
    KeepBoth,
    Retry,
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
