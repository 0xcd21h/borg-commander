use anyhow::{Result, anyhow, bail};
use borg_core::{
    ActivityData, ActivityRequest, MemoryProtection, Msg, PROCESS_LIMIT, ProcessInfo, ProcessKey, SNAPSHOT_LIMIT,
    SystemSample,
};
use borg_i18n::tr;
use std::{
    collections::{HashMap, HashSet},
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use sysinfo::{Pid, ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, System, ThreadKind, UpdateKind};
use tokio::{sync::mpsc::UnboundedSender, task::JoinHandle};

mod control;
mod helper;
mod maps;
mod process_files;
mod reading;

pub use helper::helper;
use maps::{quick_maps, regions};
pub use reading::Memory;
pub(crate) use reading::search_for_agent;
use reading::{capture, census, inspect, integrity, search_memory};

const OUTPUT_LIMIT: usize = 8 * 1024 * 1024;
const ERROR_LIMIT: usize = 8192;
const REGION_LIMIT: usize = 32768;
#[cfg(any(target_os = "macos", test))]
const TIMEOUT: Duration = Duration::from_secs(30);
const ADMIN_TIMEOUT: Duration = Duration::from_secs(120);
const KIB: f64 = 1024.0;
const CANCEL_POLL: Duration = Duration::from_millis(100);
/// How long a search for strings reads the strings of the process's files before it starts.
const FILE_STRINGS_TIME: Duration = Duration::from_secs(20);
/// How far a capture reads past zeros at the start of a region to find data.
const SEEK_LIMIT: u64 = 16 * 1024 * 1024;
const PAGE: u64 = 4096;
/// How long one sample spends reading programs' signatures; the rest wait for the next sample,
/// so a slow disk never holds the monitor up.
const SIGNATURE_TIME: Duration = Duration::from_millis(100);

/// A process sample with CPU use: CPU needs two looks a moment apart, as the activity
/// monitor's first sample does.
pub(crate) fn processes() -> SystemSample {
    const SETTLE: Duration = Duration::from_millis(250);
    let mut sampler = Sampler::default();
    sampler.sample(None);
    std::thread::sleep(SETTLE);
    sampler.sample(None)
}

#[derive(Default)]
struct Sampler {
    system: System,
    last: Option<Instant>,
    seen: HashSet<ProcessKey>,
    /// What each running program's signature says about reading its memory, read once.
    protections: HashMap<PathBuf, Option<MemoryProtection>>,
}
impl Sampler {
    /// Samples every process. The `selected` one's signature is read whatever the time left,
    /// since its details show what it says.
    fn sample(&mut self, selected: Option<ProcessKey>) -> SystemSample {
        self.system.refresh_memory();
        self.system.refresh_cpu_usage();
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_cpu().with_memory().with_disk_usage().with_exe(UpdateKind::OnlyIfNotSet),
        );
        let now = Instant::now();
        let elapsed = self.last.map(|last| now.duration_since(last).as_secs_f64()).filter(|seconds| *seconds > 0.0);
        let own = std::process::id();
        let signatures = now + SIGNATURE_TIME;
        let mut processes: Vec<_> = self
            .system
            .processes()
            .values()
            // On Linux each thread of a process is listed too, with the whole process's memory.
            .filter(|p| p.thread_kind() != Some(ThreadKind::Userland))
            .map(|p| {
                let key = ProcessKey { pid: p.pid().as_u32(), start: p.start_time() };
                let sampled = elapsed.filter(|_| self.seen.contains(&key));
                let io = p.disk_usage();
                let available = p.memory() > 0 || p.virtual_memory() > 0;
                // Borg reads its own memory whatever its signature says.
                let protection = p.exe().filter(|_| key.pid != own).and_then(|exe| match self.protections.get(exe) {
                    Some(known) => *known,
                    None if Some(key) == selected || Instant::now() < signatures => {
                        *self.protections.entry(exe.to_path_buf()).or_insert(crate::code_signature::protection(exe))
                    }
                    None => None,
                });
                ProcessInfo {
                    key,
                    parent: p.parent().map(Pid::as_u32),
                    name: p.name().to_string_lossy().into_owned(),
                    executable: p.exe().map(PathBuf::from),
                    cpu: sampled.map(|_| (p.cpu_usage().max(0.0) * 100.0) as u32),
                    resident: available.then(|| p.memory()),
                    virtual_bytes: available.then(|| p.virtual_memory()),
                    read: sampled.map(|s| (io.read_bytes as f64 / s) as u64),
                    written: sampled.map(|s| (io.written_bytes as f64 / s) as u64),
                    protection,
                    nice: i32::try_from(key.pid)
                        .ok()
                        .and_then(rustix::process::Pid::from_raw)
                        .and_then(|pid| rustix::process::getpriority_process(Some(pid)).ok()),
                    stopped: p.status() == ProcessStatus::Stop,
                }
            })
            .collect();
        processes.sort_by_key(|p| p.key.pid);
        processes.truncate(PROCESS_LIMIT);
        self.seen = processes.iter().map(|p| p.key).collect();
        let running: HashSet<&Path> = processes.iter().filter_map(|p| p.executable.as_deref()).collect();
        self.protections.retain(|exe, _| running.contains(exe.as_path()));
        self.last = Some(now);
        SystemSample {
            processes,
            total: self.system.total_memory(),
            used: self.system.used_memory(),
            swap_total: self.system.total_swap(),
            swap_used: self.system.used_swap(),
            cpu: self.system.cpus().iter().map(|c| (c.cpu_usage().max(0.0) * 100.0) as u32).collect(),
            administrator: rustix::process::geteuid().is_root(),
            own: Some(own),
        }
    }
}
/// What a failed request was doing, which its error says.
enum RequestKind {
    Inspection,
    Control,
}

