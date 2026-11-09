use super::{KEY_FILE, LINK_DIR, PRIVATE_FILE, SERVERS_FILE, Server, load, save};
use anyhow::{Context, Result, bail};
use borg_link::{Identity, hex};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs::File,
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
use zeroize::Zeroizing;

pub(super) const COMPUTER_KEY_FILE: &str = "computer-key";
pub(super) const COMPUTER_INFO_FILE: &str = "computer.json";
pub(super) const KEYS_LOCK_FILE: &str = "computer.lock";
const OPEN_TO_OTHERS: u32 = 0o077;
const PRIVATE_DIR: u32 = 0o700;
const LAG_WARNING: std::time::Duration = std::time::Duration::from_secs(7 * 24 * 60 * 60);

pub(super) struct ComputerKeys {
    pub current: Identity,
    pub retired: Vec<Identity>,
}

impl ComputerKeys {
    fn to_text(&self) -> Zeroizing<String> {
        let mut text = Zeroizing::new(String::new());
        for identity in std::iter::once(&self.current).chain(&self.retired) {
            text.push_str(&identity.to_text());
            text.push('\n');
        }
        text
    }

    fn from_text(text: &str) -> Option<Self> {
        let mut keys = text.lines().filter(|line| !line.trim().is_empty()).map(Identity::from_text);
        let current = keys.next()??;
        let retired = keys.collect::<Option<Vec<_>>>()?;
        Some(Self { current, retired })
    }

    pub fn held(&self, public: &str) -> Option<&Identity> {
        std::iter::once(&self.current).chain(&self.retired).find(|identity| hex(identity.public()) == public)
    }
}

#[derive(Serialize, Deserialize)]
pub(super) struct ComputerInfo {
    pub key: String,
    pub made: u64,
}

enum Vault {
    File(PathBuf),
}

impl Vault {
    fn of(state: &Path) -> Self {
        Self::File(state.join(LINK_DIR).join(COMPUTER_KEY_FILE))
    }

    fn read(&self) -> Result<Option<Zeroizing<String>>> {
        match self {
            Self::File(path) => read_private(path),
        }
    }

    fn write(&self, text: &str) -> Result<()> {
        match self {
            Self::File(path) => write_private(path, text),
        }
    }
}

fn read_private(path: &Path) -> Result<Option<Zeroizing<String>>> {
    let meta = match std::fs::metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).with_context(|| format!("reading {}", path.display())),
    };
    if meta.mode() & OPEN_TO_OTHERS != 0 || meta.uid() != rustix::process::getuid().as_raw() {
        bail!("{} must belong to you and be closed to others", path.display());
    }
    Ok(Some(Zeroizing::new(std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?)))
}

fn write_private(path: &Path, text: &str) -> Result<()> {
    use std::io::Write as _;
    let staged = path.with_extension(format!("{}.new", std::process::id()));
    let mut file =
        std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(PRIVATE_FILE).open(&staged)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&staged, path).with_context(|| format!("writing {}", path.display()))
}

fn cache() -> &'static Mutex<HashMap<PathBuf, Arc<ComputerKeys>>> {
    static CACHE: std::sync::OnceLock<Mutex<HashMap<PathBuf, Arc<ComputerKeys>>>> = std::sync::OnceLock::new();
    CACHE.get_or_init(Default::default)
}

pub(super) fn lock(state: &Path) -> Result<File> {
    let dir = state.join(LINK_DIR);
    std::fs::DirBuilder::new().recursive(true).mode(PRIVATE_DIR).create(&dir)?;
    let path = dir.join(KEYS_LOCK_FILE);
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(PRIVATE_FILE)
        .open(&path)
        .with_context(|| format!("opening {}", path.display()))?;
    file.lock()?;
    Ok(file)
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|elapsed| elapsed.as_secs()).unwrap_or_default()
}

