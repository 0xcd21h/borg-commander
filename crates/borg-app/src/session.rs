use std::{
    env, fs, io,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use borg_core::{AppState, ListingMode, PanelSetup, Side, Sort, SortKey, Startup};
use serde::{Deserialize, Serialize};

use crate::store::{write_atomically, write_privately};

const ENV_STATE_DIR: &str = "BORG_STATE_DIR";
const ENV_XDG_STATE: &str = "XDG_STATE_HOME";
const ENV_HOME: &str = "HOME";
const DOT_STATE: [&str; 2] = [".local", "state"];
const APP_DIR: &str = "borg-commander";
const COLLECTIONS_FILE: &str = "collections.json";
const MAX_COLLECTION_BYTES: u64 = 16 * 1024 * 1024;
const COLLECTION_FORMAT: u8 = 1;
const SEARCH_HISTORY_FILE: &str = "search-history.json";
const SEARCH_HISTORY_FORMAT: u8 = 1;
const MAX_SEARCH_HISTORY_BYTES: u64 = 1024 * 1024;
const GAME2048_FILE: &str = "2048.json";
const MAX_GAME_BYTES: u64 = 4096;
const SESSION_FILE: &str = "session.toml";
const TERMINAL_FILE: &str = "terminal.ansi";
const UPDATE_CHECK_FILE: &str = "update-check";
pub const RESTORED_LINES: usize = 2000;
pub const RESTORED_MARKER: &[u8] = b"\r\n\x1b[2m-- restored from the previous session --\x1b[0m\r\n";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum SideName {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum ModeName {
    Full,
    Brief,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum SortName {
    Name,
    Extension,
    Size,
    Modified,
    Source,
    Files,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct PanelState {
    dir: PathBuf,
    focus: Option<String>,
    mode: ModeName,
    sort: SortName,
    reverse: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Session {
    active: SideName,
    split: f32,
    left: PanelState,
    right: PanelState,
}

impl Session {
    pub fn capture(state: &AppState) -> Self {
        let panel = |side: Side| {
            let panel = state.panel(side);
            // A remote or container panel comes back as the host folder it replaced.
            PanelState {
                dir: panel
                    .remote_mount
                    .as_ref()
                    .map(|mount| mount.host.clone())
                    .or_else(|| panel.mount.as_ref().map(|mount| mount.host.clone()))
                    .unwrap_or_else(|| panel.dir().to_path_buf()),
                focus: panel
                    .hovered_entry()
                    .filter(|_| panel.mount.is_none() && panel.remote_mount.is_none())
                    .map(|entry| entry.name.clone()),
                mode: panel.mode().into(),
                sort: panel.sort().key.into(),
                reverse: panel.sort().reverse,
            }
        };
        Self {
            active: state.active.into(),
            split: state.split_target(),
            left: panel(Side::Left),
            right: panel(Side::Right),
        }
    }

    pub fn startup(&self, cwd: &Path) -> Startup {
        let active: Side = self.active.into();
        let setup = |side: Side, saved: &PanelState| {
            let dir = if side == active || !saved.dir.is_dir() { cwd.to_path_buf() } else { saved.dir.clone() };
            let focus = saved.focus.clone().filter(|_| dir == saved.dir);
            let sort = Sort { key: saved.sort.into(), reverse: saved.reverse };
            PanelSetup { dir, focus, mode: saved.mode.into(), sort }
        };
        Startup { panels: [setup(Side::Left, &self.left), setup(Side::Right, &self.right)], active, split: self.split }
    }
}

#[derive(Debug, Clone)]
pub struct StateDir {
    root: PathBuf,
    workspace: Option<String>,
}

impl StateDir {
    pub fn locate() -> Option<Self> {
        let explicit = env::var_os(ENV_STATE_DIR).map(PathBuf::from);
        let xdg = || env::var_os(ENV_XDG_STATE).map(|dir| PathBuf::from(dir).join(APP_DIR));
        let home = || {
            env::var_os(ENV_HOME)
                .map(|home| DOT_STATE.iter().fold(PathBuf::from(home), |p, part| p.join(part)).join(APP_DIR))
        };
        explicit.or_else(xdg).or_else(home).map(Self::at)
    }

    pub fn at(root: PathBuf) -> Self {
        Self { root, workspace: None }
    }

    pub fn for_session(&self, id: &str) -> Self {
        Self { root: self.root.clone(), workspace: Some(id.to_owned()) }
    }

    pub fn workspace(&self, id: &str) -> PathBuf {
        if id == "default" { self.root.clone() } else { self.root.join("sessions").join(id) }
    }

    fn session_path(&self, name: &str) -> PathBuf {
        self.workspace.as_ref().map_or_else(|| self.root.join(name), |id| self.workspace(id).join(name))
    }

    pub fn path(&self) -> &std::path::Path {
        &self.root
    }

    pub fn load_session(&self) -> Result<Option<Session>> {
        let path = self.session_path(SESSION_FILE);
        match fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text).map(Some).with_context(|| format!("parsing {}", path.display())),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
        }
    }

    pub fn save_session(&self, session: &Session) -> Result<()> {
        self.ensure()?;
        write_atomically(&self.session_path(SESSION_FILE), toml::to_string(session)?.as_bytes())
    }

    pub fn load_game2048(&self) -> Result<Option<borg_core::Game2048>> {
        use std::io::Read;
        let file = match fs::File::open(self.root.join(GAME2048_FILE)) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let mut bytes = Vec::new();
        file.take(MAX_GAME_BYTES + 1).read_to_end(&mut bytes)?;
        anyhow::ensure!(bytes.len() as u64 <= MAX_GAME_BYTES, "2048 save exceeds size limit");
        let saved = serde_json::from_slice(&bytes)?;
        borg_core::Game2048::restore(saved).map(Some).context("invalid 2048 save")
    }

    pub fn save_game2048(&self, game: &borg_core::Game2048) -> Result<()> {
        self.ensure()?;
        write_atomically(&self.root.join(GAME2048_FILE), &serde_json::to_vec(&game.saved())?)
    }

    pub fn load_collections(&self) -> Result<Vec<borg_core::Collection>> {
        use std::os::unix::ffi::OsStringExt;
        let path = self.root.join(COLLECTIONS_FILE);
        let metadata = match fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        anyhow::ensure!(metadata.len() <= MAX_COLLECTION_BYTES, "collections file exceeds size limit");
        let saved: SavedCollections = serde_json::from_slice(&fs::read(path)?)?;
        anyhow::ensure!(saved.version == COLLECTION_FORMAT, "unsupported collections format");
        anyhow::ensure!(saved.collections.len() <= borg_core::COLLECTION_LIMIT, "too many collections");
        let mut collections = Vec::new();
        for collection in saved.collections {
            anyhow::ensure!(collection.paths.len() <= borg_core::RESULT_LIMIT, "collection exceeds item limit");
            let entries = collection
                .paths
                .into_iter()
                .map(|bytes| {
                    let path = PathBuf::from(std::ffi::OsString::from_vec(bytes));
                    anyhow::ensure!(path.is_absolute(), "collection source must be absolute");
                    Ok(borg_vfs::Entry::unavailable(path))
                })
                .collect::<Result<Vec<_>>>()?;
            collections.push(borg_core::Collection { name: collection.name, entries });
        }
        Ok(collections)
    }

    pub fn save_collections(&self, collections: &[borg_core::Collection]) -> Result<()> {
        use std::os::unix::ffi::OsStrExt;
        let saved = SavedCollections {
            version: COLLECTION_FORMAT,
            collections: collections
                .iter()
                .map(|collection| SavedCollection {
                    name: collection.name.clone(),
                    paths: collection
                        .entries
                        .iter()
                        .map(|entry| entry.identity().as_os_str().as_bytes().to_vec())
                        .collect(),
                })
                .collect(),
        };
        let bytes = serde_json::to_vec(&saved)?;
        anyhow::ensure!(bytes.len() as u64 <= MAX_COLLECTION_BYTES, "collections exceed storage limit");
        self.ensure()?;
        write_atomically(&self.root.join(COLLECTIONS_FILE), &bytes)
    }

    pub fn load_search_history(&self) -> Result<borg_core::SearchHistory> {
        use std::{io::Read, os::unix::ffi::OsStringExt};
        let file = match fs::File::open(self.root.join(SEARCH_HISTORY_FILE)) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Default::default()),
            Err(error) => return Err(error.into()),
        };
        let mut bytes = Vec::new();
        file.take(MAX_SEARCH_HISTORY_BYTES + 1).read_to_end(&mut bytes)?;
        anyhow::ensure!(bytes.len() as u64 <= MAX_SEARCH_HISTORY_BYTES, "search history exceeds size limit");
        let saved: SavedSearchHistory = serde_json::from_slice(&bytes)?;
        anyhow::ensure!(saved.version == SEARCH_HISTORY_FORMAT, "unsupported search history format");
        Ok(borg_core::SearchHistory::new(
            saved
                .searches
                .into_iter()
                .map(|search| borg_core::PastSearch {
                    query: search.query,
                    root: PathBuf::from(std::ffi::OsString::from_vec(search.root)),
                })
                .collect(),
        ))
    }

    /// Saves the history only its owner can read; an empty one removes the file.
    pub fn save_search_history(&self, history: &borg_core::SearchHistory) -> Result<()> {
        use std::os::unix::ffi::OsStrExt;
        let path = self.root.join(SEARCH_HISTORY_FILE);
        if history.entries().is_empty() {
            return match fs::remove_file(&path) {
                Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error.into()),
                _ => Ok(()),
            };
        }
        let saved = SavedSearchHistory {
            version: SEARCH_HISTORY_FORMAT,
            searches: history
                .entries()
                .iter()
                .map(|search| SavedSearch {
                    query: search.query.clone(),
                    root: search.root.as_os_str().as_bytes().to_vec(),
                })
                .collect(),
        };
        self.ensure()?;
        write_privately(&path, &serde_json::to_vec(&saved)?)
    }

    pub fn load_terminal(&self) -> Option<Vec<u8>> {
        fs::read(self.session_path(TERMINAL_FILE)).ok().filter(|bytes| !bytes.is_empty())
    }

    pub fn save_terminal(&self, bytes: &[u8]) -> Result<()> {
        self.ensure()?;
        write_atomically(&self.session_path(TERMINAL_FILE), bytes)
    }

    pub fn forget_terminal(&self) -> Result<()> {
        match fs::remove_file(self.session_path(TERMINAL_FILE)) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e).context("removing saved terminal output"),
            _ => Ok(()),
        }
    }

    pub fn update_check_due(&self, now: SystemTime, every: Duration) -> bool {
        let last = fs::read_to_string(self.root.join(UPDATE_CHECK_FILE))
            .ok()
            .and_then(|text| text.trim().parse::<u64>().ok())
            .map(|secs| UNIX_EPOCH + Duration::from_secs(secs));
        last.is_none_or(|last| now.duration_since(last).is_ok_and(|age| age >= every))
    }

    pub fn record_update_check(&self, now: SystemTime) -> Result<()> {
        self.ensure()?;
        let secs = now.duration_since(UNIX_EPOCH).map(|age| age.as_secs()).unwrap_or_default();
        write_atomically(&self.root.join(UPDATE_CHECK_FILE), secs.to_string().as_bytes())
    }

    fn ensure(&self) -> Result<()> {
        let path = self.workspace.as_ref().map_or_else(|| self.root.clone(), |id| self.workspace(id));
        fs::create_dir_all(&path).with_context(|| format!("creating {}", path.display()))
    }
}

