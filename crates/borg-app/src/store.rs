use std::{fs, path::Path};

use anyhow::{Context, Result};

const STAGING_SUFFIX: &str = "part";

pub fn write_atomically(path: &Path, contents: &[u8]) -> Result<()> {
    let staging = path.with_extension(STAGING_SUFFIX);
    fs::write(&staging, contents).with_context(|| format!("writing {}", staging.display()))?;
    fs::rename(&staging, path).with_context(|| format!("replacing {}", path.display()))
}
