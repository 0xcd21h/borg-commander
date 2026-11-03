//! Scanning with rules: the user's rule files, the files a search finds, and the memory a
//! process filled, region by region.

use crate::integrity::{Access, untouched};
use borg_core::{FindingKind, MemoryRegion, ProcessKey, RulesReport, problem_finding, rule_finding};
use borg_rules::{RULE_EXTENSIONS, RuleMatch, Rules, Subject};
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
const SCAN_LIMIT: u64 = 1024 * 1024 * 1024;
const TIME_LIMIT: Duration = Duration::from_secs(45);
const PROGRESS: Duration = Duration::from_millis(100);
const FINDING_LIMIT: usize = 2000;
/// How deep the rules folder is read.
const FOLDER_DEPTH: usize = 16;

/// The folder of the user's rules, beside the configuration.
pub fn folder() -> Option<PathBuf> {
    crate::config::ConfigDir::locate().map(|dir| dir.rules())
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

/// Scans the memory the process filled with `rules`, region by region, and lists the rule
/// files left out after the matches. A rule matches a region once, at its first match.
/// `program` is the path rules see as the file's.
pub fn scan_memory(
    process: ProcessKey,
    rules: &Rules,
    program: &str,
    regions: &[MemoryRegion],
    access: &impl Access,
    cancelled: &AtomicBool,
    mut progress: impl FnMut(u64, u64, usize),
) -> RulesReport {
    let own: Vec<_> = regions.iter().filter(|region| region.readable() && region.own()).collect();
    let mut report = RulesReport {
        process,
        findings: Vec::new(),
        rules: rules.count(),
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
                            if report.findings.len() < FINDING_LIMIT {
                                report.findings.push(rule_finding(&found, region, from));
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
                progress(report.scanned, report.total, report.findings.len());
            }
        }
    }
    report.findings.extend(rules.problems().iter().map(problem_finding));
    report
        .findings
        .sort_by_key(|finding| (finding.kind == FindingKind::RuleProblem, finding.severity, finding.address));
    report
}

/// Whether the process ever touched a page from `from` to `end`, or the system does not say.
fn touched(access: &impl Access, from: u64, end: u64) -> bool {
    let count = (end - from).div_ceil(PAGE) as usize;
    access.pages(from, count).is_none_or(|pages| pages.iter().any(|page| page.present || page.swapped))
}

#[cfg(test)]
mod tests;