pub(super) fn computer_info(state: &Path) -> Option<ComputerInfo> {
    std::fs::read(state.join(LINK_DIR).join(COMPUTER_INFO_FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
}

fn store(state: &Path, keys: &ComputerKeys, made: Option<u64>) -> Result<()> {
    Vault::of(state).write(&keys.to_text())?;
    let made = made.or_else(|| computer_info(state).map(|info| info.made)).unwrap_or_else(now);
    let info = ComputerInfo { key: hex(keys.current.public()), made };
    write_private(&state.join(LINK_DIR).join(COMPUTER_INFO_FILE), &serde_json::to_string_pretty(&info)?)
}

fn read(state: &Path) -> Result<Option<ComputerKeys>> {
    let Some(text) = Vault::of(state).read()? else { return Ok(None) };
    ComputerKeys::from_text(&text).map(Some).context("this computer's key store does not hold borg keys")
}

fn remember(state: &Path, keys: ComputerKeys) -> Arc<ComputerKeys> {
    let keys = Arc::new(keys);
    cache().lock().expect("key cache").insert(state.to_path_buf(), keys.clone());
    keys
}

pub(super) fn computer_keys(state: &Path) -> Result<Arc<ComputerKeys>> {
    let cached = cache().lock().expect("key cache").get(state).cloned();
    if let Some(keys) = cached
        && computer_info(state).is_none_or(|info| info.key == hex(keys.current.public()))
    {
        return Ok(keys);
    }
    let _lock = lock(state)?;
    let keys = match read(state)? {
        Some(keys) => keys,
        None => {
            let keys = ComputerKeys { current: Identity::generate(), retired: Vec::new() };
            store(state, &keys, Some(now()))?;
            keys
        }
    };
    Ok(remember(state, keys))
}

pub(super) fn new_computer_key(state: &Path) -> Result<Identity> {
    let _lock = lock(state)?;
    let ComputerKeys { current, mut retired } =
        read(state)?.context("this computer has no key yet; pairing with a server makes one")?;
    retired.insert(0, current);
    let keys = ComputerKeys { current: Identity::generate(), retired };
    store(state, &keys, Some(now()))?;
    let current = keys.current.clone();
    remember(state, keys);
    Ok(current)
}

pub(super) fn legacy_key(state: &Path) -> Option<Identity> {
    let path = state.join(LINK_DIR).join(KEY_FILE);
    path.exists().then(|| Identity::load_or_create(&path).ok()).flatten()
}

pub(super) fn retire_unused(state: &Path) -> Result<()> {
    let _lock = lock(state)?;
    let Some(keys) = read(state)? else { return Ok(()) };
    let used: HashSet<String> = load::<Server>(&state.join(LINK_DIR).join(SERVERS_FILE))
        .into_iter()
        .flat_map(|server| [server.computer, server.offered])
        .flatten()
        .collect();
    let kept: Vec<Identity> =
        keys.retired.iter().filter(|identity| used.contains(&hex(identity.public()))).cloned().collect();
    if kept.len() == keys.retired.len() {
        return Ok(());
    }
    let keys = ComputerKeys { current: keys.current.clone(), retired: kept };
    store(state, &keys, None)?;
    remember(state, keys);
    Ok(())
}

pub(super) fn update_server(state: &Path, server: &str, change: impl FnOnce(&mut Server)) -> Result<()> {
    let _lock = lock(state)?;
    let path = state.join(LINK_DIR).join(SERVERS_FILE);
    let mut servers: Vec<Server> = load(&path);
    if let Some(entry) = servers.iter_mut().find(|entry| entry.key == server) {
        change(entry);
        save(&path, &servers)?;
    }
    Ok(())
}

pub(super) fn saved_server(state: &Path, server: &str) -> Option<Server> {
    load::<Server>(&state.join(LINK_DIR).join(SERVERS_FILE)).into_iter().find(|entry| entry.key == server)
}

pub(super) fn takes_old_key(server: &Server, info: Option<&ComputerInfo>) -> bool {
    info.is_some_and(|info| server.computer.as_deref() != Some(info.key.as_str()))
}

pub(super) fn lagging_too_long(info: &ComputerInfo) -> bool {
    now().saturating_sub(info.made) > LAG_WARNING.as_secs()
}

#[cfg(test)]
pub(super) fn forget_cached(state: &Path) {
    cache().lock().expect("key cache").remove(state);
}
