//! The entropy scan of every process: several processes at once as this user, or the ones only
//! an administrator may read in Borg's helper. Processes macOS protects, or that refuse Borg,
//! are left out before their regions are listed, since vmmap takes seconds a process.

use super::{Memory, helper::run_helper, reading::wait_cancelled, regions};
use crate::entropy_scan::{Found, SharedFile, scan_process};
use anyhow::{Result, bail};
use borg_core::{ActivityData, EntropyReport, EntropyScope, Msg, ProcessKey};
use borg_i18n::tr;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
use tokio::{sync::mpsc::UnboundedSender, task::JoinHandle, task::JoinSet};

/// Processes read at once.
const AT_ONCE: usize = 4;
/// How long a run reads before it leaves the rest for N. The helper stops sooner, so it ends
/// within its own time limit even after a slow password prompt.
const TIME_LIMIT: Duration = Duration::from_secs(90);
pub(super) const HELPER_TIME_LIMIT: Duration = Duration::from_secs(60);
const PROGRESS: Duration = Duration::from_millis(250);
/// The longest list of processes the helper reads from its arguments.
pub(super) const JOB_LIMIT: usize = 512 * 1024;

/// What the helper scans as administrator.
#[derive(Serialize, Deserialize)]
pub(super) struct Job {
    pub scope: EntropyScope,
    pub processes: Vec<ProcessKey>,
}

/// What became of one process.
enum Outcome {
    Read(Found),
    Unreadable,
    Protected,
    /// It ended, or holds no memory of its own, as a kernel thread.
    Skipped,
    Stopped,
}

/// A running process as the scan sees it.
struct Known {
    start: u64,
    name: String,
    executable: Option<PathBuf>,
    /// A Linux kernel thread, which has no memory map.
    kernel: bool,
}

fn known() -> HashMap<u32, Known> {
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_memory().with_exe(UpdateKind::OnlyIfNotSet),
    );
    system
        .processes()
        .values()
        .map(|process| {
            let kernel = cfg!(target_os = "linux") && process.memory() == 0 && process.virtual_memory() == 0;
            let known = Known {
                start: process.start_time(),
                name: process.name().to_string_lossy().into_owned(),
                executable: process.exe().map(PathBuf::from),
                kernel,
            };
            (process.pid().as_u32(), known)
        })
        .collect()
}

/// Runs the entropy scan; each run stops the one before it.
pub struct EntropyScanner {
    stop: Arc<AtomicBool>,
    task: Option<JoinHandle<()>>,
    messages: UnboundedSender<Msg>,
}

impl EntropyScanner {
    pub fn new(messages: UnboundedSender<Msg>) -> Self {
        Self { stop: Default::default(), task: None, messages }
    }

    pub fn start(&mut self, id: u64, scope: EntropyScope, processes: Vec<ProcessKey>, admin: bool) {
        self.stop();
        if let Some(task) = self.task.take() {
            task.abort();
        }
        self.stop = Arc::new(AtomicBool::new(false));
        let stop = self.stop.clone();
        let messages = self.messages.clone();
        self.task = Some(tokio::spawn(async move {
            if admin {
                let found = tokio::select! {
                    found = elevate(scope, processes) => found.map_err(|error| tr!(activity_cannot_inspect, error = error)),
                    () = wait_cancelled(&stop) => Err(tr!(entropy_scan_stopped).into()),
                };
                let _ = messages.send(Msg::EntropyScan { id, found, done: true });
                return;
            }
            scan_all(scope, processes, stop, TIME_LIMIT, |report, done| {
                let _ = messages.send(Msg::EntropyScan { id, found: Ok(report), done });
            })
            .await;
        }));
    }

