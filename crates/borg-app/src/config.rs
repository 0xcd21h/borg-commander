use std::{
    env, fs, io,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const ENV_CONFIG_DIR: &str = "BORG_CONFIG_DIR";
const ENV_XDG_CONFIG: &str = "XDG_CONFIG_HOME";
const ENV_HOME: &str = "HOME";
const DOT_CONFIG: &str = ".config";
const APP_DIR: &str = "borg-commander";
const CONFIG_FILE: &str = "config.toml";
const THEMES_DIR: &str = "themes";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub theme: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ConfigDir(PathBuf);

impl ConfigDir {
    pub fn locate() -> Option<Self> {
        let explicit = env::var_os(ENV_CONFIG_DIR).map(PathBuf::from);
        let xdg = || env::var_os(ENV_XDG_CONFIG).map(|dir| PathBuf::from(dir).join(APP_DIR));
        let home = || env::var_os(ENV_HOME).map(|home| PathBuf::from(home).join(DOT_CONFIG).join(APP_DIR));
        explicit.or_else(xdg).or_else(home).map(Self)
    }

    pub fn themes(&self) -> PathBuf {
        self.0.join(THEMES_DIR)
    }

    pub fn load(&self) -> Result<Config> {
        let path = self.file();
        match fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text).with_context(|| format!("parsing {}", path.display())),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
        }
    }

    pub fn save(&self, config: &Config) -> Result<()> {
        fs::create_dir_all(&self.0).with_context(|| format!("creating {}", self.0.display()))?;
        write_atomically(&self.file(), &toml::to_string(config)?)
    }

    fn file(&self) -> PathBuf {
        self.0.join(CONFIG_FILE)
    }
}

fn write_atomically(path: &Path, contents: &str) -> Result<()> {
    let staging = path.with_extension("toml.part");
    fs::write(&staging, contents).with_context(|| format!("writing {}", staging.display()))?;
    fs::rename(&staging, path).with_context(|| format!("replacing {}", path.display()))
}
