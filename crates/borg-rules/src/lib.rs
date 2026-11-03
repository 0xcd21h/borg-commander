//! YARA rules over files and memory: the rules Borg ships and the user's own, compiled
//! together, and what they match, with the evidence and what else could explain it.

use boreal::{
    Compiler, MetadataValue, Scanner,
    compiler::{CompilerParams, generate_description},
    scanner::{EvaluatedRule, ScanError, ScanParams},
};
use borg_i18n::tr;
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

/// The namespace of the rules Borg ships.
pub const BUILT_IN: &str = "borg";
const BUILT_IN_RULES: &str = include_str!("../rules/borg.yar");
/// The namespace of rule text compiled on its own, and the name its compiler messages give it.
pub const INLINE: &str = "inline";
/// The longest rule text compiled on its own, in bytes.
pub const INLINE_LIMIT: usize = 64 * 1024;
/// Name extensions of rule files.
pub const RULE_EXTENSIONS: &[&str] = &["yar", "yara"];
/// Bytes of a match kept as evidence.
pub const EVIDENCE_BYTES: usize = 64;
/// How long one scan of a file or a stretch of memory may take.
pub const SCAN_TIME: Duration = Duration::from_secs(10);

/// Variables public rule sets expect for each file: its name, path, extension, type and owner.
const FILENAME: &str = "filename";
const FILEPATH: &str = "filepath";
const EXTENSION: &str = "extension";
const FILETYPE: &str = "filetype";
const OWNER: &str = "owner";
const VARIABLES: [&str; 5] = [FILENAME, FILEPATH, EXTENSION, FILETYPE, OWNER];

const SEVERITY: &str = "severity";
const SCORE: &str = "score";
const DESCRIPTION: &str = "description";
const FALSE_POSITIVES: &[&str] = &["false_positives", "false_positive"];
/// Scores as THOR and signature-base give them: from 80 an alert, from 60 a warning.
const ALERT_SCORE: i64 = 80;
const WARNING_SCORE: i64 = 60;

/// How much a match matters, worst first, as the rule says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Level {
    Alert,
    Warning,
    Notice,
}

impl Level {
    fn named(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "alert" | "critical" | "high" | "malicious" => Some(Self::Alert),
            "warning" | "medium" | "suspicious" => Some(Self::Warning),
            "notice" | "low" | "info" | "informational" => Some(Self::Notice),
            _ => None,
        }
    }
    fn scored(score: i64) -> Self {
        match score {
            ALERT_SCORE.. => Self::Alert,
            WARNING_SCORE.. => Self::Warning,
            _ => Self::Notice,
        }
    }
}

/// A rule file left out because it does not compile, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Problem {
    pub file: PathBuf,
    pub message: String,
}

/// Why rule text compiled on its own cannot run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InlineError {
    /// Longer than `INLINE_LIMIT` bytes.
    TooLong,
    /// The compiler's message, which starts with `inline:LINE:COLUMN`.
    Invalid(String),
}

impl std::fmt::Display for InlineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLong => write!(f, "rule text is longer than {} KiB", INLINE_LIMIT / 1024),
            Self::Invalid(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for InlineError {}

/// The first bytes a rule's strings matched: which string, where, and the bytes themselves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    pub string: String,
    pub offset: u64,
    pub length: u64,
    /// Up to `EVIDENCE_BYTES` of the match.
    pub bytes: Vec<u8>,
}

/// A rule that matched, with what its author says of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleMatch {
    pub rule: String,
    pub namespace: String,
    pub level: Level,
    pub description: Option<String>,
    pub false_positives: Option<String>,
    /// `None` when the rule matched on its condition alone, such as a file's size or header.
    pub evidence: Option<Evidence>,
}

impl RuleMatch {
    pub fn built_in(&self) -> bool {
        self.namespace == BUILT_IN
    }
    /// What the rule looks for: Borg's own rules in the interface's language, others as their
    /// authors wrote it.
    pub fn about(&self) -> Option<&str> {
        self.built_in()
            .then(|| built_in_text(&self.rule))
            .flatten()
            .map(|(about, _)| about)
            .or(self.description.as_deref())
    }
    /// What else could explain the match.
    pub fn caveat(&self) -> Option<&str> {
        self.built_in()
            .then(|| built_in_text(&self.rule))
            .flatten()
            .map(|(_, caveat)| caveat)
            .or(self.false_positives.as_deref())
    }
}

