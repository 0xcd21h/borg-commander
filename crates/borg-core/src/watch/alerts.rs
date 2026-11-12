//! The alerts the watcher writes down, the bounded log they live in, and what the Alerts page
//! shows: the evidence kept at the moment a rule fired, so it is still there once the process
//! is gone.

use borg_i18n::tr;
use borg_rules::{Level, RuleMatch};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, time::SystemTime};

use super::{AlertDelivery, Allowance, Finding, Response, StartedProcess, WatchRule};
use crate::ProcessKey;

/// Alerts the log keeps, newest first.
pub const ALERTS_KEPT: usize = 200;
const EXCLUDE: char = '!';

/// Where the watcher's events come from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WatchSource {
    /// Every process on the machine, from Borg's own programs inside the kernel, which read
    /// each program start before the process can do anything.
    Ebpf,
    /// Every process on the machine, from the kernel, which needs the root watcher.
    Kernel,
    /// The user's own processes, by looking at the process list again and again.
    OwnProcesses,
    /// Every process on the machine, by looking at the process list again and again, where the
    /// kernel sends no process events.
    Polled,
    #[default]
    Off,
}

impl WatchSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ebpf => tr!(watch_source_ebpf),
            Self::Kernel => tr!(watch_source_kernel),
            Self::OwnProcesses => tr!(watch_source_own),
            Self::Polled => tr!(watch_source_polled),
            Self::Off => tr!(watch_source_off),
        }
    }
}

/// How the watcher stands, as the Alerts page tells the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatchStatus {
    pub source: WatchSource,
    /// When the watcher started learning this machine.
    pub since: SystemTime,
    pub learned_at: SystemTime,
    /// Whether Borg may freeze what it finds.
    pub freeze: bool,
    /// Processes the watcher has seen start.
    #[serde(default)]
    pub seen: u64,
    /// Findings left out because too many fired at once.
    #[serde(default)]
    pub suppressed: u64,
    /// Where the watcher sends alerts besides this log.
    #[serde(default)]
    pub delivery: AlertDelivery,
}

impl Default for WatchStatus {
    fn default() -> Self {
        Self {
            source: WatchSource::Off,
            since: SystemTime::UNIX_EPOCH,
            learned_at: SystemTime::UNIX_EPOCH,
            freeze: false,
            seen: 0,
            suppressed: 0,
            delivery: AlertDelivery::default(),
        }
    }
}

impl WatchStatus {
    pub fn learning(&self, now: SystemTime) -> bool {
        self.source != WatchSource::Off && now < self.learned_at
    }
}

/// What Borg kept at the moment a rule fired.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    /// The connections the process held, each as one line.
    #[serde(default)]
    pub connections: Vec<String>,
    /// The program's SHA-256, when Borg could read the file.
    #[serde(default)]
    pub program_hash: Option<String>,
    /// What Borg's rules and the user's matched in the program and its memory.
    #[serde(default)]
    pub hits: Vec<RuleMatch>,
    /// What kept the evidence from being complete.
    #[serde(default)]
    pub missing: Option<String>,
}

/// What has become of the process the alert is about.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlertState {
    /// Running, and Borg is leaving it alone.
    #[default]
    Running,
    Frozen,
    Resumed,
    Killed,
    /// The user allowed starts like this one.
    Allowed,
    Gone,
}

impl AlertState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Running => tr!(watch_state_running),
            Self::Frozen => tr!(watch_state_frozen),
            Self::Resumed => tr!(watch_state_resumed),
            Self::Killed => tr!(watch_state_killed),
            Self::Allowed => tr!(watch_state_allowed),
            Self::Gone => tr!(watch_state_gone),
        }
    }
}

/// One rule firing on one process, with everything the user needs to judge it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Alert {
    pub id: u64,
    pub at: SystemTime,
    pub finding: Finding,
    pub response: Response,
    pub process: StartedProcess,
    #[serde(default)]
    pub evidence: Evidence,
    #[serde(default)]
    pub state: AlertState,
    #[serde(default)]
    pub seen: bool,
}

impl Alert {
    pub fn rule(&self) -> WatchRule {
        self.finding.rule()
    }

    pub fn level(&self) -> Level {
        self.finding.level()
    }

    pub fn program(&self) -> &str {
        self.process.program_name()
    }

    pub fn parent(&self) -> Option<&str> {
        self.process.parent().map(super::Ancestor::program_name)
    }

    /// Whether Borg froze the process and nobody has resumed or killed it since.
    pub fn frozen(&self) -> bool {
        self.state == AlertState::Frozen
    }

    /// What the user may do to it: a frozen process can be resumed or killed.
    pub fn resumable(&self) -> bool {
        self.frozen()
    }

    /// An allowance that covers starts like this one.
    pub fn allowance(&self) -> Allowance {
        Allowance {
            program: self.process.program.as_ref().map(|path| path.display().to_string()),
            parent: self.parent().map(str::to_owned),
            user: None,
        }
    }

