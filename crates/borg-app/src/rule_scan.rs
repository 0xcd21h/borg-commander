//! Scanning with rules: the user's rule files, the files a search finds, the files below a
//! folder, and the memory a process filled, region by region.

use crate::integrity::{Access, untouched};
use borg_core::{FindingKind, MemoryRegion, ProcessKey, RulesReport, problem_finding, rule_finding};
use borg_rules::{InlineError, Problem, RULE_EXTENSIONS, RuleMatch, Rules, Subject};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs::File,
    io::{self, Read},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

const PAGE: u64 = 4096;
/// Memory read and scanned at once. Each read takes in the page before it again, so a match
/// that crosses from one read into the next is found.
const CHUNK: u64 = 16 * 1024 * 1024;
const OVERLAP: u64 = PAGE;
/// Bytes of memory read before the scan stops short; it also stops after `TIME_LIMIT`.
pub(crate) const SCAN_LIMIT: u64 = 1024 * 1024 * 1024;
/// Files a scan below a folder reads before it stops short, as many as a search lists; it
/// also stops after `TIME_LIMIT`.
pub(crate) const FILE_LIMIT: usize = borg_core::RESULT_LIMIT;
pub(crate) const TIME_LIMIT: Duration = Duration::from_secs(45);
const PROGRESS: Duration = Duration::from_millis(100);
pub(crate) const FINDING_LIMIT: usize = 2000;
/// How deep the rules folder is read.
const FOLDER_DEPTH: usize = 16;

/// The folder of the user's rules, beside the configuration.
pub fn folder() -> Option<PathBuf> {
    crate::config::ConfigDir::locate().map(|dir| dir.rules())
}

/// The rules a scan runs: Borg's with those in the user's folder, or rule text on its own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuleSet {
    Installed { folder: Option<PathBuf> },
    Inline(String),
}

impl RuleSet {
    pub fn compile(&self) -> Result<Rules, InlineError> {
        match self {
            Self::Installed { folder } => Ok(compile(folder.as_deref())),
            Self::Inline(text) => Rules::inline(text),
        }
    }
}

/// A rule's first match in a region of memory, among the bytes read from `base`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hit {
    pub found: RuleMatch,
    pub region: MemoryRegion,
    pub base: u64,
}

impl Hit {
    /// Where the match starts, or the bytes it matched on its condition alone.
    pub fn address(&self) -> u64 {
        self.base + self.found.evidence.as_ref().map_or(0, |evidence| evidence.offset)
    }
}

/// What rules found in the memory a process filled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryScan {
    pub process: ProcessKey,
    /// Worst first, then by address.
    pub hits: Vec<Hit>,
    pub rules: usize,
    /// Rule files left out because they do not compile.
    pub problems: Vec<Problem>,
    /// Bytes of the memory the process filled that were scanned, of `total`.
    pub scanned: u64,
    pub total: u64,
    pub unreadable: u64,
    /// Whether the scan ran to the end. It stops early at a limit or when cancelled.
    pub complete: bool,
}

impl MemoryScan {
    /// The scan as the memory map lists it: each match a finding, the rule files left out last.
    pub fn report(self) -> RulesReport {
        let mut findings: Vec<_> =
            self.hits.iter().map(|hit| rule_finding(&hit.found, &hit.region, hit.base)).collect();
        findings.extend(self.problems.iter().map(problem_finding));
        findings.sort_by_key(|finding| (finding.kind == FindingKind::RuleProblem, finding.severity, finding.address));
        RulesReport {
            process: self.process,
            findings,
            rules: self.rules,
            scanned: self.scanned,
            total: self.total,
            unreadable: self.unreadable,
            complete: self.complete,
        }
    }
}

/// A rule that matched a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileHit {
    pub path: PathBuf,
    pub found: RuleMatch,
}

/// What rules found in a file, or in the files below a folder.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileScan {
    /// Worst first, then by path.
    pub hits: Vec<FileHit>,
    pub rules: usize,
    pub problems: Vec<Problem>,
    /// Files read whole, and those left unread as larger than a contents search reads.
    pub read: usize,
    pub too_large: usize,
    /// Files and folders that could not be read.
    pub unreadable: usize,
    pub complete: bool,
}

/// Scans the file at `root`, or every file below the folder at `root` on its file system,
/// hidden ones included, with `rules`, until `FILE_LIMIT` files, `FINDING_LIMIT` matches or
/// `TIME_LIMIT`.
pub fn scan_files(rules: &Rules, root: &Path, cancelled: &AtomicBool) -> io::Result<FileScan> {
    std::fs::metadata(root)?;
    let mut scan =
        FileScan { rules: rules.count(), problems: rules.problems().to_vec(), complete: true, ..Default::default() };
    let began = Instant::now();
    let mut buffer = Vec::new();
    let walk = ignore::WalkBuilder::new(root)
        .standard_filters(false)
        .same_file_system(true)
        .max_depth(Some(crate::search::MAX_DEPTH))
        .build();
    for entry in walk {
        let Ok(entry) = entry else {
            scan.unreadable += 1;
            continue;
        };
        if !entry.file_type().is_some_and(|kind| kind.is_file()) {
            continue;
        }
        if cancelled.load(Ordering::Relaxed)
            || began.elapsed() >= TIME_LIMIT
            || scan.read + scan.too_large + scan.unreadable >= FILE_LIMIT
            || scan.hits.len() >= FINDING_LIMIT
        {
            scan.complete = false;
            break;
        }
        match scan_file(rules, entry.path(), &mut buffer) {
            Ok(Some(found)) => {
                scan.read += 1;
                scan.hits.extend(found.into_iter().map(|found| FileHit { path: entry.path().to_path_buf(), found }));
            }
            Ok(None) => scan.too_large += 1,
            Err(_) => scan.unreadable += 1,
        }
    }
    scan.hits.sort_by(|a, b| (a.found.level, &a.path).cmp(&(b.found.level, &b.path)));
    if scan.hits.len() > FINDING_LIMIT {
        scan.hits.truncate(FINDING_LIMIT);
        scan.complete = false;
    }
    Ok(scan)
}

