//! Offering to install a program Borg runs and cannot find: the package manager that installs
//! it on this machine, the exact command, and the dialog that asks before typing it into the
//! shell, where the package manager asks for the password and shows what it does.

use crate::{AppState, Console, Effect, Overlay, View, command::CommandLine};
use borg_i18n::tr;
use crossterm::event::{KeyCode, KeyEvent};

/// A program Borg runs from the system, which it offers to install when it is missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Tcpdump,
}

impl Tool {
    /// The program's name on `PATH`.
    pub fn program(self) -> &'static str {
        match self {
            Self::Tcpdump => "tcpdump",
        }
    }

    /// The package that holds the program in `manager`'s repositories.
    pub fn package(self, manager: PackageManager) -> &'static str {
        match (self, manager) {
            (Self::Tcpdump, _) => "tcpdump",
        }
    }

    /// What Borg says when the program is missing and it knows no way to install it.
    fn missing(self) -> &'static str {
        match self {
            Self::Tcpdump => tr!(capture_missing),
        }
    }

    /// What Borg says once the program is installed.
    fn installed(self) -> &'static str {
        match self {
            Self::Tcpdump => tr!(install_tcpdump_done),
        }
    }
}

/// The system Borg runs on, which decides the package managers it looks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Linux,
    MacOs,
}

/// The system's own package managers first, then Homebrew.
const LINUX_MANAGERS: [PackageManager; 6] = [
    PackageManager::Apt,
    PackageManager::Dnf,
    PackageManager::Pacman,
    PackageManager::Zypper,
    PackageManager::Apk,
    PackageManager::Homebrew,
];
const MACOS_MANAGERS: [PackageManager; 1] = [PackageManager::Homebrew];

impl Platform {
    /// The package managers Borg looks for here, in the order it prefers them.
    fn managers(self) -> &'static [PackageManager] {
        match self {
            Self::Linux => &LINUX_MANAGERS,
            Self::MacOs => &MACOS_MANAGERS,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageManager {
    Apt,
    Dnf,
    Pacman,
    Zypper,
    Apk,
    Homebrew,
}

impl PackageManager {
    /// The name people know it by.
    pub fn name(self) -> &'static str {
        match self {
            Self::Apt => "apt",
            Self::Dnf => "dnf",
            Self::Pacman => "pacman",
            Self::Zypper => "zypper",
            Self::Apk => "apk",
            Self::Homebrew => "Homebrew",
        }
    }

    /// The program that runs it, whose presence shows it is installed.
    pub fn program(self) -> &'static str {
        match self {
            Self::Apt => "apt-get",
            Self::Dnf => "dnf",
            Self::Pacman => "pacman",
            Self::Zypper => "zypper",
            Self::Apk => "apk",
            Self::Homebrew => "brew",
        }
    }

    /// The words between the program and the package's name.
    fn install(self) -> &'static [&'static str] {
        match self {
            Self::Apt | Self::Dnf | Self::Zypper | Self::Homebrew => &["install"],
            Self::Pacman => &["-S"],
            Self::Apk => &["add"],
        }
    }

    /// The system's package managers change the system, so they run as root. Homebrew installs
    /// as you and refuses to run as root.
    pub fn needs_root(self) -> bool {
        self != Self::Homebrew
    }
}

/// What gives a package manager root when Borg runs as someone else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Elevation {
    Sudo,
    Doas,
}

const ELEVATIONS: [Elevation; 2] = [Elevation::Sudo, Elevation::Doas];

impl Elevation {
    pub fn program(self) -> &'static str {
        match self {
            Self::Sudo => "sudo",
            Self::Doas => "doas",
        }
    }
}

/// How Borg installs a missing program on this machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Installer {
    pub manager: PackageManager,
    /// What the command runs under to get root, or `None` when Borg is root already or the
    /// manager needs no root.
    pub elevation: Option<Elevation>,
}

impl Installer {
    /// The installer on `platform`, from the programs `installed` finds: the first package
    /// manager that can run there, under `sudo` or `doas` when it needs root and Borg is not
    /// root.
    pub fn detect(platform: Platform, root: bool, installed: impl Fn(&str) -> bool) -> Option<Self> {
        let elevation = ELEVATIONS.into_iter().find(|elevation| installed(elevation.program()));
        platform.managers().iter().copied().filter(|manager| installed(manager.program())).find_map(|manager| {
            match (manager.needs_root(), root) {
                (false, true) => None,
                (false, false) | (true, true) => Some(Self { manager, elevation: None }),
                (true, false) => elevation.map(|elevation| Self { manager, elevation: Some(elevation) }),
            }
        })
    }

    fn line(self, tool: Tool) -> CommandLine {
        let words = self.elevation.map(Elevation::program).into_iter().chain([self.manager.program()]);
        let words = words.chain(self.manager.install().iter().copied()).chain([tool.package(self.manager)]);
        words.fold(CommandLine::default(), CommandLine::flag)
    }

    /// The command that installs `tool`, as the dialog shows it and the shell runs it.
    pub fn command(self, tool: Tool) -> String {
        String::from_utf8_lossy(&self.line(tool).line()).into_owned()
    }
}

/// The dialog that offers to install a missing program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstallOffer {
    pub tool: Tool,
    pub installer: Installer,
}

/// A program Borg needs is missing: the dialog offers to install it when Borg knows how and no
/// other dialog is open, and a notice says it is missing otherwise.
pub(crate) fn offer(state: &mut AppState, tool: Tool, installer: Option<Installer>) {
    match installer.filter(|_| state.overlay.is_none()) {
        Some(installer) => state.overlay = Some(Overlay::Install(InstallOffer { tool, installer })),
        None => state.notice = Some(tool.missing().to_owned()),
    }
}

pub(crate) fn on_key(state: &mut AppState, offer: InstallOffer, key: &KeyEvent) -> Vec<Effect> {
    match key.code {
        KeyCode::Enter => {
            state.overlay = None;
            run(state, offer)
        }
        KeyCode::Esc | KeyCode::F(10) => {
            state.overlay = None;
            Vec::new()
        }
        _ => Vec::new(),
    }
}

/// Types the command at this computer's shell prompt, kept in its history, and shows the shell
/// while it runs. The shell stays in its folder, so that the next command to end is this one.
fn run(state: &mut AppState, offer: InstallOffer) -> Vec<Effect> {
    let program = offer.tool.program();
    if state.console != Console::Host {
        state.notice = Some(tr!(install_host_console, program = program));
        return Vec::new();
    }
    if !crate::update::shell_ready(state) {
        state.notice = Some(tr!(shell_is_busy_finish_or_clear_the).to_owned());
        return Vec::new();
    }
    state.shell.return_view = state.view;
    state.shell.submit();
    state.installing = state.shell.integrated.then_some(offer.tool);
    state.view = View::Shell;
    vec![Effect::Write(offer.installer.line(offer.tool).submit())]
}

/// The install command ended: a notice says how it went.
pub(crate) fn finished(state: &mut AppState, exit: Option<i32>) {
    if state.console != Console::Host {
        return;
    }
    let Some(tool) = state.installing.take() else { return };
    let program = tool.program();
    state.notice = Some(match exit {
        Some(0) => tool.installed().to_owned(),
        Some(code) => tr!(install_failed, program = program, code = code),
        None => tr!(install_finished, program = program),
    });
}

#[cfg(test)]
mod tests;
