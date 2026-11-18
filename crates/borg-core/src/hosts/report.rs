//! A paired host as an agent reads it: what its card on the hosts page shows, from the same
//! cards in the same order, in plain units. What a host says in its own words comes on one
//! line, cut short, so it cannot pass for more of the answer.

use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;

use super::{ContainerFigures, Figure, HostCard, HostFigures, Hosts, Reach, WHOLE};
use crate::{LoadAverage, Severity};

/// The most characters of a host's own text in one field.
const TEXT_LIMIT: usize = 200;
/// The most sessions of one host in a report.
const SESSIONS_LIMIT: usize = 50;

/// How much a concern should worry the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConcernLevel {
    Notice,
    Warning,
    Alert,
}

impl From<Severity> for ConcernLevel {
    fn from(severity: Severity) -> Self {
        match severity {
            Severity::Alert => Self::Alert,
            Severity::Warning => Self::Warning,
            Severity::Notice => Self::Notice,
        }
    }
}

/// Whether the host answered the last time Borg asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReachReport {
    Waiting,
    Answering,
    /// Connected to the relay, but it did not answer in time.
    Silent,
    /// The relay says it is not connected.
    Offline,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HostReport {
    /// The name the user gave the host on this computer, or its own.
    pub name: String,
    /// What `use_session` and the tools' `session` argument take to work on the host.
    pub target: String,
    pub reach: ReachReport,
    /// Why reaching the host failed, which may be the host's own words.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// How long the host has not answered in time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub silent_seconds: Option<u64>,
    /// When the host last answered, as this computer remembers it, in UTC.
    pub last_answered: Option<String>,
    pub round_trip_ms: Option<u64>,
    /// Its Borg version.
    pub borg: Option<String>,
    /// Its Borg is older than the one asking, and too old to send figures when they are missing.
    pub older_borg: bool,
    /// Its system and processor.
    pub system: Option<String>,
    /// The worst of the concerns.
    pub level: Option<ConcernLevel>,
    pub concerns: Vec<ConcernReport>,
    /// Missing while the host has not answered, or when its Borg sends none.
    pub figures: Option<FiguresReport>,
    /// How old the figures are, once they are older than a refresh or two.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub figures_age_seconds: Option<u64>,
    pub sessions: Vec<SessionReport>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ConcernReport {
    pub figure: Figure,
    pub level: ConcernLevel,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SessionReport {
    pub name: String,
    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FiguresReport {
    pub cores: usize,
    /// How busy all the cores were since the host last answered.
    pub cpu_percent: Option<f64>,
    pub load: Option<LoadReport>,
    pub uptime_seconds: Option<u64>,
    pub memory: Option<MemoryReport>,
    /// The fullest filesystem that holds files.
    pub disk: Option<DiskReport>,
    /// The process watcher.
    pub watcher: Option<WatcherReport>,
    pub containers: Option<ContainerFigures>,
}

/// The number of processes waiting to run, averaged over one, five and fifteen minutes.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LoadReport {
    pub one: f64,
    pub five: f64,
    pub fifteen: f64,
    /// The last minute's load as a share of the cores.
    pub percent_of_cores: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MemoryReport {
    pub used_bytes: u64,
    pub total_bytes: u64,
    pub percent: f64,
    pub swap_used_bytes: u64,
    pub swap_total_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DiskReport {
    pub mount: String,
    pub total_bytes: u64,
    /// What the user may still write there.
    pub available_bytes: u64,
    pub percent_used: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WatcherReport {
    pub on: bool,
    /// Until when it learns what is usual, in UTC.
    pub learning_until: Option<String>,
    pub unread_alerts: usize,
    pub unread_warnings: usize,
    pub unread_notices: usize,
    /// Processes it froze that nobody let go on or ended.
    pub frozen: usize,
}

/// The hosts the page lists, in its order and through its filter.
pub fn host_reports(hosts: &Hosts, now: Instant) -> Vec<HostReport> {
    hosts.rows.iter().filter_map(|&index| hosts.cards.get(index)).map(|card| report(card, now)).collect()
}

fn report(card: &HostCard, now: Instant) -> HostReport {
    let (reach, error, silent_seconds) = match &card.reach {
        Reach::Waiting => (ReachReport::Waiting, None, None),
        Reach::Answering => (ReachReport::Answering, None, None),
        Reach::Silent { since } => {
            let seconds = now.saturating_duration_since(*since).as_secs();
            (ReachReport::Silent, None, Some(seconds).filter(|seconds| *seconds > 0))
        }
        Reach::Offline => (ReachReport::Offline, None, None),
        Reach::Failed(error) => (ReachReport::Failed, Some(plain(error)), None),
    };
    let figures = card.figures.as_ref().filter(|_| !card.offline());
    let concerns: Vec<ConcernReport> = figures
        .map(HostFigures::concerns)
        .unwrap_or_default()
        .into_iter()
        .map(|(figure, severity)| ConcernReport { figure, level: severity.into() })
        .collect();
    let target = format!("remote:{}", card.machine);
    HostReport {
        name: plain(card.name()),
        reach,
        error,
        silent_seconds,
        last_answered: card.last_seen.map(utc),
        round_trip_ms: card.info.latency_ms.filter(|_| reach == ReachReport::Answering),
        borg: card.info.version.as_deref().map(plain),
        older_borg: card.older(),
        system: card.info.platform.as_deref().map(plain),
        level: figures.and_then(HostFigures::worst).map(ConcernLevel::from),
        concerns,
        figures: figures.map(figures_report),
        figures_age_seconds: figures.and(card.stale(now)).map(|age| age.as_secs()),
        sessions: card
            .sessions
            .iter()
            .filter_map(|choice| {
                let id = choice.session.as_deref().filter(|id| usable_id(id))?;
                Some(SessionReport { name: plain(&choice.name), target: format!("{target}/{id}") })
            })
            .take(SESSIONS_LIMIT)
            .collect(),
        target,
    }
}

fn figures_report(figures: &HostFigures) -> FiguresReport {
    FiguresReport {
        cores: figures.cores,
        cpu_percent: figures.cpu.map(percent),
        load: figures.load.map(|load| load_report(load, figures)),
        uptime_seconds: figures.uptime,
        memory: figures.memory.map(|memory| MemoryReport {
            used_bytes: memory.used,
            total_bytes: memory.total,
            percent: percent(memory.share()),
            swap_used_bytes: memory.swap_used,
            swap_total_bytes: memory.swap_total,
        }),
        disk: figures.disk.as_ref().map(|disk| DiskReport {
            mount: plain(&disk.mount),
            total_bytes: disk.total,
            available_bytes: disk.available,
            percent_used: percent(disk.share()),
        }),
        watcher: figures.watch.map(|watch| WatcherReport {
            on: watch.on,
            learning_until: watch.learning_until.map(utc),
            unread_alerts: watch.unread_alerts,
            unread_warnings: watch.unread_warnings,
            unread_notices: watch.unread_notices,
            frozen: watch.frozen,
        }),
        containers: figures.containers,
    }
}

fn load_report(load: LoadAverage, figures: &HostFigures) -> LoadReport {
    let hundredths = |value: u32| f64::from(value) / 100.0;
    LoadReport {
        one: hundredths(load.one),
        five: hundredths(load.five),
        fifteen: hundredths(load.fifteen),
        percent_of_cores: figures.load_share().map(percent),
    }
}

/// A host's own text on one line: control characters become spaces, characters that hide text
/// or turn its direction go, and it ends with an ellipsis after `TEXT_LIMIT` characters.
fn plain(text: &str) -> String {
    let mut chars = text.chars().filter(|c| !hiding(*c)).map(|c| if c.is_control() { ' ' } else { c });
    let mut plain: String = chars.by_ref().take(TEXT_LIMIT).collect();
    if chars.next().is_some() {
        plain.push('…');
    }
    plain
}

/// Characters that show nothing or turn the direction of the text after them.
fn hiding(c: char) -> bool {
    matches!(c, '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{2069}' | '\u{feff}')
}

/// A session ID that goes into a target as it is: one word of visible characters.
fn usable_id(id: &str) -> bool {
    !id.is_empty()
        && id.chars().count() <= TEXT_LIMIT
        && id.chars().all(|c| !c.is_whitespace() && !c.is_control() && !hiding(c))
}

/// Hundredths of a percent as a percent.
fn percent(share: u32) -> f64 {
    f64::from(share) * 100.0 / f64::from(WHOLE)
}

/// A time as RFC 3339 in UTC, to the second.
fn utc(at: SystemTime) -> String {
    let seconds = at.duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs());
    jiff::Timestamp::from_second(i64::try_from(seconds).unwrap_or(i64::MAX))
        .map(|stamp| stamp.to_string())
        .unwrap_or_default()
}
