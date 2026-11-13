//! Replacing the text a contents search found, in every file it found or in the marked ones.

use crate::{AppState, Confirm, ConfirmPurpose, Effect, JobKind, Overlay, Panel, Prompt, PromptPurpose, ResultSource};
use borg_i18n::tr;
use std::{path::PathBuf, time::Instant};

/// What a replace job changes: `query` as the search found it, in each of `files`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplaceRequest {
    pub query: borg_view::Query,
    pub with: String,
    pub files: Vec<PathBuf>,
}

/// The text the contents search shown in `panel` looked for.
pub(crate) fn contents_query(panel: &Panel) -> Option<&borg_view::Query> {
    let ResultSource::Search { request, .. } = &panel.results.as_ref()?.source else { return None };
    request.options.contents.as_deref().filter(|query| !query.text.is_empty())
}

/// The files a replace changes: the marked ones, or else every file the results show.
fn files(panel: &Panel) -> Vec<PathBuf> {
    let dir = panel.dir().to_path_buf();
    let marked: Vec<PathBuf> =
        panel.marked().filter(|entry| replaceable(entry)).map(|entry| entry.path(&dir)).collect();
    if !marked.is_empty() {
        return marked;
    }
    panel.visible_entries().filter(|entry| replaceable(entry)).map(|entry| entry.path(&dir)).collect()
}

fn replaceable(entry: &borg_vfs::Entry) -> bool {
    !entry.kind.is_dir() && !entry.missing
}

/// Ctrl-R in a contents search's results: asks what to put in place of the text it found.
pub(crate) fn ask(state: &mut AppState) -> Vec<Effect> {
    let panel = state.active_panel();
    let Some(query) = contents_query(panel).cloned() else {
        state.notice = Some(tr!(replace_needs_contents).into());
        return Vec::new();
    };
    let files = files(panel);
    if files.is_empty() {
        state.notice = Some(tr!(nothing_selected).into());
        return Vec::new();
    }
    let count = borg_i18n::count(files.len(), borg_i18n::Count::File);
    state.overlay = Some(Overlay::Prompt(Prompt {
        title: tr!(replace_in_files_title),
        label: tr!(replace_in_files_label, text = query.text, files = count),
        input: Default::default(),
        purpose: PromptPurpose::ReplaceInFiles { query: Box::new(query), files },
        option_focused: false,
        typed: None,
    }));
    Vec::new()
}

/// Enter in the prompt: one more question, starting on No, that names the text, what replaces it
/// and the files. What was typed counts exactly as typed, even nothing.
pub(crate) fn confirm(state: &mut AppState, request: ReplaceRequest) -> Vec<Effect> {
    let files = match request.files.as_slice() {
        [one] => format!("\"{}\"", state.folders.display(one)),
        many => borg_i18n::count(many.len(), borg_i18n::Count::File),
    };
    let text = request.query.text.as_str();
    let message = if request.with.is_empty() {
        tr!(replace_remove_confirm, text = text, files = files)
    } else {
        tr!(replace_confirm, text = text, with = request.with, files = files)
    };
    state.overlay = Some(Overlay::Confirm(Confirm {
        title: tr!(replace_in_files_title),
        message,
        purpose: ConfirmPurpose::ReplaceInFiles(Box::new(request)),
        accept: false,
    }));
    Vec::new()
}

/// Yes to that question: the job changes exactly the files and the text it named.
pub(crate) fn start(state: &mut AppState, request: ReplaceRequest, now: Instant) -> Vec<Effect> {
    let (id, mut effects) = crate::jobs::begin(state, JobKind::Replace, now);
    effects.push(Effect::StartReplace { id, request });
    effects
}