/// What is scanned, for the variables rules read: a file, or a process's memory, which goes by
/// its program's path.
#[derive(Debug, Clone, Copy)]
pub struct Subject<'a> {
    pub path: &'a str,
    pub memory: bool,
}

/// What a scan found, and whether it ran to the end rather than running out of time.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scanned {
    pub matches: Vec<RuleMatch>,
    pub complete: bool,
}

/// Borg's rules and the user's, compiled.
#[derive(Debug, Clone)]
pub struct Rules {
    scanner: Scanner,
    count: usize,
    problems: Vec<Problem>,
}

impl Rules {
    /// Borg's own rules and those in `files`, each file in a namespace of its own. A file that
    /// does not compile is left out whole and listed with why. So is a file that includes others:
    /// those load on their own, and their rules would match twice.
    pub fn compile(files: &[PathBuf]) -> Self {
        let mut all = compiler();
        all.add_rules_str_in_namespace(BUILT_IN_RULES, BUILT_IN).expect("Borg's own rules compile");
        let mut problems = Vec::new();
        for file in files {
            let namespace = file.display().to_string();
            let checked = compiler().add_rules_file_in_namespace(file, &namespace);
            let added = checked.and_then(|_| all.add_rules_file_in_namespace(file, &namespace));
            if let Err(error) = added {
                problems.push(Problem { file: file.clone(), message: error.to_string() });
            }
        }
        Self::finish(all, problems)
    }

    /// Rule text on its own, without Borg's rules or the user's, in the namespace `INLINE`.
    /// It cannot include other files.
    pub fn inline(text: &str) -> Result<Self, InlineError> {
        if text.len() > INLINE_LIMIT {
            return Err(InlineError::TooLong);
        }
        let mut alone = compiler();
        alone
            .add_rules_str_in_namespace(text, INLINE)
            .map_err(|error| InlineError::Invalid(generate_description(&error.to_diagnostic(), INLINE, text)))?;
        Ok(Self::finish(alone, Vec::new()))
    }

    fn finish(compiler: Compiler, problems: Vec<Problem>) -> Self {
        let mut scanner = compiler.finalize();
        scanner.set_scan_params(
            ScanParams::default()
                .compute_full_matches(true)
                .match_max_length(EVIDENCE_BYTES)
                .timeout_duration(Some(SCAN_TIME)),
        );
        let count = scanner.rules().filter(|rule| !rule.is_private).count();
        Self { scanner, count, problems }
    }

    /// Rules that can match, Borg's own included.
    pub fn count(&self) -> usize {
        self.count
    }

    pub fn problems(&self) -> &[Problem] {
        &self.problems
    }

    /// The rules `bytes` match, worst first, with offsets counted from the first byte.
    pub fn scan(&self, bytes: &[u8], subject: Subject<'_>) -> Scanned {
        let mut scanner = self.scanner.clone();
        let path = Path::new(subject.path);
        let name = path.file_name().map_or_else(String::new, |name| name.to_string_lossy().into_owned());
        let extension = path.extension().map_or_else(String::new, |extension| extension.to_string_lossy().into_owned());
        for (variable, value) in [(FILENAME, name), (FILEPATH, subject.path.to_owned()), (EXTENSION, extension)] {
            let _ = scanner.define_symbol(variable, value);
        }
        scanner.set_scan_params(scanner.scan_params().clone().process_memory(subject.memory));
        let (result, complete) = match scanner.scan_mem(bytes) {
            Ok(result) => (result, true),
            Err((ScanError::Timeout, result)) => (result, false),
            Err((_, result)) => (result, true),
        };
        let mut matches: Vec<_> = result.rules.iter().map(|rule| rule_match(&scanner, rule)).collect();
        matches.sort_by(|a, b| (a.level, &a.namespace, &a.rule).cmp(&(b.level, &b.namespace, &b.rule)));
        Scanned { matches, complete }
    }
}

