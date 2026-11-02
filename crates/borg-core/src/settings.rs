use std::time::Duration;

pub const DEFAULT_SCREENSAVER_AFTER: Duration = Duration::from_secs(300);
pub const AMBIENT_FRAME: Duration = Duration::from_millis(80);
pub const ICON_SPACING: std::ops::RangeInclusive<u8> = 1..=3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AfterCommand {
    ShowShell,
    ReturnToPanels,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Motion {
    pub fade: Duration,
    pub frame: Duration,
}

impl Motion {
    pub const LOCAL: Self = Self { fade: Duration::from_millis(180), frame: Duration::from_millis(16) };
    pub const REMOTE: Self = Self { fade: Duration::from_millis(120), frame: Duration::from_millis(40) };
    pub const STILL: Self = Self { fade: Duration::ZERO, frame: Duration::from_millis(16) };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Preferences {
    pub language: borg_i18n::Locale,
    pub after_command: AfterCommand,
    pub show_hidden: bool,
    pub status_line: bool,
    pub menu_bar: bool,
    pub restore_terminal: bool,
    pub screensaver: bool,
    pub nerd_font: bool,
    pub icon_spacing: u8,
    pub git_plugin: bool,
    pub system_plugin: bool,
    pub docker_plugin: bool,
    /// The command line runs inside a container while its filesystem panel is active.
    pub docker_console: bool,
    /// Borg keeps a log of what connected agents do, until it exits.
    pub agent_log: bool,
    /// Paired computers reach this borg through borg cloud while it is open.
    pub agent_cloud: bool,
    /// Reads asked from another computer wait for the user's approval too.
    pub agent_ask_reads: bool,
}

impl Preferences {
    pub const DEFAULT: Self = Self {
        language: borg_i18n::Locale::English,
        after_command: AfterCommand::ReturnToPanels,
        show_hidden: false,
        status_line: true,
        menu_bar: false,
        restore_terminal: false,
        screensaver: true,
        nerd_font: false,
        icon_spacing: *ICON_SPACING.start(),
        git_plugin: true,
        system_plugin: true,
        docker_plugin: true,
        docker_console: true,
        agent_log: true,
        agent_cloud: true,
        agent_ask_reads: false,
    };
}

impl Default for Preferences {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// What recursive search skips unless a search asks for everything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchSettings {
    /// Globs for paths below the search folder, or for names, to leave out.
    pub exclude: Vec<String>,
    /// Generated folders such as `node_modules`, Cargo's `target` and folders tagged as caches.
    pub artifacts: bool,
    /// `.gitignore`, `.ignore`, `.rgignore` and `.borgignore` files.
    pub ignore_files: bool,
    /// Searches are remembered for Ctrl-P and Ctrl-N.
    pub history: bool,
}

impl Default for SearchSettings {
    fn default() -> Self {
        Self { exclude: Vec::new(), artifacts: true, ignore_files: true, history: true }
    }
}

#[derive(Debug, Clone)]
pub struct Settings {
    pub preferences: Preferences,
    pub search: SearchSettings,
    /// Folders indexed for instant search.
    pub index_roots: Vec<std::path::PathBuf>,
    /// Remember the folders the panels visit.
    pub folder_history: bool,
    pub motion: Motion,
    pub truecolor: bool,
    pub screensaver_after: Duration,
    pub listing_batch: usize,
    pub scrollback: usize,
}

impl Settings {
    pub fn effective_motion(&self) -> Motion {
        Motion::STILL
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            preferences: Preferences::default(),
            search: SearchSettings::default(),
            index_roots: Vec::new(),
            folder_history: true,
            motion: Motion::LOCAL,
            truecolor: true,
            screensaver_after: DEFAULT_SCREENSAVER_AFTER,
            listing_batch: 2048,
            scrollback: 10_000,
        }
    }
}
