use std::{
    env, fs, io,
    path::{Path, PathBuf},
    time::Duration,
};

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
const RULES_DIR: &str = "rules";
const HOME_PREFIX: &str = "~/";

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
    /// Start the background service when Borg opens and none runs.
    pub start_service: Option<bool>,
    /// The borg cloud relay, as `wss://host`.
    pub relay: Option<String>,
    pub search: Option<Search>,
    pub index: Option<Index>,
    pub folders: Option<Folders>,
    pub network: Option<Network>,
    pub watch: Option<Watch>,
}

/// `[watch]`: what the process watcher does. It watches nothing until `on` is true, which the
/// Watch switch in Options sets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct Watch {
    pub on: bool,
    /// Whether Borg may stop a process it is sure about, once it has learned the machine.
    pub freeze: bool,
    /// Days Borg learns what is normal here before it freezes anything.
    pub learning_days: u64,
    pub alerts_per_minute: usize,
    pub freezes_per_minute: usize,
    /// Starts Borg leaves alone: a program, the parent that starts it, the user it runs as, or
    /// any of those together.
    pub allow: Vec<Allowed>,
    /// Programs Borg never freezes, besides the ones it never freezes anyway.
    pub keep_running: Vec<String>,
    /// Where alerts go as they happen, besides the Alerts page.
    pub notify: Option<Notify>,
}

/// `[watch.notify]`: the places alerts go as they happen, and the heartbeat.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct Notify {
    /// How messages name this machine; its host name otherwise.
    pub name: Option<String>,
    /// The least level an alert needs to be sent: `alert`, `warning` or `notice`.
    pub level: Option<NotifyLevel>,
    pub telegram: Option<Telegram>,
    pub ntfy: Option<Ntfy>,
    pub webhook: Option<Webhook>,
    pub heartbeat: Option<Heartbeat>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NotifyLevel {
    Alert,
    Warning,
    Notice,
}

/// A Telegram bot's token and the chat it writes to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Telegram {
    pub token: String,
    pub chat: String,
}

/// An ntfy topic's whole address, and the access token where the topic needs one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Ntfy {
    #[serde(deserialize_with = "ntfy_address")]
    pub url: String,
    pub token: Option<String>,
}

/// An address that takes every message as JSON, signed with `secret` when one is given.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Webhook {
    #[serde(deserialize_with = "notify_address")]
    pub url: String,
    pub secret: Option<String>,
}

/// An address Borg calls every few minutes while the watcher runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Heartbeat {
    #[serde(deserialize_with = "notify_address")]
    pub url: String,
    pub minutes: Option<u64>,
}

/// An address alerts may go to: HTTPS, or plain HTTP to this machine itself.
fn notify_address<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let url = String::deserialize(deserializer)?;
    if crate::watch::notify::safe_address(&url) {
        Ok(url)
    } else {
        Err(serde::de::Error::custom(crate::watch::notify::UNSAFE_ADDRESS))
    }
}

/// An ntfy topic's address, which names the topic at its end.
fn ntfy_address<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let url = notify_address(deserializer)?;
    if crate::watch::notify::ntfy_parts(&url).is_some() {
        Ok(url)
    } else {
        Err(serde::de::Error::custom(crate::watch::notify::NO_TOPIC))
    }
}

impl Notify {
    /// The sender's own settings. A heartbeat comes at least once a minute.
    fn settings(self) -> crate::watch::notify::Settings {
        use crate::watch::notify;
        let level = match self.level {
            Some(NotifyLevel::Alert) => borg_rules::Level::Alert,
            Some(NotifyLevel::Notice) => borg_rules::Level::Notice,
            Some(NotifyLevel::Warning) | None => borg_rules::Level::Warning,
        };
        notify::Settings {
            name: self.name.filter(|name| !name.trim().is_empty()),
            level,
            telegram: self.telegram.map(|telegram| notify::Telegram { token: telegram.token, chat: telegram.chat }),
            ntfy: self.ntfy.map(|ntfy| notify::Ntfy { url: ntfy.url, token: ntfy.token }),
            webhook: self.webhook.map(|webhook| notify::Webhook { url: webhook.url, secret: webhook.secret }),
            heartbeat: self.heartbeat.map(|heartbeat| notify::Heartbeat {
                url: heartbeat.url,
                every: Duration::from_secs(heartbeat.minutes.unwrap_or(notify::HEARTBEAT_MINUTES).max(1) * 60),
            }),
        }
    }
}

impl Default for Watch {
    fn default() -> Self {
        let limits = borg_core::Limits::default();
        Self {
            on: borg_core::Preferences::DEFAULT.process_watch,
            freeze: true,
            learning_days: borg_core::LEARNING_DAYS,
            alerts_per_minute: limits.alerts_per_minute,
            freezes_per_minute: limits.freezes_per_minute,
            allow: Vec::new(),
            keep_running: Vec::new(),
            notify: None,
        }
    }
}

/// One start the watcher leaves alone. Every field that is given must match.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct Allowed {
    pub program: Option<String>,
    pub parent: Option<String>,
    pub user: Option<String>,
}

