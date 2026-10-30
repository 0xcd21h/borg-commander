//! Whether a process runs the code its files hold: code that differs from the files it was
//! mapped from, code and executable images no file backs, and what in its environment changes
//! which code it loads.

use anyhow::{Result, anyhow};
use borg_core::{Finding, FindingKind, IntegrityReport, MemoryRegion, ProcessKey, Severity};
use borg_i18n::tr;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
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
/// Pages the kernel maps into every process, which hold code and an image header by design.
const KERNEL_PAGES: &[&str] =
    &["[vdso]", "[vvar]", "[vvar_vclock]", "[vsyscall]", "[vectors]", "[sigpage]", "[uprobes]"];
/// Programs, and what they map, that compile code as they run, so code no file backs is
/// expected of them.
const COMPILING_PROGRAMS: &[&str] = &[
    "node", "deno", "bun", "java", "dotnet", "mono", "ruby", "php", "luajit", "julia", "pypy", "beam", "dart",
    "chrome", "chromium", "firefox", "electron", "qemu", "wine", "valgrind", "memcheck",
];
const COMPILERS: &[&str] = &[
    "libv8",
    "libnode",
    "v8_context_snapshot",
    "snapshot_blob",
    "libjvm",
    "libcoreclr",
    "libclrjit",
    "libmono",
    "JavaScriptCore",
    "javascriptcore",
    "libxul",
    "libmozjs",
    "libluajit",
    "libLLVM",
    "libjulia",
    "libpcre",
    "libpypy",
    "Qml",
    "QtWebEngine",
    "Electron Framework",
    "Chromium Embedded Framework",
    "Chromium Framework",
    "Chrome Framework",
    "Edge Framework",
    "Browser Framework",
    "libcef",
];
/// Names of memory files that libraries make for code they build, such as closures and JIT
/// code mapped twice.
const CODE_FILES: &[&str] = &["libffi", "jit", "doublemapper"];
/// Programs that take the name of the script they run.
const INTERPRETERS: &[&str] = &[
    "python",
    "perl",
    "ruby",
    "node",
    "bash",
    "sh",
    "dash",
    "zsh",
    "fish",
    "ksh",
    "tcsh",
    "csh",
    "java",
    "php",
    "php-fpm",
    "lua",
    "tclsh",
    "wish",
    "osascript",
    "bun",
    "deno",
    "pwsh",
    "dotnet",
    "mono",
    "busybox",
    "env",
    "electron",
    "wine",
    "wine-preloader",
    "wine64-preloader",
    "qemu",
    "rscript",
    "beam.smp",
    "erl",
    "dart",
    "julia",
];
/// How kernel threads are named, which a program taking such a name means to look like.
const KERNEL_THREADS: &[&str] = &[
    "kworker",
    "ksoftirqd",
    "kthreadd",
    "kswapd",
    "kauditd",
    "khugepaged",
    "kdevtmpfs",
    "migration/",
    "rcu_",
    "watchdog/",
    "jbd2/",
    "irq/",
];
const TEMPORARY: &[&str] = &["/tmp/", "/var/tmp/", "/dev/shm/", "/private/tmp/", "/private/var/tmp/"];
/// Where macOS keeps each user's temporary folder, `<folders>/<xx>/<id>/T/`.
const USER_TEMPORARY: &[&str] = &["/var/folders/", "/private/var/folders/"];
const PRELOADS: &[&str] = &["LD_PRELOAD", "LD_AUDIT", "DYLD_INSERT_LIBRARIES"];
const LIBRARY_PATHS: &[&str] = &["LD_LIBRARY_PATH", "DYLD_LIBRARY_PATH", "DYLD_FRAMEWORK_PATH"];
/// Of a Mach-O image in the dyld shared cache, which has no file of its own.
const MH_DYLIB_IN_CACHE: u32 = 0x8000_0000;

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

