//! What an agent asks to scan, read from its call, and what the scan found, as the agent reads
//! it: each finding with its rule, place and evidence, then how much was read.

use super::Reply;
use crate::activity::ProcessScan;
use crate::rule_scan::{FINDING_LIMIT, FileScan, Hit, MemoryScan};
use borg_core::{Finding, FindingKind, IntegrityReport, MemoryRegion, ScanChecks, Severity};
use borg_rules::{INLINE_LIMIT, InlineError, Problem, RuleMatch};
use borg_ui::size_text;
use serde_json::Value;
use std::{fmt::Write, path::Path};

/// Findings an answer shows unless the call asks for another number.
const SHOWN: usize = 100;
const RULES: &str = "rules";
const INTEGRITY: &str = "integrity";
const BOTH: &str = "both";

/// What a scan reads: a file or folder, by its location, or a running process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanTarget {
    Location(String),
    Process(u32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanCall {
    pub target: ScanTarget,
    pub checks: ScanChecks,
    /// YARA rule text that runs in place of Borg's rules and the user's.
    pub rules: Option<String>,
    pub limit: usize,
    pub admin: bool,
}

/// Reads a scan call's arguments, or says what is wrong with them.
pub fn scan_call(args: &Value) -> Result<ScanCall, String> {
    let path = args["path"].as_str().map(str::trim).filter(|path| !path.is_empty());
    let target = match (path, &args["pid"]) {
        (Some(_), pid) if !pid.is_null() => return Err("scan takes either a path or a pid".into()),
        (Some(path), _) => ScanTarget::Location(path.to_owned()),
        (None, Value::Null) => {
            return Err("scan needs a path to a file or folder, or the pid of a running process".into());
        }
        (None, pid) => ScanTarget::Process(
            pid.as_u64()
                .and_then(|pid| u32::try_from(pid).ok())
                .filter(|&pid| pid > 0)
                .ok_or("pid must be the PID of a running process")?,
        ),
    };
    let process = matches!(target, ScanTarget::Process(_));
    let checks = match &args["check"] {
        Value::Null if process => ScanChecks::Both,
        Value::Null => ScanChecks::Rules,
        value => match value.as_str().unwrap_or_default() {
            RULES => ScanChecks::Rules,
            INTEGRITY => ScanChecks::Integrity,
            BOTH => ScanChecks::Both,
            other => return Err(format!("scan has no check {other:?}: use rules, integrity or both")),
        },
    };
    if !process && checks.integrity() {
        return Err("integrity checks a running process: give its pid, or check \"rules\" for files".into());
    }
    let rules = match &args["rules"] {
        Value::Null => None,
        Value::String(text) if !text.trim().is_empty() => Some(text.clone()),
        _ => return Err("rules must be YARA rule text".into()),
    };
    if rules.is_some() && !checks.rules() {
        return Err("rules run only with check \"rules\" or \"both\"".into());
    }
    if rules.as_ref().is_some_and(|text| text.len() > INLINE_LIMIT) {
        return Err(rule_text_problem(&InlineError::TooLong));
    }
    let admin = args["admin"] == true;
    if admin && !process {
        return Err("admin reads a process as administrator, so it needs a pid".into());
    }
    let limit = args["limit"].as_u64().map_or(SHOWN, |limit| limit.clamp(1, FINDING_LIMIT as u64) as usize);
    Ok(ScanCall { target, checks, rules, limit, admin })
}

/// Why the agent's own rule text cannot run.
pub fn rule_text_problem(error: &InlineError) -> String {
    format!("rules do not compile: {error}")
}

/// What rules found in a file or below a folder.
pub fn files_answer(scan: &FileScan, root: &Path, limit: usize) -> Reply {
    let mut text = format!(
        "Rules over {} · {} · {} read · {} too large to read · {} unreadable · {}\n",
        root.display(),
        counted(scan.rules, "rule", "rules"),
        counted(scan.read, "file", "files"),
        scan.too_large,
        scan.unreadable,
        finished(scan.complete),
    );
    let shown = scan.hits.len().min(limit);
    match scan.hits.len() {
        0 => text.push_str("No rule matched. Rules find only what they describe.\n"),
        found => {
            let _ = writeln!(text, "{}, worst first{}:", counted(found, "match", "matches"), showing(found, shown));
        }
    }
    for (number, hit) in (1..).zip(&scan.hits[..shown]) {
        let _ = writeln!(text, "{number}. {}", rule_heading(&hit.found));
        let _ = writeln!(text, "   file: {}", hit.path.display());
        rule_lines(&mut text, &hit.found, |offset| format!("offset {offset}"));
    }
    problem_lines(&mut text, &scan.problems);
    Reply::ok(text)
}

/// What the checks of a process found, worst first.
pub fn process_answer(scan: &ProcessScan, name: &str, pid: u32, limit: usize, admin: bool) -> Reply {
    let mut text = format!("{name} (PID {pid})\n");
    if let Some(report) = &scan.integrity {
        let _ = writeln!(text, "{}", integrity_summary(report));
    }
    if let Some(scan) = &scan.rules {
        let _ = writeln!(text, "{}", rules_summary(scan));
    }
    let mut findings: Vec<(Severity, Option<u64>, String)> = Vec::new();
    for finding in scan.integrity.iter().flat_map(|report| &report.findings) {
        findings.push((finding.severity, finding.address, integrity_finding(finding)));
    }
    for hit in scan.rules.iter().flat_map(|scan| &scan.hits) {
        findings.push((hit.found.level.into(), Some(hit.address()), rule_hit(hit)));
    }
    findings.sort_by_key(|(severity, address, _)| (*severity, address.is_some(), *address));
    let shown = findings.len().min(limit);
    match findings.len() {
        0 => text.push_str("Nothing found. That proves little: checks find only what they look for.\n"),
        found => {
            let _ = writeln!(text, "{}, worst first{}:", counted(found, "finding", "findings"), showing(found, shown));
        }
    }
    for (number, (.., finding)) in (1..).zip(&findings[..shown]) {
        let _ = write!(text, "{number}. {finding}");
    }
    if let Some(scan) = &scan.rules {
        problem_lines(&mut text, &scan.problems);
    }
    let unreadable = scan.integrity.as_ref().map_or(0, |report| report.unreadable)
        + scan.rules.as_ref().map_or(0, |scan| scan.unreadable);
    if unreadable > 0 && !admin {
        text.push_str("Some memory could not be read as the user: admin true reads it as administrator.\n");
    }
    Reply::ok(text)
}

fn integrity_summary(report: &IntegrityReport) -> String {
    format!(
        "Integrity: {} of code compared with its files and {} left uncompared · {} of {} memory no file backs searched for executable images · {} unreadable · {}",
        size_text(report.compared),
        size_text(report.uncompared),
        size_text(report.scanned),
        size_text(report.unbacked),
        size_text(report.unreadable),
        finished(report.complete),
    )
}

fn rules_summary(scan: &MemoryScan) -> String {
    format!(
        "Rules: {} · {} of {} the process filled scanned · {} unreadable · {}",
        counted(scan.rules, "rule", "rules"),
        size_text(scan.scanned),
        size_text(scan.total),
        size_text(scan.unreadable),
        finished(scan.complete),
    )
}

fn integrity_finding(finding: &Finding) -> String {
    let mut text = format!("{} · {}\n", severity_name(finding.severity), kind_name(finding.kind));
    if let Some(region) = &finding.region {
        let address = finding.address.unwrap_or(region.start);
        let _ = writeln!(text, "   at: {address:#x}, {} bytes, in {}", finding.length, region_text(region));
    }
    let _ = writeln!(text, "   detail: {}", finding.detail);
    let _ = writeln!(text, "   could also be: {}", finding.caveat());
    text
}

fn rule_hit(hit: &Hit) -> String {
    let mut text = format!("{}\n", rule_heading(&hit.found));
    let _ = writeln!(text, "   in: {}", region_text(&hit.region));
    rule_lines(&mut text, &hit.found, |offset| format!("{:#x}", hit.base + offset));
    text
}

/// Where a rule matched, the bytes it matched, what it looks for and what else could explain it.
fn rule_lines(text: &mut String, found: &RuleMatch, place: impl Fn(u64) -> String) {
    match &found.evidence {
        Some(evidence) => {
            let _ = writeln!(text, "   at: {}, {} bytes", place(evidence.offset), evidence.length);
            let cut = if (evidence.bytes.len() as u64) < evidence.length { " (first bytes)" } else { "" };
            let _ = writeln!(text, "   evidence {}{cut}: {}", evidence.string, evidence_text(&evidence.bytes));
        }
        None => text.push_str("   at: the rule matched on its condition alone\n"),
    }
    if let Some(about) = found.about() {
        let _ = writeln!(text, "   looks for: {about}");
    }
    let caveat = found.caveat().unwrap_or(FindingKind::RuleMatch.caveat());
    let _ = writeln!(text, "   could also be: {caveat}");
}

fn problem_lines(text: &mut String, problems: &[Problem]) {
    if problems.is_empty() {
        return;
    }
    text.push_str("Rule files left out because they do not compile:\n");
    for problem in problems {
        let _ = writeln!(text, "  {}: {}", problem.file.display(), problem.message);
    }
}

/// Matched bytes as quoted text when they read as text, and as hex otherwise.
pub(super) fn evidence_text(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) if text.chars().all(|c| !c.is_control() || matches!(c, '\t' | '\n' | '\r')) => format!("{text:?}"),
        _ => bytes.iter().map(|byte| format!("{byte:02x}")).collect::<Vec<_>>().join(" "),
    }
}

