//! Reports saved to files: one host's port scan, or what the Neighbors page knows. Ctrl-S opens a
//! dialog that asks for the file and the format, a file that exists is replaced only after a
//! question, and the app writes the report readable by its owner alone.

mod form;
mod neighbors;
mod scan;
mod text;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use borg_i18n::tr;

use crate::{AppState, Confirm, ConfirmPurpose, Effect, Overlay, PortScanRecord, Side};

pub use form::{ReportField, ReportForm};
pub(crate) use form::{on_key, paste};
pub use neighbors::{HostFlag, NeighborsReport, ReportHost, ReportPresence};

/// The folder a report goes to when no panel shows a folder on this machine.
const HOME: &str = "~";
const STEM_SEPARATOR: char = '-';
const PORT_SCAN_STEM: &str = "port-scan";
const NEIGHBORS_STEM: &str = "neighbors";
const MARKDOWN: &str = "Markdown";
const JSON: &str = "JSON";
const CSV: &str = "CSV";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportFormat {
    /// Text with tables, for reading.
    Markdown,
    /// Everything the report holds, for tools.
    Json,
    /// One row per open port or per host, for spreadsheets.
    Csv,
}

impl ReportFormat {
    pub const ALL: [Self; 3] = [Self::Markdown, Self::Json, Self::Csv];

    pub fn name(self) -> &'static str {
        match self {
            Self::Markdown => MARKDOWN,
            Self::Json => JSON,
            Self::Csv => CSV,
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Markdown => "md",
            Self::Json => "json",
            Self::Csv => "csv",
        }
    }
}

/// What a report is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReportSource {
    PortScan(Box<PortScanRecord>),
    Neighbors(Box<NeighborsReport>),
}

impl ReportSource {
    pub fn render(&self, format: ReportFormat) -> String {
        match (self, format) {
            (Self::PortScan(record), ReportFormat::Markdown) => scan::markdown(record),
            (Self::PortScan(record), ReportFormat::Json) => scan::json(record),
            (Self::PortScan(record), ReportFormat::Csv) => scan::csv(record),
            (Self::Neighbors(report), ReportFormat::Markdown) => report.markdown(),
            (Self::Neighbors(report), ReportFormat::Json) => report.json(),
            (Self::Neighbors(report), ReportFormat::Csv) => report.csv(),
        }
    }

    /// The file name without its extension: what the report is about and the day it was made.
    pub fn stem(&self) -> String {
        let (kind, subject, at) = match self {
            Self::PortScan(record) => (PORT_SCAN_STEM, Some(record.address.to_string()), record.at),
            Self::Neighbors(report) => (NEIGHBORS_STEM, None, report.read_at),
        };
        let subject = subject.map(|subject| subject.replace(':', &STEM_SEPARATOR.to_string()));
        [Some(kind.to_owned()), subject, Some(text::day(at))]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(&STEM_SEPARATOR.to_string())
    }
}

/// Why a report was not written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReportError {
    /// A file of that name is there, and replacing it was not asked for.
    Exists,
    Failed(String),
}

/// The folder a new report goes to: the folder of the panel the network screen covers, of the
/// active panel or of the other one, whichever is on this machine first, or else the home folder.
fn folder(state: &AppState) -> PathBuf {
    let sides = state.network.side.into_iter().chain([state.active, state.active.other()]);
    sides
        .map(|side| state.panel(side))
        .find(|panel| !panel.foreign())
        .map_or_else(|| PathBuf::from(HOME), |panel| panel.dir().to_path_buf())
}

/// Opens the save dialog on `source`. `back` is the dialog Esc returns to.
pub(crate) fn open(state: &mut AppState, source: ReportSource, back: Option<Overlay>) -> Vec<Effect> {
    let folder = folder(state);
    state.overlay = Some(Overlay::SaveReport(Box::new(ReportForm::new(source, folder, back))));
    Vec::new()
}

/// Ctrl-S on the Neighbors page: a report of every host the page knows.
pub(crate) fn save_neighbors(state: &mut AppState) -> Vec<Effect> {
    let report = NeighborsReport::of(state);
    open(state, ReportSource::Neighbors(Box::new(report)), None)
}

/// Where a typed path points: as typed when it is absolute or starts at the home folder, else in
/// the dialog's folder.
fn target(folder: &Path, typed: &str) -> PathBuf {
    let path = PathBuf::from(typed);
    if path.is_absolute() || path.starts_with(HOME) { path } else { folder.join(path) }
}

/// Enter in the dialog: writes the report, unless the file is there already.
pub(crate) fn submit(state: &mut AppState, mut form: ReportForm) -> Vec<Effect> {
    if form.path.text().trim().is_empty() {
        form.error = Some(tr!(report_no_path).to_owned());
        state.overlay = Some(Overlay::SaveReport(Box::new(form)));
        return Vec::new();
    }
    write(state, form, false)
}

fn write(state: &mut AppState, mut form: ReportForm, replace: bool) -> Vec<Effect> {
    let path = target(form.folder(), form.path.text().trim());
    let text = form.source.render(form.format);
    form.error = None;
    form.saving = Some(path.clone());
    state.overlay = Some(Overlay::SaveReport(Box::new(form)));
    vec![Effect::SaveReport { path, text, replace }]
}

/// The app wrote the report asked for at `path`, found a file there, or failed.
pub(crate) fn saved(state: &mut AppState, path: PathBuf, result: Result<PathBuf, ReportError>) -> Vec<Effect> {
    let mut form = match state.overlay.take() {
        Some(Overlay::SaveReport(form)) if form.saving.as_ref() == Some(&path) => *form,
        other => {
            state.overlay = other;
            return result.map_or_else(|_| Vec::new(), |written| report_saved(state, &written));
        }
    };
    form.saving = None;
    match result {
        Ok(written) => {
            state.overlay = form.back.map(|back| *back);
            report_saved(state, &written)
        }
        Err(ReportError::Exists) => {
            let name =
                path.file_name().map_or_else(|| path.display().to_string(), |name| name.to_string_lossy().into());
            state.overlay = Some(Overlay::Confirm(Confirm {
                title: tr!(report_question_title),
                message: tr!(report_replace_question, name = name),
                purpose: ConfirmPurpose::ReplaceReport(Box::new(form)),
                accept: false,
            }));
            Vec::new()
        }
        Err(ReportError::Failed(error)) => {
            form.error = Some(tr!(report_failed, error = error));
            state.overlay = Some(Overlay::SaveReport(Box::new(form)));
            Vec::new()
        }
    }
}

/// The answer to replacing a file: yes writes over it, and no goes back to the dialog.
pub(crate) fn replace(state: &mut AppState, form: ReportForm, yes: bool) -> Vec<Effect> {
    if !yes {
        state.overlay = Some(Overlay::SaveReport(Box::new(form)));
        return Vec::new();
    }
    write(state, form, true)
}

/// Says where the report went, and the panels that show its folder list it.
fn report_saved(state: &mut AppState, path: &Path) -> Vec<Effect> {
    state.notice = Some(tr!(report_saved, path = path.display()));
    let Some(folder) = path.parent() else { return Vec::new() };
    let showing: Vec<Side> = Side::ALL.into_iter().filter(|&side| state.panel(side).destination() == folder).collect();
    showing.into_iter().map(|side| state.reload(side)).collect()
}
