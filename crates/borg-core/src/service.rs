//! Setting up the background service from the Remote menu. The app says how the service stands
//! on this machine, and Borg offers the command that installs or starts it, typed into the
//! host shell, where sudo asks for the password.

use crate::{
    AppState, Console, Effect, Overlay, View,
    command::{AND_THEN, CommandLine},
};
use borg_i18n::tr;
use crossterm::event::{KeyCode, KeyEvent};

const SERVICE: &str = "service";
const INSTALL: &str = "install";
const ENABLE: &str = "enable";
const START: &str = "start";

/// How the background service stands on this machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceStatus {
    /// Nothing here starts a service Borg installs: Linux without systemd, as in most containers.
    Unmanaged,
    Missing,
    /// Installed, and not running.
    Stopped,
    Running,
}

/// What the app found when asked about the background service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceCheck {
    pub status: ServiceStatus,
    /// The Borg program the service would run, as the shell finds it.
    pub program: String,
    /// Borg runs as root, so setting up the service asks for no password.
    pub root: bool,
}

/// What the dialog offers to do with the service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceStep {
    /// Install it, start it at every boot, and start it now.
    Install,
    /// Start the installed service at every boot, and now.
    Start,
}

impl ServiceStep {
    fn subcommands(self) -> &'static [&'static str] {
        match self {
            Self::Install => &[INSTALL, ENABLE, START],
            Self::Start => &[ENABLE, START],
        }
    }
}

/// The dialog that offers to set up the background service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceOffer {
    pub step: ServiceStep,
    pub program: String,
    pub root: bool,
}

impl ServiceOffer {
    fn commands(&self) -> impl Iterator<Item = CommandLine> + '_ {
        self.step
            .subcommands()
            .iter()
            .map(|subcommand| CommandLine::default().word(&self.program).flag(SERVICE).flag(subcommand))
    }

    /// The whole command, each part run once the one before it succeeds.
    fn line(&self) -> CommandLine {
        self.commands().reduce(CommandLine::and_then).unwrap_or_default()
    }

    /// The command as the dialog shows it: one part a row, each but the last ending in `&&`.
    pub fn rows(&self) -> Vec<String> {
        let last = self.step.subcommands().len().saturating_sub(1);
        self.commands()
            .enumerate()
            .map(|(index, command)| if index < last { command.flag(AND_THEN) } else { command })
            .map(|command| String::from_utf8_lossy(&command.line()).into_owned())
            .collect()
    }
}

/// The Remote menu's Background service: the app looks at the service, and `checked` gets the
/// answer.
pub(crate) fn check() -> Vec<Effect> {
    vec![Effect::CheckService]
}

/// How the service stands: a notice when there is nothing to set up, else the dialog, unless
/// another one opened in the meantime.
pub(crate) fn checked(state: &mut AppState, check: ServiceCheck) {
    let step = match check.status {
        ServiceStatus::Unmanaged => {
            state.notice = Some(tr!(service_unmanaged).to_owned());
            return;
        }
        ServiceStatus::Running => {
            state.notice = Some(tr!(service_running).to_owned());
            return;
        }
        ServiceStatus::Missing => ServiceStep::Install,
        ServiceStatus::Stopped => ServiceStep::Start,
    };
    if state.overlay.is_none() {
        state.overlay = Some(Overlay::Service(ServiceOffer { step, program: check.program, root: check.root }));
    }
}

pub(crate) fn on_key(state: &mut AppState, offer: ServiceOffer, key: &KeyEvent) -> Vec<Effect> {
    match key.code {
        KeyCode::Enter => {
            state.overlay = None;
            run(state, &offer)
        }
        KeyCode::Esc | KeyCode::F(10) => {
            state.overlay = None;
            Vec::new()
        }
        _ => Vec::new(),
    }
}

/// Types the command at this computer's shell prompt, kept in its history, and shows the shell
/// while it runs, as installing a missing program does.
fn run(state: &mut AppState, offer: &ServiceOffer) -> Vec<Effect> {
    if state.console != Console::Host {
        state.notice = Some(tr!(service_host_console).to_owned());
        return Vec::new();
    }
    if !crate::update::shell_ready(state) {
        state.notice = Some(tr!(shell_is_busy_finish_or_clear_the).to_owned());
        return Vec::new();
    }
    state.shell.return_view = state.view;
    state.shell.submit();
    state.setting_up_service = state.shell.integrated;
    state.view = View::Shell;
    vec![Effect::Write(offer.line().submit())]
}

/// The command that sets up the service ended: a notice says how it went.
pub(crate) fn finished(state: &mut AppState, exit: Option<i32>) {
    if state.console != Console::Host || !std::mem::take(&mut state.setting_up_service) {
        return;
    }
    state.notice = Some(match exit {
        Some(0) => tr!(service_done).to_owned(),
        Some(code) => tr!(service_failed, code = code),
        None => tr!(service_finished).to_owned(),
    });
}

#[cfg(test)]
mod tests;
