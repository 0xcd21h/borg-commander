//! Where the running Borg's program file is, read once at start. On Linux the link the kernel
//! keeps to a program file that was replaced, as `borg update` replaces it, names the old file
//! as deleted, and new processes started from that name fail.

use std::{
    io,
    path::{Path, PathBuf},
    sync::OnceLock,
};

static PROGRAM: OnceLock<Option<PathBuf>> = OnceLock::new();

/// Reads the path now, before an update can replace the file.
pub fn remember() {
    let _ = path();
}

/// The program file the running Borg started from.
pub fn path() -> io::Result<&'static Path> {
    PROGRAM
        .get_or_init(|| std::env::current_exe().ok())
        .as_deref()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, borg_i18n::tr!(program_path_unknown)))
}
