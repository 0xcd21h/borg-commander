//! A paired host as an agent reads it: what its card on the hosts page shows, from the same
//! cards in the same order, in plain units.

use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;

use super::{ContainerFigures, Figure, HostCard, HostFigures, Hosts, Reach, WHOLE};
use crate::{LoadAverage, Severity};

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
    /// Why reaching the host failed.
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
        Reach::Silent { since } => (ReachReport::Silent, None, Some(now.saturating_duration_since(*since).as_secs())),
        Reach::Offline => (ReachReport::Offline, None, None),
        Reach::Failed(error) => (ReachReport::Failed, Some(error.clone()), None),
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
        name: card.name().to_owned(),
        reach,
        error,
        silent_seconds,
        last_answered: card.last_seen.map(utc),
        round_trip_ms: card.info.latency_ms.filter(|_| reach == ReachReport::Answering),
        borg: card.info.version.clone(),
        older_borg: card.older(),
        system: card.info.platform.clone(),
        level: figures.and_then(HostFigures::worst).map(ConcernLevel::from),
        concerns,
        figures: figures.map(figures_report),
        figures_age_seconds: figures.and(card.stale(now)).map(|age| age.as_secs()),
        sessions: card
            .sessions
            .iter()
            .filter_map(|choice| {
                let id = choice.session.as_deref()?;
                Some(SessionReport { name: choice.name.clone(), target: format!("{target}/{id}") })
            })
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
            mount: disk.mount.clone(),
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