/// A rule's match as a finding: how bad, and the rule with where it comes from, which is
/// `borg` for Borg's own rules, the path of the user's file, or `inline` for the agent's text.
fn rule_heading(found: &RuleMatch) -> String {
    format!(
        "{} · {} · rule {} from {}",
        severity_name(found.level.into()),
        kind_name(FindingKind::RuleMatch),
        found.rule,
        found.namespace
    )
}

fn region_text(region: &MemoryRegion) -> String {
    format!("{} {} {:#x}-{:#x}", region.name, region.permissions, region.start, region.end)
}

fn counted(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

fn showing(found: usize, shown: usize) -> String {
    if shown < found { format!("; showing {shown}") } else { String::new() }
}

fn finished(complete: bool) -> &'static str {
    if complete { "finished" } else { "stopped before the end" }
}

/// A severity as the agent reads it, the same in every language.
fn severity_name(severity: Severity) -> &'static str {
    match severity {
        Severity::Alert => "alert",
        Severity::Warning => "warning",
        Severity::Notice => "notice",
    }
}

/// A finding's kind as the agent reads it, the same in every language.
fn kind_name(kind: FindingKind) -> &'static str {
    match kind {
        FindingKind::HiddenImage => "hidden_image",
        FindingKind::CodeChanged => "code_changed",
        FindingKind::UnbackedCode => "unbacked_code",
        FindingKind::WritableCode => "writable_code",
        FindingKind::DeletedCode => "deleted_code",
        FindingKind::TemporaryCode => "temporary_code",
        FindingKind::ReplacedFile => "replaced_file",
        FindingKind::Preload => "preload",
        FindingKind::LibraryPath => "library_path",
        FindingKind::Traced => "traced",
        FindingKind::ProgramGone => "program_gone",
        FindingKind::NameMismatch => "name_mismatch",
        FindingKind::RuleMatch => "rule_match",
        FindingKind::RuleProblem => "rule_problem",
    }
}

#[cfg(test)]
mod tests;
