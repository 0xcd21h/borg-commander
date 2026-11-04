//! What Borg does about a finding: tell the user, and freeze the process so it stops where it
//! is without exiting, which leaves its service running and gives supervisors nothing to
//! restart. The guardrails here decide what is never frozen.

use borg_i18n::tr;
use borg_rules::Level;
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    path::Path,
    time::{Duration, SystemTime},
};

use super::{Baseline, Finding, History, StartedProcess, examine};

/// Processes Borg writes alerts about in one minute; over that it only counts what it left out.
pub const ALERTS_PER_MINUTE: usize = 20;
/// Processes Borg freezes in one minute.
pub const FREEZES_PER_MINUTE: usize = 5;
const MINUTE: Duration = Duration::from_secs(60);
/// The system's first process, which nothing may freeze.
const FIRST_PROCESS: u32 = 1;
/// Programs Borg never freezes, whatever fires: losing them locks the owner out of the machine
/// or stops it from running at all.
pub const NEVER_FROZEN: &[&str] = &[
    "init",
    "systemd",
    "systemd-logind",
    "launchd",
    "sshd",
    "sudo",
    "su",
    "doas",
    "login",
    "agetty",
    "getty",
    "dbus-daemon",
    "polkitd",
    "borg",
];
/// Programs that open a session on a terminal for somebody who logged in: over SSH, at the
/// console, in a terminal multiplexer, as another user or in a terminal window. A terminal any
/// other program opened for itself does not say that a person is working in it.
const LOGINS: &[&str] = &[
    "sshd",
    "sshd-session",
    "login",
    "agetty",
    "getty",
    "mingetty",
    "mosh-server",
    "sudo",
    "su",
    "doas",
    "runuser",
    "gnome-terminal-server",
    "konsole",
    "xterm",
    "xfce4-terminal",
    "mate-terminal",
    "lxterminal",
    "tilix",
    "terminator",
    "kitty",
    "alacritty",
    "wezterm-gui",
    "foot",
    "urxvt",
];
/// Terminal multiplexers, which open a session for whoever starts them. A service account can
/// start one as easily as a person can, so they count only for root and people's accounts.
const MULTIPLEXERS: &[&str] = &["tmux", "screen"];
/// The account of the machine's owner.
const ROOT: u32 = 0;
/// The first number Linux gives an account for a person where the system does not say otherwise;
/// the ones below it belong to the system and its services.
pub const FIRST_PERSON: u32 = 1000;
/// Folders that only the machine's owner may write to, where a program Borg spares by its name
/// must lie, so a copy named after it elsewhere is not spared.
const SYSTEM_FOLDERS: &[&str] = &["/usr", "/bin", "/sbin", "/lib", "/lib64", "/opt", "/nix/store", "/snap"];

/// Why a process was left running although a rule fired.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Protection {
    /// The system's first process.
    FirstProcess,
    /// Borg itself, or something Borg started.
    Borg,
    /// It belongs to a terminal somebody logged in to, so they may be working in it.
    Terminal,
    /// A program on the list Borg never freezes.
    NeverFrozen(String),
    /// The user allowed starts like this one.
    Allowed,
    /// Freezing is turned off in the configuration.
    FreezeOff,
    /// Only a notice, which is not worth stopping a program for.
    OnlyANotice,
    /// Borg had already frozen as much as it may in this minute.
    EnoughForNow,
    /// The user has not yet seen what Borg would have frozen while it learned.
    Unreviewed,
}

impl Protection {
    /// Why Borg left it running, in one sentence.
    pub fn reason(&self) -> String {
        match self {
            Self::FirstProcess => tr!(watch_spared_first_process).into(),
            Self::Borg => tr!(watch_spared_borg).into(),
            Self::Terminal => tr!(watch_spared_terminal).into(),
            Self::NeverFrozen(program) => tr!(watch_spared_never_frozen, program = program),
            Self::Allowed => tr!(watch_spared_allowed).into(),
            Self::FreezeOff => tr!(watch_spared_freeze_off).into(),
            Self::OnlyANotice => tr!(watch_spared_only_a_notice).into(),
            Self::EnoughForNow => tr!(watch_spared_enough_for_now).into(),
            Self::Unreviewed => tr!(watch_spared_unreviewed).into(),
        }
    }
}

