//! What rules find in the memory a process filled: each match as a finding, with the bytes it
//! matched as evidence and what its author says could explain it.

use super::{
    Activity, ActivityPage, ActivityRequest, Finding, FindingKind, IntegrityReport, MemoryRegion, ProcessKey, Severity,
};
use crate::Effect;
use borg_i18n::tr;
use borg_rules::{Level, Problem, RuleMatch};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Characters of matched text shown, and bytes shown as hex when they do not read as text.
const SHOWN_TEXT: usize = 40;
const SHOWN_HEX: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RulesReport {
    pub process: ProcessKey,
    /// Worst first, then by address; rule files left out come last.
    pub findings: Vec<Finding>,
    pub rules: usize,
    /// Bytes of the memory the process filled that were scanned, of `total`.
    pub scanned: u64,
    pub total: u64,
    pub unreadable: u64,
    /// Whether the scan ran to the end, rather than stopping at a limit or on Ctrl-C.
    pub complete: bool,
}

/// The findings of the last check of a process: of its integrity, or of rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FindingsReport {
    Integrity(IntegrityReport),
    Rules(RulesReport),
}

impl FindingsReport {
    pub fn findings(&self) -> &[Finding] {
        match self {
            Self::Integrity(report) => &report.findings,
            Self::Rules(report) => &report.findings,
        }
    }
    pub fn unreadable(&self) -> u64 {
        match self {
            Self::Integrity(report) => report.unreadable,
            Self::Rules(report) => report.unreadable,
        }
    }
    /// The check that found these, to run again.
    pub fn check(&self) -> ActivityRequest {
        match self {
            Self::Integrity(report) => ActivityRequest::Integrity { process: report.process, admin: false },
            Self::Rules(report) => ActivityRequest::Rules { process: report.process, admin: false },
        }
    }
}

impl From<Level> for Severity {
    fn from(level: Level) -> Self {
        match level {
            Level::Alert => Self::Alert,
            Level::Warning => Self::Warning,
            Level::Notice => Self::Notice,
        }
    }
}

/// Where a rule comes from: Borg's own rules, or the name of the user's file.
pub fn rule_source(found: &RuleMatch) -> String {
    if found.built_in() {
        return tr!(rules_source_built_in).into();
    }
    Path::new(&found.namespace)
        .file_name()
        .map_or_else(|| found.namespace.clone(), |name| name.to_string_lossy().into())
}

/// Matched bytes as quoted text when they read as text, and as hex otherwise.
pub fn evidence_text(bytes: &[u8]) -> String {
    if bytes.iter().all(|&byte| byte == b' ' || byte.is_ascii_graphic()) {
        let text = String::from_utf8_lossy(bytes);
        let shown: String = text.chars().take(SHOWN_TEXT).collect();
        let more = if text.chars().count() > SHOWN_TEXT { "…" } else { "" };
        format!("\"{shown}{more}\"")
    } else {
        let shown: Vec<_> = bytes.iter().take(SHOWN_HEX).map(|byte| format!("{byte:02x}")).collect();
        let more = if bytes.len() > SHOWN_HEX { " …" } else { "" };
        format!("{}{more}", shown.join(" "))
    }
}

/// A rule's match among the bytes read from `base` in `region`, with the bytes it matched as
/// evidence, or the start of those bytes when it matched on its condition alone. The detail
/// names the rule and the evidence before what the rule looks for, which can run long.
pub fn rule_finding(found: &RuleMatch, region: &MemoryRegion, base: u64) -> Finding {
    let source = rule_source(found);
    let evidence = found.evidence.as_ref().map(|evidence| {
        let text = evidence_text(&evidence.bytes);
        tr!(detail_rule_evidence, string = evidence.string, text = text)
    });
    let detail =
        [Some(tr!(detail_rule, rule = found.rule, source = source)), evidence, found.about().map(String::from)]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" · ");
    let (address, length) =
        found.evidence.as_ref().map_or((base, 0), |evidence| (base + evidence.offset, evidence.bytes.len()));
    Finding {
        kind: FindingKind::RuleMatch,
        severity: found.level.into(),
        detail,
        region: Some(region.clone()),
        address: Some(address),
        length,
        note: found.caveat().map(String::from),
    }
}

/// A rule file left out, which is about no place in memory.
pub fn problem_finding(problem: &Problem) -> Finding {
    let file = problem.file.display().to_string();
    Finding {
        kind: FindingKind::RuleProblem,
        severity: Severity::Notice,
        detail: tr!(rule_file_left_out_notice, file = file, message = problem.message),
        region: None,
        address: None,
        length: 0,
        note: None,
    }
}

/// Scans the memory the process filled with Borg's rules and the user's; the matches replace
/// the list until Esc goes back to where the scan started.
pub(super) fn scan_with_rules(activity: &mut Activity, process: ProcessKey) -> Vec<Effect> {
    if !matches!(activity.list, ActivityPage::Search | ActivityPage::Findings) {
        activity.results_origin = activity.list;
    }
    activity.search_progress = Some((0, 0, 0));
    activity.request(ActivityRequest::Rules { process, admin: false })
}
