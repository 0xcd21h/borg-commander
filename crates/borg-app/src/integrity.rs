//! Whether a process runs the code its files hold: code that differs from the files it was
//! mapped from, code and executable images no file backs, and what in its environment changes
//! which code it loads.

use anyhow::{Result, anyhow};
use borg_core::{Finding, IntegrityReport, MemoryRegion, ProcessKey};
use borg_i18n::tr;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

mod compare;
mod findings;
mod rules;

use compare::{compare, scan};
use findings::{Stretch, executable, hex, label, map_findings, process_findings, size, stretches, unbacked, untouched};
use rules::{
    CODE_FILES, INTERPRETERS, KERNEL_PAGES, KERNEL_THREADS, LIBRARY_PATHS, PRELOADS, compiles_code, temporary,
};

const PAGE: u64 = 4096;
const CHUNK: u64 = 1024 * 1024;
/// Bytes of code read to compare with files, and of unbacked memory read to search for
/// executable images, before the check stops short; it also stops after `TIME_LIMIT`.
const COMPARE_LIMIT: u64 = 512 * 1024 * 1024;
const SCAN_LIMIT: u64 = 1024 * 1024 * 1024;
const TIME_LIMIT: Duration = Duration::from_secs(45);
const PROGRESS: Duration = Duration::from_millis(100);
const FINDING_LIMIT: usize = 2000;
/// Bytes of a difference shown, from its first.
const SHOWN: usize = 8;

/// What is known of a process apart from its memory.
#[derive(Debug, Default)]
pub struct Facts {
    pub name: String,
    /// The program's path, and whether the system says its file was deleted since it started.
    pub program: Option<(String, bool)>,
    /// `NAME=value` pairs.
    pub environment: Vec<String>,
    /// Libraries the system loads into every program, from Linux's `/etc/ld.so.preload`.
    pub preloaded: Vec<String>,
    pub tracer: Option<(u32, String)>,
}

/// What the page tables say of a page: whether it is in memory or swapped out, and whether it
/// is still the file's own page rather than a private copy.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PageUse {
    pub present: bool,
    pub swapped: bool,
    pub file: bool,
}

/// How the check reaches a process and its files, so tests can stand in for both.
pub trait Access {
    fn read(&self, address: u64, len: usize) -> Result<Vec<u8>>;
    /// How each of `count` pages from `address` is used, when the system says.
    fn pages(&self, address: u64, count: usize) -> Option<Vec<PageUse>>;
    /// Up to `len` bytes of a file as the process sees it, from `offset`.
    fn read_file(&self, path: &str, offset: u64, len: usize) -> Option<Vec<u8>>;
    /// Whether a file is there as the process sees it; `None` when its files cannot be seen.
    fn exists(&self, path: &str) -> Option<bool>;
}

/// Runs every check against `regions`, the process's memory map.
pub fn check(
    process: ProcessKey,
    facts: &Facts,
    regions: &[MemoryRegion],
    access: &impl Access,
    cancelled: &AtomicBool,
    progress: impl FnMut(u64, u64, usize),
) -> IntegrityReport {
    let report = IntegrityReport {
        process,
        findings: Vec::new(),
        compared: 0,
        uncompared: 0,
        scanned: 0,
        unbacked: 0,
        unreadable: 0,
        complete: true,
    };
    let mut run = Run {
        report,
        began: Instant::now(),
        shown: None,
        cancelled,
        progress,
        total: 0,
        compare_read: 0,
        scan_read: 0,
    };
    let jit = compiles_code(facts, regions);
    let exists = |path: &str| access.exists(path);
    for finding in process_findings(facts, exists).into_iter().chain(map_findings(regions, jit, exists)) {
        run.add(finding);
    }
    let stretches = stretches(regions, access, &mut run.report);
    let candidates: Vec<_> = (0..regions.len()).filter(|&i| unbacked(&regions[i])).collect();
    run.report.unbacked = candidates.iter().map(|&i| size(&regions[i])).sum();
    let code: u64 = stretches.iter().map(|s| size(s.region)).sum();
    run.total = code.min(COMPARE_LIMIT) + run.report.unbacked.min(SCAN_LIMIT);
    for stretch in &stretches {
        compare(&mut run, access, stretch);
    }
    // Executable memory first: an image there runs.
    let (mut first, rest): (Vec<_>, Vec<_>) = candidates.into_iter().partition(|&i| executable(&regions[i]));
    first.extend(rest);
    for i in first {
        let next = regions.get(i + 1).filter(|next| next.start == regions[i].end && unbacked(next));
        scan(&mut run, access, &regions[i], next.is_some_and(executable));
    }
    let mut report = run.report;
    report.findings.sort_by_key(|f| (f.severity, f.address.is_some(), f.address));
    report
}