    /// Everything the filter looks through, in lower case.
    fn haystack(&self) -> String {
        let rule = self.rule();
        let words = [
            rule.label().to_owned(),
            rule.code().to_owned(),
            self.finding.reason(),
            self.program().to_owned(),
            self.parent().unwrap_or_default().to_owned(),
            self.process.user.clone(),
            self.process.command_line(),
            self.response.label().to_owned(),
            self.state.label().to_owned(),
        ];
        words.join("\n").to_lowercase()
    }
}

/// The alerts on this machine and how the watcher stands, as the state folder keeps them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlertLog {
    #[serde(default)]
    pub watch: WatchStatus,
    /// Newest first.
    #[serde(default)]
    pub alerts: Vec<Alert>,
    #[serde(default)]
    next_id: u64,
    /// Where an earlier log went that Borg could not read, kept whole beside this one while
    /// that file is there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kept_aside: Option<PathBuf>,
}

impl AlertLog {
    /// An empty log for a watcher in this state.
    pub fn watching(watch: WatchStatus) -> Self {
        Self { watch, ..Self::default() }
    }

    /// A new log beside one Borg could not read, which it kept at `aside`.
    pub fn beside(aside: PathBuf) -> Self {
        Self { kept_aside: Some(aside), ..Self::default() }
    }

    /// Puts an alert among the others under an id of its own, newest first, and drops the oldest
    /// over the limit. An alert about a process that is still frozen stays, since it is the only
    /// place the user can let that process go on. The alert keeps the process with its commands cut to
    /// [`super::COMMAND_KEPT`] bytes.
    pub fn add(&mut self, at: SystemTime, finding: Finding, response: Response, process: StartedProcess) -> u64 {
        self.next_id = self.next_id.saturating_add(1);
        let id = self.next_id;
        let state = if response.froze() { AlertState::Frozen } else { AlertState::Running };
        let alert = Alert {
            id,
            at,
            finding,
            response,
            process: process.kept(),
            evidence: Evidence::default(),
            state,
            seen: false,
        };
        let place = self.alerts.iter().position(|other| other.at <= at).unwrap_or(self.alerts.len());
        self.alerts.insert(place, alert);
        while self.alerts.len() > ALERTS_KEPT && self.drop_oldest_unfrozen().is_some() {}
        id
    }

    /// Takes out the oldest alert that is not about a process still frozen.
    pub fn drop_oldest_unfrozen(&mut self) -> Option<Alert> {
        let place = self.alerts.iter().rposition(|alert| !alert.frozen())?;
        Some(self.alerts.remove(place))
    }

    /// Takes out the oldest alert, about a frozen process only when no other is left.
    pub fn drop_oldest(&mut self) -> Option<Alert> {
        self.drop_oldest_unfrozen().or_else(|| self.alerts.pop())
    }

    pub fn find(&mut self, id: u64) -> Option<&mut Alert> {
        self.alerts.iter_mut().find(|alert| alert.id == id)
    }

    pub fn unread(&self) -> usize {
        self.alerts.iter().filter(|alert| !alert.seen).count()
    }

    /// Whether the user has seen every alert that says Borg would have frozen a process while
    /// it learned.
    pub fn reviewed(&self) -> bool {
        !self.alerts.iter().any(|alert| !alert.seen && alert.response == Response::Learning { would_freeze: true })
    }

    /// Until when freezing waits for the user to see what Borg would have frozen while it
    /// learned, while it does.
    pub fn freezing_waits(&self, now: SystemTime) -> Option<SystemTime> {
        let ends = super::review_ends(self.watch.learned_at);
        let waits = self.watch.freeze && !self.watch.learning(now) && now < ends && !self.reviewed();
        waits.then_some(ends)
    }

    pub fn mark_seen(&mut self) {
        for alert in &mut self.alerts {
            alert.seen = true;
        }
    }
}

/// The alerts `filter` keeps: each word must appear, and none of those after `!`.
pub fn filtered(alerts: &[Alert], filter: &str) -> Vec<usize> {
    let words: Vec<String> = filter.split_whitespace().map(str::to_lowercase).collect();
    alerts
        .iter()
        .enumerate()
        .filter(|(_, alert)| {
            let haystack = alert.haystack();
            words.iter().all(|word| match word.strip_prefix(EXCLUDE) {
                Some(excluded) => excluded.is_empty() || !haystack.contains(excluded),
                None => haystack.contains(word.as_str()),
            })
        })
        .map(|(index, _)| index)
        .collect()
}

/// What the Alerts page asks the app to do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlertsRequest {
    /// Reads the log and marks what it holds as read, as opening the page does.
    Open,
    /// Reads the log again, leaving unread alerts unread.
    Read,
    /// Lets a frozen process go on running.
    Resume {
        alert: u64,
        process: ProcessKey,
    },
    Kill {
        alert: u64,
        process: ProcessKey,
    },
    /// Allows starts like the alert's from now on.
    Allow {
        alert: u64,
        allowance: Allowance,
    },
}

/// The log after the app did what the page asked, and what to tell the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlertsReply {
    pub log: AlertLog,
    pub notice: Option<String>,
}
