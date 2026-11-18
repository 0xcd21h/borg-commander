//! What a paired host says about itself for its card on the hosts screen, and how much each
//! figure should worry the user.

use std::time::SystemTime;

use borg_docker::{Container, Health, State};
use serde::{Deserialize, Serialize};

use crate::{AlertLog, LoadAverage, Severity, WatchSource, load_reading};

/// A whole, in hundredths of a percent.
pub const WHOLE: u32 = 10_000;
/// The load's share of the cores past which they cannot keep up, and past which far more waits
/// than they run.
const LOAD_WARNING: u32 = WHOLE;
const LOAD_ALERT: u32 = 2 * WHOLE;
/// Shares of the RAM in use.
const MEMORY_WARNING: u32 = 9_000;
const MEMORY_ALERT: u32 = 9_500;
/// Shares of the fullest filesystem in use.
const DISK_WARNING: u32 = 9_000;
const DISK_ALERT: u32 = 9_500;
/// A filesystem with less room left than this is short, however large it is.
const DISK_ALERT_ROOM: u64 = 1024 * 1024 * 1024;
/// Filesystems that live in memory, stack on others or hold no files of their own.
const NOT_DISKS: [&str; 16] = [
    "tmpfs",
    "devtmpfs",
    "ramfs",
    "overlay",
    "squashfs",
    "efivarfs",
    "autofs",
    "proc",
    "sysfs",
    "devfs",
    "nsfs",
    "cgroup",
    "cgroup2",
    "iso9660",
    "nullfs",
    "fuse.lxcfs",
];

/// What a concern on a host's card is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Figure {
    Load,
    Memory,
    Disk,
    Watcher,
    Containers,
}

/// What a host sends about itself, every figure left out when it cannot say.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HostFigures {
    pub cores: usize,
    /// How busy all the cores were since the last call, in hundredths of a percent.
    pub cpu: Option<u32>,
    pub load: Option<LoadAverage>,
    /// Seconds since the host started.
    pub uptime: Option<u64>,
    pub memory: Option<MemoryFigures>,
    /// The fullest filesystem that holds files.
    pub disk: Option<DiskFigures>,
    pub watch: Option<WatchFigures>,
    pub containers: Option<ContainerFigures>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryFigures {
    pub used: u64,
    pub total: u64,
    pub swap_used: u64,
    pub swap_total: u64,
}