struct Run<'a, P> {
    report: IntegrityReport,
    began: Instant,
    shown: Option<Instant>,
    cancelled: &'a AtomicBool,
    progress: P,
    total: u64,
    compare_read: u64,
    scan_read: u64,
}
impl<P: FnMut(u64, u64, usize)> Run<'_, P> {
    fn stopped(&mut self) -> bool {
        let stopped = self.cancelled.load(Ordering::Relaxed) || self.began.elapsed() >= TIME_LIMIT;
        self.report.complete &= !stopped;
        stopped
    }
    fn add(&mut self, finding: Finding) {
        if self.report.findings.len() < FINDING_LIMIT {
            self.report.findings.push(finding);
        } else {
            self.report.complete = false;
        }
    }
    fn tick(&mut self) {
        if self.shown.is_none_or(|shown| shown.elapsed() >= PROGRESS) {
            self.shown = Some(Instant::now());
            let checked = self.report.compared + self.report.scanned;
            (self.progress)(checked.min(self.total), self.total, self.report.findings.len());
        }
    }
}

/// The process's memory and files.
pub struct Live {
    memory: Option<crate::activity::Memory>,
    root: PathBuf,
    #[cfg(target_os = "linux")]
    residency: crate::memory_census::Residency,
}
impl Live {
    pub fn open(process: ProcessKey) -> Self {
        let pid = process.pid;
        Self {
            memory: crate::activity::Memory::open(process).ok(),
            // A process in a container sees its own files.
            root: if cfg!(target_os = "linux") { format!("/proc/{pid}/root").into() } else { "/".into() },
            #[cfg(target_os = "linux")]
            residency: crate::memory_census::Residency::open(pid),
        }
    }
    fn file(&self, path: &str) -> PathBuf {
        self.root.join(path.trim_start_matches('/'))
    }
}
impl Access for Live {
    fn read(&self, address: u64, len: usize) -> Result<Vec<u8>> {
        self.memory.as_ref().ok_or_else(|| anyhow!(tr!(activity_unavailable)))?.read(address, len)
    }
    fn pages(&self, address: u64, count: usize) -> Option<Vec<PageUse>> {
        #[cfg(target_os = "linux")]
        {
            use crate::memory_census::Residency;
            let entries = self.residency.entries(address, count)?;
            Some(
                entries
                    .into_iter()
                    .map(|entry| PageUse {
                        present: entry & Residency::PRESENT != 0,
                        swapped: entry & Residency::SWAPPED != 0,
                        file: entry & Residency::FILE != 0,
                    })
                    .collect(),
            )
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (address, count);
            None
        }
    }
    fn read_file(&self, path: &str, offset: u64, len: usize) -> Option<Vec<u8>> {
        use std::os::unix::fs::FileExt;
        let file = std::fs::File::open(self.file(path)).ok()?;
        let mut bytes = vec![0; len];
        let mut filled = 0;
        while filled < len {
            match file.read_at(&mut bytes[filled..], offset + filled as u64).ok()? {
                0 => break,
                n => filled += n,
            }
        }
        bytes.truncate(filled);
        Some(bytes)
    }
    fn exists(&self, path: &str) -> Option<bool> {
        match std::fs::metadata(self.file(path)) {
            Ok(_) => Some(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(false),
            Err(_) => None,
        }
    }
}

/// What the system says of the process apart from its memory.
#[cfg(target_os = "linux")]
pub fn facts(pid: u32) -> Facts {
    let read = |file: &str| std::fs::read(format!("/proc/{pid}/{file}")).ok();
    let text = |file: &str| read(file).map(|bytes| String::from_utf8_lossy(&bytes).trim_end().to_owned());
    let program = std::fs::read_link(format!("/proc/{pid}/exe")).ok().map(|path| {
        let path = path.to_string_lossy().into_owned();
        match path.strip_suffix(" (deleted)") {
            Some(path) => (path.to_owned(), true),
            None => (path, false),
        }
    });
    let tracer = text("status")
        .and_then(|status| status.lines().find_map(|line| line.strip_prefix("TracerPid:")?.trim().parse::<u32>().ok()))
        .filter(|&tracer| tracer != 0)
        .map(|tracer| {
            let name = std::fs::read_to_string(format!("/proc/{tracer}/comm")).unwrap_or_default();
            (tracer, name.trim_end().to_owned())
        });
    let environment = read("environ").unwrap_or_default();
    Facts {
        name: text("comm").unwrap_or_default(),
        program,
        environment: environment
            .split(|&byte| byte == 0)
            .filter(|variable| !variable.is_empty())
            .map(|variable| String::from_utf8_lossy(variable).into_owned())
            .collect(),
        preloaded: text("root/etc/ld.so.preload")
            .unwrap_or_default()
            .lines()
            .filter(|line| !line.trim_start().starts_with('#'))
            .flat_map(|line| line.split([' ', '\t', ':']))
            .filter(|library| !library.is_empty())
            .map(String::from)
            .collect(),
        tracer,
    }
}

/// What the system says of the process apart from its memory.
#[cfg(target_os = "macos")]
pub fn facts(pid: u32) -> Facts {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
    let pid = Pid::from_u32(pid);
    let mut system = System::new();
    let refresh = ProcessRefreshKind::nothing().with_exe(UpdateKind::Always).with_environ(UpdateKind::Always);
    system.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), true, refresh);
    let Some(process) = system.process(pid) else {
        return Facts::default();
    };
    Facts {
        name: process.name().to_string_lossy().into_owned(),
        program: process.exe().map(|path| (path.to_string_lossy().into_owned(), false)),
        environment: process.environ().iter().map(|variable| variable.to_string_lossy().into_owned()).collect(),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests;
