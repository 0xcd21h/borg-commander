//! Telling what kind of drive a path lies on, from what the system says of its filesystem and of
//! the device beneath it.

use std::path::Path;

use crate::Drive;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;

/// How many disk images, loop devices or overlays deep Borg looks for the drive beneath.
const DEPTH: usize = 2;
/// Filesystems that copy a file by sharing its blocks.
const CLONING_FILESYSTEMS: [&str; 4] = ["apfs", "btrfs", "xfs", "bcachefs"];

pub(crate) fn detect(path: &Path) -> Drive {
    detect_at(path, DEPTH)
}

#[cfg(target_os = "linux")]
fn detect_at(path: &Path, depth: usize) -> Drive {
    linux::detect(path, depth)
}

#[cfg(target_os = "macos")]
fn detect_at(path: &Path, depth: usize) -> Drive {
    macos::detect(path, depth)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn detect_at(path: &Path, _depth: usize) -> Drive {
    Drive::unknown(path.display().to_string())
}

fn clones(filesystem: &str) -> bool {
    CLONING_FILESYSTEMS.contains(&filesystem)
}

/// The nearest folder of `path` that exists, which says where a file about to be written lands.
pub(crate) fn existing(path: &Path) -> Option<&Path> {
    path.ancestors().find(|ancestor| ancestor.exists())
}
