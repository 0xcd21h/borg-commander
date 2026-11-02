use std::{
    env, fs, io,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result, bail};
use borg_core::{Motion, Settings, Startup};
use borg_gfx::{Capabilities, Hints, Preference, TerminalColors, probe};
use borg_ui::ThemeSet;

use crate::{
    config::{Config, ConfigDir},
    session::StateDir,
};

const ENV_HOME: &str = "HOME";
const ENV_COLORTERM: &str = "COLORTERM";
const ENV_TERM_PROGRAM: &str = "TERM_PROGRAM";
const ENV_TERM: &str = "TERM";
const ENV_RUNTIME_DIR: &str = "XDG_RUNTIME_DIR";
const ENV_SSH: [&str; 2] = ["SSH_CONNECTION", "SSH_TTY"];
const TRUECOLOR_MARKERS: [&str; 2] = ["truecolor", "24bit"];
const APP_DIR: &str = "borg-commander";
const PROBE_TIMEOUT: Duration = Duration::from_secs(1);
const PRIVATE_DIR: u32 = 0o700;
const OPEN_TO_OTHERS: u32 = 0o077;
/// How many names `private_dir` tries before giving up.
const PRIVATE_NAMES: usize = 16;
const PRIVATE_FILE: u32 = 0o600;
/// The file in the runtime folder that says until when Borg uses it.
const TTL_FILE: &str = "TTL";
/// How long the runtime folder stays in use after a running borg last said so.
const TTL: Duration = Duration::from_secs(24 * 60 * 60);
/// How often a running borg says it still uses the runtime folder.
pub(crate) const TTL_REFRESH: Duration = Duration::from_secs(60 * 60);
const TTL_NOTE: &str = "Borg Commander keeps the files of running sessions in this folder: shell integration
scripts, folder requests for the shell, and files copied out of containers.
Every running borg moves the time above forward each hour. Once it has passed, no borg
uses this folder and it can be removed.
";

#[derive(Debug, Clone)]
pub struct Launch {
    pub session_id: Option<String>,
    pub remote: Option<crate::remote::Target>,
    pub shell: PathBuf,
    pub home: Option<PathBuf>,
    pub runtime_dir: PathBuf,
    pub settings: Settings,
    pub config_dir: Option<ConfigDir>,
    pub config: Config,
    pub themes: ThemeSet,
    pub state_dir: Option<StateDir>,
    pub startup: Startup,
    pub restored: Option<Vec<u8>>,
    pub graphics: Preference,
    pub warnings: Vec<String>,
}

