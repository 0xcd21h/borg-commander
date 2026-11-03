//! The variables a capture sets in Borg's shell so that the programs started there give their
//! TLS keys away: `SSLKEYLOGFILE`, or the variables that send HTTPS through Borg's proxy and
//! make programs trust it. Borg types each line at the prompt, where it shows and stays in the
//! history: one sets the variables when the capture starts and one puts back what Borg started
//! with when it ends. A third goes in front of a program started anywhere else.

use super::capture::ProxyEndpoint;
use crate::{AppState, Console, Effect, View, command::CommandLine, update::shell_ready};
use borg_i18n::tr;
use borg_term::ShellKind;
use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    os::unix::ffi::OsStrExt,
    path::Path,
};

/// The variable browsers, curl and other programs read for the file to write their TLS keys to.
pub const KEY_LOG_VARIABLE: &str = "SSLKEYLOGFILE";
/// The variables that send a program's HTTPS through a proxy, in both spellings programs read.
pub const PROXY_VARIABLES: [&str; 2] = ["HTTPS_PROXY", "https_proxy"];
/// The variables that point OpenSSL, curl, Python's requests and Node.js at the certificates
/// they trust.
pub const TRUST_VARIABLES: [&str; 4] = ["SSL_CERT_FILE", "CURL_CA_BUNDLE", "REQUESTS_CA_BUNDLE", "NODE_EXTRA_CA_CERTS"];
const PROXY_SCHEME: &str = "http://";
const POSIX_SET: &str = "export";
const POSIX_CLEAR: &str = "unset";
const FISH_SET: [&str; 2] = ["set", "-gx"];
const FISH_CLEAR: [&str; 2] = ["set", "-e"];

/// Every variable a capture can set in Borg's shell.
pub fn shell_variables() -> impl Iterator<Item = &'static str> {
    std::iter::once(KEY_LOG_VARIABLE).chain(PROXY_VARIABLES).chain(TRUST_VARIABLES)
}

/// Variables with the values a capture gives them.
pub type Assignments = Vec<(&'static str, OsString)>;

/// `SSLKEYLOGFILE` pointing at a capture's key log.
pub fn key_log_assignments(key_log: &Path) -> Assignments {
    vec![(KEY_LOG_VARIABLE, key_log.as_os_str().to_owned())]
}

/// HTTPS through Borg's proxy, trusting the certificates in its bundle.
pub fn proxy_assignments(proxy: &ProxyEndpoint) -> Assignments {
    let address = OsString::from(format!("{PROXY_SCHEME}{}", proxy.address));
    let trust = proxy.trust.as_os_str();
    let proxies = PROXY_VARIABLES.map(|name| (name, address.clone()));
    let trusts = TRUST_VARIABLES.map(|name| (name, trust.to_owned()));
    proxies.into_iter().chain(trusts).collect()
}

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

    /// One `export` for every variable, or one `set -gx` each in fish.
    fn set<'a>(self, assignments: impl IntoIterator<Item = (&'a str, &'a OsStr)>) -> Option<CommandLine> {
        match self {
            Self::Posix => {
                let mut assignments = assignments.into_iter().peekable();
                assignments.peek()?;
                let export = CommandLine::program(POSIX_SET);
                Some(assignments.fold(export, |line, (name, value)| line.assigned(name, value.as_bytes())))
            }
            Self::Fish => {
                sequence(assignments.into_iter().map(|(name, value)| fish(FISH_SET, name).arg(value.as_bytes())))
            }
        }
    }

    /// One `unset` for every variable, or one `set -e` each in fish.
    fn clear<'a>(self, names: impl IntoIterator<Item = &'a str>) -> Option<CommandLine> {
        match self {
            Self::Posix => {
                let mut names = names.into_iter().peekable();
                names.peek()?;
                Some(names.fold(CommandLine::program(POSIX_CLEAR), CommandLine::flag))
            }
            Self::Fish => sequence(names.into_iter().map(|name| fish(FISH_CLEAR, name))),
        }
    }
}

fn fish([program, flag]: [&str; 2], name: &str) -> CommandLine {
    CommandLine::program(program).flag(flag).flag(name)
}

/// The commands one after another, or none when there are none.
fn sequence(commands: impl IntoIterator<Item = CommandLine>) -> Option<CommandLine> {
    commands.into_iter().reduce(CommandLine::then)
}

/// What Borg's shell has for the variables around captures that give TLS keys away.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShellVariables {
    pub dialect: Dialect,
    /// The values Borg started with, which its shell gets back after a capture.
    pub inherited: BTreeMap<&'static str, OsString>,
    /// The variables Borg set for a capture and has not put back.
    pub set: Vec<&'static str>,
    /// The capture ended while the shell was busy, and the line that puts the variables back
    /// waits for it.
    pub restoring: bool,
}

