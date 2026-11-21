//! Archives for Borg's panels: what an archive holds, folder by folder, unpacking chosen entries
//! safely, and changing zip, 7z and tar archives in place. Zip, 7z and the tar family are read in
//! process; RAR goes through an installed program and stays read only.

mod cost;
mod error;
mod format;
mod index;
mod meter;
mod name;
mod read;
mod unpack;
mod write;

use std::path::PathBuf;

pub use error::Error;
pub use format::{Compression, Format, named, suffix};
pub use index::{ENTRY_LIMIT, Index, Item, ItemKind, Totals};
pub use name::base;
pub use read::{Archive, Flow, Visit, Watch};
pub use unpack::{UNPACK_PATIENCE, Unpack};
pub use write::{Change, List, REWRITE_PATIENCE, Rewrite, writable};

/// How a job on an archive went, with what it was given that it did not take whole, as when a
/// name already there was skipped. A move leaves those where they were.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report<P> {
    pub outcome: borg_jobs::Outcome,
    pub kept: Vec<P>,
}

impl<P> Report<P> {
    pub fn whole(outcome: borg_jobs::Outcome) -> Self {
        Self { outcome, kept: Vec::new() }
    }
}

/// A program that reads RAR archives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolKind {
    /// 7-Zip, as `7zz` or `7z`.
    SevenZip,
    /// libarchive's `bsdtar`.
    Bsdtar,
}

/// The programs that read RAR, in the order Borg prefers them.
pub const RAR_PROGRAMS: [(&str, ToolKind); 3] =
    [("7zz", ToolKind::SevenZip), ("7z", ToolKind::SevenZip), ("bsdtar", ToolKind::Bsdtar)];

/// An installed program that reads RAR archives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tool {
    pub kind: ToolKind,
    pub program: PathBuf,
}

impl Tool {
    /// The program's name, as messages show it.
    pub fn name(&self) -> String {
        self.program.file_name().map_or_else(String::new, |name| name.to_string_lossy().into_owned())
    }
}

#[cfg(test)]
mod tests;
