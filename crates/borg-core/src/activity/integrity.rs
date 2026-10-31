//! What the integrity check finds wrong with a process, and how badly.

use super::{Activity, ActivityPage, ActivityRequest, MemoryRegion, ProcessKey};
use crate::Effect;
use borg_i18n::tr;
use serde::{Deserialize, Serialize};

/// How much a finding matters, worst first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Severity {
    /// Seldom innocent: look at it.
    Alert,
    /// Often has an ordinary reason, worth knowing which.
    Warning,
    /// Usually fine; listed so the picture is complete.
    Notice,
}

impl Severity {
    pub fn label(self) -> &'static str {
        match self {
            Self::Alert => tr!(severity_alert),
            Self::Warning => tr!(severity_warning),
            Self::Notice => tr!(severity_notice),
        }
    }
}

/// What an integrity check found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FindingKind {
    /// An executable image's header in memory that no file backs.
    HiddenImage,
    /// Code that differs from the file it was mapped from.
    CodeChanged,
    /// Executable memory that no file backs.
    UnbackedCode,
    /// Memory that is writable and executable at once.
    WritableCode,
    /// Code mapped from a deleted file, or from memory that never had a file.
    DeletedCode,
    /// Code loaded from a temporary folder.
    TemporaryCode,
    /// A mapped file replaced on disk since it was loaded.
    ReplacedFile,
    /// Libraries added through the environment, such as `LD_PRELOAD` or `DYLD_INSERT_LIBRARIES`.
    Preload,
    /// A library search path set in the environment.
    LibraryPath,
    /// Another process is tracing this one.
    Traced,
    /// The program's own file is gone.
    ProgramGone,
    /// The process goes by a name other than its program's.
    NameMismatch,
}

impl FindingKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::HiddenImage => tr!(finding_hidden_image),
            Self::CodeChanged => tr!(finding_code_changed),
            Self::UnbackedCode => tr!(finding_unbacked_code),
            Self::WritableCode => tr!(finding_writable_code),
            Self::DeletedCode => tr!(finding_deleted_code),
            Self::TemporaryCode => tr!(finding_temporary_code),
            Self::ReplacedFile => tr!(finding_replaced_file),
            Self::Preload => tr!(finding_preload),
            Self::LibraryPath => tr!(finding_library_path),
            Self::Traced => tr!(finding_traced),
            Self::ProgramGone => tr!(finding_program_gone),
            Self::NameMismatch => tr!(finding_name_mismatch),
        }
    }
    /// The ordinary reasons for the finding, so it is read as a lead rather than a verdict.
    pub fn caveat(self) -> &'static str {
        match self {
            Self::HiddenImage => tr!(caveat_hidden_image),
            Self::CodeChanged => tr!(caveat_code_changed),
            Self::UnbackedCode => tr!(caveat_unbacked_code),
            Self::WritableCode => tr!(caveat_writable_code),
            Self::DeletedCode => tr!(caveat_deleted_code),
            Self::TemporaryCode => tr!(caveat_temporary_code),
            Self::ReplacedFile => tr!(caveat_replaced_file),
            Self::Preload => tr!(caveat_preload),
            Self::LibraryPath => tr!(caveat_library_path),
            Self::Traced => tr!(caveat_traced),
            Self::ProgramGone => tr!(caveat_program_gone),
            Self::NameMismatch => tr!(caveat_name_mismatch),
        }
    }
}

/// One finding, with the evidence: where in memory, when it is about a place there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub kind: FindingKind,
    pub severity: Severity,
    pub detail: String,
    pub region: Option<MemoryRegion>,
    pub address: Option<u64>,
    pub length: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntegrityReport {
    pub process: ProcessKey,
    /// Worst first, then by address.
    pub findings: Vec<Finding>,
    /// Bytes of code compared with the files they were mapped from.
    pub compared: u64,
    /// Bytes of code that could not be compared: unreadable, or with no file to compare with.
    pub uncompared: u64,
    /// Bytes of memory no file backs that were searched for executable images, of `unbacked`.
    pub scanned: u64,
    pub unbacked: u64,
    /// Bytes that could not be read.
    pub unreadable: u64,
    /// Whether every check ran to the end, rather than stopping at a limit or on Ctrl-C.
    pub complete: bool,
}

/// Rows below the findings list that explain the selected finding.
pub const FINDING_NOTE: u16 = 6;

/// Checks the process against its files and for signs of injected code; the findings replace
/// the list until Esc goes back to where the check started.
pub(super) fn check_integrity(activity: &mut Activity, process: ProcessKey) -> Vec<Effect> {
    if !matches!(activity.list, ActivityPage::Search | ActivityPage::Integrity) {
        activity.results_origin = activity.list;
    }
    activity.search_progress = Some((0, 0, 0));
    activity.request(ActivityRequest::Integrity { process, admin: false })
}
