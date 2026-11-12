use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use anyhow::{Context, Result};

use borg_core::STAGING_SUFFIX;

/// Writes this process has staged, so each one stages under a name of its own.
static STAGED: AtomicU64 = AtomicU64::new(0);

/// Replaces the file whole with one only its owner may read, as everything Borg saves is: its
/// configuration holds tokens, and its state says what the user did and where.
pub fn write_privately(path: &Path, contents: &[u8]) -> Result<()> {
    use std::{io::Write, os::unix::fs::OpenOptionsExt};
    let staging = staging(path);
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&staging)
        .and_then(|mut file| file.write_all(contents))
        .with_context(|| format!("writing {}", staging.display()))?;
    replace(&staging, path)
}

/// A name beside `path` for this process's next write, which no other write shares, so that
/// processes saving the same file at once never move each other's half-written copy.
fn staging(path: &Path) -> PathBuf {
    let write = STAGED.fetch_add(1, Ordering::Relaxed);
    path.with_extension(format!("{}.{write}.{STAGING_SUFFIX}", std::process::id()))
}

fn replace(staging: &Path, path: &Path) -> Result<()> {
    let replaced = fs::rename(staging, path).with_context(|| format!("replacing {}", path.display()));
    if replaced.is_err() {
        let _ = fs::remove_file(staging);
    }
    replaced
}

#[cfg(test)]
mod tests;
