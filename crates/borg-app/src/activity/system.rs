//! What the activity monitor reads of the whole system beside its processes: what the disks
//! read and write, the load average, the uptime and, on Linux, the threads.

use borg_core::{DiskActivity, LoadAverage, Threads};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};
use sysinfo::System;

/// A busy disk's share of the time, in hundredths of a percent.
const FULL_TIME: f64 = 10_000.0;
const HUNDREDTHS: f64 = 100.0;

/// A disk's counters since the system started: the bytes it read and wrote and, where the
/// system keeps it, the time it spent with requests under way.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct DiskTotals {
    pub read: u64,
    pub written: u64,
    pub busy: Option<Duration>,
}

/// Each disk's counters, by name.
pub(super) type DiskReading = BTreeMap<String, DiskTotals>;

/// What the disks did between two readings `elapsed` apart, counting the disks in both: the
/// bytes they read and wrote per second, and the busiest disk's share of the time.
pub(super) fn disk_activity(previous: &DiskReading, current: &DiskReading, elapsed: Duration) -> Option<DiskActivity> {
    let seconds = elapsed.as_secs_f64();
    if seconds <= 0.0 {
        return None;
    }
    let changes: Vec<_> =
        current.iter().filter_map(|(name, now)| previous.get(name).map(|before| (before, now))).collect();
    let per_second = |bytes: u64| (bytes as f64 / seconds) as u64;
    let busy = changes
        .iter()
        .filter_map(|(before, now)| Some(now.busy?.saturating_sub(before.busy?)))
        .max()
        .map(|busy| (busy.as_secs_f64() / seconds * FULL_TIME).round().min(FULL_TIME) as u32);
    Some(DiskActivity {
        read: per_second(changes.iter().map(|(before, now)| now.read.saturating_sub(before.read)).sum()),
        written: per_second(changes.iter().map(|(before, now)| now.written.saturating_sub(before.written)).sum()),
        busy,
    })
}

/// The load average, in hundredths.
pub(super) fn load() -> LoadAverage {
    let load = System::load_average();
    let hundredths = |value: f64| (value.max(0.0) * HUNDREDTHS).round() as u32;
    LoadAverage { one: hundredths(load.one), five: hundredths(load.five), fifteen: hundredths(load.fifteen) }
}

pub(super) fn uptime() -> Duration {
    Duration::from_secs(System::uptime())
}

/// Reads the disks' counters, and remembers the last reading to tell what they did since.
#[derive(Default)]
pub(super) struct DiskSampler {
    last: Option<(Instant, DiskReading)>,
    #[cfg(target_os = "macos")]
    volumes: macos::Volumes,
}
impl DiskSampler {
    /// What the disks did since the last call, from the second call on.
    pub(super) fn sample(&mut self) -> Option<DiskActivity> {
        let reading = self.read();
        let now = Instant::now();
        let activity = self.last.as_ref().and_then(|(at, last)| disk_activity(last, &reading, now.duration_since(*at)));
        self.last = Some((now, reading));
        activity
    }

    #[cfg(target_os = "linux")]
    fn read(&mut self) -> DiskReading {
        linux::disks(std::path::Path::new(ROOT))
    }

    #[cfg(target_os = "macos")]
    fn read(&mut self) -> DiskReading {
        self.volumes.read()
    }
}

/// The system's threads, where it counts them.
/// Where Linux shows `/proc` and `/sys`.
#[cfg(target_os = "linux")]
const ROOT: &str = "/";

#[cfg(target_os = "linux")]
pub(super) fn threads() -> Option<Threads> {
    linux::threads(std::path::Path::new(ROOT))
}

#[cfg(target_os = "macos")]
pub(super) fn threads() -> Option<Threads> {
    None
}

/// Linux's own counters under `/proc` and `/sys`, read from a root folder so tests can build one.
#[cfg(any(target_os = "linux", test))]
mod linux {
    use super::{DiskReading, DiskTotals};
    use borg_core::Threads;
    use std::{collections::HashSet, fs, path::Path, time::Duration};

    const DISKSTATS: &str = "proc/diskstats";
    const LOADAVG: &str = "proc/loadavg";
    const BLOCK_DEVICES: &str = "sys/block";
    /// What a block device has when it stands for hardware, which loop, RAID, device-mapper and
    /// RAM disks, built on other disks or on memory, lack.
    const HARDWARE: &str = "device";
    /// `/proc/diskstats` counts sectors of 512 bytes whatever the disk's own sector size.
    const SECTOR: u64 = 512;
    const NAME_FIELD: usize = 2;
    const SECTORS_READ_FIELD: usize = 5;
    const SECTORS_WRITTEN_FIELD: usize = 9;
    const BUSY_MS_FIELD: usize = 12;
    /// The field of `/proc/loadavg` with the running and all scheduling entities, as `3/517`.
    const THREADS_FIELD: usize = 3;