pub struct Monitor {
    sampler: Arc<Mutex<Sampler>>,
    cancelled: Arc<AtomicBool>,
    task: Option<JoinHandle<()>>,
    messages: UnboundedSender<Msg>,
}
impl Monitor {
    pub fn new(messages: UnboundedSender<Msg>) -> Self {
        Self { sampler: Default::default(), cancelled: Default::default(), task: None, messages }
    }
    pub fn cancel_search(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
    pub fn cancel(&mut self) {
        self.cancel_search();
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
    /// Starts `request`; a sample reads the signature of the `selected` process first.
    pub fn request(&mut self, id: u64, request: ActivityRequest, selected: Option<ProcessKey>) {
        self.cancel();
        self.cancelled = Arc::new(AtomicBool::new(false));
        let cancelled = self.cancelled.clone();
        let sampler = self.sampler.clone();
        let messages = self.messages.clone();
        let kind = if matches!(request, ActivityRequest::Control { .. }) {
            RequestKind::Control
        } else {
            RequestKind::Inspection
        };
        self.task = Some(tokio::spawn(async move {
            let result = match request {
                ActivityRequest::Sample => tokio::task::spawn_blocking(move || {
                    sampler
                        .lock()
                        .map(|mut s| ActivityData::Sample(s.sample(selected)))
                        .map_err(|e| anyhow!(e.to_string()))
                })
                .await
                .map_err(anyhow::Error::from)
                .and_then(|r| r),
                request => inspect(request, cancelled, Some((id, messages.clone()))).await,
            }
            .map_err(|error| match kind {
                RequestKind::Control => tr!(process_control_failed, error = error),
                RequestKind::Inspection => tr!(activity_cannot_inspect, error = error),
            });
            let _ = messages.send(Msg::Activity { id, result });
        }));
    }
}
impl Drop for Monitor {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// The running process with `pid` as Borg tells processes apart, with its name and program.
pub(crate) fn identify(pid: u32) -> Option<(ProcessKey, String, Option<PathBuf>)> {
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[Pid::from_u32(pid)]),
        true,
        ProcessRefreshKind::nothing().with_exe(UpdateKind::OnlyIfNotSet),
    );
    let process = system.process(Pid::from_u32(pid))?;
    let key = ProcessKey { pid, start: process.start_time() };
    Some((key, process.name().to_string_lossy().into_owned(), process.exe().map(PathBuf::from)))
}
/// Whether `process` still runs under its PID, started when it was first seen.
fn running(process: ProcessKey) -> bool {
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[Pid::from_u32(process.pid)]),
        true,
        ProcessRefreshKind::nothing(),
    );
    system.process(Pid::from_u32(process.pid)).is_some_and(|p| p.start_time() == process.start)
}
fn verify(process: ProcessKey) -> Result<()> {
    if !running(process) {
        bail!(tr!(activity_identity_changed));
    }
    Ok(())
}
pub fn export(path: &std::path::Path, bytes: &[u8]) -> Result<()> {
    if bytes.len() > SNAPSHOT_LIMIT {
        bail!(tr!(activity_output_limit));
    }
    let mut file = OpenOptions::new().write(true).create_new(true).mode(0o600).open(path)?;
    if let Err(error) = file.write_all(bytes).and_then(|_| file.sync_all()) {
        let _ = fs::remove_file(path);
        return Err(error.into());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
