use std::{fs, path::Path};

use anyhow::{Context, Result};

const STAGING_SUFFIX: &str = "part";

pub fn write_atomically(path: &Path, contents: &[u8]) -> Result<()> {
    let staging = path.with_extension(STAGING_SUFFIX);
    fs::write(&staging, contents).with_context(|| format!("writing {}", staging.display()))?;
    fs::rename(&staging, path).with_context(|| format!("replacing {}", path.display()))
}

/// Like `write_atomically`, for a file only its owner may read.
pub fn write_privately(path: &Path, contents: &[u8]) -> Result<()> {
    use std::{io::Write, os::unix::fs::OpenOptionsExt};
    let staging = path.with_extension(STAGING_SUFFIX);
    let _ = fs::remove_file(&staging);
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&staging)
        .and_then(|mut file| file.write_all(contents))
        .with_context(|| format!("writing {}", staging.display()))?;
    fs::rename(&staging, path).with_context(|| format!("replacing {}", path.display()))
}