    /// The whole disks backed by hardware, by name.
    pub(super) fn hardware_disks(root: &Path) -> HashSet<String> {
        let Ok(entries) = fs::read_dir(root.join(BLOCK_DEVICES)) else { return HashSet::new() };
        entries
            .flatten()
            .filter(|entry| entry.path().join(HARDWARE).exists())
            .filter_map(|entry| entry.file_name().into_string().ok())
            .collect()
    }

    /// The counters of the disks in `disks`, from the text of `/proc/diskstats`.
    pub(super) fn parse_diskstats(text: &str, disks: &HashSet<String>) -> DiskReading {
        text.lines()
            .filter_map(|line| {
                let fields: Vec<&str> = line.split_whitespace().collect();
                let name = *fields.get(NAME_FIELD)?;
                let number = |index: usize| fields.get(index)?.parse::<u64>().ok();
                disks.contains(name).then_some(())?;
                Some((
                    name.to_owned(),
                    DiskTotals {
                        read: number(SECTORS_READ_FIELD)?.saturating_mul(SECTOR),
                        written: number(SECTORS_WRITTEN_FIELD)?.saturating_mul(SECTOR),
                        busy: number(BUSY_MS_FIELD).map(Duration::from_millis),
                    },
                ))
            })
            .collect()
    }

    pub(super) fn disks(root: &Path) -> DiskReading {
        fs::read_to_string(root.join(DISKSTATS))
            .map(|text| parse_diskstats(&text, &hardware_disks(root)))
            .unwrap_or_default()
    }

    /// The running and all threads, from the text of `/proc/loadavg`.
    pub(super) fn parse_threads(text: &str) -> Option<Threads> {
        let (running, total) = text.split_whitespace().nth(THREADS_FIELD)?.split_once('/')?;
        Some(Threads { total: total.parse().ok()?, running: running.parse().ok()? })
    }

    pub(super) fn threads(root: &Path) -> Option<Threads> {
        parse_threads(&fs::read_to_string(root.join(LOADAVG)).ok()?)
    }
}

/// The disks behind macOS's mounted volumes, as sysinfo reads them.
#[cfg(any(target_os = "macos", test))]
mod macos {
    use super::{DiskReading, DiskTotals};
    #[cfg(target_os = "macos")]
    use std::time::{Duration, Instant};

    /// Where macOS mounts the volumes that share the startup disk with `/`.
    const SYSTEM_VOLUMES: &str = "/System/Volumes/";

    /// Each drive once, by the first of its volumes: every volume reports the counters of the
    /// drive it is on, so volumes with the same counters share a drive.
    pub(super) fn drives(mut volumes: Vec<(String, DiskTotals)>) -> DiskReading {
        volumes.retain(|(mount, _)| !mount.starts_with(SYSTEM_VOLUMES));
        volumes.sort_by(|a, b| a.0.cmp(&b.0));
        let mut reading = DiskReading::new();
        for (mount, totals) in volumes {
            if !reading.values().any(|seen| *seen == totals) {
                reading.insert(mount, totals);
            }
        }
        reading
    }

    /// How often the list of mounted volumes is read again, for drives plugged in since.
    #[cfg(target_os = "macos")]
    const LIST_EVERY: Duration = Duration::from_secs(10);

    #[cfg(target_os = "macos")]
    #[derive(Default)]
    pub(super) struct Volumes {
        disks: Option<sysinfo::Disks>,
        listed: Option<Instant>,
    }

