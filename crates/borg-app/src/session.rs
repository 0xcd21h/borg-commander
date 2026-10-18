use std::{
    env, fs, io,
    path::{Path, PathBuf},
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
const SESSION_FILE: &str = "session.toml";
const TERMINAL_FILE: &str = "terminal.ansi";
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
