//! Where programs are set to start on their own on each system, and how each place's files
//! read.

use borg_i18n::tr;
use std::path::{Path, PathBuf};

/// The system whose places are read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupSystem {
    MacOs,
    Linux,
}

/// A kind of place a program starts from, in the order the screen ranks places.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StartupPlace {
    UserAgent,
    Agent,
    Daemon,
    SystemAgent,
    SystemDaemon,
    UserService,
    UserTimer,
    Service,
    Timer,
    Autostart,
    Crontab,
    CronScript,
    Anacron,
    Periodic,
    RcLocal,
    Preload,
    ShellStartup,
    AuthorizedKey,
}

impl StartupPlace {
    pub fn label(self) -> &'static str {
        match self {
            Self::UserAgent => tr!(startup_place_user_agent),
            Self::Agent => tr!(startup_place_agent),
            Self::Daemon => tr!(startup_place_daemon),
            Self::SystemAgent => tr!(startup_place_system_agent),
            Self::SystemDaemon => tr!(startup_place_system_daemon),
            Self::UserService => tr!(startup_place_user_service),
            Self::UserTimer => tr!(startup_place_user_timer),
            Self::Service => tr!(startup_place_service),
            Self::Timer => tr!(startup_place_timer),
            Self::Autostart => tr!(startup_place_autostart),
            Self::Crontab => tr!(startup_place_crontab),
            Self::CronScript => tr!(startup_place_cron_script),
            Self::Anacron => tr!(startup_place_anacron),
            Self::Periodic => tr!(startup_place_periodic),
            Self::RcLocal => tr!(startup_place_rc_local),
            Self::Preload => tr!(startup_place_preload),
            Self::ShellStartup => tr!(startup_place_shell),
            Self::AuthorizedKey => tr!(startup_place_ssh_key),
        }
    }

    /// The name the agent's answer uses, the same in every language.
    pub fn code(self) -> &'static str {
        match self {
            Self::UserAgent => "user-launch-agent",
            Self::Agent => "launch-agent",
            Self::Daemon => "launch-daemon",
            Self::SystemAgent => "system-launch-agent",
            Self::SystemDaemon => "system-launch-daemon",
            Self::UserService => "user-systemd-service",
            Self::UserTimer => "user-systemd-timer",
            Self::Service => "systemd-service",
            Self::Timer => "systemd-timer",
            Self::Autostart => "xdg-autostart",
            Self::Crontab => "crontab",
            Self::CronScript => "cron-script",
            Self::Anacron => "anacron",
            Self::Periodic => "periodic",
            Self::RcLocal => "rc-local",
            Self::Preload => "ld-so-preload",
            Self::ShellStartup => "shell-startup",
            Self::AuthorizedKey => "ssh-authorized-key",
        }
    }
}

/// How a place's files read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupFormat {
    /// A launchd property list.
    Launchd,
    /// A user's crontab: five time fields or an @ word, then the command.
    Crontab,
    /// `/etc/crontab` and `/etc/cron.d`: the user to run as comes before the command.
    SystemCrontab,
    Anacrontab,
    /// A systemd unit: a service's `ExecStart`, or the unit a timer starts.
    Unit,
    /// An XDG `.desktop` file's `Exec`.
    Desktop,
    /// A script that runs as a whole.
    Script,
    /// Libraries loaded into every program.
    Preload,
    /// A file a shell reads as it starts.
    Shell,
    AuthorizedKeys,
}

/// A file, or a folder whose files are each read, where programs start from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupSpot {
    pub place: StartupPlace,
    pub format: StartupFormat,
    pub path: PathBuf,
    pub folder: bool,
    /// Name endings a folder's files must have; any name but hidden ones when empty.
    pub extensions: &'static [&'static str],
    /// On macOS's sealed system volume, which nothing changes while the system runs.
    pub read_only: bool,
}

const PLISTS: &[&str] = &["plist"];
const UNITS: &[&str] = &["service", "timer"];
const DESKTOP: &[&str] = &["desktop"];
const SHELL_SCRIPTS: &[&str] = &["sh"];
const ANY: &[&str] = &[];
const TIMER: &str = "timer";

/// Subfolders of a systemd folder whose units another unit pulls in.
pub const UNIT_LINK_FOLDERS: &[&str] = &[".wants", ".requires"];