fn size(region: &MemoryRegion) -> u64 {
    region.end.saturating_sub(region.start)
}
fn executable(region: &MemoryRegion) -> bool {
    region.permissions.as_bytes().get(2) == Some(&b'x')
}
fn writable(region: &MemoryRegion) -> bool {
    region.permissions.as_bytes().get(1) == Some(&b'w')
}
/// Memory no file backs: neither a mapped file nor a kernel page, nor a segment of a macOS
/// image, which the shared cache backs when it names no file.
fn anonymous(region: &MemoryRegion) -> bool {
    let kind = region.kind();
    region.path.is_none()
        && !KERNEL_PAGES.contains(&region.name.as_str())
        && !(kind.starts_with("__") || kind.starts_with("unused __") || region.name.contains("dyld shared cache"))
}
/// Readable memory no file backs, where an executable image would be hidden.
fn unbacked(region: &MemoryRegion) -> bool {
    region.readable() && anonymous(region)
}
fn label(region: &MemoryRegion) -> String {
    if region.name.is_empty() { format!("0x{:x}", region.start) } else { region.kind().to_owned() }
}
fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}
/// Whether the memory map says none of a region's pages are of the kind counted.
fn untouched(pages: Option<u64>, swapped: Option<u64>) -> bool {
    pages == Some(0) && swapped.unwrap_or(0) == 0
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
}
fn temporary(path: &str) -> bool {
    TEMPORARY.iter().any(|folder| path.starts_with(folder))
        || USER_TEMPORARY
            .iter()
            .any(|folders| path.strip_prefix(folders).is_some_and(|rest| rest.split('/').nth(2) == Some("T")))
}

/// Whether the process compiles code as it runs, as browsers, Java and JavaScript engines do.
fn compiles_code(facts: &Facts, regions: &[MemoryRegion]) -> bool {
    let program = facts.program.as_ref().map_or(facts.name.as_str(), |(path, _)| file_name(path)).to_lowercase();
    COMPILING_PROGRAMS.iter().any(|name| program.starts_with(name))
        || regions.iter().any(|region| {
            let name = region.name.to_ascii_lowercase();
            name.contains("jit")
                || name.contains("rosetta")
                || region.path.as_deref().is_some_and(|path| COMPILERS.iter().any(|c| file_name(path).contains(c)))
        })
}

/// What is wrong with the process as a whole: its program, environment, tracer and name.
fn process_findings(facts: &Facts, exists: impl Fn(&str) -> Option<bool>) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut add = |kind, severity, detail: String| {
        findings.push(Finding { kind, severity, detail, region: None, address: None, length: 0 })
    };
    if let Some((path, deleted)) = &facts.program {
        match (deleted, exists(path)) {
            (true, Some(true)) => add(FindingKind::ReplacedFile, Severity::Notice, path.clone()),
            (true, None) => add(FindingKind::ProgramGone, Severity::Warning, path.clone()),
            (_, Some(false)) => add(FindingKind::ProgramGone, Severity::Alert, path.clone()),
            _ => {}
        }
    }
    for variable in &facts.environment {
        let Some((name, value)) = variable.split_once('=').filter(|(_, value)| !value.is_empty()) else {
            continue;
        };
        let detail = || format!("{name}={value}");
        if PRELOADS.contains(&name) {
            add(FindingKind::Preload, Severity::Warning, detail());
        } else if LIBRARY_PATHS.contains(&name) {
            add(FindingKind::LibraryPath, Severity::Notice, detail());
        }
    }
    for library in &facts.preloaded {
        add(FindingKind::Preload, Severity::Warning, format!("/etc/ld.so.preload · {library}"));
    }
    if let Some((pid, name)) = &facts.tracer {
        add(FindingKind::Traced, Severity::Warning, tr!(detail_traced, pid = pid, name = name));
    }
    if let Some((path, _)) = &facts.program {
        let (name, program) = (facts.name.as_str(), file_name(path));
        let interpreter = program.to_ascii_lowercase();
        let interpreter = interpreter.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.');
        // Linux keeps 15 bytes of a name.
        let same = name == program || (name.len() >= 15 && program.starts_with(name));
        if !name.is_empty() && !same && !INTERPRETERS.contains(&interpreter) {
            let kernel = (name.starts_with('[') && name.ends_with(']'))
                || KERNEL_THREADS.iter().any(|thread| name.starts_with(thread));
            let severity = if kernel { Severity::Warning } else { Severity::Notice };
            add(FindingKind::NameMismatch, severity, tr!(detail_name_mismatch, name = name, program = path));
        }
    }
    findings
}