impl MemoryFigures {
    pub fn share(&self) -> u32 {
        share(self.used, self.total)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiskFigures {
    pub mount: String,
    pub total: u64,
    /// What the user may still write there.
    pub available: u64,
}

impl DiskFigures {
    pub fn share(&self) -> u32 {
        share(self.total.saturating_sub(self.available), self.total)
    }
}

/// The process watcher on the host: whether it runs, until when it learns, and what nobody has
/// read yet.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WatchFigures {
    pub on: bool,
    pub learning_until: Option<SystemTime>,
    pub unread_alerts: usize,
    pub unread_warnings: usize,
    pub unread_notices: usize,
    /// Processes Borg froze and nobody let go on or ended.
    pub frozen: usize,
}

impl WatchFigures {
    /// The watcher as its log says, while it is turned on.
    pub fn of(log: &AlertLog, turned_on: bool, now: SystemTime) -> Self {
        let unread =
            |level: borg_rules::Level| log.alerts.iter().filter(|alert| !alert.seen && alert.level() == level).count();
        let on = turned_on && log.watch.source != WatchSource::Off;
        Self {
            on,
            learning_until: (on && log.watch.learning(now)).then_some(log.watch.learned_at),
            unread_alerts: unread(borg_rules::Level::Alert),
            unread_warnings: unread(borg_rules::Level::Warning),
            unread_notices: unread(borg_rules::Level::Notice),
            frozen: log.alerts.iter().filter(|alert| alert.frozen()).count(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContainerFigures {
    pub running: usize,
    pub total: usize,
    /// Containers whose health check fails, or that keep restarting.
    pub troubled: usize,
}

impl ContainerFigures {
    pub fn of(containers: &[Container]) -> Self {
        Self {
            running: containers.iter().filter(|container| container.state == State::Running).count(),
            total: containers.len(),
            troubled: containers
                .iter()
                .filter(|container| {
                    container.state == State::Restarting || container.health() == Some(Health::Unhealthy)
                })
                .count(),
        }
    }
}

/// `part` of `whole`, in hundredths of a percent.
pub fn share(part: u64, whole: u64) -> u32 {
    if whole == 0 {
        return 0;
    }
    let share = u128::from(part.min(whole)) * u128::from(WHOLE) / u128::from(whole);
    u32::try_from(share).unwrap_or(WHOLE)
}

/// Whether a mounted filesystem holds files worth counting: written to, with room, and not one
/// that lives in memory or stacks on another.
pub fn holds_files(file_system: &str, read_only: bool, total: u64) -> bool {
    !read_only && total > 0 && !NOT_DISKS.contains(&file_system) && !file_system.starts_with("fuse.")
}

/// The filesystem with the largest share in use; the larger one when two are as full.
pub fn fullest(disks: impl IntoIterator<Item = DiskFigures>) -> Option<DiskFigures> {
    disks.into_iter().filter(|disk| disk.total > 0).max_by_key(|disk| (disk.share(), disk.total))
}

/// The worse of two concerns.
pub fn worse(a: Option<Severity>, b: Option<Severity>) -> Option<Severity> {
    let rank = |severity: Option<Severity>| match severity {
        Some(Severity::Alert) => 3,
        Some(Severity::Warning) => 2,
        Some(Severity::Notice) => 1,
        None => 0,
    };
    if rank(b) > rank(a) { b } else { a }
}

fn above(value: u32, warning: u32, alert: u32) -> Option<Severity> {
    if value > alert {
        Some(Severity::Alert)
    } else if value > warning {
        Some(Severity::Warning)
    } else {
        None
    }
}

fn at_least(value: u32, warning: u32, alert: u32) -> Option<Severity> {
    if value >= alert {
        Some(Severity::Alert)
    } else if value >= warning {
        Some(Severity::Warning)
    } else {
        None
    }
}

impl HostFigures {
    /// The last minute's load as a share of the cores, in hundredths of a percent.
    pub fn load_share(&self) -> Option<u32> {
        load_reading(self.load?, self.cores).map(|reading| reading.share)
    }

    /// About how many processes waited for a core, while the cores could not keep up.
    pub fn waiting(&self) -> Option<u32> {
        load_reading(self.load?, self.cores).and_then(|reading| reading.waiting)
    }

    pub fn load_concern(&self) -> Option<Severity> {
        above(self.load_share()?, LOAD_WARNING, LOAD_ALERT)
    }

    pub fn memory_concern(&self) -> Option<Severity> {
        at_least(self.memory?.share(), MEMORY_WARNING, MEMORY_ALERT)
    }

    pub fn disk_concern(&self) -> Option<Severity> {
        let disk = self.disk.as_ref()?;
        if disk.available < DISK_ALERT_ROOM {
            return Some(Severity::Alert);
        }
        at_least(disk.share(), DISK_WARNING, DISK_ALERT)
    }

    pub fn watch_concern(&self) -> Option<Severity> {
        let watch = self.watch?;
        if watch.frozen > 0 || watch.unread_alerts > 0 {
            Some(Severity::Alert)
        } else if watch.unread_warnings > 0 {
            Some(Severity::Warning)
        } else {
            None
        }
    }

    pub fn containers_concern(&self) -> Option<Severity> {
        (self.containers?.troubled > 0).then_some(Severity::Warning)
    }

    /// Each figure that should worry the user, and how much, in the order a card shows them.
    pub fn concerns(&self) -> Vec<(Figure, Severity)> {
        [
            (Figure::Load, self.load_concern()),
            (Figure::Memory, self.memory_concern()),
            (Figure::Disk, self.disk_concern()),
            (Figure::Watcher, self.watch_concern()),
            (Figure::Containers, self.containers_concern()),
        ]
        .into_iter()
        .filter_map(|(figure, severity)| Some((figure, severity?)))
        .collect()
    }

    /// The worst concern among the figures.
    pub fn worst(&self) -> Option<Severity> {
        self.concerns().into_iter().map(|(_, severity)| Some(severity)).fold(None, worse)
    }
}

/// Whether Borg `version` is older than `ours`, by its numbers.
pub fn older_than(version: &str, ours: &str) -> bool {
    let numbers = |text: &str| -> Vec<u64> {
        let release = text.trim().trim_start_matches('v');
        let release = release.split(['-', '+']).next().unwrap_or_default();
        release.split('.').map(|part| part.parse().unwrap_or_default()).collect()
    };
    numbers(version) < numbers(ours)
}