fn compiler() -> Compiler {
    let mut compiler = Compiler::new();
    compiler.set_params(CompilerParams::default().disable_includes(true));
    for variable in VARIABLES {
        compiler.define_symbol(variable, "");
    }
    compiler
}

fn rule_match(scanner: &Scanner, rule: &EvaluatedRule<'_>) -> RuleMatch {
    let meta = |names: &[&str]| {
        rule.metadatas.iter().find(|meta| names.contains(&scanner.get_string_symbol(meta.name))).map(|meta| meta.value)
    };
    let text = |names: &[&str]| match meta(names)? {
        MetadataValue::Bytes(bytes) => Some(String::from_utf8_lossy(scanner.get_bytes_symbol(bytes)).into_owned()),
        MetadataValue::Integer(_) | MetadataValue::Boolean(_) => None,
    };
    let level = match meta(&[SEVERITY]) {
        Some(MetadataValue::Bytes(bytes)) => Level::named(&String::from_utf8_lossy(scanner.get_bytes_symbol(bytes))),
        Some(MetadataValue::Integer(score)) => Some(Level::scored(score)),
        _ => None,
    }
    .or_else(|| match meta(&[SCORE]) {
        Some(MetadataValue::Integer(score)) => Some(Level::scored(score)),
        _ => None,
    })
    .unwrap_or(Level::Warning);
    let evidence = rule
        .matches
        .iter()
        .flat_map(|string| string.matches.iter().map(move |found| (string.name, found)))
        .min_by_key(|(_, found)| found.base + found.offset)
        .map(|(string, found)| Evidence {
            string: format!("${string}"),
            offset: (found.base + found.offset) as u64,
            length: found.length as u64,
            bytes: found.data.to_vec(),
        });
    RuleMatch {
        rule: rule.name.to_owned(),
        namespace: rule.namespace.to_owned(),
        level,
        description: text(&[DESCRIPTION]),
        false_positives: text(FALSE_POSITIVES),
        evidence,
    }
}

/// What Borg's own rules look for, and what else could explain a match, in the interface's
/// language.
fn built_in_text(rule: &str) -> Option<(&'static str, &'static str)> {
    Some(match rule {
        "reverse_shell" => (tr!(rule_reverse_shell), tr!(caveat_rule_reverse_shell)),
        "download_and_run" => (tr!(rule_download_and_run), tr!(caveat_rule_download_and_run)),
        "decode_and_run" => (tr!(rule_decode_and_run), tr!(caveat_rule_decode_and_run)),
        "crypto_miner" => (tr!(rule_crypto_miner), tr!(caveat_rule_crypto_miner)),
        "upx_packed" => (tr!(rule_upx_packed), tr!(caveat_rule_upx_packed)),
        "execve_shellcode" => (tr!(rule_execve_shellcode), tr!(caveat_rule_execve_shellcode)),
        "ssh_key_planted" => (tr!(rule_ssh_key_planted), tr!(caveat_rule_ssh_key_planted)),
        "reboot_job_fetch" => (tr!(rule_reboot_job_fetch), tr!(caveat_rule_reboot_job_fetch)),
        "password_prompt" => (tr!(rule_password_prompt), tr!(caveat_rule_password_prompt)),
        "launch_agent_from_temp" => (tr!(rule_launch_agent_from_temp), tr!(caveat_rule_launch_agent_from_temp)),
        "preload_write" => (tr!(rule_preload_write), tr!(caveat_rule_preload_write)),
        "history_wiping" => (tr!(rule_history_wiping), tr!(caveat_rule_history_wiping)),
        "agent_unattended" => (tr!(rule_agent_unattended), tr!(caveat_rule_agent_unattended)),
        "hidden_prompt" => (tr!(rule_hidden_prompt), tr!(caveat_rule_hidden_prompt)),
        "pickle_runs_code" => (tr!(rule_pickle_runs_code), tr!(caveat_rule_pickle_runs_code)),
        _ => return None,
    })
}

#[cfg(test)]
mod tests;
