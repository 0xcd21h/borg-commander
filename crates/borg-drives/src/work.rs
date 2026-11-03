use std::time::Duration;

use crate::Drive;

/// What a job reads and writes, on which drives, and how long it spends computing, as when it
/// compresses what it writes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Work {
    pub reads: Vec<(Drive, u64)>,
    pub writes: Vec<(Drive, u64)>,
    pub compute: Duration,
}

impl Work {
    pub fn read(mut self, drive: &Drive, bytes: u64) -> Self {
        self.reads.push((drive.clone(), bytes));
        self
    }

    pub fn write(mut self, drive: &Drive, bytes: u64) -> Self {
        self.writes.push((drive.clone(), bytes));
        self
    }

    pub fn computing(mut self, compute: Duration) -> Self {
        self.compute += compute;
        self
    }

    /// The larger of what the job reads and what it writes.
    pub fn bytes(&self) -> u64 {
        let total = |side: &[(Drive, u64)]| side.iter().map(|(_, bytes)| *bytes).sum::<u64>();
        total(&self.reads).max(total(&self.writes))
    }

    /// The bytes read and written on each drive the job touches.
    pub(crate) fn by_drive(&self) -> Vec<(&Drive, u64, u64)> {
        let mut drives: Vec<(&Drive, u64, u64)> = Vec::new();
        let reads = self.reads.iter().map(|(drive, bytes)| (drive, *bytes, 0));
        let writes = self.writes.iter().map(|(drive, bytes)| (drive, 0, *bytes));
        for (drive, read, write) in reads.chain(writes) {
            match drives.iter_mut().find(|(known, _, _)| known.id == drive.id) {
                Some(entry) => {
                    entry.1 += read;
                    entry.2 += write;
                }
                None => drives.push((drive, read, write)),
            }
        }
        drives
    }
}
