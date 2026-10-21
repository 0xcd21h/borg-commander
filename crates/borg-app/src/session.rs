use std::{
    env, fs, io,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use borg_core::{AppState, ListingMode, PanelSetup, Side, Sort, SortKey, Startup};
use serde::{Deserialize, Serialize};

use crate::store::write_atomically;

const ENV_STATE_DIR: &str = "BORG_STATE_DIR";
const ENV_XDG_STATE: &str = "XDG_STATE_HOME";
const ENV_HOME: &str = "HOME";
const DOT_STATE: [&str; 2] = [".local", "state"];
const APP_DIR: &str = "borg-commander";
const COLLECTIONS_FILE: &str = "collections.json";
const MAX_COLLECTION_BYTES: u64 = 16 * 1024 * 1024;
const COLLECTION_FORMAT: u8 = 1;
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
            PanelState {
                dir: panel.dir().to_path_buf(),
                focus: panel.hovered_entry().map(|entry| entry.name.clone()),
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
pub struct StateDir(PathBuf);

impl StateDir {
    pub fn locate() -> Option<Self> {
        let explicit = env::var_os(ENV_STATE_DIR).map(PathBuf::from);
        let xdg = || env::var_os(ENV_XDG_STATE).map(|dir| PathBuf::from(dir).join(APP_DIR));
        let home = || {
            env::var_os(ENV_HOME)
                .map(|home| DOT_STATE.iter().fold(PathBuf::from(home), |p, part| p.join(part)).join(APP_DIR))
        };
        explicit.or_else(xdg).or_else(home).map(Self)
    }

    pub fn load_session(&self) -> Result<Option<Session>> {
        let path = self.0.join(SESSION_FILE);
        match fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text).map(Some).with_context(|| format!("parsing {}", path.display())),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
        }
    }

    pub fn save_session(&self, session: &Session) -> Result<()> {
        self.ensure()?;
        write_atomically(&self.0.join(SESSION_FILE), toml::to_string(session)?.as_bytes())
    }

    pub fn load_game2048(&self) -> Result<Option<borg_core::Game2048>> {
        use std::io::Read;
        let file = match fs::File::open(self.0.join(GAME2048_FILE)) {
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
        write_atomically(&self.0.join(GAME2048_FILE), &serde_json::to_vec(&game.saved())?)
    }

    pub fn load_collections(&self) -> Result<Vec<borg_core::Collection>> {
        use std::os::unix::ffi::OsStringExt;
        let path = self.0.join(COLLECTIONS_FILE);
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
        write_atomically(&self.0.join(COLLECTIONS_FILE), &bytes)
    }

    pub fn load_terminal(&self) -> Option<Vec<u8>> {
        fs::read(self.0.join(TERMINAL_FILE)).ok().filter(|bytes| !bytes.is_empty())
    }

    pub fn save_terminal(&self, bytes: &[u8]) -> Result<()> {
        self.ensure()?;
        write_atomically(&self.0.join(TERMINAL_FILE), bytes)
    }

    pub fn forget_terminal(&self) -> Result<()> {
        match fs::remove_file(self.0.join(TERMINAL_FILE)) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e).context("removing saved terminal output"),
            _ => Ok(()),
        }
    }

    pub fn update_check_due(&self, now: SystemTime, every: Duration) -> bool {
        let last = fs::read_to_string(self.0.join(UPDATE_CHECK_FILE))
            .ok()
            .and_then(|text| text.trim().parse::<u64>().ok())
            .map(|secs| UNIX_EPOCH + Duration::from_secs(secs));
        last.is_none_or(|last| now.duration_since(last).is_ok_and(|age| age >= every))
    }

    pub fn record_update_check(&self, now: SystemTime) -> Result<()> {
        self.ensure()?;
        let secs = now.duration_since(UNIX_EPOCH).map(|age| age.as_secs()).unwrap_or_default();
        write_atomically(&self.0.join(UPDATE_CHECK_FILE), secs.to_string().as_bytes())
    }

    fn ensure(&self) -> Result<()> {
        fs::create_dir_all(&self.0).with_context(|| format!("creating {}", self.0.display()))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn game2048_round_trips_and_rejects_corrupt_or_oversized_saves() {
        let temp = tempfile::tempdir().unwrap();
        let dir = StateDir(temp.path().join("state"));
        assert!(dir.load_game2048().unwrap().is_none());
        let mut game = borg_core::Game2048::new(42);
        game.slide(borg_core::GameDirection::Left);
        dir.save_game2048(&game).unwrap();
        assert_eq!(dir.load_game2048().unwrap().unwrap().saved(), game.saved());
        let file = dir.0.join(GAME2048_FILE);
        fs::write(&file, b"{invalid").unwrap();
        assert!(dir.load_game2048().is_err());
        fs::write(&file, vec![b' '; MAX_GAME_BYTES as usize + 1]).unwrap();
        assert!(dir.load_game2048().is_err());
        let mut saved = game.saved();
        saved.cells[0] = u8::MAX;
        fs::write(&file, serde_json::to_vec(&saved).unwrap()).unwrap();
        assert!(dir.load_game2048().is_err());
    }

    #[test]
    fn update_checks_are_due_once_a_day() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let state = StateDir(dir.path().join("state"));
        let day = Duration::from_secs(86_400);
        let now = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        assert!(state.update_check_due(now, day));
        state.record_update_check(now).unwrap_or_else(|e| panic!("{e}"));
        assert!(!state.update_check_due(now + day / 2, day));
        assert!(state.update_check_due(now + day, day));
    }

    fn saved(left: &Path, right: &Path) -> Session {
        let panel = |dir: &Path, focus: &str| PanelState {
            dir: dir.to_path_buf(),
            focus: Some(focus.to_owned()),
            mode: ModeName::Brief,
            sort: SortName::Size,
            reverse: true,
        };
        Session { active: SideName::Right, split: 0.6, left: panel(left, "a"), right: panel(right, "b") }
    }

    #[test]
    fn active_panel_starts_in_cwd_and_other_panel_is_restored() {
        let other = std::env::temp_dir();
        let cwd = PathBuf::from("/somewhere/else");
        let startup = saved(&other, Path::new("/old")).startup(&cwd);

        assert_eq!(startup.active, Side::Right);
        assert_eq!(startup.panels[1].dir, cwd);
        assert_eq!(startup.panels[1].focus, None);
        assert_eq!(startup.panels[0].dir, other);
        assert_eq!(startup.panels[0].focus.as_deref(), Some("a"));
        assert_eq!(startup.panels[0].mode, ListingMode::Brief);
        assert_eq!(startup.panels[0].sort, Sort { key: SortKey::Size, reverse: true });
        assert!((startup.split - 0.6).abs() < f32::EPSILON);
    }

    #[test]
    fn missing_directories_fall_back_to_cwd() {
        let cwd = PathBuf::from("/cwd");
        let startup = saved(Path::new("/does/not/exist"), Path::new("/cwd")).startup(&cwd);
        assert_eq!(startup.panels[0].dir, cwd);
        assert_eq!(startup.panels[0].focus, None);
    }

    #[test]
    fn session_round_trips_through_toml() {
        let session = saved(Path::new("/a"), Path::new("/b"));
        let text = toml::to_string(&session).unwrap_or_default();
        assert_eq!(toml::from_str::<Session>(&text).ok(), Some(session));
    }
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
mod collection_tests {
    use super::*;
    use std::os::unix::ffi::OsStringExt;

    #[test]
    fn saved_collections_survive_missing_sources_and_non_utf8_names() {
        let temp = tempfile::tempdir().unwrap();
        let dir = StateDir(temp.path().into());
        let raw = PathBuf::from(std::ffi::OsString::from_vec(b"/work/raw-\xff".to_vec()));
        let entries =
            vec![borg_vfs::Entry::unavailable(raw.clone()), borg_vfs::Entry::unavailable("/missing/file".into())];
        dir.save_collections(&[borg_core::Collection { name: "Bundle".into(), entries }]).unwrap();
        let loaded = dir.load_collections().unwrap();
        assert_eq!(loaded[0].name, "Bundle");
        assert_eq!(loaded[0].entries[0].identity(), raw);
        assert!(loaded[0].entries[1].missing);
    }
}
