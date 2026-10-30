use crate::{memory_search::Query, memory_strings::Known};
use anyhow::{Context, Result, anyhow, bail};
use borg_core::{
    ActivityData, ActivityRequest, IntegrityReport, MemoryCensus, MemoryRegion, MemorySnapshot, Msg, PROCESS_LIMIT,
    ProcessInfo, ProcessKey, SNAPSHOT_LIMIT, SystemSample,
};
use borg_i18n::tr;
#[cfg(target_os = "linux")]
use std::io::Read;
use std::{
    collections::HashSet,
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, ThreadKind, UpdateKind};
use tokio::{io::AsyncReadExt, process::Command, sync::mpsc::UnboundedSender, task::JoinHandle};
mod process_files;

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

/// A process sample with CPU use: CPU needs two looks a moment apart, as the activity
/// monitor's first sample does.
pub(crate) fn processes() -> SystemSample {
    const SETTLE: Duration = Duration::from_millis(250);
    let mut sampler = Sampler::default();
    sampler.sample();
    std::thread::sleep(SETTLE);
    sampler.sample()
}

#[derive(Default)]
struct Sampler {
    system: System,
    last: Option<Instant>,
    seen: HashSet<ProcessKey>,
}
impl Sampler {
    fn sample(&mut self) -> SystemSample {
        self.system.refresh_memory();
        self.system.refresh_cpu_usage();
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_cpu().with_memory().with_disk_usage().with_exe(UpdateKind::OnlyIfNotSet),
        );
        let now = Instant::now();
        let elapsed = self.last.map(|last| now.duration_since(last).as_secs_f64()).filter(|seconds| *seconds > 0.0);
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
                }
            })
            .collect();
        processes.sort_by_key(|p| p.key.pid);
        processes.truncate(PROCESS_LIMIT);
        self.seen = processes.iter().map(|p| p.key).collect();
        self.last = Some(now);
        SystemSample {
            processes,
            total: self.system.total_memory(),
            used: self.system.used_memory(),
            swap_total: self.system.total_swap(),
            swap_used: self.system.used_swap(),
            cpu: self.system.cpus().iter().map(|c| (c.cpu_usage().max(0.0) * 100.0) as u32).collect(),
        }
    }
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
    pub fn request(&mut self, id: u64, request: ActivityRequest) {
        self.cancel();
        self.cancelled = Arc::new(AtomicBool::new(false));
        let cancelled = self.cancelled.clone();
        let sampler = self.sampler.clone();
        let messages = self.messages.clone();
        self.task = Some(tokio::spawn(async move {
            let result = match request {
                ActivityRequest::Sample => tokio::task::spawn_blocking(move || {
                    sampler.lock().map(|mut s| ActivityData::Sample(s.sample())).map_err(|e| anyhow!(e.to_string()))
                })
                .await
                .map_err(anyhow::Error::from)
                .and_then(|r| r),
                request => inspect(request, cancelled, Some((id, messages.clone()))).await,
            }
            .map_err(|error| tr!(activity_cannot_inspect, error = error));
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
/// Searches a process's memory for an agent: as this user, or as administrator through the
/// system's own password prompt.
pub(crate) async fn search_for_agent(
    process: ProcessKey,
    query: Query,
    admin: bool,
    cancelled: Arc<AtomicBool>,
) -> Result<borg_core::MemorySearchReport> {
    let request = ActivityRequest::Search {
        process,
        query: query.text,
        mode: query.mode,
        match_case: query.match_case,
        snapshot: None,
        start: None,
        admin,
    };
    match inspect(request, cancelled, None).await? {
        ActivityData::Search(report) => Ok(report),
        _ => bail!(tr!(activity_request_invalid)),
    }
}
fn verify(process: ProcessKey) -> Result<()> {
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[Pid::from_u32(process.pid)]),
        true,
        ProcessRefreshKind::nothing(),
    );
    if !system.process(Pid::from_u32(process.pid)).is_some_and(|p| p.start_time() == process.start) {
        bail!(tr!(activity_identity_changed));
    }
    Ok(())
}
/// A process opened for reading its memory. On macOS a failed open or read carries no reason
/// (the errno left beside it belongs to something else), so the error says what failed instead.
pub struct Memory(read_process_memory::ProcessHandle);
impl Memory {
    pub fn open(process: ProcessKey) -> Result<Self> {
        match i32::try_from(process.pid)?.try_into() {
            Ok(handle) => Ok(Self(handle)),
            Err(_) if cfg!(target_os = "macos") => {
                verify(process)?;
                bail!(tr!(activity_memory_refused))
            }
            Err(error) => Err(error.into()),
        }
    }
    pub fn read(&self, address: u64, len: usize) -> Result<Vec<u8>> {
        match read_process_memory::copy_address(usize::try_from(address)?, len, &self.0) {
            Ok(bytes) => Ok(bytes),
            Err(_) if cfg!(target_os = "macos") => bail!(tr!(activity_memory_unread, address = address)),
            Err(error) => Err(error.into()),
        }
    }
}
async fn bounded_output(mut command: Command, timeout: Duration) -> Result<Vec<u8>> {
    command
        .env("LC_ALL", "C")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let mut child = command.spawn()?;
    let stdout = child.stdout.take().context(tr!(activity_unavailable))?;
    let stderr = child.stderr.take().context(tr!(activity_unavailable))?;
    tokio::time::timeout(timeout, async move {
        let (bytes, error) = tokio::try_join!(read_output(stdout, OUTPUT_LIMIT), read_output(stderr, ERROR_LIMIT))?;
        let status = child.wait().await?;
        if !status.success() {
            bail!("{}", String::from_utf8_lossy(&error).trim());
        }
        Ok(bytes)
    })
    .await
    .map_err(|_| anyhow!(tr!(activity_inspection_timeout)))?
}
async fn read_output(reader: impl tokio::io::AsyncRead + Unpin, limit: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(limit as u64 + 1).read_to_end(&mut bytes).await?;
    if bytes.len() > limit {
        bail!(tr!(activity_output_limit));
    }
    Ok(bytes)
}
async fn regions(process: ProcessKey) -> Result<Vec<MemoryRegion>> {
    verify(process)?;
    #[cfg(target_os = "macos")]
    let result = {
        let mut command = Command::new("/usr/bin/vmmap");
        command.args(["-w", "-interleaved", "-noCoalesce", &process.pid.to_string()]);
        let bytes = bounded_output(command, TIMEOUT).await?;
        parse_vmmap(&String::from_utf8_lossy(&bytes))
    };
    #[cfg(target_os = "linux")]
    let result = tokio::task::spawn_blocking(move || {
        let mut bytes = Vec::new();
        fs::File::open(format!("/proc/{}/smaps", process.pid))?
            .take(OUTPUT_LIMIT as u64 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > OUTPUT_LIMIT {
            bail!(tr!(activity_output_limit));
        }
        Ok::<_, anyhow::Error>(parse_smaps(&String::from_utf8_lossy(&bytes)))
    })
    .await??;
    verify(process)?;
    if result.is_empty() {
        bail!(tr!(activity_no_regions));
    }
    if result.len() > REGION_LIMIT {
        bail!(tr!(activity_output_limit));
    }
    Ok(result)
}
/// The process's mappings from the cheap maps file on Linux; macOS lists them only through
/// vmmap, which takes seconds on large processes, so there a read outside them simply fails.
async fn quick_maps(process: ProcessKey) -> Result<Option<Vec<MemoryRegion>>> {
    #[cfg(target_os = "linux")]
    return tokio::task::spawn_blocking(move || {
        let mut bytes = Vec::new();
        fs::File::open(format!("/proc/{}/maps", process.pid))?.take(OUTPUT_LIMIT as u64).read_to_end(&mut bytes)?;
        Ok(Some(parse_smaps(&String::from_utf8_lossy(&bytes))))
    })
    .await?;
    #[cfg(target_os = "macos")]
    {
        let _ = process;
        Ok(None)
    }
}
/// Reads at most a snapshot from the start of `region`. With `seek`, a window of zeros there
/// reads on, up to `SEEK_LIMIT`, to the first page holding data. The administrator helper
/// checks the bounds against the full memory map (`checked`); otherwise the cheap one is used.
async fn capture(process: ProcessKey, region: MemoryRegion, seek: bool, checked: bool) -> Result<MemorySnapshot> {
    let limit = region.end;
    let window = move |start: u64| {
        start.checked_add(limit.saturating_sub(start).min(SNAPSHOT_LIMIT as u64)).context(tr!(activity_region_invalid))
    };
    let end = window(region.start)?;
    let needed = if seek { region.end } else { end };
    let maps = if checked { Some(regions(process).await?) } else { quick_maps(process).await? };
    if !region.readable()
        || maps.is_some_and(|maps| !maps.iter().any(|r| r.readable() && r.start <= region.start && r.end >= needed))
    {
        bail!(tr!(activity_region_invalid));
    }
    tokio::task::spawn_blocking(move || {
        verify(process)?;
        let memory = Memory::open(process)?;
        let read = |start: u64, end: u64| memory.read(start, usize::try_from(end - start)?);
        let mut address = region.start;
        let mut bytes = read(address, end)?;
        if seek
            && bytes.iter().all(|&b| b == 0)
            && let Some(found) = first_data(read, end, region.end, region.start)
            && let Ok(data) = read(found, window(found)?)
        {
            (address, bytes) = (found, data);
        }
        verify(process)?;
        Ok(MemorySnapshot {
            process,
            address,
            bytes,
            captured: SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis().try_into()?,
        })
    })
    .await?
}
/// The page where data starts after `from`, reading at most `SEEK_LIMIT` bytes of the region
/// that ends at `end`; `None` when it is all zeros, or a read fails first.
fn first_data(read: impl Fn(u64, u64) -> Result<Vec<u8>>, from: u64, end: u64, floor: u64) -> Option<u64> {
    let stop = end.min(from.saturating_add(SEEK_LIMIT));
    let mut at = from;
    while at < stop {
        let next = at.saturating_add(SNAPSHOT_LIMIT as u64).min(stop);
        let chunk = read(at, next).ok()?;
        if let Some(i) = chunk.iter().position(|&b| b != 0) {
            return Some(((at + i as u64) & !(PAGE - 1)).max(floor));
        }
        at = next;
    }
    None
}
/// Classifies the pages of `region`; see `memory_census`. `mapped` are the process's readable
/// mappings.
async fn census(
    process: ProcessKey,
    region: MemoryRegion,
    mut mapped: Vec<(u64, u64)>,
    cancelled: Arc<AtomicBool>,
) -> Result<MemoryCensus> {
    mapped.sort_unstable();
    if !region.readable() || region.end <= region.start {
        bail!(tr!(activity_region_invalid));
    }
    tokio::task::spawn_blocking(move || {
        verify(process)?;
        let memory = Memory::open(process)?;
        let read = |address: u64, len: usize| -> Result<Vec<u8>> {
            if cancelled.load(Ordering::Relaxed) {
                bail!(tr!(memory_search_stopped));
            }
            memory.read(address, len)
        };
        #[cfg(target_os = "linux")]
        let residency = crate::memory_census::Residency::open(process.pid);
        #[cfg(target_os = "linux")]
        let resident = |address| residency.resident(address);
        #[cfg(not(target_os = "linux"))]
        let resident = |_| None;
        let census = crate::memory_census::census(process, &region, &mapped, read, resident);
        if cancelled.load(Ordering::Relaxed) {
            bail!(tr!(memory_search_stopped));
        }
        verify(process)?;
        Ok(census)
    })
    .await?
}
/// Checks the process against its files and for signs of injected code; see `integrity`.
async fn integrity(
    process: ProcessKey,
    cancelled: Arc<AtomicBool>,
    messages: Option<(u64, UnboundedSender<Msg>)>,
) -> Result<IntegrityReport> {
    let maps = regions(process).await?;
    tokio::task::spawn_blocking(move || {
        verify(process)?;
        let facts = crate::integrity::facts(process.pid);
        let access = crate::integrity::Live::open(process);
        let progress = |checked, total, findings| {
            if let Some((id, messages)) = &messages {
                let _ = messages.send(Msg::MemorySearchProgress { id: *id, scanned: checked, total, hits: findings });
            }
        };
        let report = crate::integrity::check(process, &facts, &maps, &access, &cancelled, progress);
        verify(process)?;
        Ok(report)
    })
    .await?
}
async fn wait_cancelled(cancelled: &AtomicBool) {
    while !cancelled.load(Ordering::Relaxed) {
        tokio::time::sleep(CANCEL_POLL).await;
    }
}
async fn inspect(
    request: ActivityRequest,
    cancelled: Arc<AtomicBool>,
    messages: Option<(u64, UnboundedSender<Msg>)>,
) -> Result<ActivityData> {
    match request {
        request @ (ActivityRequest::Regions { admin: true, .. }
        | ActivityRequest::Files { admin: true, .. }
        | ActivityRequest::Capture { admin: true, .. }
        | ActivityRequest::Census { admin: true, .. }
        | ActivityRequest::Integrity { admin: true, .. }
        | ActivityRequest::Search { admin: true, snapshot: None, .. }) => tokio::select! {
            result = elevate(request) => result,
            () = wait_cancelled(&cancelled) => bail!(tr!(memory_search_stopped)),
        },
        ActivityRequest::Regions { process, .. } => Ok(ActivityData::Regions(regions(process).await?)),
        ActivityRequest::Files { process, .. } => Ok(ActivityData::Files(process_files::list(process).await?)),
        ActivityRequest::Capture { process, region, seek, .. } => {
            Ok(ActivityData::Snapshot(capture(process, region, seek, false).await?))
        }
        ActivityRequest::Census { process, region, mapped, .. } => {
            Ok(ActivityData::Census(census(process, region, mapped, cancelled).await?))
        }
        ActivityRequest::Integrity { process, .. } => {
            Ok(ActivityData::Integrity(integrity(process, cancelled, messages).await?))
        }
        ActivityRequest::Search { process, query, mode, match_case, snapshot, start, .. } => {
            let query = Query { text: query, mode, match_case };
            search_memory(process, query, snapshot, start, cancelled, messages).await.map(ActivityData::Search)
        }
        ActivityRequest::Sample => bail!(tr!(activity_request_invalid)),
    }
}
fn helper_args(request: &ActivityRequest) -> Result<Vec<String>> {
    let mut args = vec!["--inspect-process".into()];
    match request {
        request @ ActivityRequest::Search { snapshot: None, .. } => {
            args.extend(["search".into(), serde_json::to_string(request)?]);
        }
        ActivityRequest::Regions { process, .. } => {
            args.extend(["regions".into(), process.pid.to_string(), process.start.to_string()])
        }
        ActivityRequest::Files { process, .. } => {
            args.extend(["files".into(), process.pid.to_string(), process.start.to_string()])
        }
        ActivityRequest::Integrity { process, .. } => {
            args.extend(["integrity".into(), process.pid.to_string(), process.start.to_string()])
        }
        ActivityRequest::Capture { process, region, seek, .. } => args.extend([
            "capture".into(),
            process.pid.to_string(),
            process.start.to_string(),
            region.start.to_string(),
            region.end.to_string(),
            u8::from(*seek).to_string(),
        ]),
        ActivityRequest::Census { process, region, .. } => args.extend([
            "census".into(),
            process.pid.to_string(),
            process.start.to_string(),
            region.start.to_string(),
            region.end.to_string(),
        ]),
        _ => bail!(tr!(activity_request_invalid)),
    }
    Ok(args)
}
async fn elevate(request: ActivityRequest) -> Result<ActivityData> {
    let exe = std::env::current_exe()?;
    let args = helper_args(&request)?;
    #[cfg(target_os = "macos")]
    let command = {
        let words =
            std::iter::once(exe.to_str().context(tr!(activity_unavailable))?).chain(args.iter().map(String::as_str));
        let script = crate::admin::apple_script(words)?;
        let mut command = Command::new("/usr/bin/osascript");
        command.arg("-e").arg(script);
        command
    };
    #[cfg(target_os = "linux")]
    let command = {
        let mut command = Command::new("pkexec");
        command.arg(exe).args(args);
        command
    };
    let bytes = bounded_output(command, ADMIN_TIMEOUT)
        .await
        .map_err(|e| anyhow!("{}", tr!(activity_admin_unavailable, error = e)))?;
    let result: Result<ActivityData, String> = serde_json::from_slice(&bytes)?;
    result.map_err(anyhow::Error::msg)
}
pub async fn helper(args: &[String]) -> Result<()> {
    let result = async {
        let kind = args.first().context(tr!(activity_request_invalid))?;
        if kind == "search" && args.len() == 2 {
            if args[1].len() > 4096 {
                bail!(tr!(activity_request_invalid));
            }
            let request: ActivityRequest = serde_json::from_str(&args[1])?;
            if let ActivityRequest::Search { process, query, mode, match_case, snapshot: None, start, .. } = request {
                let query = Query { text: query, mode, match_case };
                return search_memory(process, query, None, start, Arc::new(AtomicBool::new(false)), None)
                    .await
                    .map(ActivityData::Search);
            }
            bail!(tr!(activity_request_invalid));
        }

        let number =
            |i: usize| args.get(i).context(tr!(activity_request_invalid))?.parse::<u64>().map_err(anyhow::Error::from);
        let process = ProcessKey { pid: number(1)?.try_into()?, start: number(2)? };
        if process.pid == 0 {
            bail!(tr!(activity_request_invalid));
        }
        match (kind.as_str(), args.len()) {
            ("regions", 3) => Ok(ActivityData::Regions(regions(process).await?)),
            ("files", 3) => Ok(ActivityData::Files(process_files::list(process).await?)),
            ("integrity", 3) => {
                Ok(ActivityData::Integrity(integrity(process, Arc::new(AtomicBool::new(false)), None).await?))
            }
            ("capture", 6) => {
                let start = number(3)?;
                let end = number(4)?;
                let seek = match number(5)? {
                    0 => false,
                    1 => true,
                    _ => bail!(tr!(activity_request_invalid)),
                };
                if end <= start {
                    bail!(tr!(activity_request_invalid));
                }
                let region = MemoryRegion { start, end, permissions: "r--".into(), ..Default::default() };
                Ok(ActivityData::Snapshot(capture(process, region, seek, true).await?))
            }
            ("census", 5) => {
                let (start, end) = (number(3)?, number(4)?);
                let maps = regions(process).await?;
                let region =
                    maps.iter().find(|r| r.start == start && r.end == end).context(tr!(activity_region_invalid))?;
                let mapped = maps.iter().filter(|r| r.readable()).map(|r| (r.start, r.end)).collect();
                let cancelled = Arc::new(AtomicBool::new(false));
                Ok(ActivityData::Census(census(process, region.clone(), mapped, cancelled).await?))
            }
            _ => bail!(tr!(activity_request_invalid)),
        }
    }
    .await
    .map_err(|error: anyhow::Error| error.to_string());
    println!("{}", serde_json::to_string(&result)?);
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
fn range(text: &str) -> Option<(u64, u64)> {
    let (a, b) = text.split_once('-')?;
    let start = u64::from_str_radix(a, 16).ok()?;
    let end = u64::from_str_radix(b, 16).ok()?;
    (end > start).then_some((start, end))
}
fn amount(text: &str) -> Option<u64> {
    let split = text.find(|c: char| !c.is_ascii_digit() && c != '.').unwrap_or(text.len());
    let n: f64 = text[..split].parse().ok()?;
    let factor = match text[split..].trim() {
        "" | "B" => 1.0,
        "K" | "kB" => KIB,
        "M" => KIB.powi(2),
        "G" => KIB.powi(3),
        "T" => KIB.powi(4),
        _ => return None,
    };
    (n.is_finite() && n >= 0.0).then_some((n * factor) as u64)
}
#[cfg(any(target_os = "macos", test))]
fn parse_vmmap(text: &str) -> Vec<MemoryRegion> {
    let mut regions = Vec::new();
    for line in text.lines() {
        let words: Vec<_> = line.split_whitespace().collect();
        let Some((i, (start, end))) = words.iter().enumerate().find_map(|(i, w)| range(w).map(|r| (i, r))) else {
            continue;
        };
        let Some(perms) = words
            .iter()
            .skip(i + 1)
            .find(|w| w.contains('/') && w.len() >= 7 && w.chars().take(3).all(|c| "rwx-".contains(c)))
        else {
            continue;
        };
        // [ VSIZE RSDNT DIRTY SWAP ]
        let sizes: Vec<_> = line
            .split_once('[')
            .and_then(|(_, r)| r.split_once(']'))
            .map(|(r, _)| r.split_whitespace().map(amount).collect())
            .unwrap_or_default();
        let size = |n: usize| sizes.get(n).copied().flatten();
        let kind = words[..i].join(" ");
        let detail = words
            .iter()
            .skip_while(|w| !w.starts_with("SM="))
            .skip(1)
            .skip_while(|w| w.starts_with("PURGE="))
            .copied()
            .collect::<Vec<_>>()
            .join(" ");
        let path = detail.starts_with('/').then(|| detail.clone());
        regions.push(MemoryRegion {
            start,
            end,
            permissions: perms.split('/').next().unwrap_or_default().into(),
            name: if detail.is_empty() { kind } else { format!("{kind} · {detail}") },
            resident: size(1),
            private: None,
            shared: None,
            swap: size(3),
            dirty: size(2),
            path,
            offset: None,
        });
    }
    regions.sort_by_key(|r| r.start);
    regions.dedup_by_key(|r| r.start);
    regions
}
#[cfg(any(target_os = "linux", test))]
fn parse_smaps(text: &str) -> Vec<MemoryRegion> {
    let mut regions: Vec<MemoryRegion> = Vec::new();
    for line in text.lines() {
        let words: Vec<_> = line.split_whitespace().collect();
        if let Some((start, end)) = words.first().and_then(|w| range(w)) {
            if let Some(perms) = words.get(1) {
                let name = words.iter().skip(5).copied().collect::<Vec<_>>().join(" ");
                let path = name.starts_with('/').then(|| name.clone());
                regions.push(MemoryRegion {
                    start,
                    end,
                    permissions: (*perms).into(),
                    offset: path.as_ref().and(words.get(2)).and_then(|w| u64::from_str_radix(w, 16).ok()),
                    path,
                    name,
                    ..Default::default()
                });
            }
        } else if let (Some(region), Some(key), Some(value)) =
            (regions.last_mut(), words.first(), words.get(1).and_then(|w| amount(&format!("{w}kB"))))
        {
            match *key {
                "Rss:" => region.resident = Some(value),
                "Swap:" => region.swap = Some(value),
                "Private_Clean:" | "Private_Dirty:" => {
                    region.private = Some(region.private.unwrap_or(0).saturating_add(value))
                }
                "Shared_Clean:" | "Shared_Dirty:" => {
                    region.shared = Some(region.shared.unwrap_or(0).saturating_add(value))
                }
                _ => {}
            }
        }
    }
    regions
}

async fn search_memory(
    process: ProcessKey,
    query: Query,
    snapshot: Option<MemorySnapshot>,
    start: Option<u64>,
    cancelled: Arc<AtomicBool>,
    messages: Option<(u64, UnboundedSender<Msg>)>,
) -> Result<borg_core::MemorySearchReport> {
    crate::memory_search::validate(&query.text, query.mode)?;
    let maps = if let Some(snapshot) = &snapshot {
        if snapshot.process != process || snapshot.bytes.len() > SNAPSHOT_LIMIT {
            bail!(tr!(activity_request_invalid));
        }
        vec![MemoryRegion {
            start: snapshot.address,
            end: snapshot.address.checked_add(snapshot.bytes.len() as u64).context(tr!(activity_region_invalid))?,
            permissions: "r--".into(),
            name: tr!(memory_search_snapshot).into(),
            ..Default::default()
        }]
    } else {
        regions(process).await?
    };
    tokio::task::spawn_blocking(move || {
        let mut progress = |scanned, total, hits| {
            if let Some((id, messages)) = &messages {
                let _ = messages.send(Msg::MemorySearchProgress { id: *id, scanned, total, hits });
            }
        };
        if let Some(snapshot) = snapshot {
            crate::memory_search::scan(
                crate::memory_search::Target { process, regions: maps, known: &Known::default() },
                &query,
                start,
                &cancelled,
                |address, count| {
                    let offset = usize::try_from(address - snapshot.address)?;
                    Ok(snapshot.bytes.get(offset..offset + count).context(tr!(activity_region_invalid))?.to_vec())
                },
                &mut progress,
            )
        } else {
            verify(process)?;
            let memory = Memory::open(process)?;
            // Strings come from memory the process filled itself; the rest is in its files,
            // whose strings tell which of those only memory holds.
            let (maps, known) = if query.mode.strings() {
                let root = PathBuf::from(if cfg!(target_os = "linux") {
                    format!("/proc/{}/root", process.pid)
                } else {
                    "/".into()
                });
                let files: std::collections::BTreeSet<_> =
                    maps.iter().filter_map(|region| region.path.as_deref()).collect();
                let files = files.into_iter().map(|path| root.join(path.trim_start_matches('/')));
                let known = Known::read(files, Instant::now() + FILE_STRINGS_TIME);
                (maps.into_iter().filter(MemoryRegion::own).collect(), known)
            } else {
                (maps, Known::default())
            };
            let report = crate::memory_search::scan(
                crate::memory_search::Target { process, regions: maps, known: &known },
                &query,
                start,
                &cancelled,
                |address, count| memory.read(address, count),
                &mut progress,
            )?;
            verify(process)?;
            Ok(report)
        }
    })
    .await?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn linux_regions_keep_private_shared_swap_and_file_names_with_spaces() {
        let text = "1000-3000 rw-p 0002a000 08:01 1234 /tmp/file with spaces\nRss: 8 kB\nPrivate_Clean: 2 kB\nPrivate_Dirty: 3 kB\nShared_Clean: 1 kB\nShared_Dirty: 2 kB\nSwap: 4 kB\nVmFlags: rd wr\n3000-4000 ---p 00000000 00:00 0\n";
        let regions = parse_smaps(text);
        assert_eq!(regions.len(), 2);
        assert_eq!(regions[0].name, "/tmp/file with spaces");
        assert_eq!(regions[0].path.as_deref(), Some("/tmp/file with spaces"));
        assert_eq!((regions[0].offset, regions[1].offset), (Some(0x2a000), None));
        assert_eq!(regions[0].footprint(), Some(5120 + 4096));
        assert_eq!(regions[0].resident, Some(8192));
        assert_eq!(regions[0].private, Some(5120));
        assert_eq!(regions[0].shared, Some(3072));
        assert_eq!(regions[0].swap, Some(4096));
        assert!(!regions[1].readable());
    }
    #[test]
    fn mac_regions_parse_real_vmmap_layout_and_ignore_summary_totals() {
        let text = "__TEXT 0000000100000000-0000000100004000 [ 16K 12K 0K 0K] r-x/r-x SM=COW /Applications/Test App\nMALLOC_SMALL 0000000200000000-0000000200100000 [ 1024K 64K 48K 2048] rw-/rwx SM=PRV\nStack 0000000300000000-0000000300004000 [ 16K 16K 16K 0K] rw-/rwx SM=PRV          thread 2\nTOTAL 4G 3G\n";
        let regions = parse_vmmap(text);
        assert_eq!(regions.len(), 3);
        assert_eq!(regions[0].start, 0x100000000);
        assert_eq!(regions[0].resident, Some(12 * 1024));
        assert_eq!(regions[0].name, "__TEXT · /Applications/Test App");
        assert_eq!(regions[0].path.as_deref(), Some("/Applications/Test App"));
        assert_eq!(regions[0].kind(), "__TEXT");
        assert_eq!(regions[0].footprint(), Some(0));
        assert_eq!(regions[1].end - regions[1].start, 1024 * 1024);
        assert_eq!(regions[1].footprint(), Some(48 * 1024 + 2048));
        assert!(regions[1].readable());
        assert_eq!((regions[2].name.as_str(), regions[2].path.as_deref()), ("Stack · thread 2", None));
        assert!(parse_vmmap("vmmap: process protected").is_empty());
    }
    #[test]
    fn snapshot_exports_are_private_bounded_and_never_replace_existing_files() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("snapshot.bin");
        export(&path, b"sample bytes").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"sample bytes");
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        assert!(export(&path, b"replace").is_err());
        assert!(export(&temp.path().join("large"), &vec![0; SNAPSHOT_LIMIT + 1]).is_err());
        assert!(!temp.path().join("large").exists());
    }
    #[test]
    fn sampler_reports_its_own_process_and_rejects_reused_identity() {
        let mut sampler = Sampler::default();
        let sample = sampler.sample();
        let own = sample.processes.iter().find(|p| p.key.pid == std::process::id()).unwrap();
        assert!(own.resident.is_some());
        assert!(own.cpu.is_none() && own.read.is_none());
        assert!(verify(own.key).is_ok());
        assert!(verify(ProcessKey { start: own.key.start.saturating_add(1), ..own.key }).is_err());
    }
    #[test]
    fn seeking_skips_zero_pages_to_the_page_holding_data_within_the_limit() {
        let mut memory = vec![0u8; 3 * SNAPSHOT_LIMIT];
        memory[2 * SNAPSHOT_LIMIT + 5000] = 7;
        let read = |start: u64, end: u64| Ok(memory[start as usize..end as usize].to_vec());
        let end = memory.len() as u64;
        assert_eq!(first_data(read, SNAPSHOT_LIMIT as u64, end, 0), Some(2 * SNAPSHOT_LIMIT as u64 + 4096));
        assert_eq!(first_data(read, 2 * SNAPSHOT_LIMIT as u64 + 6000, end, 0), None, "zeros to the end");
        let unreadable = |_: u64, _: u64| -> Result<Vec<u8>> { bail!("unreadable") };
        assert_eq!(first_data(unreadable, 0, end, 0), None);
    }
    #[test]
    fn memory_reader_can_capture_known_bytes_from_its_own_address_space() {
        let bytes = b"Borg memory capture test\0";
        let memory = Memory::open(ProcessKey { pid: std::process::id(), start: 0 }).unwrap();
        assert_eq!(memory.read(bytes.as_ptr() as u64, bytes.len()).unwrap(), bytes);
    }
    #[tokio::test]
    async fn integrity_finds_an_image_hidden_in_its_own_memory_and_runs_its_files_unchanged() {
        use borg_core::FindingKind;
        let own = Sampler::default().sample().processes.into_iter().find(|p| p.key.pid == std::process::id());
        // An executable header at the start of a page of memory no file backs.
        let mut buffer = vec![0u8; 64 * PAGE as usize];
        let at = (buffer.as_ptr() as usize).next_multiple_of(PAGE as usize) - buffer.as_ptr() as usize;
        buffer[at..at + 6].copy_from_slice(&[0x7f, b'E', b'L', b'F', 2, 1]);
        buffer[at + 16] = 3;
        let address = buffer.as_ptr() as u64 + at as u64;
        let report = integrity(own.unwrap().key, Arc::new(AtomicBool::new(false)), None).await.unwrap();
        std::hint::black_box(&buffer);
        let kinds: Vec<_> = report.findings.iter().map(|f| (f.kind, f.address, f.detail.as_str())).collect();
        assert!(
            kinds.iter().any(|&(kind, at, _)| kind == FindingKind::HiddenImage && at == Some(address)),
            "{kinds:?}"
        );
        assert!(!kinds.iter().any(|&(kind, ..)| kind == FindingKind::CodeChanged), "{kinds:?}");
        // Tests running beside this one free memory as it reads, so some may be gone.
        assert!(report.compared > 0 && report.scanned > 0, "{report:?}");
    }
    #[tokio::test]
    async fn command_output_is_bounded_and_inspection_has_a_deadline() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "printf '1234567890'"]);
        assert_eq!(bounded_output(command, TIMEOUT).await.unwrap(), b"1234567890");
        assert!(read_output(&b"12345"[..], 4).await.is_err());
        let mut command = Command::new("/bin/sleep");
        command.arg("5");
        assert!(
            bounded_output(command, Duration::from_millis(10)).await.unwrap_err().to_string().contains("timed out")
        );
    }
    #[tokio::test]
    async fn snapshot_search_needs_no_live_process_and_preserves_unicode_addresses() {
        let process = ProcessKey { pid: u32::MAX, start: 1 };
        let bytes: Vec<_> = "--𝐁𝐨𝐫𝐠--".encode_utf16().flat_map(u16::to_le_bytes).collect();
        let snapshot = MemorySnapshot { process, address: 0x4000, bytes, captured: 1 };
        let query = Query { text: "Borg".into(), mode: borg_core::MemorySearchMode::Unicode, match_case: true };
        let report =
            search_memory(process, query, Some(snapshot), None, Arc::new(AtomicBool::new(false)), None).await.unwrap();
        assert!(report.hits.iter().any(|h| h.address == 0x4004 && h.encoding == "UTF-16LE"));
        assert_eq!(report.skipped, 0);
        assert!(report.next.is_none());
    }
}