/// The rule files in `folder` and the folders in it, by path.
pub fn files(folder: &Path) -> Vec<PathBuf> {
    let rule_file = |path: &Path| {
        let extension = path.extension().and_then(|extension| extension.to_str()).unwrap_or_default();
        RULE_EXTENSIONS.iter().any(|known| extension.eq_ignore_ascii_case(known))
    };
    let mut files: Vec<_> = ignore::WalkBuilder::new(folder)
        .standard_filters(false)
        .follow_links(true)
        .max_depth(Some(FOLDER_DEPTH))
        .build()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_some_and(|kind| kind.is_file()))
        .map(ignore::DirEntry::into_path)
        .filter(|path| rule_file(path))
        .collect();
    files.sort();
    files
}

/// Borg's rules and those in `folder`, when there is one.
pub fn compile(folder: Option<&Path>) -> Rules {
    Rules::compile(&folder.map(files).unwrap_or_default())
}

/// The rules the file at `path` matches, or `None` when it is larger than a contents search
/// reads. `buffer` holds the file's bytes and is kept for the next file. Linux leaves the
/// file's access time as it was where it can.
pub fn scan_file(rules: &Rules, path: &Path, buffer: &mut Vec<u8>) -> io::Result<Option<Vec<RuleMatch>>> {
    let file = open_quietly(path)?;
    if file.metadata()?.len() > borg_core::CONTENTS_SIZE_LIMIT {
        return Ok(None);
    }
    buffer.clear();
    file.take(borg_core::CONTENTS_SIZE_LIMIT + 1).read_to_end(buffer)?;
    if buffer.len() as u64 > borg_core::CONTENTS_SIZE_LIMIT {
        return Ok(None);
    }
    Ok(Some(rules.scan(buffer, Subject { path: &path.to_string_lossy(), memory: false }).matches))
}

#[cfg(target_os = "linux")]
fn open_quietly(path: &Path) -> io::Result<File> {
    use rustix::fs::{Mode, OFlags};
    match rustix::fs::open(path, OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOATIME, Mode::empty()) {
        Ok(file) => Ok(File::from(file)),
        Err(rustix::io::Errno::PERM) => File::open(path),
        Err(error) => Err(error.into()),
    }
}

#[cfg(not(target_os = "linux"))]
fn open_quietly(path: &Path) -> io::Result<File> {
    File::open(path)
}

/// Scans the memory the process filled with `rules`, region by region. A rule matches a
/// region once, at its first match. `program` is the path rules see as the file's.
pub fn scan_memory(
    process: ProcessKey,
    rules: &Rules,
    program: &str,
    regions: &[MemoryRegion],
    access: &impl Access,
    cancelled: &AtomicBool,
    mut progress: impl FnMut(u64, u64, usize),
) -> MemoryScan {
    let own: Vec<_> = regions.iter().filter(|region| region.readable() && region.own()).collect();
    let mut report = MemoryScan {
        process,
        hits: Vec::new(),
        rules: rules.count(),
        problems: rules.problems().to_vec(),
        scanned: 0,
        total: own.iter().map(|region| region.mapped()).sum(),
        unreadable: 0,
        complete: true,
    };
    let subject = Subject { path: program, memory: true };
    let began = Instant::now();
    let mut shown: Option<Instant> = None;
    let mut read = 0;
    'regions: for region in own {
        if untouched(region.resident, region.swap) {
            report.scanned += region.mapped();
            continue;
        }
        let mut matched = HashSet::new();
        let mut at = region.start;
        while at < region.end {
            if cancelled.load(Ordering::Relaxed) || began.elapsed() >= TIME_LIMIT || read >= SCAN_LIMIT {
                report.complete = false;
                break 'regions;
            }
            let end = at.saturating_add(CHUNK).min(region.end);
            let from = at.saturating_sub(OVERLAP).max(region.start);
            if touched(access, from, end) {
                match access.read(from, (end - from) as usize) {
                    Ok(bytes) => {
                        read += bytes.len() as u64;
                        let scanned = rules.scan(&bytes, subject);
                        report.complete &= scanned.complete;
                        for found in scanned.matches {
                            if !matched.insert((found.namespace.clone(), found.rule.clone())) {
                                continue;
                            }
                            if report.hits.len() < FINDING_LIMIT {
                                report.hits.push(Hit { found, region: region.clone(), base: from });
                            } else {
                                report.complete = false;
                            }
                        }
                    }
                    Err(_) => report.unreadable += end - at,
                }
            }
            report.scanned += end - at;
            at = end;
            if shown.is_none_or(|shown| shown.elapsed() >= PROGRESS) {
                shown = Some(Instant::now());
                progress(report.scanned, report.total, report.hits.len());
            }
        }
    }
    report.hits.sort_by_key(|hit| (hit.found.level, hit.address()));
    report
}

/// Whether the process ever touched a page from `from` to `end`, or the system does not say.
fn touched(access: &impl Access, from: u64, end: u64) -> bool {
    let count = (end - from).div_ceil(PAGE) as usize;
    access.pages(from, count).is_none_or(|pages| pages.iter().any(|page| page.present || page.swapped))
}

#[cfg(test)]
mod tests;