/// `[network]`: the development CA that signs the certificates of Borg's capture proxy, and
/// the space captures leave free on the disk.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct Network {
    /// A folder with a CA certificate and its key, such as mkcert's.
    pub proxy_ca: Option<String>,
    /// The MiB a capture leaves free on its folder's disk; without it a capture runs until the
    /// disk is full.
    pub keep_free_mib: Option<u64>,
}

/// `[folders]`: the folder hotlist, and whether Borg remembers the folders the panels visit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct Folders {
    pub hotlist: Vec<String>,
    pub history: bool,
}

impl Default for Folders {
    fn default() -> Self {
        Self { hotlist: Vec::new(), history: borg_core::Settings::default().folder_history }
    }
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
            process_watch: self.watch.as_ref().map_or(base.process_watch, |watch| watch.on),
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
        expand_home(&self.index.clone().unwrap_or_default().roots)
    }

    /// The folder hotlist, with `~/` read as the home folder.
    pub fn hotlist(&self) -> Vec<PathBuf> {
        expand_home(&self.folders.clone().unwrap_or_default().hotlist)
    }

    /// Who signs the certificates of Borg's capture proxy: the CA in `proxy-ca`, with `~/` read
    /// as the home folder, or a temporary one.
    pub fn proxy_authority(&self) -> borg_core::Authority {
        let folder = self.network.as_ref().and_then(|network| network.proxy_ca.clone());
        expand_home(folder.as_slice()).pop().map_or(borg_core::Authority::Temporary, borg_core::Authority::Folder)
    }

    /// What the process watcher does, as the watcher's own settings.
    pub fn watch(&self) -> crate::watch::Settings {
        let watch = self.watch.clone().unwrap_or_default();
        crate::watch::Settings {
            on: watch.on,
            freeze: watch.freeze,
            learning: borg_core::learning_period(watch.learning_days),
            limits: borg_core::Limits {
                alerts_per_minute: watch.alerts_per_minute,
                freezes_per_minute: watch.freezes_per_minute,
            },
            allow: watch
                .allow
                .into_iter()
                .map(|allowed| borg_core::Allowance {
                    program: allowed.program,
                    parent: allowed.parent,
                    user: allowed.user,
                })
                .collect(),
            keep_running: watch.keep_running,
            notify: watch.notify.map(Notify::settings).unwrap_or_default(),
        }
    }

    /// The MiB captures leave free on their folder's disk, from `keep-free-mib`.
    pub fn capture_keep_free_mib(&self) -> Option<u64> {
        self.network.as_ref().and_then(|network| network.keep_free_mib)
    }

    pub fn folder_history(&self) -> bool {
        self.folders.clone().unwrap_or_default().history
    }

    pub fn set_hotlist(&mut self, hotlist: &[PathBuf]) {
        self.folders = Some(Folders { hotlist: abbreviate_home(hotlist), ..self.folders.clone().unwrap_or_default() });
    }

    pub fn index_refresh(&self) -> Duration {
        Duration::from_secs(self.index.clone().unwrap_or_default().refresh_minutes.max(1) * 60)
    }

    pub fn set_index_roots(&mut self, roots: &[PathBuf]) {
        self.index = Some(Index { roots: abbreviate_home(roots), ..self.index.clone().unwrap_or_default() });
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

    pub fn start_service(&self) -> bool {
        self.start_service.unwrap_or(true)
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
            start_service: self.start_service,
            relay: self.relay.clone(),
            search: self.search.clone(),
            index: self.index.clone(),
            folders: self.folders.clone(),
            network: self.network.clone(),
            watch: Some(Watch { on: prefs.process_watch, ..self.watch.clone().unwrap_or_default() }),
        }
    }
}

/// Absolute paths from the configuration, with `~/` read as the home folder.
fn expand_home(paths: &[String]) -> Vec<PathBuf> {
    let home = env::var_os(ENV_HOME).map(PathBuf::from);
    paths
        .iter()
        .filter_map(|path| match (path.strip_prefix(HOME_PREFIX), &home) {
            (Some(rest), Some(home)) => Some(home.join(rest)),
            (Some(_), None) => None,
            (None, _) => Some(PathBuf::from(path)),
        })
        .filter(|path| path.is_absolute())
        .collect()
}

/// Paths as the configuration keeps them, with the home folder written as `~/`.
fn abbreviate_home(paths: &[PathBuf]) -> Vec<String> {
    let home = env::var_os(ENV_HOME).map(PathBuf::from);
    paths
        .iter()
        .map(|path| match home.as_ref().and_then(|home| path.strip_prefix(home).ok()) {
            Some(rest) if !rest.as_os_str().is_empty() => format!("{HOME_PREFIX}{}", rest.display()),
            _ => path.display().to_string(),
        })
        .collect()
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

    pub fn path(&self) -> &Path {
        &self.0
    }

    pub fn themes(&self) -> PathBuf {
        self.0.join(THEMES_DIR)
    }

    /// The user's own rule files, which scans add to Borg's.
    pub fn rules(&self) -> PathBuf {
        self.0.join(RULES_DIR)
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
mod tests;
