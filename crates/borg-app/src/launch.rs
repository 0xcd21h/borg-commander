use std::{env, path::PathBuf};

use anyhow::{Context, Result};
use borg_core::{Motion, Settings};
use borg_ui::ThemeSet;

use crate::config::{Config, ConfigDir};

const ENV_SHELL: &str = "SHELL";
const ENV_COLORTERM: &str = "COLORTERM";
const ENV_RUNTIME_DIR: &str = "XDG_RUNTIME_DIR";
const ENV_SSH: [&str; 2] = ["SSH_CONNECTION", "SSH_TTY"];
const TRUECOLOR_MARKERS: [&str; 2] = ["truecolor", "24bit"];
const POSIX_SHELL: &str = "/bin/sh";
const APP_DIR: &str = "borg-commander";

#[derive(Debug, Clone)]
pub struct Launch {
    pub shell: PathBuf,
    pub cwd: PathBuf,
    pub runtime_dir: PathBuf,
    pub settings: Settings,
    pub config_dir: Option<ConfigDir>,
    pub config: Config,
    pub themes: ThemeSet,
    pub warnings: Vec<String>,
}

impl Launch {
    pub fn detect() -> Result<Self> {
        let shell = env::var_os(ENV_SHELL).map_or_else(|| PathBuf::from(POSIX_SHELL), PathBuf::from);
        let cwd = env::current_dir().context("reading current directory")?;
        let settings = Settings { motion: motion(), ..Settings::default() };
        let config_dir = ConfigDir::locate();

        let (config, mut warnings) = match config_dir.as_ref().map(ConfigDir::load) {
            Some(Ok(config)) => (config, Vec::new()),
            Some(Err(e)) => (Config::default(), vec![format!("{e:#}")]),
            None => (Config::default(), Vec::new()),
        };

        let builtin = ThemeSet::builtin()?;
        let (themes, errors) = match &config_dir {
            Some(dir) => builtin.with_user_dir(&dir.themes()),
            None => (builtin, Vec::new()),
        };
        warnings.extend(errors.iter().map(ToString::to_string));

        Ok(Self { shell, cwd, runtime_dir: runtime_dir(), settings, config_dir, config, themes, warnings })
    }
}

fn motion() -> Motion {
    let truecolor = env::var(ENV_COLORTERM).is_ok_and(|value| TRUECOLOR_MARKERS.contains(&value.as_str()));
    let remote = ENV_SSH.iter().any(|name| env::var_os(name).is_some());
    match (truecolor, remote) {
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