/// What Borg did about a finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Response {
    /// Borg is still learning what is normal here, so it only wrote down what it would do.
    Learning {
        would_freeze: bool,
    },
    /// Told the user and left the process running.
    Alerted,
    Frozen,
    /// Left running, for a reason that protects it.
    Spared(Protection),
}

impl Response {
    /// The word the Alerts page puts in its Response column.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Learning { would_freeze: true } => tr!(watch_response_would_freeze),
            Self::Learning { would_freeze: false } => tr!(watch_response_learning),
            Self::Alerted => tr!(watch_response_alerted),
            Self::Frozen => tr!(watch_response_frozen),
            Self::Spared(_) => tr!(watch_response_spared),
        }
    }

    pub fn froze(&self) -> bool {
        *self == Self::Frozen
    }
}

/// What the machine looks like when Borg decides, so the decision stays a pure function.
#[derive(Debug, Clone, Copy)]
pub struct Guard<'a> {
    /// The process Borg's watcher runs as.
    pub own: u32,
    /// Programs the configuration adds to those Borg never freezes.
    pub keep_running: &'a [String],
    /// Whether Borg may freeze at all.
    pub freeze: bool,
    /// Whether the user has seen every alert that says Borg would have frozen a process while it
    /// learned. Freezing waits until they have, so nothing stops before they have checked.
    pub reviewed: bool,
    /// The first account number the system gives a person.
    pub first_person: u32,
}

/// How much the watcher does in a minute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub alerts_per_minute: usize,
    pub freezes_per_minute: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self { alerts_per_minute: ALERTS_PER_MINUTE, freezes_per_minute: FREEZES_PER_MINUTE }
    }
}

/// How often something happened lately, so the watcher can stop doing it too much.
#[derive(Debug, Clone, Default)]
struct Rate {
    moments: VecDeque<SystemTime>,
}

impl Rate {
    fn allow(&mut self, limit: usize, now: SystemTime) -> bool {
        self.moments.retain(|moment| now.duration_since(*moment).is_ok_and(|age| age < MINUTE));
        if self.moments.len() >= limit {
            return false;
        }
        self.moments.push_back(now);
        true
    }
}

/// Whether a program lies where only the machine's owner could have put it. A program Borg
/// could not read is judged by its name alone.
fn installed(program: Option<&Path>) -> bool {
    program.is_none_or(|program| SYSTEM_FOLDERS.iter().any(|folder| program.starts_with(folder)))
}

/// Whether the account a process runs as belongs to root or to a person.
fn personal(process: &StartedProcess, first_person: u32) -> bool {
    process.uid == ROOT || process.uid >= first_person
}

/// Whether somebody logged in to the terminal a process belongs to. A terminal a program opened
/// for itself does not count.
fn logged_in(process: &StartedProcess, first_person: u32) -> bool {
    process.has_terminal()
        && process.session_parent.as_ref().is_some_and(|opener| {
            let program = opener.program_name();
            let login =
                LOGINS.contains(&program) || (MULTIPLEXERS.contains(&program) && personal(process, first_person));
            login && installed(opener.program.as_deref())
        })
}

/// Why a process is never frozen, whatever fired.
fn protection(process: &StartedProcess, baseline: &Baseline, guard: &Guard<'_>) -> Option<Protection> {
    if process.key.pid == FIRST_PROCESS {
        return Some(Protection::FirstProcess);
    }
    if process.key.pid == guard.own || process.ancestors.iter().any(|parent| parent.pid == guard.own) {
        return Some(Protection::Borg);
    }
    let program = process.program_name();
    if (NEVER_FROZEN.contains(&program) && installed(process.program.as_deref()))
        || guard.keep_running.iter().any(|kept| kept == program)
    {
        return Some(Protection::NeverFrozen(program.to_owned()));
    }
    if logged_in(process, guard.first_person) {
        return Some(Protection::Terminal);
    }
    if baseline.allows(process) {
        return Some(Protection::Allowed);
    }
    None
}