    #[cfg(target_os = "macos")]
    impl Volumes {
        pub(super) fn read(&mut self) -> DiskReading {
            let kind = sysinfo::DiskRefreshKind::nothing().with_io_usage();
            let stale = self.listed.is_none_or(|listed| listed.elapsed() >= LIST_EVERY);
            let disks = self.disks.get_or_insert_with(sysinfo::Disks::new);
            if stale {
                disks.refresh_specifics(true, kind);
                self.listed = Some(Instant::now());
            } else {
                for disk in disks.list_mut() {
                    disk.refresh_specifics(kind);
                }
            }
            drives(
                disks
                    .list()
                    .iter()
                    .map(|disk| {
                        let usage = disk.usage();
                        let totals =
                            DiskTotals { read: usage.total_read_bytes, written: usage.total_written_bytes, busy: None };
                        (disk.mount_point().to_string_lossy().into_owned(), totals)
                    })
                    .collect(),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::HashSet, fs};

    const DISKSTATS: &str = "\
 259       0 nvme0n1 120 4 2048 30 80 2 4096 50 0 900 80 0 0 0 0
 259       1 nvme0n1p1 100 4 1024 20 60 2 2048 40 0 700 60 0 0 0 0
   8       0 sda 10 0 512 5 4 0 256 3 0 2000 8 0 0 0 0
   7       0 loop0 50 0 800 9 0 0 0 0 0 40 9 0 0 0 0
 253       0 dm-0 90 0 1500 22 70 0 3000 45 0 850 67 0 0 0 0
";

    fn totals(read: u64, written: u64, busy: Option<u64>) -> DiskTotals {
        DiskTotals { read, written, busy: busy.map(Duration::from_millis) }
    }

    #[test]
    fn only_whole_hardware_disks_are_read_from_diskstats() {
        let disks = HashSet::from(["nvme0n1".to_owned(), "sda".to_owned()]);
        let reading = linux::parse_diskstats(DISKSTATS, &disks);
        assert_eq!(
            reading,
            DiskReading::from([
                ("nvme0n1".to_owned(), totals(2048 * 512, 4096 * 512, Some(900))),
                ("sda".to_owned(), totals(512 * 512, 256 * 512, Some(2000))),
            ])
        );
    }

    #[test]
    fn a_short_or_garbled_diskstats_line_is_skipped() {
        let disks = HashSet::from(["sdb".to_owned(), "sdc".to_owned()]);
        let reading = linux::parse_diskstats("8 16 sdb 1 2\n8 32 sdc 1 0 x 0 0 0 0 0 0 0 0\n", &disks);
        assert!(reading.is_empty());
    }

    #[test]
    fn hardware_disks_are_the_block_devices_with_a_device() {
        let root = tempfile::tempdir().unwrap();
        for (name, hardware) in [("sda", true), ("nvme0n1", true), ("loop0", false), ("dm-0", false)] {
            let folder = root.path().join("sys/block").join(name);
            fs::create_dir_all(&folder).unwrap();
            if hardware {
                fs::create_dir(folder.join("device")).unwrap();
            }
        }
        fs::create_dir_all(root.path().join("proc")).unwrap();
        fs::write(root.path().join("proc/diskstats"), DISKSTATS).unwrap();
        assert_eq!(linux::hardware_disks(root.path()), HashSet::from(["sda".to_owned(), "nvme0n1".to_owned()]));
        assert_eq!(linux::disks(root.path()).keys().collect::<Vec<_>>(), ["nvme0n1", "sda"]);
    }

    #[test]
    fn threads_come_from_loadavg() {
        assert_eq!(linux::parse_threads("1.52 0.98 0.40 3/517 40211\n"), Some(Threads { total: 517, running: 3 }));
        assert_eq!(linux::parse_threads("1.52 0.98 0.40\n"), None);
        assert_eq!(linux::parse_threads("1.52 0.98 0.40 x/517 40211\n"), None);
        let root = tempfile::tempdir().unwrap();
        assert_eq!(linux::threads(root.path()), None);
        fs::create_dir_all(root.path().join("proc")).unwrap();
        fs::write(root.path().join("proc/loadavg"), "0.20 0.18 0.12 1/80 11206\n").unwrap();
        assert_eq!(linux::threads(root.path()), Some(Threads { total: 80, running: 1 }));
    }

    #[test]
    fn disk_activity_is_per_second_with_the_busiest_disks_share() {
        let before = DiskReading::from([
            ("nvme0n1".to_owned(), totals(1_000, 2_000, Some(100))),
            ("sda".to_owned(), totals(0, 0, Some(0))),
        ]);
        let after = DiskReading::from([
            ("nvme0n1".to_owned(), totals(5_000, 4_000, Some(400))),
            ("sda".to_owned(), totals(2_000, 0, Some(1_900))),
            ("sdb".to_owned(), totals(9_000, 9_000, Some(2_000))),
        ]);
        let activity = disk_activity(&before, &after, Duration::from_secs(2)).unwrap();
        assert_eq!(activity, DiskActivity { read: 3_000, written: 1_000, busy: Some(9_500) });
    }

    #[test]
    fn busy_time_is_capped_at_the_whole_time_and_absent_where_unknown() {
        let before = DiskReading::from([("disk".to_owned(), totals(0, 0, Some(0)))]);
        let after = DiskReading::from([("disk".to_owned(), totals(0, 0, Some(1_500)))]);
        assert_eq!(disk_activity(&before, &after, Duration::from_secs(1)).unwrap().busy, Some(10_000));
        let unknown = DiskReading::from([("disk".to_owned(), totals(10, 20, None))]);
        let later = DiskReading::from([("disk".to_owned(), totals(30, 60, None))]);
        let activity = disk_activity(&unknown, &later, Duration::from_secs(1)).unwrap();
        assert_eq!(activity, DiskActivity { read: 20, written: 40, busy: None });
        assert_eq!(disk_activity(&unknown, &later, Duration::ZERO), None);
    }

    #[test]
    fn volumes_on_one_drive_count_once_and_system_volumes_go_with_the_startup_disk() {
        let startup = totals(7_000, 3_000, None);
        let external = totals(400, 100, None);
        let reading = macos::drives(vec![
            ("/System/Volumes/Data".to_owned(), startup),
            ("/Volumes/Photos".to_owned(), external),
            ("/".to_owned(), startup),
            ("/Volumes/Backup".to_owned(), external),
        ]);
        assert_eq!(reading, DiskReading::from([("/".to_owned(), startup), ("/Volumes/Backup".to_owned(), external)]));
    }
}
