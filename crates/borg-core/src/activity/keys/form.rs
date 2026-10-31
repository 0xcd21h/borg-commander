//! The memory search form: the query, how to read it, whether case matters and whether to
//! search only the snapshot.

use borg_i18n::tr;
use crossterm::event::{KeyCode, KeyEvent};

use crate::{
    Effect,
    activity::{Activity, ActivityRequest, MemorySearchForm, MemorySearchMode},
};

/// The longest query, in bytes.
const QUERY_LIMIT: usize = 512;

/// An empty form, for text that matches its case.
pub(super) fn new() -> MemorySearchForm {
    MemorySearchForm {
        input: crate::LineInput::default(),
        mode: MemorySearchMode::Text,
        match_case: true,
        snapshot_only: false,
        selected: 0,
    }
}

/// A key in the form, which the monitor holds again unless Esc closes it or Enter searches.
/// Tab and the arrows move between the fields; the arrows and Space change the switches.
pub(super) fn on_key(activity: &mut Activity, mut form: MemorySearchForm, key: &KeyEvent) -> Vec<Effect> {
    match key.code {
        KeyCode::Esc => {
            activity.error = None;
            return Vec::new();
        }
        KeyCode::Tab | KeyCode::Down => form.selected = (form.selected + 1) % 4,
        KeyCode::BackTab | KeyCode::Up => form.selected = (form.selected + 3) % 4,
        KeyCode::Enter => {
            if let Some(effects) = submit(activity, &form) {
                return effects;
            }
        }
        KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if form.selected == 1 => {
            form.mode = next_mode(form.mode, key.code == KeyCode::Left);
        }
        KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if form.selected == 2 => form.match_case = !form.match_case,
        KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if form.selected == 3 => {
            form.snapshot_only = !form.snapshot_only
        }
        _ if form.selected == 0 => {
            form.input.apply(key);
        }
        _ => {}
    }
    activity.search_form = Some(form);
    Vec::new()
}

/// Pastes into the query, when it has the focus, as much as fits.
pub(super) fn paste(form: &mut MemorySearchForm, text: &str) {
    if form.selected != 0 {
        return;
    }
    for c in text.chars().take(QUERY_LIMIT) {
        if form.input.text().len() + c.len_utf8() > QUERY_LIMIT {
            break;
        }
        form.input.apply(&KeyCode::Char(c).into());
    }
}

/// The mode after `mode`, or the one before it.
fn next_mode(mode: MemorySearchMode, back: bool) -> MemorySearchMode {
    let modes = MemorySearchMode::ALL;
    let at = modes.iter().position(|&each| each == mode).unwrap_or(0);
    let step = if back { modes.len() - 1 } else { 1 };
    modes[(at + step) % modes.len()]
}

/// Searches the selected process's memory, or its snapshot, for the query. The form stays,
/// saying why, when the query cannot be read or there is no snapshot of the process.
fn submit(activity: &mut Activity, form: &MemorySearchForm) -> Option<Vec<Effect>> {
    if !readable(form) {
        activity.error = Some(tr!(memory_query_invalid).into());
        return None;
    }
    let process = activity.selected?;
    let snapshot = if form.snapshot_only { activity.snapshot.clone().filter(|s| s.process == process) } else { None };
    if form.snapshot_only && snapshot.is_none() {
        activity.error = Some(tr!(memory_no_snapshot).into());
        return None;
    }
    activity.search_progress = Some((0, 0, 0));
    Some(activity.request(ActivityRequest::Search {
        process,
        query: form.input.text().to_owned(),
        mode: form.mode,
        match_case: form.match_case,
        snapshot,
        start: None,
        admin: false,
    }))
}

/// Whether the query can be searched for: some text, unless the mode finds strings without
/// one, no longer than the limit, and in hex whole bytes of hex digits.
fn readable(form: &MemorySearchForm) -> bool {
    let query = form.input.text();
    let hex = || {
        let digits = query.split_whitespace().collect::<String>();
        !digits.is_empty() && digits.len() % 2 == 0 && digits.chars().all(|c| c.is_ascii_hexdigit())
    };
    (form.mode.strings() || !query.is_empty())
        && query.len() <= QUERY_LIMIT
        && (form.mode != MemorySearchMode::Hex || hex())
}
