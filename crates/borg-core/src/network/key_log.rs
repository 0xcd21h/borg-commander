//! `SSLKEYLOGFILE` in Borg's shell while a capture records TLS keys. Borg types each line at the
//! prompt, where it shows and stays in the history: one sets the variable when the capture
//! starts and one puts back what Borg started with when it ends. A third goes in front of a
//! program started anywhere else.

use crate::{AppState, Console, Effect, View, command::CommandLine, update::shell_ready};
use borg_i18n::tr;
use borg_term::ShellKind;
use std::path::{Path, PathBuf};

/// The variable browsers, curl and other programs read for the file to write their TLS keys to.
pub const KEY_LOG_VARIABLE: &str = "SSLKEYLOGFILE";
const ASSIGN: &str = "=";
const POSIX_SET: &str = "export";
const POSIX_CLEAR: &str = "unset";
const FISH_SET: [&str; 2] = ["set", "-gx"];
const FISH_CLEAR: [&str; 2] = ["set", "-e"];

/// How a shell sets and clears a variable for the programs it starts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Dialect {
    /// `export` and `unset`, as bash, zsh and sh take them.
    #[default]
    Posix,
    /// `set -gx` and `set -e`.
    Fish,
}

impl Dialect {
    pub fn of(kind: ShellKind) -> Self {
        match kind {
            ShellKind::Fish => Self::Fish,
            ShellKind::Bash | ShellKind::Zsh | ShellKind::Other => Self::Posix,
        }
    }

    fn set(self, value: &Path) -> CommandLine {
        match self {
            Self::Posix => CommandLine::program(POSIX_SET).glued(&assignment(), value),
            Self::Fish => fish(FISH_SET).path(value),
        }
    }

    fn clear(self) -> CommandLine {
        match self {
            Self::Posix => CommandLine::program(POSIX_CLEAR).flag(KEY_LOG_VARIABLE),
            Self::Fish => fish(FISH_CLEAR),
        }
    }
}

fn fish([program, flag]: [&str; 2]) -> CommandLine {
    CommandLine::program(program).flag(flag).flag(KEY_LOG_VARIABLE)
}

fn assignment() -> String {
    format!("{KEY_LOG_VARIABLE}{ASSIGN}")
}

/// What Borg's shell has for the variable around captures that record TLS keys.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShellKeyLog {
    pub dialect: Dialect,
    /// The value Borg started with, which its shell gets back after a capture.
    pub inherited: Option<PathBuf>,
    /// Borg set the variable for a capture and has not put it back.
    pub set: bool,
    /// The capture ended while the shell was busy, and the line that puts the variable back
    /// waits for it.
    pub restoring: bool,
}

impl ShellKeyLog {
    pub fn new(dialect: Dialect, inherited: Option<PathBuf>) -> Self {
        Self { dialect, inherited, set: false, restoring: false }
    }

    /// The line that points the variable at a capture's key log.
    pub fn set_line(&self, key_log: &Path) -> CommandLine {
        self.dialect.set(key_log)
    }

    /// The line that puts back what Borg started with.
    pub fn restore_line(&self) -> CommandLine {
        self.inherited.as_deref().map_or_else(|| self.dialect.clear(), |value| self.dialect.set(value))
    }
}

/// What goes in front of a program started outside Borg's shell, for it to write its TLS keys
/// to `key_log`.
pub fn key_log_prefix(key_log: &Path) -> String {
    format!("{} ", text(&CommandLine::default().glued(&assignment(), key_log)))
}

fn text(line: &CommandLine) -> String {
    String::from_utf8_lossy(&line.line()).into_owned()
}

/// Borg can type at its own shell's prompt: it is the console, and nothing runs or is typed there.
fn typeable(state: &AppState) -> bool {
    state.console == Console::Host && shell_ready(state)
}

/// A capture started writing TLS keys to `key_log`: Borg types the line that sets the variable
/// at its shell's prompt and shows the shell while it runs. When the shell is busy, belongs to a
/// container or another machine, or a dialog is open, a notice says the variable is not set.
pub(crate) fn started(state: &mut AppState, key_log: &Path) -> Vec<Effect> {
    if !typeable(state) || state.overlay.is_some() {
        state.notice = Some(tr!(capture_tls_not_set).to_owned());
        return Vec::new();
    }
    let keys = &mut state.network.shell_key_log;
    let line = keys.set_line(key_log);
    keys.set = true;
    keys.restoring = false;
    state.shell.return_view = state.view;
    state.shell.submit();
    state.view = View::Shell;
    vec![Effect::Write(line.submit())]
}

/// A capture that recorded TLS keys ended: Borg puts the variable back at its shell's prompt
/// without leaving the screen, and a notice names the line unless another one is showing. A
/// busy shell gets the line once it is free.
pub(crate) fn ended(state: &mut AppState) -> Vec<Effect> {
    if !state.network.shell_key_log.set {
        return Vec::new();
    }
    if !typeable(state) {
        state.network.shell_key_log.restoring = true;
        state.notice.get_or_insert_with(|| tr!(capture_tls_still_set).to_owned());
        return Vec::new();
    }
    let (effects, said) = restore(state);
    state.notice.get_or_insert(said);
    effects
}

/// Types the line that puts the variable back once the shell that was busy is free.
pub(crate) fn sync(state: &mut AppState) -> Vec<Effect> {
    if !state.network.shell_key_log.restoring || !typeable(state) {
        return Vec::new();
    }
    let (effects, said) = restore(state);
    state.notice = Some(said);
    effects
}

/// Types the line that puts the variable back, and what the notice says about it.
fn restore(state: &mut AppState) -> (Vec<Effect>, String) {
    let keys = &mut state.network.shell_key_log;
    let line = keys.restore_line();
    keys.set = false;
    keys.restoring = false;
    state.shell.return_view = state.view;
    state.shell.submit();
    (vec![Effect::Write(line.submit())], tr!(capture_tls_restored, line = text(&line)))
}

#[cfg(test)]
mod tests;