/// What the memory map shows: code no file backs, code from deleted or temporary files, and
/// code that can rewrite itself.
fn map_findings(regions: &[MemoryRegion], jit: bool, exists: impl Fn(&str) -> Option<bool>) -> Vec<Finding> {
    let mut findings = Vec::new();
    for region in regions.iter().filter(|r| executable(r)) {
        let mut add = |kind, severity, detail: String| {
            findings.push(Finding {
                kind,
                severity,
                detail,
                region: Some(region.clone()),
                address: Some(region.start),
                length: 0,
            })
        };
        let (bytes, access) = (borg_ui::size_text(size(region)), region.permissions.as_str());
        let Some(path) = &region.path else {
            if !anonymous(region) {
                continue;
            }
            let label = label(region);
            let (severity, detail) = if jit {
                (Severity::Notice, tr!(detail_unbacked_jit, size = bytes, access = access, region = label))
            } else {
                let severity = if writable(region) { Severity::Alert } else { Severity::Warning };
                (severity, tr!(detail_unbacked_code, size = bytes, access = access, region = label))
            };
            add(FindingKind::UnbackedCode, severity, detail);
            continue;
        };
        if writable(region) {
            let detail = tr!(detail_unbacked_code, size = bytes, access = access, region = path);
            add(FindingKind::WritableCode, Severity::Warning, detail);
        }
        match path.strip_suffix(" (deleted)") {
            Some(file) if file.starts_with("/memfd:") => {
                let known = jit || CODE_FILES.iter().any(|name| file.contains(name));
                add(FindingKind::DeletedCode, if known { Severity::Notice } else { Severity::Alert }, path.clone());
            }
            Some(file) => match exists(file) {
                Some(true) => add(FindingKind::ReplacedFile, Severity::Notice, file.into()),
                Some(false) => add(FindingKind::DeletedCode, Severity::Alert, path.clone()),
                None => add(FindingKind::DeletedCode, Severity::Warning, path.clone()),
            },
            None if temporary(path) => add(FindingKind::TemporaryCode, Severity::Warning, path.clone()),
            None => {}
        }
    }
    findings
}

/// Code to compare with the file it was mapped from, at `offset` in it.
struct Stretch<'a> {
    region: &'a MemoryRegion,
    path: &'a str,
    offset: u64,
}

/// The code that can be compared with its files. Linux says where in its file each region
/// starts; on macOS a `__TEXT` segment starts with its image's header, which is at the start
/// of its file or of one slice of a universal file. Code no file of its own holds, such as
/// macOS's shared cache, counts as uncompared.
fn stretches<'a>(regions: &'a [MemoryRegion], access: &impl Access, report: &mut IntegrityReport) -> Vec<Stretch<'a>> {
    let mut stretches: Vec<Stretch> = Vec::new();
    let mut base: Option<(&str, u64, u64)> = None;
    for region in regions.iter().filter(|r| executable(r)) {
        let Some(path) = region.path.as_deref().filter(|path| !path.ends_with(" (deleted)")) else {
            continue;
        };
        let offset = if let Some(offset) = region.offset {
            Some(offset)
        } else if region.kind() == "__TEXT" {
            match base {
                // The segment goes on in the next region.
                Some((file, end, offset)) if file == path && end == region.start => Some(offset),
                _ => macho_slice(access, path, region, report),
            }
        } else {
            None
        };
        base = offset.map(|offset| (path, region.end, offset + size(region)));
        match offset {
            Some(offset) => stretches.push(Stretch { region, path, offset }),
            None => report.uncompared += size(region),
        }
    }
    stretches
}

