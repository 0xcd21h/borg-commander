//! Reading a process's memory: a region's snapshot, its page census, integrity checks and searches.

use super::helper::elevate;
use super::{CANCEL_POLL, FILE_STRINGS_TIME, PAGE, SEEK_LIMIT, identify, process_files, quick_maps, regions, verify};
use crate::{memory_search::Query, memory_strings::Known};
use anyhow::{Context, Result, bail};
use borg_core::{
    ActivityData, ActivityRequest, IntegrityReport, MemoryCensus, MemoryRegion, MemorySnapshot, Msg, ProcessKey,
    SNAPSHOT_LIMIT,
};
use borg_i18n::tr;
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::mpsc::UnboundedSender;

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

/// A process opened for reading its memory. On macOS a failed open or read carries no reason
/// (the errno left beside it belongs to something else), so the error says what failed instead.
pub struct Memory(read_process_memory::ProcessHandle);

impl Memory {
    pub fn open(process: ProcessKey) -> Result<Self> {
        match i32::try_from(process.pid)?.try_into() {
            Ok(handle) => Ok(Self(handle)),
            Err(_) if cfg!(target_os = "macos") => {
                verify(process)?;
                let program = identify(process.pid).and_then(|(_, _, program)| program);
                if program.is_some_and(|program| crate::code_signature::protection(&program).is_some()) {
                    bail!(tr!(activity_memory_protected))
                }
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

/// Reads at most a snapshot from the start of `region`. With `seek`, a window of zeros there
/// reads on, up to `SEEK_LIMIT`, to the first page holding data. The administrator helper
/// checks the bounds against the full memory map (`checked`); otherwise the cheap one is used.
pub(super) async fn capture(
    process: ProcessKey,
    region: MemoryRegion,
    seek: bool,
    checked: bool,
) -> Result<MemorySnapshot> {
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
pub(super) fn first_data(read: impl Fn(u64, u64) -> Result<Vec<u8>>, from: u64, end: u64, floor: u64) -> Option<u64> {
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
pub(super) async fn census(
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
pub(super) async fn integrity(
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

pub(super) async fn wait_cancelled(cancelled: &AtomicBool) {
    while !cancelled.load(Ordering::Relaxed) {
        tokio::time::sleep(CANCEL_POLL).await;
    }
}

pub(super) async fn inspect(
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
        | ActivityRequest::Search { admin: true, snapshot: None, .. }
        | ActivityRequest::Control { admin: true, .. }) => tokio::select! {
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
        ActivityRequest::Control { process, control, .. } => {
            tokio::task::spawn_blocking(move || super::control::control(process, control))
                .await?
                .map(ActivityData::Control)
        }
        ActivityRequest::Sample => bail!(tr!(activity_request_invalid)),
    }
}

pub(super) async fn search_memory(
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