    /// Stops the run under way, which then reports the processes it did not reach.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Drop for EntropyScanner {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

async fn elevate(scope: EntropyScope, processes: Vec<ProcessKey>) -> Result<EntropyReport> {
    let job = serde_json::to_string(&Job { scope, processes })?;
    match run_helper(vec!["--inspect-process".into(), "entropy".into(), job]).await? {
        ActivityData::Entropy(report) => Ok(report),
        _ => bail!(tr!(activity_request_invalid)),
    }
}

/// Reads `processes`, `AT_ONCE` at a time, until they are done, `stop` is set or `limit` has
/// passed. `send` gets what was found every `PROGRESS`, and the processes not reached last.
pub(super) async fn scan_all(
    scope: EntropyScope,
    processes: Vec<ProcessKey>,
    stop: Arc<AtomicBool>,
    limit: Duration,
    mut send: impl FnMut(EntropyReport, bool),
) {
    let deadline = Instant::now() + limit;
    let known = Arc::new(tokio::task::spawn_blocking(known).await.unwrap_or_default());
    let seen = Arc::new(Mutex::new(HashSet::new()));
    let mut queue = processes.into_iter();
    let mut tasks = JoinSet::new();
    let mut report = EntropyReport::default();
    let mut sent = Instant::now();
    loop {
        while tasks.len() < AT_ONCE && !stopped(&stop, deadline) {
            let Some(process) = queue.next() else { break };
            let (known, seen, stop) = (known.clone(), seen.clone(), stop.clone());
            tasks.spawn(async move { (process, scan_one(process, scope, &known, seen, stop, deadline).await) });
        }
        let Some(joined) = tasks.join_next().await else { break };
        let Ok((process, outcome)) = joined else { continue };
        match outcome {
            Outcome::Read(found) if found.pages == 0 && found.failed > 0 => report.unreadable.push(process),
            Outcome::Read(found) => {
                report.scanned += 1;
                report.hits.extend(found.hits);
            }
            Outcome::Unreadable => report.unreadable.push(process),
            Outcome::Protected => report.protected += 1,
            Outcome::Skipped => {}
            Outcome::Stopped => report.remaining.push(process),
        }
        if sent.elapsed() >= PROGRESS {
            sent = Instant::now();
            report.rank();
            send(std::mem::take(&mut report), false);
        }
    }
    report.remaining.extend(queue);
    report.rank();
    send(report, true);
}

fn stopped(stop: &AtomicBool, deadline: Instant) -> bool {
    stop.load(Ordering::Relaxed) || Instant::now() >= deadline
}

/// Reads one process: what its signature or the system allows, then its memory map, then a
/// sample of its pages.
async fn scan_one(
    process: ProcessKey,
    scope: EntropyScope,
    known: &HashMap<u32, Known>,
    seen: Arc<Mutex<HashSet<SharedFile>>>,
    stop: Arc<AtomicBool>,
    deadline: Instant,
) -> Outcome {
    let Some(found) = known.get(&process.pid).filter(|found| found.start == process.start && !found.kernel) else {
        return Outcome::Skipped;
    };
    let (name, executable) = (found.name.clone(), found.executable.clone());
    let own = process.pid == std::process::id();
    let allowed = tokio::task::spawn_blocking(move || {
        let protected = !own && executable.is_some_and(|path| crate::code_signature::protection(&path).is_some());
        if protected {
            Outcome::Protected
        } else if Memory::open(process).is_err() {
            Outcome::Unreadable
        } else {
            Outcome::Skipped
        }
    })
    .await;
    match allowed {
        Ok(Outcome::Skipped) => {}
        Ok(outcome) => return outcome,
        Err(_) => return Outcome::Unreadable,
    }
    let maps = tokio::select! {
        maps = regions(process) => maps,
        () = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)) => return Outcome::Stopped,
        () = wait_cancelled(&stop) => return Outcome::Stopped,
    };
    let Ok(maps) = maps else { return Outcome::Unreadable };
    let flag = stop.clone();
    let read = tokio::task::spawn_blocking(move || {
        let memory = Memory::open(process).ok()?;
        #[cfg(target_os = "linux")]
        let residency = crate::memory_census::Residency::open(process.pid);
        #[cfg(target_os = "linux")]
        let resident = |address| residency.resident(address);
        #[cfg(not(target_os = "linux"))]
        let resident = |_| None;
        let go_on = || !stopped(&flag, deadline);
        let read = |address, len| memory.read(address, len);
        let found = scan_process(process, &name, scope, &maps, read, resident, &seen, go_on);
        go_on().then_some(found)
    })
    .await;
    match read {
        Ok(Some(found)) => Outcome::Read(found),
        Ok(None) if stopped(&stop, deadline) => Outcome::Stopped,
        _ => Outcome::Unreadable,
    }
}

/// The helper's scan, as administrator: the whole run in one report, the best hits kept.
pub(super) async fn scan_as_helper(job: Job) -> EntropyReport {
    let mut whole = EntropyReport::default();
    let stop = Arc::new(AtomicBool::new(false));
    scan_all(job.scope, job.processes, stop, HELPER_TIME_LIMIT, |report, _| whole.merge(report)).await;
    whole
}
