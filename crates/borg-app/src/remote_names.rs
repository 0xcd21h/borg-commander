use std::{fs, io::Read, os::unix::fs::OpenOptionsExt, path::Path};

use anyhow::{Context, Result, bail};
use borg_core::RemoteNames;

const FILE: &str = "remote-names.json";
const LOCK: &str = "remote-names.lock";
const MAX_BYTES: u64 = 1024 * 1024;

pub fn load(state: &Path) -> Result<RemoteNames> {
    let file = match fs::File::open(state.join(FILE)) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(RemoteNames::default()),
        Err(error) => return Err(error).context("reading remote names"),
    };
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_BYTES {
        bail!("remote names file is too large");
    }
    let names: RemoteNames = serde_json::from_slice(&bytes).context("reading remote names")?;
    for labels in names.0.values() {
        for name in labels.host.iter().chain(labels.sessions.values()) {
            RemoteNames::validate(name).map_err(anyhow::Error::msg)?;
        }
    }
    Ok(names)
}

pub fn rename(state: &Path, identity: &str, session: Option<&str>, name: &str) -> Result<RemoteNames> {
    RemoteNames::validate(name).map_err(anyhow::Error::msg)?;
    fs::create_dir_all(state)?;
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(state.join(LOCK))?;
    lock.lock()?;
    let mut names = load(state)?;
    names.set(identity, session, name).map_err(anyhow::Error::msg)?;
    let bytes = serde_json::to_vec_pretty(&names)?;
    if bytes.len() as u64 > MAX_BYTES {
        bail!("remote names file is too large");
    }
    crate::store::write_atomically(&state.join(FILE), &bytes)?;
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_persist_by_identity_and_session_and_can_be_reset() -> Result<()> {
        let dir = tempfile::tempdir()?;
        rename(dir.path(), "key-one", None, "Production")?;
        rename(dir.path(), "key-one", Some("default"), "Deploy")?;
        rename(dir.path(), "key-two", None, "Testing")?;
        let names = load(dir.path())?;
        assert_eq!(names.0["key-one"].host.as_deref(), Some("Production"));
        assert_eq!(names.0["key-one"].sessions["default"], "Deploy");
        assert_eq!(names.0["key-two"].host.as_deref(), Some("Testing"));
        rename(dir.path(), "key-one", None, "")?;
        let names = rename(dir.path(), "key-one", Some("default"), "")?;
        assert!(!names.0.contains_key("key-one"));
        assert_eq!(names.0.len(), 1);
        Ok(())
    }

    #[test]
    fn a_broken_file_is_not_overwritten_and_parallel_edits_are_merged() -> Result<()> {
        let dir = tempfile::tempdir()?;
        std::thread::scope(|scope| {
            for i in 0..8 {
                let path = dir.path();
                scope.spawn(move || rename(path, &format!("key-{i}"), None, "Host").unwrap());
            }
        });
        assert_eq!(load(dir.path())?.0.len(), 8);
        fs::write(dir.path().join(FILE), "broken")?;
        assert!(rename(dir.path(), "key-one", None, "Host").is_err());
        assert_eq!(fs::read_to_string(dir.path().join(FILE))?, "broken");
        Ok(())
    }
}
