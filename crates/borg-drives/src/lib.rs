//! What Borg knows of the drives it reads and writes: the kind of each, how fast it turned out to
//! be, and the answers given about long work on it. Jobs ask before work that would take long and
//! learn from the time their work took.

mod detect;
mod drives;
mod kind;
mod known;
mod work;

pub use drives::{Drives, Facts};
pub use kind::{DriveKind, Rates};
pub use work::Work;

/// The drive a path lies on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drive {
    /// What names the drive from one run to the next: its device, share or host.
    pub id: String,
    pub kind: DriveKind,
    /// The filesystem's name as the system gives it, such as `apfs` or `ext4`.
    pub filesystem: String,
    /// Whether a copy of a file shares the original's blocks until one of them changes, which
    /// makes copying a large file there instant.
    pub clones: bool,
}

impl Drive {
    pub(crate) fn unknown(id: String) -> Self {
        Self { id, kind: DriveKind::Unknown, filesystem: String::new(), clones: false }
    }
}

/// What an answer about long work keeps Borg from asking again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Remember {
    /// Work on this one file, such as an archive.
    File,
    /// Work on any drive of this kind.
    Kind,
}

#[cfg(test)]
mod tests;
