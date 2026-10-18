use std::{env, path::PathBuf, time::Duration};

use anyhow::{Context, Result};
use borg_core::{Motion, Settings, Startup};
use borg_gfx::{Capabilities, Hints, Preference, probe};
use borg_ui::ThemeSet;

use crate::{
    config::{Config, ConfigDir},
    session::StateDir,
};

const ENV_SHELL: &str = "SHELL";
const ENV_HOME: &str = "HOME";
const ENV_COLORTERM: &str = "COLORTERM";
const ENV_RUNTIME_DIR: &str = "XDG_RUNTIME_DIR";
const ENV_SSH: [&str; 2] = ["SSH_CONNECTION", "SSH_TTY"];
const TRUECOLOR_MARKERS: [&str; 2] = ["truecolor", "24bit"];
const POSIX_SHELL: &str = "/bin/sh";
const APP_DIR: &str = "borg-commander";
const PROBE_TIMEOUT: Duration = Duration::from_secs(1);

#[derive(Debug, Clone)]
pub struct Launch {
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
    pub fn detect() -> Result<Self> {
        let shell = env::var_os(ENV_SHELL).map_or_else(|| PathBuf::from(POSIX_SHELL), PathBuf::from);
        let cwd = env::current_dir().context("reading current directory")?;
        let config_dir = ConfigDir::locate();
        let (config, mut warnings) = match config_dir.as_ref().map(ConfigDir::load) {
            Some(Ok(config)) => (config, Vec::new()),
            Some(Err(e)) => (Config::default(), vec![format!("{e:#}")]),
            None => (Config::default(), Vec::new()),
        };
        let defaults = Settings::default();
        let settings = Settings {
            motion: motion(),
            truecolor: truecolor(),
            preferences: config.preferences(),
            screensaver_after: config.screensaver_after().unwrap_or(defaults.screensaver_after),
            ..defaults
        };

        let state_dir = StateDir::locate();
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
            shell,
            home: env::var_os(ENV_HOME).map(PathBuf::from),
            runtime_dir: runtime_dir(),
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

pub fn capabilities(preference: Preference) -> Capabilities {
    if preference == Preference::Off {
        return Capabilities::CELLS;
    }
    let replies = probe(PROBE_TIMEOUT).unwrap_or_default();
    Capabilities::choose(preference, &replies, Hints::from_env(|name| env::var(name).ok()))
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

fn runtime_dir() -> PathBuf {
    match env::var_os(ENV_RUNTIME_DIR) {
        Some(dir) => PathBuf::from(dir).join(APP_DIR),
        None => env::temp_dir().join(format!("{APP_DIR}-{}", rustix::process::getuid().as_raw())),
    }
}