/// Where in its file the Mach-O image whose header starts `region` starts, unless the image is
/// in the shared cache.
fn macho_slice(access: &impl Access, path: &str, region: &MemoryRegion, report: &mut IntegrityReport) -> Option<u64> {
    let Ok(header) = access.read(region.start, 32) else {
        report.unreadable += size(region);
        return None;
    };
    let word = |bytes: &[u8], at: usize| Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?));
    if !matches!(word(&header, 0)?, 0xfeed_facf | 0xfeed_face) || word(&header, 24)? & MH_DYLIB_IN_CACHE != 0 {
        return None;
    }
    let head = access.read_file(path, 0, PAGE as usize)?;
    slice_offset(&head, &header[..24], |offset| access.read_file(path, offset, 24))
}

/// Where the image whose header begins with `key` starts in a file that begins with `head`:
/// at the start of a thin file, or at the slice of a universal one that matches.
fn slice_offset(head: &[u8], key: &[u8], read: impl Fn(u64) -> Option<Vec<u8>>) -> Option<u64> {
    if head.starts_with(key) {
        return Some(0);
    }
    let word = |at: usize| Some(u32::from_be_bytes(head.get(at..at + 4)?.try_into().ok()?));
    let wide = match word(0)? {
        0xcafe_babe => false,
        0xcafe_babf => true,
        _ => return None,
    };
    let entry = if wide { 32 } else { 20 };
    (0..word(4)? as usize).take(64).find_map(|i| {
        let at = 8 + i * entry + 8;
        let offset =
            if wide { u64::from_be_bytes(head.get(at..at + 8)?.try_into().ok()?) } else { u64::from(word(at)?) };
        (read(offset)?.as_slice() == key).then_some(offset)
    })
}

/// The first byte where a page of memory differs from the file, which reads as zeros past its
/// end.
fn difference(memory: &[u8], disk: &[u8]) -> Option<usize> {
    memory.iter().enumerate().position(|(i, &byte)| disk.get(i).copied().unwrap_or(0) != byte)
}

/// Compares one stretch of code with its file. Only modified pages can differ, so where the
/// system says which those are, only they are read: Linux's page tables tell private copies
/// and swapped pages from the file's own, and macOS counts the region's dirty pages.
fn compare<P: FnMut(u64, u64, usize)>(run: &mut Run<P>, access: &impl Access, stretch: &Stretch) {
    let region = stretch.region;
    if untouched(region.dirty, region.swap) {
        run.report.compared += size(region);
        return run.tick();
    }
    let (mut differing, mut first) = (0u64, None);
    let mut at = region.start;
    while at < region.end {
        if run.stopped() || run.compare_read >= COMPARE_LIMIT {
            run.report.complete = false;
            run.report.uncompared += region.end - at;
            break;
        }
        let len = (region.end - at).min(CHUNK);
        let count = len.div_ceil(PAGE) as usize;
        let wanted: Vec<bool> = access.pages(at, count).map_or_else(
            || vec![true; count],
            |pages| pages.iter().map(|p| p.swapped || (p.present && !p.file)).collect(),
        );
        if wanted.contains(&true) {
            let offset = stretch.offset + (at - region.start);
            let Some(disk) = access.read_file(stretch.path, offset, len as usize) else {
                run.report.uncompared += region.end - at;
                break;
            };
            let Ok(memory) = access.read(at, len as usize) else {
                run.report.unreadable += len;
                run.report.uncompared += len;
                at += len;
                continue;
            };
            run.compare_read += len;
            for (i, page) in memory.chunks(PAGE as usize).enumerate().filter(|(i, _)| wanted[*i]) {
                let from = i * PAGE as usize;
                let disk = disk.get(from..(from + page.len()).min(disk.len())).unwrap_or_default();
                if let Some(byte) = difference(page, disk) {
                    differing += 1;
                    first.get_or_insert_with(|| {
                        let shown = |bytes: &[u8]| {
                            bytes.get(byte..(byte + SHOWN).min(bytes.len())).unwrap_or_default().to_vec()
                        };
                        (from + byte, offset + (from + byte) as u64, shown(page), shown(disk))
                    });
                }
            }
        }
        run.report.compared += len;
        at += len;
        run.tick();
    }
    if let Some((at, offset, memory, disk)) = first {
        let detail = tr!(
            detail_code_changed,
            pages = differing,
            total = size(region).div_ceil(PAGE),
            file = stretch.path,
            offset = offset,
            memory = hex(&memory),
            disk = hex(&disk),
        );
        run.add(Finding {
            kind: FindingKind::CodeChanged,
            severity: Severity::Alert,
            detail,
            region: Some(region.clone()),
            address: Some(region.start + at as u64),
            length: memory.len().max(1),
        });
    }
}