const USER_SHELL_FILES: &[&str] = &[
    ".zshenv",
    ".zprofile",
    ".zshrc",
    ".zlogin",
    ".zlogout",
    ".profile",
    ".bash_profile",
    ".bash_login",
    ".bashrc",
    ".bash_logout",
    ".config/fish/config.fish",
];
const MACOS_SHELL_FILES: &[&str] =
    &["/etc/zshenv", "/etc/zprofile", "/etc/zshrc", "/etc/zlogin", "/etc/zlogout", "/etc/profile", "/etc/bashrc"];
const LINUX_SHELL_FILES: &[&str] = &[
    "/etc/profile",
    "/etc/bash.bashrc",
    "/etc/bashrc",
    "/etc/zsh/zshenv",
    "/etc/zsh/zprofile",
    "/etc/zsh/zshrc",
    "/etc/zsh/zlogin",
    "/etc/zsh/zlogout",
    "/etc/zshenv",
    "/etc/zprofile",
    "/etc/zshrc",
];
const AUTHORIZED_KEYS: &[&str] = &[".ssh/authorized_keys", ".ssh/authorized_keys2"];
const PERIODS: &[&str] = &["daily", "weekly", "monthly"];
const CRON_PERIODS: &[&str] = &["hourly", "daily", "weekly", "monthly"];
const SYSTEM_UNIT_FOLDERS: &[&str] =
    &["/etc/systemd/system", "/usr/local/lib/systemd/system", "/usr/lib/systemd/system", "/lib/systemd/system"];
const USER_UNIT_FOLDERS: &[&str] = &["/etc/systemd/user", "/usr/local/lib/systemd/user", "/usr/lib/systemd/user"];
const HOME_UNIT_FOLDERS: &[&str] = &[".config/systemd/user", ".local/share/systemd/user"];
const USER_CRONTABS: &[&str] = &["/var/spool/cron/crontabs", "/var/spool/cron"];
const USER_AGENTS: &str = "Library/LaunchAgents";
const AGENTS: &str = "/Library/LaunchAgents";
const DAEMONS: &str = "/Library/LaunchDaemons";
const SYSTEM_AGENTS: &str = "/System/Library/LaunchAgents";
const SYSTEM_DAEMONS: &str = "/System/Library/LaunchDaemons";
const MACOS_CRONTABS: &str = "/usr/lib/cron/tabs";
const SYSTEM_CRONTAB: &str = "/etc/crontab";
const PERIODIC_ROOTS: &[&str] = &["/etc/periodic", "/usr/local/etc/periodic"];
const USER_AUTOSTART: &str = ".config/autostart";
const AUTOSTART: &str = "/etc/xdg/autostart";
const CRON_FOLDER: &str = "/etc/cron.d";
const CRON_SCRIPTS: &str = "/etc/cron.";
const ANACRONTAB: &str = "/etc/anacrontab";
const RC_LOCAL: &str = "/etc/rc.local";
const PRELOAD: &str = "/etc/ld.so.preload";
const PROFILE_FOLDER: &str = "/etc/profile.d";

fn file(place: StartupPlace, format: StartupFormat, path: impl Into<PathBuf>) -> StartupSpot {
    StartupSpot { place, format, path: path.into(), folder: false, extensions: ANY, read_only: false }
}

fn folder(
    place: StartupPlace,
    format: StartupFormat,
    path: impl Into<PathBuf>,
    extensions: &'static [&'static str],
) -> StartupSpot {
    StartupSpot { place, format, path: path.into(), folder: true, extensions, read_only: false }
}

fn sealed(spot: StartupSpot) -> StartupSpot {
    StartupSpot { read_only: true, ..spot }
}

/// Every place programs start from on `system`, for the user whose home is `home`.
pub fn spots(system: StartupSystem, home: &Path) -> Vec<StartupSpot> {
    let mut spots = match system {
        StartupSystem::MacOs => macos(home),
        StartupSystem::Linux => linux(home),
    };
    let shell = match system {
        StartupSystem::MacOs => MACOS_SHELL_FILES,
        StartupSystem::Linux => LINUX_SHELL_FILES,
    };
    spots.extend(
        USER_SHELL_FILES.iter().map(|name| file(StartupPlace::ShellStartup, StartupFormat::Shell, home.join(name))),
    );
    spots.extend(shell.iter().map(|path| file(StartupPlace::ShellStartup, StartupFormat::Shell, *path)));
    spots.extend(
        AUTHORIZED_KEYS
            .iter()
            .map(|name| file(StartupPlace::AuthorizedKey, StartupFormat::AuthorizedKeys, home.join(name))),
    );
    spots
}

