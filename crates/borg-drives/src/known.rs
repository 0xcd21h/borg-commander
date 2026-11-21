//! What Borg keeps between runs: the speed each drive turned out to have, and the answers that
//! keep it from asking before long work.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{DriveKind, Rates, Remember};

pub(crate) const FORMAT: u8 = 1;
/// How many files Borg remembers not to ask about; the oldest go first.
const QUIET_FILES: usize = 256;
/// How far one measurement moves the speed Borg knew, from 0 to 1.
const WEIGHT: f64 = 0.4;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Known {
    pub format: u8,
    #[serde(default)]
    pub drives: BTreeMap<String, Learned>,
    #[serde(default)]
    pub quiet: Quiet,
}

impl Default for Known {
    fn default() -> Self {
        Self { format: FORMAT, drives: BTreeMap::new(), quiet: Quiet::default() }
    }
}

/// The speeds measured on one drive, in bytes per second.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Learned {
    pub kind: DriveKind,
    pub read: Option<u64>,
    pub write: Option<u64>,
}

/// The work Borg no longer asks about.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Quiet {
    #[serde(default)]
    pub files: Vec<PathBuf>,
    #[serde(default)]
    pub kinds: Vec<DriveKind>,
}

impl Known {
    /// The drive's speeds, measured where Borg measured them, and whether it did. A drive that
    /// turns out to be of another kind than before starts over.
    pub fn rates(&self, id: &str, kind: DriveKind) -> (Rates, bool) {
        let typical = kind.typical();
        match self.drives.get(id).filter(|learned| learned.kind == kind) {
            Some(learned) => (
                Rates { read: learned.read.unwrap_or(typical.read), write: learned.write.unwrap_or(typical.write) },
                learned.read.is_some() || learned.write.is_some(),
            ),
            None => (typical, false),
        }
    }

    /// Takes in speeds measured on a drive.
    pub fn learn(&mut self, id: &str, kind: DriveKind, read: Option<u64>, write: Option<u64>) {
        let (rates, _) = self.rates(id, kind);
        let entry = self.drives.entry(id.to_owned()).or_insert(Learned { kind, read: None, write: None });
        if entry.kind != kind {
            *entry = Learned { kind, read: None, write: None };
        }
        if let Some(read) = read {
            entry.read = Some(blend(rates.read, read));
        }
        if let Some(write) = write {
            entry.write = Some(blend(rates.write, write));
        }
    }

    pub fn asks(&self, file: Option<&Path>, kind: DriveKind) -> bool {
        let quiet_file = file.is_some_and(|file| self.quiet.files.iter().any(|known| known == file));
        !quiet_file && !self.quiet.kinds.contains(&kind)
    }

    pub fn remember(&mut self, remember: Remember, file: Option<&Path>, kind: DriveKind) {
        match (remember, file) {
            (Remember::File, Some(file)) => {
                self.quiet.files.retain(|known| known != file);
                self.quiet.files.push(file.to_path_buf());
                let excess = self.quiet.files.len().saturating_sub(QUIET_FILES);
                self.quiet.files.drain(..excess);
            }
            (Remember::Kind, _) if !self.quiet.kinds.contains(&kind) => self.quiet.kinds.push(kind),
            _ => {}
        }
    }
}

fn blend(known: u64, measured: u64) -> u64 {
    (known as f64 * (1.0 - WEIGHT) + measured as f64 * WEIGHT) as u64
}
