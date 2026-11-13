//! The job that replaces the text a contents search found, a file at a time, through the
//! editor's own reads and writes: text files only, and none that changed while Borg worked.

use crate::edit_file;
use borg_core::ReplaceRequest;
use borg_jobs::{Control, Event, Outcome, Progress, Totals};
use borg_view::Search;
use std::path::Path;

/// Why a file was left as it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Skipped;

pub(crate) fn run(request: &ReplaceRequest, control: &Control, emit: &mut dyn FnMut(Event)) -> Outcome {
    let search = match request.query.compile() {
        Ok(Some(search)) => search,
        Ok(None) => return Outcome::Done { done: Totals::default(), skipped: 0 },
        Err(reason) => return Outcome::Failed(reason),
    };
    let total = Totals { files: request.files.len() as u64, bytes: 0 };
    let mut changed = Totals::default();
    let mut skipped = 0;
    for (seen, path) in (0..).zip(&request.files) {
        if !control.proceed() {
            return Outcome::Cancelled { done: changed };
        }
        let done = Totals { files: seen, bytes: 0 };
        emit(Event::Progress(Progress { done, total, current: path.clone() }));
        match replace_in(&search, path, &request.with) {
            Ok(true) => changed.files += 1,
            Ok(false) => {}
            Err(Skipped) => skipped += 1,
        }
    }
    Outcome::Done { done: changed, skipped }
}

/// Whether `path` changed. A file that is not text, is too large for the editor, or changed
/// since it was read is skipped.
fn replace_in(search: &Search, path: &Path, with: &str) -> Result<bool, Skipped> {
    let file = edit_file::read(path).map_err(|_| Skipped)?;
    let (text, count) = search.replace_all(&file.text, with);
    if count == 0 {
        return Ok(false);
    }
    edit_file::write(path, text.as_bytes(), file.stamp, false).map(|_| true).map_err(|_| Skipped)
}

#[cfg(test)]
mod tests;