/// What Borg does about one finding. Nothing is frozen while Borg is still learning; the log
/// then says what it would have done, and freezing waits until the user has seen that.
pub fn decide(
    finding: &Finding,
    process: &StartedProcess,
    baseline: &Baseline,
    guard: &Guard<'_>,
    now: SystemTime,
) -> Response {
    let spared = if !guard.freeze {
        Some(Protection::FreezeOff)
    } else if finding.level() == Level::Notice {
        Some(Protection::OnlyANotice)
    } else {
        protection(process, baseline, guard)
    };
    if baseline.learning(now) {
        return Response::Learning { would_freeze: spared.is_none() };
    }
    match spared {
        Some(protection) => Response::Spared(protection),
        None if !guard.reviewed && now < baseline.review_ends() => Response::Spared(Protection::Unreviewed),
        None => Response::Frozen,
    }
}

/// A finding and what Borg did about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub finding: Finding,
    pub response: Response,
}

/// The watcher's own memory: the baseline it learns, the starts it has seen, and how much it
/// has done lately. The app feeds it processes and carries out what it decides.
#[derive(Debug, Clone, Default)]
pub struct Watcher {
    pub baseline: Baseline,
    pub limits: Limits,
    history: History,
    /// Processes written about for a warning or an alert this minute.
    alerts: Rate,
    /// Processes written about for a notice, counted apart so a run of them never uses up the
    /// room for warnings and alerts.
    notices: Rate,
    /// Processes Borg would have frozen past the freeze limit, counted apart so none of them is
    /// lost among the others.
    unfrozen: Rate,
    freezes: Rate,
    suppressed: u64,
    seen: u64,
}

impl Watcher {
    pub fn new(baseline: Baseline, limits: Limits) -> Self {
        Self { baseline, limits, ..Self::default() }
    }

    /// Processes the watcher has seen start.
    pub fn seen(&self) -> u64 {
        self.seen
    }

    /// Findings left out because too many fired at once.
    pub fn suppressed(&self) -> u64 {
        self.suppressed
    }

    /// Looks at a process that has just started: what fired, and what Borg does about it.
    /// While Borg learns, the start joins the baseline, so what a machine does becomes what it
    /// may do; a start a rule fired on that learning never quiets stays out of it.
    ///
    /// The limit on alerts only ever leaves out a finding Borg would not freeze for: a frozen
    /// process is always written down, or nobody would know to let it go on.
    pub fn examine(&mut self, process: &StartedProcess, guard: &Guard<'_>, now: SystemTime) -> Vec<Outcome> {
        self.seen = self.seen.saturating_add(1);
        let findings = examine(process, &self.baseline, &self.history, now);
        self.history.note(process);
        if self.baseline.learning(now) && findings.iter().all(|finding| finding.rule().learnable()) {
            self.baseline.note(process);
        }
        let Some(worst) = findings.first() else { return Vec::new() };
        let mut response = decide(worst, process, &self.baseline, guard, now);
        let wanted = response == Response::Frozen;
        if wanted && !self.freezes.allow(self.limits.freezes_per_minute, now) {
            response = Response::Spared(Protection::EnoughForNow);
        }
        let room = if wanted {
            &mut self.unfrozen
        } else if worst.level() == Level::Notice {
            &mut self.notices
        } else {
            &mut self.alerts
        };
        if !response.froze() && !room.allow(self.limits.alerts_per_minute, now) {
            self.suppressed = self.suppressed.saturating_add(findings.len() as u64);
            return Vec::new();
        }
        findings.into_iter().map(|finding| Outcome { finding, response: response.clone() }).collect()
    }

    /// Allows starts like this one from now on.
    pub fn allow(&mut self, allowance: super::Allowance) {
        self.baseline.allow(allowance);
    }
}
