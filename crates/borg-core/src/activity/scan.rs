//! What an agent's scan of a process runs, as the approval dialog shows it.

use crate::{Ask, AskKind, DiffLine};
use borg_i18n::tr;
use std::path::Path;

/// The checks a scan of a process runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanChecks {
    Integrity,
    Rules,
    Both,
}

impl ScanChecks {
    pub fn integrity(self) -> bool {
        matches!(self, Self::Integrity | Self::Both)
    }
    pub fn rules(self) -> bool {
        matches!(self, Self::Rules | Self::Both)
    }
}

/// The process a scan reads, as the dialog names it.
#[derive(Debug, Clone, Copy)]
pub struct ScanSubject<'a> {
    pub name: &'a str,
    pub pid: u32,
    pub program: Option<&'a Path>,
}

/// What the approval dialog shows when an agent asks to scan a process's memory: the process,
/// its program, the checks, and the agent's own rule text when it brings some. `place` says
/// who asked.
pub fn scan_ask(subject: ScanSubject<'_>, checks: ScanChecks, inline: Option<&str>, place: String) -> Ask {
    let mut body = vec![DiffLine::plain(tr!(agent_memory_process, name = subject.name, pid = subject.pid))];
    if let Some(program) = subject.program {
        body.push(DiffLine::plain(tr!(agent_memory_program, path = program.display())));
    }
    let inline = inline.filter(|_| checks.rules());
    let rules = if inline.is_some() { tr!(agent_scan_inline) } else { tr!(agent_scan_rules) };
    let names: Vec<_> = [checks.integrity().then_some(tr!(agent_scan_integrity)), checks.rules().then_some(rules)]
        .into_iter()
        .flatten()
        .collect();
    body.extend([
        DiffLine::plain(tr!(agent_scan_checks, checks = names.join(" · "))),
        DiffLine::plain(""),
        DiffLine::plain(tr!(agent_scan_warning, bytes = borg_rules::EVIDENCE_BYTES)),
    ]);
    if let Some(text) = inline {
        body.push(DiffLine::plain(""));
        body.extend(text.lines().map(DiffLine::plain));
    }
    let summary = tr!(process_control_subject, name = subject.name, pid = subject.pid);
    Ask { kind: AskKind::Scan, summary, place, body }
}
