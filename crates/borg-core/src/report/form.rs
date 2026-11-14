//! The save dialog: the file to write and its format.

use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent};

use super::{ReportFormat, ReportSource};
use crate::{AppState, Effect, Overlay, input::LineInput, port_scan::cycled};

/// The longest path the dialog takes.
const PATH_LIMIT: usize = 4096;
const EXTENSION_DOT: char = '.';

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportField {
    Path,
    Format,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportForm {
    pub source: ReportSource,
    pub path: LineInput,
    pub format: ReportFormat,
    pub field: ReportField,
    pub error: Option<String>,
    /// The file being written, while the dialog waits for the app.
    pub saving: Option<PathBuf>,
    /// The dialog Esc goes back to, and the one that shows again once the report is saved.
    pub back: Option<Box<Overlay>>,
    folder: PathBuf,
}

impl ReportForm {
    /// The dialog over `source`, with a file named after it in `folder`.
    pub fn new(source: ReportSource, folder: PathBuf, back: Option<Overlay>) -> Self {
        let format = ReportFormat::Markdown;
        let name = format!("{}{EXTENSION_DOT}{}", source.stem(), format.extension());
        Self {
            source,
            path: LineInput::new(folder.join(name).to_string_lossy().into_owned()),
            format,
            field: ReportField::Path,
            error: None,
            saving: None,
            back: back.map(Box::new),
            folder,
        }
    }

    /// The folder a relative path is taken from.
    pub fn folder(&self) -> &Path {
        &self.folder
    }

    /// Another format, and the path's extension with it when the path still has the old one.
    fn reformat(&mut self, format: ReportFormat) {
        let old = format!("{EXTENSION_DOT}{}", self.format.extension());
        if let Some(stem) = self.path.text().strip_suffix(&old) {
            self.path = LineInput::new(format!("{stem}{EXTENSION_DOT}{}", format.extension()));
        }
        self.format = format;
    }
}

pub(crate) fn on_key(state: &mut AppState, mut form: ReportForm, key: &KeyEvent) -> Vec<Effect> {
    if form.saving.is_some() {
        state.overlay = Some(Overlay::SaveReport(Box::new(form)));
        return Vec::new();
    }
    let code = crate::keyboard::latin(key).code;
    match code {
        KeyCode::Esc => {
            state.overlay = form.back.map(|back| *back);
            return Vec::new();
        }
        KeyCode::Enter => return super::submit(state, form),
        KeyCode::Tab | KeyCode::BackTab | KeyCode::Down | KeyCode::Up => {
            form.field = match form.field {
                ReportField::Path => ReportField::Format,
                ReportField::Format => ReportField::Path,
            };
        }
        KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if form.field == ReportField::Format => {
            let format = cycled(&ReportFormat::ALL, form.format, code != KeyCode::Left);
            form.reformat(format);
        }
        _ if form.field == ReportField::Path => {
            let before = form.path.clone();
            if form.path.apply(key) == crate::input::Edit::Changed {
                if form.path.text().chars().count() > PATH_LIMIT {
                    form.path = before;
                }
                form.error = None;
            }
        }
        _ => {}
    }
    state.overlay = Some(Overlay::SaveReport(Box::new(form)));
    Vec::new()
}

/// Pastes into the path.
pub(crate) fn paste(form: &mut ReportForm, text: &str) {
    if form.field != ReportField::Path || form.saving.is_some() {
        return;
    }
    let room = PATH_LIMIT.saturating_sub(form.path.text().chars().count());
    let pasted: String = text.chars().filter(|c| !c.is_control()).take(room).collect();
    form.path = LineInput::new(format!("{}{pasted}", form.path.text()));
    form.error = None;
}
