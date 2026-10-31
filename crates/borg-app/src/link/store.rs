//! The pairing files: this borg's identity and claim on borg cloud, the computers paired with it, and the servers this computer is paired with.

use super::{CLAIM_FILE, KEY_FILE, LINK_DIR, NAME_LIMIT, PRIVATE_FILE, SERVERS_FILE};
use anyhow::{Context, Result, bail};
use borg_link::Identity;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};

/// A computer paired with this server.
#[derive(Clone, Serialize, Deserialize)]
pub struct Computer {
    pub name: String,
    pub key: String,
}

/// A server this computer paired with.
#[derive(Clone, Serialize, Deserialize)]
pub struct Server {
    pub name: String,
    pub relay: String,
    pub key: String,
}

pub fn identity(state: &Path) -> Result<Identity> {
    let path = state.join(LINK_DIR).join(KEY_FILE);
    Identity::load_or_create(&path).with_context(|| format!("reading the key in {}", path.display()))
}

/// Takes borg cloud for this borg. Borgs on one machine share a key, and at the relay each
/// would take the other's place, ending its sessions; so the first one keeps it, and the
/// others say which borg has it. The claim lasts while the file stays open.
pub(super) fn claim(state: &Path) -> Result<std::fs::File> {
    use std::io::{Read, Seek, Write};
    let path = state.join(LINK_DIR).join(CLAIM_FILE);
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(PRIVATE_FILE)
        .open(&path)
        .with_context(|| format!("opening {}", path.display()))?;
    match file.try_lock() {
        Ok(()) => {
            file.set_len(0)?;
            file.rewind()?;
            write!(file, "{}", std::process::id())?;
            Ok(file)
        }
        Err(std::fs::TryLockError::WouldBlock) => {
            let mut holder = String::new();
            let _ = file.read_to_string(&mut holder);
            bail!("another borg on this machine (PID {}) has borg cloud; quit it to use borg cloud here", holder.trim())
        }
        Err(std::fs::TryLockError::Error(error)) => Err(error.into()),
    }
}

pub(super) fn load<T: DeserializeOwned>(path: &Path) -> Vec<T> {
    std::fs::read(path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

/// Writes a list owner-only, replacing the old one whole.
pub(super) fn save<T: Serialize>(path: &Path, items: &[T]) -> Result<()> {
    use std::io::Write as _;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let staged = path.with_extension(format!("{}.new", std::process::id()));
    let mut file =
        std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(PRIVATE_FILE).open(&staged)?;
    file.write_all(&serde_json::to_vec_pretty(items)?)?;
    drop(file);
    std::fs::rename(&staged, path)?;
    Ok(())
}

/// A name from the other side, kept short and printable.
pub(super) fn clean_name(name: &str) -> String {
    let name: String = name.chars().filter(|c| !c.is_control()).take(NAME_LIMIT).collect();
    if name.trim().is_empty() { "computer".to_owned() } else { name.trim().to_owned() }
}

/// Compares secrets in time that does not depend on where they differ.
pub(super) fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0, |differ, (x, y)| differ | (x ^ y)) == 0
}

/// Who this computer is to a server it pairs with: the user and the machine.
pub(super) fn this_computer() -> String {
    let user = std::env::var("USER").or_else(|_| std::env::var("LOGNAME")).unwrap_or_default();
    let host = crate::agent::hostname();
    if user.is_empty() { host } else { format!("{user}@{host}") }
}

pub fn servers(state: &Path) -> Vec<Server> {
    load(&state.join(LINK_DIR).join(SERVERS_FILE))
}

/// Forgets a paired server by name; whether there was one.
pub fn forget_server(state: &Path, name: &str) -> Result<bool> {
    let path = state.join(LINK_DIR).join(SERVERS_FILE);
    let mut servers: Vec<Server> = load(&path);
    let before = servers.len();
    servers.retain(|server| server.name != name);
    save(&path, &servers)?;
    Ok(servers.len() < before)
}

pub fn server(state: &Path, name: &str) -> Result<Server> {
    servers(state).into_iter().find(|server| server.name == name).with_context(|| {
        format!("no server named {name} is paired; `borg pair` lists them, and pairs with a link borg shows")
    })
}

/// The folder this computer keeps its key and paired servers in.
pub(super) fn state_dir() -> Result<PathBuf> {
    crate::session::StateDir::locate()
        .map(|dir| dir.path().to_path_buf())
        .context("borg has no folder to keep its key in: set HOME or BORG_STATE_DIR")
}