impl From<Side> for SideName {
    fn from(side: Side) -> Self {
        match side {
            Side::Left => Self::Left,
            Side::Right => Self::Right,
        }
    }
}

impl From<SideName> for Side {
    fn from(side: SideName) -> Self {
        match side {
            SideName::Left => Self::Left,
            SideName::Right => Self::Right,
        }
    }
}

impl From<ListingMode> for ModeName {
    fn from(mode: ListingMode) -> Self {
        match mode {
            ListingMode::Full => Self::Full,
            ListingMode::Brief => Self::Brief,
        }
    }
}

impl From<ModeName> for ListingMode {
    fn from(mode: ModeName) -> Self {
        match mode {
            ModeName::Full => Self::Full,
            ModeName::Brief => Self::Brief,
        }
    }
}

impl From<SortKey> for SortName {
    fn from(key: SortKey) -> Self {
        match key {
            SortKey::Name => Self::Name,
            SortKey::Extension => Self::Extension,
            SortKey::Size => Self::Size,
            SortKey::Modified => Self::Modified,
            SortKey::Files => Self::Files,
            SortKey::Source => Self::Source,
        }
    }
}

impl From<SortName> for SortKey {
    fn from(key: SortName) -> Self {
        match key {
            SortName::Name => Self::Name,
            SortName::Extension => Self::Extension,
            SortName::Size => Self::Size,
            SortName::Modified => Self::Modified,
            SortName::Files => Self::Files,
            SortName::Source => Self::Source,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct SavedSearchHistory {
    version: u8,
    searches: Vec<SavedSearch>,
}
#[derive(Serialize, Deserialize)]
struct SavedSearch {
    query: String,
    root: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
struct SavedCollections {
    version: u8,
    collections: Vec<SavedCollection>,
}
#[derive(Serialize, Deserialize)]
struct SavedCollection {
    name: String,
    paths: Vec<Vec<u8>>,
}

#[cfg(test)]
mod tests;