impl Launch {
    pub fn detect(session_id: Option<&str>, remote: Option<crate::remote::Target>) -> Result<Self> {
        let shell = borg_term::user_shell();
        let cwd = env::current_dir().context("reading current directory")?;
        let config_dir = ConfigDir::locate();
        let (config, mut warnings) = match config_dir.as_ref().map(ConfigDir::load) {
            Some(Ok(config)) => (config, Vec::new()),
            Some(Err(e)) => (Config::default(), vec![format!("{e:#}")]),
            None => (Config::default(), Vec::new()),
        };
        let defaults = Settings::default();
        let mut settings = Settings {
            motion: motion(),
            truecolor: truecolor(),
            preferences: config.preferences_for_terminal(
                env::var(ENV_TERM_PROGRAM).ok().as_deref(),
                env::var(ENV_TERM).ok().as_deref(),
            ),
            screensaver_after: config.screensaver_after().unwrap_or(defaults.screensaver_after),
            search: config.search(),
            index_roots: config.index_roots(),
            folder_history: config.folder_history(),
            ..defaults
        };

        settings.preferences.language = borg_i18n::locale();

        let state_dir = StateDir::locate().map(|state| session_id.map_or(state.clone(), |id| state.for_session(id)));
        let session = match state_dir.as_ref().map(StateDir::load_session) {
            Some(Ok(session)) => session,
            Some(Err(e)) => {
                warnings.push(format!("{e:#}"));
                None
            }
            None => None,
        };
        let startup = session.map_or_else(|| Startup::at(cwd.clone()), |session| session.startup(&cwd));
        let restored = settings
            .preferences
            .restore_terminal
            .then(|| state_dir.as_ref().and_then(StateDir::load_terminal))
            .flatten();

        let graphics = config.graphics().unwrap_or_else(|error| {
            warnings.push(error);
            Preference::Off
        });

        let builtin = ThemeSet::builtin()?;
        let (themes, errors) = match &config_dir {
            Some(dir) => builtin.with_user_dir(&dir.themes()),
            None => (builtin, Vec::new()),
        };
        warnings.extend(errors.iter().map(ToString::to_string));

        Ok(Self {
            session_id: session_id.map(str::to_owned),
            remote,
            shell,
            home: env::var_os(ENV_HOME).map(PathBuf::from),
            runtime_dir: private_dir(&runtime_dir())?,
            settings,
            config_dir,
            config,
            themes,
            state_dir,
            startup,
            restored,
            graphics,
            warnings,
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Terminal {
    pub capabilities: Capabilities,
    pub colors: TerminalColors,
    /// The terminal draws emoji two columns wide, as Borg lays them out.
    pub wide_emoji: bool,
}

pub fn terminal(preference: Preference) -> Terminal {
    let replies = probe(PROBE_TIMEOUT).unwrap_or_default();
    let capabilities = Capabilities::choose(preference, &replies, Hints::from_env(|name| env::var(name).ok()));
    Terminal { capabilities, colors: replies.colors, wide_emoji: replies.wide_emoji == Some(true) }
}

fn truecolor() -> bool {
    env::var(ENV_COLORTERM).is_ok_and(|value| TRUECOLOR_MARKERS.contains(&value.as_str()))
}

fn motion() -> Motion {
    let remote = ENV_SSH.iter().any(|name| env::var_os(name).is_some());
    match (truecolor(), remote) {
        (false, _) => Motion::STILL,
        (true, true) => Motion::REMOTE,
        (true, false) => Motion::LOCAL,
    }
}

/// Where Borg keeps the files of a running session: shell integration scripts, folder
/// requests for the shell, Docker copies and agent logs. Use it through `private_dir`.
pub(crate) fn runtime_dir() -> PathBuf {
    if let Some(dir) = env::var_os("BORG_RUNTIME_DIR") {
        return PathBuf::from(dir);
    }
    match env::var_os(ENV_RUNTIME_DIR) {
        Some(dir) => PathBuf::from(dir).join(APP_DIR),
        None => env::temp_dir().join(format!("{APP_DIR}-{}", rustix::process::getuid().as_raw())),
    }
}

/// `dir`, or the first free name after it, as a folder that belongs to this user and is
/// closed to others. Without `XDG_RUNTIME_DIR` the folder sits in the shared temporary folder
/// under a name anyone can guess, and the shell runs scripts from it: a folder there that
/// someone else made, or that others can enter, is passed over, never used or repaired.
pub(crate) fn private_dir(dir: &Path) -> Result<PathBuf> {
    let parent = dir.parent().unwrap_or(Path::new("/"));
    fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    let name = dir.file_name().unwrap_or_default().to_string_lossy();
    let names =
        std::iter::once(dir.to_path_buf()).chain((1..PRIVATE_NAMES).map(|n| parent.join(format!("{name}-{n}"))));
    for candidate in names {
        match fs::create_dir(&candidate) {
            Ok(()) => {
                // Set the mode after creating, so no umask widens or narrows it.
                fs::set_permissions(&candidate, fs::Permissions::from_mode(PRIVATE_DIR))?;
                return Ok(candidate);
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                let meta = fs::symlink_metadata(&candidate)?;
                let ours = meta.is_dir() && meta.uid() == rustix::process::getuid().as_raw();
                if ours && meta.mode() & OPEN_TO_OTHERS == 0 {
                    return Ok(candidate);
                }
            }
            Err(error) => return Err(error).with_context(|| format!("creating {}", candidate.display())),
        }
    }
    bail!("{} and the names after it belong to someone else or are open to others", dir.display())
}

/// Writes the TTL file: until when this folder is in use, then what it is.
pub(crate) fn mark_in_use(dir: &Path) -> Result<()> {
    use std::{io::Write as _, os::unix::fs::OpenOptionsExt};
    let until = jiff::Timestamp::now().checked_add(TTL)?;
    let staged = dir.join(format!("{TTL_FILE}.{}", std::process::id()));
    let mut out = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(PRIVATE_FILE).open(&staged)?;
    write!(out, "{}\n\n{TTL_NOTE}", until.strftime("%Y-%m-%dT%H:%M:%SZ"))?;
    drop(out);
    fs::rename(&staged, dir.join(TTL_FILE))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_runtime_folder_is_ours_and_closed_and_an_open_one_is_passed_over() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("borg-commander-501");
        let made = private_dir(&dir).unwrap();
        assert_eq!(made, dir);
        assert_eq!(fs::metadata(&dir).unwrap().mode() & 0o777, PRIVATE_DIR);
        assert_eq!(private_dir(&dir).unwrap(), dir, "an existing private folder is used again");

        fs::set_permissions(&dir, fs::Permissions::from_mode(0o777)).unwrap();
        let next = private_dir(&dir).unwrap();
        assert_eq!(next, temp.path().join("borg-commander-501-1"));
        assert_eq!(fs::metadata(&dir).unwrap().mode() & 0o777, 0o777, "an open folder is left alone");

        fs::remove_dir(&next).unwrap();
        std::os::unix::fs::symlink(temp.path(), &next).unwrap();
        assert_eq!(private_dir(&dir).unwrap(), temp.path().join("borg-commander-501-2"), "a link is not followed");
    }

    #[test]
    fn the_ttl_file_says_until_when_the_folder_is_in_use() {
        let temp = tempfile::tempdir().unwrap();
        mark_in_use(temp.path()).unwrap();
        let text = fs::read_to_string(temp.path().join(TTL_FILE)).unwrap();
        let until: jiff::Timestamp = text.lines().next().unwrap().parse().unwrap();
        let ahead = until.duration_since(jiff::Timestamp::now()).as_secs();
        assert!((TTL.as_secs() as i64 - 60..=TTL.as_secs() as i64).contains(&ahead), "{text}");
        assert!(text.contains("can be removed"));
        assert_eq!(fs::metadata(temp.path().join(TTL_FILE)).unwrap().mode() & 0o777, PRIVATE_FILE);
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1, "nothing staged is left behind");
    }
}
