use std::{env, fs, io, path::PathBuf, time::Duration};

use anyhow::{Context, Result};
use borg_core::{AfterCommand, ICON_SPACING, Preferences};
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IconStyle {
    #[default]
    Auto,
    Nerd,
    Portable,
}

impl IconStyle {
    fn nerd_font(self, program: Option<&str>, term: Option<&str>) -> bool {
        match self {
            Self::Nerd => true,
            Self::Portable => false,
            Self::Auto => match program {
                Some(program) => program.eq_ignore_ascii_case("ghostty"),
                None => term == Some("xterm-ghostty"),
            },
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct Config {
    pub theme: Option<String>,
    pub status_line: Option<bool>,
    pub menu_bar: Option<bool>,
    #[serde(skip_serializing)]
    pub animations: Option<bool>,
    pub show_hidden: Option<bool>,
    pub return_to_panels: Option<bool>,
    pub restore_terminal: Option<bool>,
    #[serde(skip_serializing)]
    pub startup_animation: Option<bool>,
    pub screensaver: Option<bool>,
    pub screensaver_after: Option<u64>,
    #[serde(skip_serializing)]
    pub delight: Option<bool>,
    #[serde(skip_serializing)]
    pub icons: Option<bool>,
    pub icon_spacing: Option<u8>,
    #[serde(skip_serializing)]
    pub nerd_font: Option<bool>,
    pub icon_style: Option<IconStyle>,
    #[serde(rename = "pixel-chrome", skip_serializing)]
    pub _legacy_pixel_chrome: Option<bool>,
    pub unlocked: Option<Vec<String>>,
    pub graphics: Option<String>,
    pub update_check: Option<bool>,
}

impl Config {
    #[cfg(test)]
    pub fn preferences(&self) -> Preferences {
        self.preferences_for_terminal(None, None)
    }

    pub fn preferences_for_terminal(&self, program: Option<&str>, term: Option<&str>) -> Preferences {
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
            restore_terminal: self.restore_terminal.unwrap_or(base.restore_terminal),
            screensaver: self.screensaver.unwrap_or(base.screensaver),
            nerd_font: self.effective_icon_style().nerd_font(program, term),
            icon_spacing: self
                .icon_spacing
                .map_or(base.icon_spacing, |gap| gap.clamp(*ICON_SPACING.start(), *ICON_SPACING.end())),
        }
    }

    fn effective_icon_style(&self) -> IconStyle {
        self.icon_style.unwrap_or(if self.nerd_font == Some(true) { IconStyle::Nerd } else { IconStyle::Auto })
    }

    pub fn screensaver_after(&self) -> Option<Duration> {
        self.screensaver_after.map(Duration::from_secs)
    }

    pub fn graphics(&self) -> Result<Preference, String> {
        self.graphics.as_deref().map_or(Ok(Preference::default()), str::parse)
    }

    pub fn update_check(&self) -> bool {
        self.update_check.unwrap_or(true)
    }

    pub fn unlocked(&self) -> &[String] {
        self.unlocked.as_deref().unwrap_or_default()
    }

    pub fn capture(&self, theme: Option<&str>, prefs: &Preferences, unlocked: &[String]) -> Self {
        Self {
            theme: theme.map(str::to_owned),
            status_line: Some(prefs.status_line),
            menu_bar: Some(prefs.menu_bar),
            animations: None,
            show_hidden: Some(prefs.show_hidden),
            return_to_panels: Some(prefs.after_command == AfterCommand::ReturnToPanels),
            restore_terminal: Some(prefs.restore_terminal),
            startup_animation: None,
            screensaver: Some(prefs.screensaver),
            screensaver_after: self.screensaver_after,
            delight: None,
            icons: None,
            icon_spacing: self.icon_spacing,
            nerd_font: None,
            icon_style: Some(self.effective_icon_style()),
            _legacy_pixel_chrome: None,
            unlocked: (!unlocked.is_empty()).then(|| unlocked.to_vec()),
            graphics: self.graphics.clone(),
            update_check: self.update_check,
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
    fn retired_pixel_chrome_setting_loads_but_is_not_saved() {
        for enabled in [true, false] {
            let config: Config = toml::from_str(&format!("pixel-chrome = {enabled}\ntheme = \"neon\"\n")).unwrap();
            assert_eq!(config.preferences(), Preferences::default());
            assert_eq!(config.theme.as_deref(), Some("neon"));
            let captured = config.capture(config.theme.as_deref(), &config.preferences(), &[]);
            assert!(!toml::to_string(&captured).unwrap().contains("pixel-chrome"));
        }
    }

    #[test]
    fn rejects_unknown_keys() {
        assert!(toml::from_str::<Config>("colour = 1").is_err());
    }
}

#[cfg(test)]
mod retired_preferences_tests {
    use super::*;

    #[test]
    fn old_switches_are_ignored_and_new_defaults_are_saved() {
        let config: Config =
            toml::from_str("animations = true\nstartup-animation = true\ndelight = false\nicons = false\n").unwrap();
        let prefs = config.preferences();
        assert_eq!(prefs, Preferences::default());
        assert_eq!(prefs.after_command, AfterCommand::ReturnToPanels);
        assert!(prefs.screensaver);
        assert!(!prefs.nerd_font);
        let saved = toml::to_string(&config.capture(None, &prefs, &[])).unwrap();
        for old in ["animations", "startup-animation", "delight", "icons"] {
            assert!(!saved.contains(old));
        }
        assert!(saved.contains("icon-style = \"auto\""));
    }
}

#[cfg(test)]
mod icon_tests {
    use super::*;

    #[test]
    fn automatic_icons_follow_the_terminal() {
        let config = Config::default();
        assert!(config.preferences_for_terminal(Some("ghostty"), Some("xterm-256color")).nerd_font);
        assert!(config.preferences_for_terminal(None, Some("xterm-ghostty")).nerd_font);
        assert!(!config.preferences_for_terminal(Some("Apple_Terminal"), Some("xterm-256color")).nerd_font);
        assert!(!config.preferences_for_terminal(None, Some("xterm-256color")).nerd_font);
    }

    #[test]
    fn saved_fallback_migrates_to_auto_without_locking_icons_to_one_terminal() {
        let config: Config = toml::from_str("nerd-font = false\n").unwrap();
        let ghostty = config.preferences_for_terminal(Some("ghostty"), None);
        assert!(ghostty.nerd_font);
        let saved = toml::to_string(&config.capture(None, &ghostty, &[])).unwrap();
        assert!(!saved.contains("nerd-font"));
        let reloaded: Config = toml::from_str(&saved).unwrap();
        assert_eq!(reloaded.icon_style, Some(IconStyle::Auto));
        assert!(reloaded.preferences_for_terminal(Some("ghostty"), None).nerd_font);
        assert!(!reloaded.preferences_for_terminal(Some("Apple_Terminal"), None).nerd_font);
    }

    #[test]
    fn explicit_icon_style_overrides_terminal_detection() {
        let portable: Config = toml::from_str("icon-style = \"portable\"\n").unwrap();
        assert!(!portable.preferences_for_terminal(Some("ghostty"), None).nerd_font);
        let nerd: Config = toml::from_str("icon-style = \"nerd\"\n").unwrap();
        assert!(nerd.preferences_for_terminal(Some("Apple_Terminal"), None).nerd_font);
        let legacy: Config = toml::from_str("nerd-font = true\n").unwrap();
        let prefs = legacy.preferences_for_terminal(Some("Apple_Terminal"), None);
        assert!(prefs.nerd_font);
        assert_eq!(legacy.capture(None, &prefs, &[]).icon_style, Some(IconStyle::Nerd));
    }
}
