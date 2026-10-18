use std::{env, fs, io, path::PathBuf, time::Duration};

use anyhow::{Context, Result};
use borg_core::{AfterCommand, Preferences};
use borg_gfx::Preference;
use serde::{Deserialize, Serialize};

use crate::store::write_atomically;

const ENV_CONFIG_DIR: &str = "BORG_CONFIG_DIR";
const ENV_XDG_CONFIG: &str = "XDG_CONFIG_HOME";
const ENV_HOME: &str = "HOME";
const DOT_CONFIG: &str = ".config";
const APP_DIR: &str = "borg-commander";
const CONFIG_FILE: &str = "config.toml";
const THEMES_DIR: &str = "themes";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct Config {
    pub theme: Option<String>,
    pub status_line: Option<bool>,
    pub menu_bar: Option<bool>,
    pub animations: Option<bool>,
    pub show_hidden: Option<bool>,
    pub return_to_panels: Option<bool>,
    pub restore_terminal: Option<bool>,
    pub startup_animation: Option<bool>,
    pub screensaver: Option<bool>,
    pub screensaver_after: Option<u64>,
    pub delight: Option<bool>,
    pub icons: Option<bool>,
    pub unlocked: Option<Vec<String>>,
    pub graphics: Option<String>,
}

impl Config {
    pub fn preferences(&self) -> Preferences {
        let base = Preferences::default();
        let after_command = match self.return_to_panels {
            Some(true) => AfterCommand::ReturnToPanels,
            Some(false) => AfterCommand::ShowShell,
            None => base.after_command,
        };
        Preferences {
            after_command,
            show_hidden: self.show_hidden.unwrap_or(base.show_hidden),
            status_line: self.status_line.unwrap_or(base.status_line),
            menu_bar: self.menu_bar.unwrap_or(base.menu_bar),
            animations: self.animations.unwrap_or(base.animations),
            restore_terminal: self.restore_terminal.unwrap_or(base.restore_terminal),
            splash: self.startup_animation.unwrap_or(base.splash),
            screensaver: self.screensaver.unwrap_or(base.screensaver),
            delight: self.delight.unwrap_or(base.delight),
            icons: self.icons.unwrap_or(base.icons),
        }
    }

    pub fn screensaver_after(&self) -> Option<Duration> {
        self.screensaver_after.map(Duration::from_secs)
    }

    pub fn graphics(&self) -> Result<Preference, String> {
        self.graphics.as_deref().map_or(Ok(Preference::default()), str::parse)
    }

    pub fn unlocked(&self) -> &[String] {
        self.unlocked.as_deref().unwrap_or_default()
    }

    pub fn capture(&self, theme: Option<&str>, prefs: &Preferences, unlocked: &[String]) -> Self {
        Self {
            theme: theme.map(str::to_owned),
            status_line: Some(prefs.status_line),
            menu_bar: Some(prefs.menu_bar),
            animations: Some(prefs.animations),
            show_hidden: Some(prefs.show_hidden),
            return_to_panels: Some(prefs.after_command == AfterCommand::ReturnToPanels),
            restore_terminal: Some(prefs.restore_terminal),
            startup_animation: Some(prefs.splash),
            screensaver: Some(prefs.screensaver),
            screensaver_after: self.screensaver_after,
            delight: Some(prefs.delight),
            icons: Some(prefs.icons),
            unlocked: (!unlocked.is_empty()).then(|| unlocked.to_vec()),
            graphics: self.graphics.clone(),
        }
    }
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
        write_atomically(&self.file(), toml::to_string(config)?.as_bytes())
    }

    fn file(&self) -> PathBuf {
        self.0.join(CONFIG_FILE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_keys_fall_back_to_defaults() {
        let config: Config = toml::from_str("theme = \"aurora\"\nmenu-bar = true\n").unwrap_or_default();
        let prefs = config.preferences();
        assert!(prefs.menu_bar);
        assert_eq!(prefs.status_line, Preferences::default().status_line);
        assert_eq!(config.theme.as_deref(), Some("aurora"));
    }

    #[test]
    fn captured_preferences_round_trip() {
        let prefs =
            Preferences { menu_bar: true, after_command: AfterCommand::ReturnToPanels, ..Preferences::default() };
        let original = Config { screensaver_after: Some(42), ..Config::default() };
        let unlocked = ["cube".to_owned()];
        let text = toml::to_string(&original.capture(Some("ember"), &prefs, &unlocked)).unwrap_or_default();
        let back: Config = toml::from_str(&text).unwrap_or_default();
        assert_eq!(back.preferences(), prefs);
        assert_eq!(back.unlocked(), unlocked);
        assert_eq!(back.screensaver_after(), Some(Duration::from_secs(42)));
        assert!(text.contains("return-to-panels = true"));
    }

    #[test]
    fn rejects_unknown_keys() {
        assert!(toml::from_str::<Config>("colour = 1").is_err());
    }
}