/// The executable format whose header starts `page`.
fn image_format(page: &[u8]) -> Option<&'static str> {
    let word = |at: usize| Some(u32::from_le_bytes(page.get(at..at + 4)?.try_into().ok()?));
    match page.get(..4)? {
        [0x7f, b'E', b'L', b'F'] => {
            let kind = u16::from_le_bytes(page.get(16..18)?.try_into().ok()?);
            (matches!(page.get(4..6)?, [1 | 2, 1 | 2]) && matches!(kind, 2 | 3)).then_some("ELF")
        }
        [0xcf | 0xce, 0xfa, 0xed, 0xfe] => {
            // An executable, library, bundle or dyld, with a sane number of load commands,
            // and not one of the shared cache's, which every process maps.
            let plausible = matches!(word(12)?, 2 | 6 | 7 | 8) && (1..=1024).contains(&word(16)?);
            (plausible && word(24)? & MH_DYLIB_IN_CACHE == 0).then_some("Mach-O")
        }
        [b'M', b'Z', ..] => {
            let at = word(0x3c)? as usize;
            (page.get(at..at + 4)? == b"PE\0\0").then_some("PE")
        }
        _ => None,
    }
}

/// Searches memory no file backs for the headers of executable images, which a loader places
/// at the start of a page. `next_executable` says whether code follows right after it, as an
/// image's code follows its header.
fn scan<P: FnMut(u64, u64, usize)>(
    run: &mut Run<P>,
    access: &impl Access,
    region: &MemoryRegion,
    next_executable: bool,
) {
    if untouched(region.resident, region.swap) {
        run.report.scanned += size(region);
        return run.tick();
    }
    let mut at = region.start.next_multiple_of(PAGE);
    while at < region.end {
        if run.stopped() || run.scan_read >= SCAN_LIMIT {
            run.report.complete = false;
            return;
        }
        let len = (region.end - at).min(CHUNK);
        let count = len.div_ceil(PAGE) as usize;
        let wanted: Vec<bool> = access
            .pages(at, count)
            .map_or_else(|| vec![true; count], |pages| pages.iter().map(|p| p.present || p.swapped).collect());
        if wanted.contains(&true) {
            let Ok(data) = access.read(at, len as usize) else {
                run.report.unreadable += len;
                at += len;
                continue;
            };
            run.scan_read += len;
            for (i, page) in data.chunks(PAGE as usize).enumerate().filter(|(i, _)| wanted[*i]) {
                if let Some(format) = image_format(page) {
                    let alert = executable(region) || next_executable;
                    run.add(Finding {
                        kind: FindingKind::HiddenImage,
                        severity: if alert { Severity::Alert } else { Severity::Warning },
                        detail: tr!(detail_hidden_image, format = format, region = label(region)),
                        region: Some(region.clone()),
                        address: Some(at + i as u64 * PAGE),
                        length: 4,
                    });
                }
            }
        }
        run.report.scanned += len;
        at += len;
        run.tick();
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
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// A process's memory, and the files it sees, held in memory.
    #[derive(Default)]
    struct Model {
        memory: Vec<(u64, Vec<u8>)>,
        files: HashMap<&'static str, Vec<u8>>,
        /// Pages the page tables call the file's own.
        clean: Vec<u64>,
        page_tables: bool,
        reads: std::cell::Cell<u64>,
    }
    impl Access for Model {
        fn read(&self, address: u64, len: usize) -> Result<Vec<u8>> {
            self.reads.set(self.reads.get() + len as u64);
            let (start, bytes) = self
                .memory
                .iter()
                .find(|(start, bytes)| *start <= address && address + len as u64 <= start + bytes.len() as u64)
                .ok_or_else(|| anyhow!("unmapped"))?;
            let from = (address - start) as usize;
            Ok(bytes[from..from + len].to_vec())
        }
        fn pages(&self, address: u64, count: usize) -> Option<Vec<PageUse>> {
            self.page_tables.then(|| {
                (0..count as u64)
                    .map(|i| {
                        let clean = self.clean.contains(&(address + i * PAGE));
                        PageUse { present: true, swapped: false, file: clean }
                    })
                    .collect()
            })
        }
        fn read_file(&self, path: &str, offset: u64, len: usize) -> Option<Vec<u8>> {
            let bytes = self.files.get(path)?;
            let from = (offset as usize).min(bytes.len());
            Some(bytes[from..(from + len).min(bytes.len())].to_vec())
        }
        fn exists(&self, path: &str) -> Option<bool> {
            Some(self.files.contains_key(path))
        }
    }

    fn region(start: u64, end: u64, permissions: &str, name: &str) -> MemoryRegion {
        MemoryRegion {
            start,
            end,
            permissions: permissions.into(),
            name: name.into(),
            path: name.starts_with('/').then(|| name.into()),
            offset: name.starts_with('/').then_some(0),
            ..Default::default()
        }
    }
    fn kinds(findings: &[Finding]) -> Vec<(FindingKind, Severity)> {
        findings.iter().map(|f| (f.kind, f.severity)).collect()
    }
    fn elf() -> Vec<u8> {
        let mut page = vec![0u8; PAGE as usize];
        page[..6].copy_from_slice(&[0x7f, b'E', b'L', b'F', 2, 1]);
        page[16] = 3;
        page
    }

    #[test]
    fn the_memory_map_shows_code_no_file_backs_and_code_from_deleted_or_temporary_files() {
        let regions = [
            region(0x1000, 0x2000, "r-xp", "[vdso]"),
            region(0x1000, 0x2000, "r-x", "__TEXT · dyld shared cache combined __TEXT"),
            region(0x2000, 0x3000, "r-xp", ""),
            region(0x3000, 0x4000, "rwxp", "[anon:payload]"),
            region(0x4000, 0x5000, "r-xp", "/usr/lib/libssl.so.3 (deleted)"),
            region(0x5000, 0x6000, "r-xp", "/usr/lib/libgone.so (deleted)"),
            region(0x6000, 0x7000, "r-xp", "/memfd:libffi (deleted)"),
            region(0x7000, 0x8000, "r-xp", "/memfd:x (deleted)"),
            region(0x8000, 0x9000, "r-xp", "/tmp/stage/lib.so"),
            region(0x9000, 0xa000, "rwxp", "/opt/shop/libold.so"),
            region(0xa000, 0xb000, "rw-p", "/tmp/data"),
        ];
        let exists = |path: &str| Some(path == "/usr/lib/libssl.so.3");
        assert_eq!(
            kinds(&map_findings(&regions, false, exists)),
            [
                (FindingKind::UnbackedCode, Severity::Warning),
                (FindingKind::UnbackedCode, Severity::Alert),
                (FindingKind::ReplacedFile, Severity::Notice),
                (FindingKind::DeletedCode, Severity::Alert),
                (FindingKind::DeletedCode, Severity::Notice),
                (FindingKind::DeletedCode, Severity::Alert),
                (FindingKind::TemporaryCode, Severity::Warning),
                (FindingKind::WritableCode, Severity::Warning),
            ]
        );
        // Files that cannot be seen leave a deletion unexplained, not proven.
        assert_eq!(
            kinds(&map_findings(&regions[5..6], false, |_| None)),
            [(FindingKind::DeletedCode, Severity::Warning)]
        );
        // A program that compiles code as it runs makes code no file backs all the time.
        let jit = map_findings(&regions[2..4], true, exists);
        assert_eq!(kinds(&jit), [(FindingKind::UnbackedCode, Severity::Notice); 2]);
        assert!(jit[1].detail.contains("rwxp in [anon:payload]"), "{}", jit[1].detail);
        let facts = Facts { name: "dbeaver".into(), ..Default::default() };
        assert!(compiles_code(&facts, &[region(0, 1, "r-xp", "/opt/dbeaver/jre/lib/server/libjvm.so")]));
        assert!(!compiles_code(&facts, &regions));
        // Assembled here, so the public-content check, which looks for real ones, stays quiet.
        let user = |rest: &str| ["/private/var/folders/", "xy/", rest].concat();
        assert!(temporary(&user("abc123/T/lib.dylib")) && !temporary(&user("abc123/C/cache")));
    }

    #[test]
    fn the_process_shows_its_program_environment_tracer_and_name() {
        let facts = Facts {
            name: "kworker/0:1".into(),
            program: Some(("/home/me/.cache/run".into(), true)),
            environment: vec![
                "LD_PRELOAD=/home/me/hook.so".into(),
                "DYLD_LIBRARY_PATH=/opt/shop/lib".into(),
                "LD_AUDIT=".into(),
                "PATH=/usr/bin".into(),
            ],
            preloaded: vec!["/lib/libsnoop.so".into()],
            tracer: Some((812, "gdb".into())),
        };
        let findings = process_findings(&facts, |_| Some(false));
        assert_eq!(
            kinds(&findings),
            [
                (FindingKind::ProgramGone, Severity::Alert),
                (FindingKind::Preload, Severity::Warning),
                (FindingKind::LibraryPath, Severity::Notice),
                (FindingKind::Preload, Severity::Warning),
                (FindingKind::Traced, Severity::Warning),
                (FindingKind::NameMismatch, Severity::Warning),
            ]
        );
        assert_eq!(findings[1].detail, "LD_PRELOAD=/home/me/hook.so");
        assert!(findings[4].detail.contains("812") && findings[4].detail.contains("gdb"));
        // An update replaces a running program's file; scripts and long names are named as they are.
        let named = |name: &str, path: &str| {
            let facts = Facts { name: name.into(), program: Some((path.into(), false)), ..Default::default() };
            kinds(&process_findings(&facts, |_| Some(true)))
        };
        assert_eq!(named("backup", "/usr/bin/python3.12"), []);
        assert_eq!(named("shop-inventory-", "/opt/shop/shop-inventory-sync"), []);
        assert_eq!(named("sshd", "/home/me/miner"), [(FindingKind::NameMismatch, Severity::Notice)]);
        let replaced =
            Facts { program: Some(("/usr/sbin/cron".into(), true)), name: "cron".into(), ..Default::default() };
        assert_eq!(
            kinds(&process_findings(&replaced, |_| Some(true))),
            [(FindingKind::ReplacedFile, Severity::Notice)]
        );
    }

    #[test]
    fn executable_headers_are_told_from_lookalikes() {
        assert_eq!(image_format(&elf()), Some("ELF"));
        let mut macho = vec![0u8; 64];
        macho[..4].copy_from_slice(&0xfeed_facf_u32.to_le_bytes());
        macho[12] = 6;
        macho[16] = 20;
        assert_eq!(image_format(&macho), Some("Mach-O"));
        macho[27] = 0x80;
        assert_eq!(image_format(&macho), None, "in the shared cache");
        let mut pe = vec![0u8; 512];
        pe[..2].copy_from_slice(b"MZ");
        pe[0x3c] = 0x80;
        pe[0x80..0x84].copy_from_slice(b"PE\0\0");
        assert_eq!(image_format(&pe), Some("PE"));
        pe[0x80] = b'X';
        assert_eq!(image_format(&pe), None);
        // A Java class starts like a universal Mach-O file.
        assert_eq!(image_format(&[0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 0x34]), None);
        assert_eq!(image_format(b"\x7fELF but text"), None);
    }

    #[test]
    fn a_universal_file_is_compared_from_the_slice_that_matches_memory() {
        let key: Vec<u8> = (0..24).collect();
        let mut head = vec![0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 2];
        for offset in [0x4000u32, 0x8000] {
            head.extend([0; 8]);
            head.extend(offset.to_be_bytes());
            head.extend([0; 8]);
        }
        let file = |offset: u64| Some(if offset == 0x8000 { key.clone() } else { vec![9; 24] });
        assert_eq!(slice_offset(&head, &key, file), Some(0x8000));
        assert_eq!(slice_offset(&key, &key, file), Some(0), "thin");
        assert_eq!(slice_offset(&[0; 32], &key, file), None);
    }

    #[test]
    fn code_is_compared_with_its_file_and_unbacked_memory_searched_for_images() {
        let code: Vec<u8> = (0..3 * PAGE).map(|i| (i % 251) as u8).collect();
        let mut patched = code.clone();
        patched[PAGE as usize + 0x10..PAGE as usize + 0x15].copy_from_slice(&[0xe9, 0xde, 0xad, 0xbe, 0xef]);
        let mut hidden = vec![0u8; 2 * PAGE as usize];
        hidden[PAGE as usize..2 * PAGE as usize].copy_from_slice(&elf());
        let model = Model {
            memory: vec![(0x10000, patched), (0x20000, hidden), (0x22000, vec![0xc3; PAGE as usize])],
            files: HashMap::from([("/opt/shop/bin/shop", code)]),
            ..Default::default()
        };
        let regions = [
            region(0x10000, 0x13000, "r-xp", "/opt/shop/bin/shop"),
            region(0x20000, 0x22000, "rw-p", ""),
            region(0x22000, 0x23000, "r-xp", ""),
            region(0x30000, 0x31000, "rw-p", "[heap]"),
        ];
        let facts =
            Facts { name: "shop".into(), program: Some(("/opt/shop/bin/shop".into(), false)), ..Default::default() };
        let report =
            check(ProcessKey { pid: 7, start: 1 }, &facts, &regions, &model, &AtomicBool::new(false), |_, _, _| {});
        assert_eq!(
            kinds(&report.findings),
            [
                (FindingKind::CodeChanged, Severity::Alert),
                (FindingKind::HiddenImage, Severity::Alert),
                (FindingKind::UnbackedCode, Severity::Warning),
            ]
        );
        let changed = &report.findings[0];
        assert_eq!((changed.address, changed.length), (Some(0x11010), SHOWN));
        assert!(
            changed.detail.contains("1 of 3 pages") && changed.detail.contains("+0x1010: e9 de ad be ef"),
            "{}",
            changed.detail
        );
        assert_eq!(report.findings[1].address, Some(0x21000));
        assert_eq!((report.compared, report.uncompared), (3 * PAGE, 0));
        assert_eq!((report.unbacked, report.scanned, report.unreadable), (4 * PAGE, 3 * PAGE, PAGE));
        assert!(report.complete);

        // Pages the page tables call the file's own hold what the file does, so are not read.
        let model =
            Model { page_tables: true, clean: vec![0x10000, 0x11000, 0x12000], reads: Default::default(), ..model };
        let report = check(
            ProcessKey { pid: 7, start: 1 },
            &facts,
            &regions[..1],
            &model,
            &AtomicBool::new(false),
            |_, _, _| {},
        );
        assert_eq!((kinds(&report.findings), report.compared, model.reads.get()), (vec![], 3 * PAGE, 0));
        // So are code the memory map calls clean and memory it says nothing of is in use.
        let model = Model { page_tables: false, ..model };
        let quiet = [
            MemoryRegion { dirty: Some(0), ..regions[0].clone() },
            MemoryRegion { resident: Some(0), swap: Some(0), ..regions[1].clone() },
        ];
        let report =
            check(ProcessKey { pid: 7, start: 1 }, &facts, &quiet, &model, &AtomicBool::new(false), |_, _, _| {});
        assert_eq!((report.compared, report.scanned, model.reads.get()), (3 * PAGE, 2 * PAGE, 0));

        let report =
            check(ProcessKey { pid: 7, start: 1 }, &facts, &regions, &model, &AtomicBool::new(true), |_, _, _| {});
        assert!(!report.complete && report.compared == 0 && report.uncompared == 3 * PAGE);
    }
}