fn macos(home: &Path) -> Vec<StartupSpot> {
    let mut spots = vec![
        folder(StartupPlace::UserAgent, StartupFormat::Launchd, home.join(USER_AGENTS), PLISTS),
        folder(StartupPlace::Agent, StartupFormat::Launchd, AGENTS, PLISTS),
        folder(StartupPlace::Daemon, StartupFormat::Launchd, DAEMONS, PLISTS),
        sealed(folder(StartupPlace::SystemAgent, StartupFormat::Launchd, SYSTEM_AGENTS, PLISTS)),
        sealed(folder(StartupPlace::SystemDaemon, StartupFormat::Launchd, SYSTEM_DAEMONS, PLISTS)),
        folder(StartupPlace::Crontab, StartupFormat::Crontab, MACOS_CRONTABS, ANY),
        file(StartupPlace::Crontab, StartupFormat::SystemCrontab, SYSTEM_CRONTAB),
    ];
    for root in PERIODIC_ROOTS {
        spots.extend(
            PERIODS
                .iter()
                .map(|period| folder(StartupPlace::Periodic, StartupFormat::Script, format!("{root}/{period}"), ANY)),
        );
    }
    spots
}

fn linux(home: &Path) -> Vec<StartupSpot> {
    let mut spots: Vec<StartupSpot> = SYSTEM_UNIT_FOLDERS
        .iter()
        .map(|path| folder(StartupPlace::Service, StartupFormat::Unit, *path, UNITS))
        .collect();
    spots.extend(
        USER_UNIT_FOLDERS.iter().map(|path| folder(StartupPlace::UserService, StartupFormat::Unit, *path, UNITS)),
    );
    spots.extend(
        HOME_UNIT_FOLDERS
            .iter()
            .map(|name| folder(StartupPlace::UserService, StartupFormat::Unit, home.join(name), UNITS)),
    );
    spots.extend([
        folder(StartupPlace::Autostart, StartupFormat::Desktop, home.join(USER_AUTOSTART), DESKTOP),
        folder(StartupPlace::Autostart, StartupFormat::Desktop, AUTOSTART, DESKTOP),
        file(StartupPlace::Crontab, StartupFormat::SystemCrontab, SYSTEM_CRONTAB),
        folder(StartupPlace::Crontab, StartupFormat::SystemCrontab, CRON_FOLDER, ANY),
    ]);
    spots.extend(USER_CRONTABS.iter().map(|path| folder(StartupPlace::Crontab, StartupFormat::Crontab, *path, ANY)));
    spots.extend(
        CRON_PERIODS.iter().map(|period| {
            folder(StartupPlace::CronScript, StartupFormat::Script, format!("{CRON_SCRIPTS}{period}"), ANY)
        }),
    );
    spots.extend([
        file(StartupPlace::Anacron, StartupFormat::Anacrontab, ANACRONTAB),
        file(StartupPlace::RcLocal, StartupFormat::Script, RC_LOCAL),
        file(StartupPlace::Preload, StartupFormat::Preload, PRELOAD),
        folder(StartupPlace::ShellStartup, StartupFormat::Shell, PROFILE_FOLDER, SHELL_SCRIPTS),
    ]);
    spots
}

impl StartupSpot {
    /// The place a file found here is: a systemd folder's timers are timers.
    pub fn place_of(&self, path: &Path) -> StartupPlace {
        let timer = path.extension().is_some_and(|extension| extension == TIMER);
        match (self.place, timer) {
            (StartupPlace::Service, true) => StartupPlace::Timer,
            (StartupPlace::UserService, true) => StartupPlace::UserTimer,
            (place, _) => place,
        }
    }

    /// Whether a file named `name` in this folder is read: not hidden, not an editor's or a
    /// package manager's leftover, and with one of the folder's endings.
    pub fn takes(&self, name: &str) -> bool {
        const LEFTOVERS: &[&str] = &["~", ".dpkg-old", ".dpkg-new", ".dpkg-dist", ".rpmsave", ".rpmnew", ".swp"];
        if name.starts_with('.') || LEFTOVERS.iter().any(|ending| name.ends_with(ending)) {
            return false;
        }
        self.extensions.is_empty()
            || Path::new(name)
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| self.extensions.iter().any(|wanted| extension.eq_ignore_ascii_case(wanted)))
    }
}
