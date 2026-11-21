//! The figures this Borg sends a paired computer's hosts screen: how busy the cores are, the
//! load, the memory, the fullest filesystem, the process watcher and the containers. Only the
//! machine's totals are read, never each process, and only while a computer keeps asking.

use std::{
    sync::Arc,
    time::{Duration, Instant, SystemTime},
};

use borg_core::{AlertLog, ContainerFigures, DiskFigures, HostFigures, MemoryFigures, WatchFigures};
use sysinfo::{DiskRefreshKind, Disks, MINIMUM_CPU_UPDATE_INTERVAL, System};
use tokio::sync::Mutex;

/// A computer that asks again within this keeps the CPU reading; after it, the next call reads
/// the cores afresh.
const KEPT_FOR: Duration = Duration::from_secs(30);
/// How long a list of the containers stands before Borg asks Docker again.
const CONTAINERS_FOR: Duration = Duration::from_secs(30);
/// How long the watcher's figures stand while its log stays the same.
const WATCH_FOR: Duration = Duration::from_secs(60);
/// How long the filesystems may take to say how full they are.
const DISKS_WAIT: Duration = Duration::from_secs(2);
const HUNDREDTHS: f32 = 100.0;
/// Network filesystems, which can hang whoever asks how full they are.
const NETWORK_FILESYSTEMS: [&str; 9] = ["nfs", "nfs4", "cifs", "smbfs", "smb3", "9p", "ceph", "glusterfs", "afpfs"];
/// macOS mounts the volumes of its system container here; only the data volume holds files.
#[cfg(target_os = "macos")]
const SYSTEM_VOLUMES: &str = "/System/Volumes/";
#[cfg(target_os = "macos")]
const DATA_VOLUME: &str = "/System/Volumes/Data";

/// What this Borg read for the last computer that asked, kept between calls.
#[derive(Clone, Default)]
pub struct Overview(Arc<Mutex<Readings>>);

#[derive(Default)]
struct Readings {
    system: Option<(System, Instant)>,
    containers: Option<(Option<ContainerFigures>, Instant)>,
    watch: Option<WatchReading>,
}

struct WatchReading {
    changed: Option<SystemTime>,
    log: AlertLog,
    at: Instant,
}

impl Overview {
    pub async fn figures(&self) -> HostFigures {
        let mut readings = self.0.lock().await;
        let (cores, cpu, memory) = readings.system().await;
        let containers = readings.containers().await;
        let watch = readings.watch().await;
        let load = crate::activity::system::load();
        HostFigures {
            cores,
            cpu,
            load: (cores > 0).then_some(load),
            uptime: Some(System::uptime()),
            memory: Some(memory),
            disk: fullest_disk().await,
            watch: Some(watch),
            containers,
        }
    }
}

impl Readings {
    /// The cores, how busy they were since the last call, and the memory. A first call reads
    /// the cores twice, a moment apart.
    async fn system(&mut self) -> (usize, Option<u32>, MemoryFigures) {
        let kept = self.system.as_ref().is_some_and(|(_, at)| at.elapsed() < KEPT_FOR);
        if !kept {
            let mut system = System::new();
            system.refresh_cpu_usage();
            tokio::time::sleep(MINIMUM_CPU_UPDATE_INTERVAL).await;
            self.system = Some((system, Instant::now()));
        }
        let Some((system, at)) = self.system.as_mut() else { return Default::default() };
        system.refresh_cpu_usage();
        system.refresh_memory();
        *at = Instant::now();
        let cores = system.cpus().len();
        let cpu = (cores > 0).then(|| (system.global_cpu_usage() * HUNDREDTHS).round() as u32);
        let memory = MemoryFigures {
            used: system.used_memory(),
            total: system.total_memory(),
            swap_used: system.used_swap(),
            swap_total: system.total_swap(),
        };
        (cores, cpu, memory)
    }

    /// The containers, when Docker answers, as Docker last told.
    async fn containers(&mut self) -> Option<ContainerFigures> {
        if let Some((figures, at)) = &self.containers
            && at.elapsed() < CONTAINERS_FOR
        {
            return *figures;
        }
        let figures = borg_docker::list().await.ok().map(|containers| ContainerFigures::of(&containers));
        self.containers = Some((figures, Instant::now()));
        figures
    }

    /// The watcher as its log says, read again only when the log changed.
    async fn watch(&mut self) -> WatchFigures {
        let seen =
            self.watch.as_ref().filter(|reading| reading.at.elapsed() < WATCH_FOR).map(|reading| reading.changed);
        let read = tokio::task::spawn_blocking(move || crate::watch::alert_log_since(seen.flatten())).await;
        if let Ok(Some((changed, log))) = read {
            self.watch = Some(WatchReading { changed: Some(changed), log, at: Instant::now() });
        } else if let Some(reading) = self.watch.as_mut() {
            reading.at = Instant::now();
        }
        let log = self.watch.as_ref().map(|reading| &reading.log);
        WatchFigures::of(log.unwrap_or(&AlertLog::default()), watcher_on(), SystemTime::now())
    }
}

/// Whether the configuration turns the process watcher on.
fn watcher_on() -> bool {
    crate::config::ConfigDir::locate()
        .and_then(|dir| dir.load().ok())
        .is_some_and(|config| config.preferences_for_terminal(|| false).process_watch)
}

/// The fullest filesystem that holds files, leaving out network filesystems, which can hang.
async fn fullest_disk() -> Option<DiskFigures> {
    let read = tokio::task::spawn_blocking(|| {
        let mut disks = Disks::new_with_refreshed_list_specifics(DiskRefreshKind::nothing());
        let mut found = Vec::new();
        for disk in disks.list_mut() {
            let file_system = disk.file_system().to_string_lossy().into_owned();
            let mount = disk.mount_point().to_string_lossy().into_owned();
            if NETWORK_FILESYSTEMS.contains(&file_system.as_str()) || !counted_mount(&mount) {
                continue;
            }
            disk.refresh_specifics(DiskRefreshKind::nothing().with_storage());
            if borg_core::holds_files(&file_system, disk.is_read_only(), disk.total_space()) {
                found.push(DiskFigures { mount, total: disk.total_space(), available: disk.available_space() });
            }
        }
        borg_core::fullest(found)
    });
    tokio::time::timeout(DISKS_WAIT, read).await.ok()?.ok()?
}

#[cfg(target_os = "macos")]
fn counted_mount(mount: &str) -> bool {
    !mount.starts_with(SYSTEM_VOLUMES) || mount == DATA_VOLUME
}

#[cfg(not(target_os = "macos"))]
fn counted_mount(_mount: &str) -> bool {
    true
}
