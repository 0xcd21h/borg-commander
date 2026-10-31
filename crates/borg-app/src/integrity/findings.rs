//! Findings from a process's environment and its memory map, with the region facts they rest on.

use super::compare::macho_slice;
use super::{
    Access, CODE_FILES, Facts, INTERPRETERS, KERNEL_PAGES, KERNEL_THREADS, LIBRARY_PATHS, PRELOADS, temporary,
};
use borg_core::{Finding, FindingKind, IntegrityReport, MemoryRegion, Severity};
use borg_i18n::tr;

pub(super) fn size(region: &MemoryRegion) -> u64 {
    region.end.saturating_sub(region.start)
}

pub(super) fn executable(region: &MemoryRegion) -> bool {
    region.permissions.as_bytes().get(2) == Some(&b'x')
}

pub(super) fn writable(region: &MemoryRegion) -> bool {
    region.permissions.as_bytes().get(1) == Some(&b'w')
}

/// Memory no file backs: neither a mapped file nor a kernel page, nor a segment of a macOS
/// image, which the shared cache backs when it names no file.
pub(super) fn anonymous(region: &MemoryRegion) -> bool {
    let kind = region.kind();
    region.path.is_none()
        && !KERNEL_PAGES.contains(&region.name.as_str())
        && !(kind.starts_with("__") || kind.starts_with("unused __") || region.name.contains("dyld shared cache"))
}

/// Readable memory no file backs, where an executable image would be hidden.
pub(super) fn unbacked(region: &MemoryRegion) -> bool {
    region.readable() && anonymous(region)
}

pub(super) fn label(region: &MemoryRegion) -> String {
    if region.name.is_empty() { format!("0x{:x}", region.start) } else { region.kind().to_owned() }
}

pub(super) fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Whether the memory map says none of a region's pages are of the kind counted.
pub(super) fn untouched(pages: Option<u64>, swapped: Option<u64>) -> bool {
    pages == Some(0) && swapped.unwrap_or(0) == 0
}

pub(super) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
}

/// What is wrong with the process as a whole: its program, environment, tracer and name.
pub(super) fn process_findings(facts: &Facts, exists: impl Fn(&str) -> Option<bool>) -> Vec<Finding> {
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
pub(super) fn map_findings(regions: &[MemoryRegion], jit: bool, exists: impl Fn(&str) -> Option<bool>) -> Vec<Finding> {
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
pub(super) struct Stretch<'a> {
    pub(super) region: &'a MemoryRegion,
    pub(super) path: &'a str,
    pub(super) offset: u64,
}

/// The code that can be compared with its files. Linux says where in its file each region
/// starts; on macOS a `__TEXT` segment starts with its image's header, which is at the start
/// of its file or of one slice of a universal file. Code no file of its own holds, such as
/// macOS's shared cache, counts as uncompared.
pub(super) fn stretches<'a>(
    regions: &'a [MemoryRegion],
    access: &impl Access,
    report: &mut IntegrityReport,
) -> Vec<Stretch<'a>> {
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
