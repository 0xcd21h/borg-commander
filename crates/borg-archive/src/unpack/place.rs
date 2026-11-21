//! Writing inside the destination folder through folder handles, never following a link on the
//! way, so no entry of an archive can make Borg write outside it.

use std::{fs::File, io, os::fd::OwnedFd, path::Path};

use rustix::fs::{AtFlags, FileType, Mode, OFlags, Stat, mkdirat, openat, statat};

const FOLDER_FLAGS: OFlags = OFlags::DIRECTORY.union(OFlags::RDONLY).union(OFlags::NOFOLLOW).union(OFlags::CLOEXEC);
const CREATE_FLAGS: OFlags =
    OFlags::WRONLY.union(OFlags::CREATE).union(OFlags::EXCL).union(OFlags::NOFOLLOW).union(OFlags::CLOEXEC);
const PERMISSIONS: u32 = 0o777;
/// Folders stay open to their owner while they fill up.
const OWNER_ACCESS: u32 = 0o700;

/// The folder an archive unpacks into.
pub(crate) struct Destination {
    root: OwnedFd,
}

/// What is already at a name inside the destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Existing {
    pub folder: bool,
    pub size: u64,
    pub modified: i64,
}

impl Destination {
    pub(crate) fn open(path: &Path) -> io::Result<Self> {
        let root = openat(rustix::fs::CWD, path, FOLDER_FLAGS.difference(OFlags::NOFOLLOW), Mode::empty())?;
        Ok(Self { root })
    }

    /// The folder at `parts` below the destination, made where it is missing. A link anywhere on
    /// the way stops it.
    pub(crate) fn folder(&self, parts: &[&str], mode: u32) -> io::Result<OwnedFd> {
        let mut current = self.root.try_clone()?;
        for part in parts {
            current = match openat(&current, *part, FOLDER_FLAGS, Mode::empty()) {
                Ok(folder) => folder,
                Err(rustix::io::Errno::NOENT) => {
                    match mkdirat(&current, *part, mode_bits(mode | OWNER_ACCESS)) {
                        Ok(()) | Err(rustix::io::Errno::EXIST) => {}
                        Err(error) => return Err(error.into()),
                    }
                    openat(&current, *part, FOLDER_FLAGS, Mode::empty())?
                }
                Err(error) => return Err(error.into()),
            };
        }
        Ok(current)
    }

    /// What is at `name` in `folder`, without following a link there.
    pub(crate) fn existing(folder: &OwnedFd, name: &str) -> io::Result<Option<Existing>> {
        match statat(folder, name, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(stat) => Ok(Some(existing(&stat))),
            Err(rustix::io::Errno::NOENT) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    /// A new file at `name`, which must not exist yet.
    pub(crate) fn create(folder: &OwnedFd, name: &str, mode: u32) -> io::Result<File> {
        Ok(File::from(openat(folder, name, CREATE_FLAGS, mode_bits(mode))?))
    }
}

fn mode_bits(mode: u32) -> Mode {
    Mode::from_raw_mode((mode & PERMISSIONS) as rustix::fs::RawMode)
}

fn existing(stat: &Stat) -> Existing {
    Existing {
        folder: FileType::from_raw_mode(stat.st_mode as _) == FileType::Directory,
        size: u64::try_from(stat.st_size).unwrap_or_default(),
        modified: stat.st_mtime,
    }
}
