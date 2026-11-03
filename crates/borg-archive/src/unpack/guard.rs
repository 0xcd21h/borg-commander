//! What Borg asks before it starts writing: about an archive that expands far beyond its size
//! or into a crowd of files, and about a folder without the room for what comes out.

use std::path::Path;

use borg_jobs::Ask;

use crate::Totals;

/// How many times its own size an archive may unpack to before Borg asks.
pub(crate) const EXPANSION_RATIO: u64 = 100;
/// Below this much unpacked data Borg does not ask about the ratio.
pub(crate) const EXPANSION_FLOOR: u64 = 1 << 30;
/// More files than this make Borg ask first.
pub(crate) const FILE_CEILING: u64 = 100_000;

/// The questions to ask before unpacking `totals` from the archive at `archive`, `packed` bytes
/// on disk, into `target`, which has `free` bytes free when that is known.
pub(crate) fn questions(archive: &Path, target: &Path, packed: u64, totals: Totals, free: Option<u64>) -> Vec<Ask> {
    let expands = totals.bytes > EXPANSION_FLOOR && totals.bytes / packed.max(1) > EXPANSION_RATIO;
    let crowded = totals.files > FILE_CEILING;
    let expand = (expands || crowded).then(|| Ask::Expand {
        archive: archive.to_path_buf(),
        packed,
        unpacked: totals.bytes,
        files: totals.files,
    });
    let room = free.filter(|free| *free < totals.bytes).map(|free| Ask::Room {
        target: target.to_path_buf(),
        needed: totals.bytes,
        free,
    });
    expand.into_iter().chain(room).collect()
}

/// The bytes free for this user in the filesystem that holds `folder`.
pub(crate) fn free_space(folder: &Path) -> Option<u64> {
    let stats = rustix::fs::statvfs(folder).ok()?;
    stats.f_bavail.checked_mul(stats.f_frsize)
}
