//! Reports written where the save dialog says, readable by their owner alone.

use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};

use borg_core::ReportError;

const HOME: &str = "~";
const ENV_HOME: &str = "HOME";
const PRIVATE_FILE: u32 = 0o600;

/// The home folder a leading `~` stands for.
pub fn home() -> Option<PathBuf> {
    std::env::var_os(ENV_HOME).map(PathBuf::from)
}

/// `path` with a leading `~` read as `home`.
fn expanded(path: &Path, home: Option<&Path>) -> Result<PathBuf, ReportError> {
    match (path.strip_prefix(HOME), home) {
        (Ok(rest), Some(home)) => Ok(home.join(rest)),
        (Ok(_), None) => Err(ReportError::Failed(io::Error::from(io::ErrorKind::NotFound).to_string())),
        (Err(_), _) => Ok(path.to_path_buf()),
    }
}

/// A new file at `path`, left alone when one is there already.
fn create(path: &Path, text: &str) -> Result<(), ReportError> {
    let mut file = match OpenOptions::new().write(true).create_new(true).mode(PRIVATE_FILE).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => return Err(ReportError::Exists),
        Err(error) => return Err(ReportError::Failed(error.to_string())),
    };
    file.write_all(text.as_bytes()).and_then(|()| file.sync_all()).map_err(|error| {
        let _ = fs::remove_file(path);
        ReportError::Failed(error.to_string())
    })
}

/// Writes `text` at `path`, over a file that is there only with `replace`, and says where it went.
pub fn save(path: &Path, text: &str, replace: bool, home: Option<&Path>) -> Result<PathBuf, ReportError> {
    let path = expanded(path, home)?;
    if replace {
        crate::store::write_privately(&path, text.as_bytes())
            .map_err(|error| ReportError::Failed(format!("{error:#}")))?;
    } else {
        create(&path, text)?;
    }
    Ok(path)
}

#[cfg(test)]
mod tests;
