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
    pub plugins: Option<Plugins>,
    pub theme: Option<String>,
    pub language: Option<String>,
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
    /// The borg cloud relay, as `wss://host`.
    pub relay: Option<String>,
    pub search: Option<Search>,
    pub index: Option<Index>,
}

/// `[index]`: folders indexed for instant search, and how old an index may grow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct Index {
    pub roots: Vec<String>,
    pub refresh_minutes: u64,
}

impl Default for Index {
    fn default() -> Self {
        Self { roots: Vec::new(), refresh_minutes: 15 }
    }
}

/// `[search]`: what recursive search leaves out, and whether it remembers searches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct Search {
    pub exclude: Vec<String>,
    pub artifacts: bool,
    pub ignore_files: bool,
    pub history: bool,
}

impl Default for Search {
    fn default() -> Self {
        let defaults = borg_core::SearchSettings::default();
        Self {
            exclude: defaults.exclude,
            artifacts: defaults.artifacts,
            ignore_files: defaults.ignore_files,
            history: defaults.history,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Plugins {
    pub git: bool,
    pub system: bool,
    pub docker: bool,
    /// The command line follows container panels into their containers.
    #[serde(rename = "docker-console")]
    pub docker_console: bool,
    /// Borg logs what connected agents do.
    #[serde(rename = "agent-log")]
    pub agent_log: bool,
    /// Paired computers reach this borg through borg cloud.
    #[serde(rename = "agent-cloud")]
    pub agent_cloud: bool,
    /// Reads from another computer wait for approval.
    #[serde(rename = "agent-ask-reads")]
    pub agent_ask_reads: bool,
}

impl Default for Plugins {
    fn default() -> Self {
        Self {
            git: true,
            system: true,
            docker: true,
            docker_console: true,
            agent_log: true,
            agent_cloud: true,
            agent_ask_reads: false,
        }
    }
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
            language: self.language.as_deref().and_then(borg_i18n::Locale::parse).unwrap_or(base.language),
            git_plugin: self.plugins.as_ref().map_or(base.git_plugin, |plugins| plugins.git),
            system_plugin: self.plugins.as_ref().map_or(base.system_plugin, |plugins| plugins.system),
            docker_plugin: self.plugins.as_ref().map_or(base.docker_plugin, |plugins| plugins.docker),
            docker_console: self.plugins.as_ref().map_or(base.docker_console, |plugins| plugins.docker_console),
            agent_log: self.plugins.as_ref().map_or(base.agent_log, |plugins| plugins.agent_log),
            agent_cloud: self.plugins.as_ref().map_or(base.agent_cloud, |plugins| plugins.agent_cloud),
            agent_ask_reads: self.plugins.as_ref().map_or(base.agent_ask_reads, |plugins| plugins.agent_ask_reads),
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

    pub fn search(&self) -> borg_core::SearchSettings {
        let search = self.search.clone().unwrap_or_default();
        borg_core::SearchSettings {
            exclude: search.exclude,
            artifacts: search.artifacts,
            ignore_files: search.ignore_files,
            history: search.history,
        }
    }

    /// The configuration with what searches leave out and remember as `search` has it. A file
    /// without a `[search]` table gains one only once something differs from the defaults.
    pub fn with_search(mut self, search: &borg_core::SearchSettings) -> Self {
        let search = Search {
            exclude: search.exclude.clone(),
            artifacts: search.artifacts,
            ignore_files: search.ignore_files,
            history: search.history,
        };
        self.search = (self.search.is_some() || search != Search::default()).then_some(search);
        self
    }

    /// The indexed folders, with `~/` read as the home folder.
    pub fn index_roots(&self) -> Vec<PathBuf> {
        let home = env::var_os(ENV_HOME).map(PathBuf::from);
        let index = self.index.clone().unwrap_or_default();
        index
            .roots
            .iter()
            .filter_map(|root| match (root.strip_prefix("~/"), &home) {
                (Some(rest), Some(home)) => Some(home.join(rest)),
                (Some(_), None) => None,
                (None, _) => Some(PathBuf::from(root)),
            })
            .filter(|root| root.is_absolute())
            .collect()
    }

    pub fn index_refresh(&self) -> Duration {
        Duration::from_secs(self.index.clone().unwrap_or_default().refresh_minutes.max(1) * 60)
    }

    pub fn set_index_roots(&mut self, roots: &[PathBuf]) {
        let home = env::var_os(ENV_HOME).map(PathBuf::from);
        let roots = roots
            .iter()
            .map(|root| match home.as_ref().and_then(|home| root.strip_prefix(home).ok()) {
                Some(rest) if !rest.as_os_str().is_empty() => format!("~/{}", rest.display()),
                _ => root.display().to_string(),
            })
            .collect();
        self.index = Some(Index { roots, ..self.index.clone().unwrap_or_default() });
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
            plugins: Some(Plugins {
                git: prefs.git_plugin,
                system: prefs.system_plugin,
                docker: prefs.docker_plugin,
                docker_console: prefs.docker_console,
                agent_log: prefs.agent_log,
                agent_cloud: prefs.agent_cloud,
                agent_ask_reads: prefs.agent_ask_reads,
            }),
            theme: theme.map(str::to_owned),
            language: Some(prefs.language.code().to_owned()),
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
            relay: self.relay.clone(),
            search: self.search.clone(),
            index: self.index.clone(),
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
    fn search_settings_default_to_skipping_artifacts_and_survive_saving_preferences() {
        let config: Config = toml::from_str("[search]\nexclude = [\"*.min.js\"]\nignore-files = false\n").unwrap();
        let search = config.search();
        assert_eq!(search.exclude, ["*.min.js"]);
        assert!(search.artifacts && !search.ignore_files && search.history);
        assert!(Config::default().search().ignore_files);
        let saved = config.capture(None, &Preferences::default(), &[]);
        assert_eq!(saved.search(), search);
        let edited = borg_core::SearchSettings { exclude: vec!["Library/Caches/".into()], artifacts: false, ..search };
        let text = toml::to_string(&saved.with_search(&edited)).unwrap();
        assert_eq!(toml::from_str::<Config>(&text).unwrap().search(), edited);
        let untouched = Config::default().with_search(&borg_core::SearchSettings::default());
        assert!(!toml::to_string(&untouched).unwrap().contains("[search]"));
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

#[cfg(test)]
mod plugin_tests {
    use super::*;

    #[test]
    fn git_is_enabled_by_default_and_the_toggle_survives_saving_other_preferences() {
        assert!(Config::default().preferences().git_plugin);
        let config: Config = toml::from_str("[plugins]\ngit = false\n").unwrap();
        let mut prefs = config.preferences();
        assert!(!prefs.git_plugin);
        prefs.status_line = false;
        let saved = toml::to_string(&config.capture(None, &prefs, &[])).unwrap();
        let restored: Config = toml::from_str(&saved).unwrap();
        assert!(!restored.preferences().git_plugin);
        assert!(!restored.preferences().status_line);
        assert!(toml::from_str::<Config>("[plugins]\ngti = false\n").is_err());
    }
}

#[cfg(test)]
mod language_tests {
    use super::*;

    #[test]
    fn saved_language_survives_other_preference_changes() {
        for language in borg_i18n::Locale::ALL {
            let source = format!("language = {:?}\n", language.code());
            let config: Config = toml::from_str(&source).unwrap();
            let mut prefs = config.preferences();
            assert_eq!(prefs.language, language);
            prefs.menu_bar = true;
            let saved = config.capture(None, &prefs, &[]);
            let restored: Config = toml::from_str(&toml::to_string(&saved).unwrap()).unwrap();
            assert_eq!(restored.preferences().language, language);
            assert!(restored.preferences().menu_bar);
        }
    }
}