impl ShellVariables {
    pub fn new(dialect: Dialect, inherited: BTreeMap<&'static str, OsString>) -> Self {
        Self { dialect, inherited, set: Vec::new(), restoring: false }
    }

    /// The line that gives the variables a capture's values.
    pub fn set_line(&self, assignments: &Assignments) -> Option<CommandLine> {
        self.dialect.set(assignments.iter().map(|(name, value)| (*name, value.as_os_str())))
    }

    /// The line that puts back what Borg started with: the values it inherited, and no value
    /// for the rest.
    pub fn restore_line(&self) -> Option<CommandLine> {
        let (kept, cleared): (Vec<&str>, Vec<&str>) =
            self.set.iter().partition(|name| self.inherited.contains_key(*name));
        let kept =
            self.dialect.set(kept.into_iter().filter_map(|name| Some((name, self.inherited.get(name)?.as_os_str()))));
        let cleared = self.dialect.clear(cleared);
        sequence(kept.into_iter().chain(cleared))
    }

    /// Whether the variables Borg set point programs at its proxy.
    fn proxy(&self) -> bool {
        !self.set.contains(&KEY_LOG_VARIABLE)
    }
}

/// What goes in front of a program started outside Borg's shell, for it to give its TLS keys
/// away as `assignments` say.
pub fn prefix(assignments: &Assignments) -> String {
    let line =
        assignments.iter().fold(CommandLine::default(), |line, (name, value)| line.assigned(name, value.as_bytes()));
    format!("{} ", text(&line))
}

fn text(line: &CommandLine) -> String {
    String::from_utf8_lossy(&line.line()).into_owned()
}

/// Borg can type at its own shell's prompt: it is the console, and nothing runs or is typed there.
fn typeable(state: &AppState) -> bool {
    state.console == Console::Host && shell_ready(state)
}

/// A capture started giving TLS keys away as `assignments` say: Borg types the line that sets
/// the variables at its shell's prompt and shows the shell while it runs. When the shell is
/// busy, belongs to a container or another machine, or a dialog is open, a notice says they
/// are not set.
pub(crate) fn started(state: &mut AppState, assignments: &Assignments) -> Vec<Effect> {
    let variables = &state.network.shell_variables;
    let line = variables.set_line(assignments);
    let Some(line) = line.filter(|_| typeable(state) && state.overlay.is_none()) else {
        let proxy = !assignments.iter().any(|(name, _)| *name == KEY_LOG_VARIABLE);
        state.notice = Some(if proxy { tr!(capture_proxy_not_set) } else { tr!(capture_tls_not_set) }.to_owned());
        return Vec::new();
    };
    let variables = &mut state.network.shell_variables;
    variables.set = assignments.iter().map(|(name, _)| *name).collect();
    variables.restoring = false;
    state.shell.return_view = state.view;
    state.shell.submit();
    state.view = View::Shell;
    vec![Effect::Write(line.submit())]
}

/// A capture that gave TLS keys away ended: Borg puts the variables back at its shell's prompt
/// without leaving the screen, and a notice names the line unless another one is showing. A
/// busy shell gets the line once it is free.
pub(crate) fn ended(state: &mut AppState) -> Vec<Effect> {
    if state.network.shell_variables.set.is_empty() {
        return Vec::new();
    }
    if !typeable(state) {
        let variables = &mut state.network.shell_variables;
        variables.restoring = true;
        let still = if variables.proxy() { tr!(capture_proxy_still_set) } else { tr!(capture_tls_still_set) };
        state.notice.get_or_insert_with(|| still.to_owned());
        return Vec::new();
    }
    let (effects, said) = restore(state);
    if let Some(said) = said {
        state.notice.get_or_insert(said);
    }
    effects
}

/// Types the line that puts the variables back once the shell that was busy is free.
pub(crate) fn sync(state: &mut AppState) -> Vec<Effect> {
    if !state.network.shell_variables.restoring || !typeable(state) {
        return Vec::new();
    }
    let (effects, said) = restore(state);
    if said.is_some() {
        state.notice = said;
    }
    effects
}

/// Types the line that puts the variables back, and what the notice says about it.
fn restore(state: &mut AppState) -> (Vec<Effect>, Option<String>) {
    let variables = &mut state.network.shell_variables;
    let line = variables.restore_line();
    variables.set.clear();
    variables.restoring = false;
    let Some(line) = line else { return (Vec::new(), None) };
    state.shell.return_view = state.view;
    state.shell.submit();
    (vec![Effect::Write(line.submit())], Some(tr!(capture_tls_restored, line = text(&line))))
}

#[cfg(test)]
mod tests;
