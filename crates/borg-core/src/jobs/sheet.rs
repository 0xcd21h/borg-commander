//! The jobs panel Alt-J opens: moving through the jobs, folding and unfolding them, steering the
//! running ones, and opening or dismissing the finished ones.

use std::time::Instant;

use borg_i18n::tr;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{JobCommand, JobList, Link, Reveal, cancel, drop_job, find_mut, shown};
use crate::{AppState, Effect, Overlay, Side};

const PAUSE_KEY: char = 'p';
const CANCEL_KEY: char = 'c';
const OPEN_KEY: char = 'o';
const ANSWER_KEY: char = 'a';
const DISMISS_KEY: char = 'x';
const DISMISS_ALL_KEY: char = 'X';
const ALL_KEY: char = '*';
const CLOSE_KEY: char = 'j';
/// Jobs PgUp and PgDn move.
const PAGE: usize = 5;

/// Alt-J: the question a job waits with, or else the panel, on the job of the newest toast.
pub(crate) fn open_list(state: &mut AppState, now: Instant) -> Vec<Effect> {
    if super::files::open_question(state) {
        return Vec::new();
    }
    let toasted = super::toasts::visible(state, now).last().map(|toast| toast.job);
    let selected = toasted.and_then(|id| shown(&state.jobs).iter().position(|job| job.id == id));
    if let Some(id) = toasted.filter(|_| selected.is_some())
        && let Some(job) = find_mut(state, id)
    {
        job.folded = false;
    }
    state.toasts.clear();
    state.overlay = Some(Overlay::Jobs(JobList { selected: selected.unwrap_or_default() }));
    Vec::new()
}

fn select(state: &mut AppState, selected: usize) {
    let last = shown(&state.jobs).len().saturating_sub(1);
    state.overlay = Some(Overlay::Jobs(JobList { selected: selected.min(last) }));
}

fn fold(state: &mut AppState, id: Option<u64>, folded: impl FnOnce(bool) -> bool) {
    if let Some(job) = id.and_then(|id| find_mut(state, id)) {
        job.folded = folded(job.folded);
    }
}

/// `*`: unfolds every job, or folds them all when all are unfolded.
fn fold_all(state: &mut AppState) {
    let folded = state.jobs.iter().all(|job| !job.folded);
    state.jobs.iter_mut().for_each(|job| job.folded = folded);
}

fn pause(state: &mut AppState, id: u64) -> Vec<Effect> {
    let Some(job) = find_mut(state, id).filter(|job| job.running() && job.kind.pauses() && job.link == Link::Runner)
    else {
        return Vec::new();
    };
    job.paused = !job.paused;
    vec![Effect::JobControl { id, command: JobCommand::Pause(job.paused) }]
}

fn answer(state: &mut AppState, id: u64) {
    if state.jobs.iter().any(|job| job.id == id && job.asking()) {
        state.overlay = Some(Overlay::Question(super::Question { id, choice: 0 }));
    }
}

fn dismiss(state: &mut AppState, id: u64) {
    if state.jobs.iter().any(|job| job.id == id && !job.running()) {
        drop_job(state, id);
    }
}

fn dismiss_all(state: &mut AppState) {
    let ended: Vec<u64> = state.jobs.iter().filter(|job| !job.running()).map(|job| job.id).collect();
    ended.into_iter().for_each(|id| drop_job(state, id));
}

pub(crate) fn on_list_key(state: &mut AppState, list: JobList, key: &KeyEvent, now: Instant) -> Vec<Effect> {
    let ids: Vec<u64> = shown(&state.jobs).iter().map(|job| job.id).collect();
    let last = ids.len().saturating_sub(1);
    let selected = list.selected.min(last);
    let id = ids.get(selected).copied();
    let key = crate::keyboard::latin(key);
    let mut effects = Vec::new();
    match key.code {
        KeyCode::Esc | KeyCode::F(10) => state.overlay = None,
        KeyCode::Char(CLOSE_KEY) if key.modifiers.contains(KeyModifiers::ALT) => state.overlay = None,
        KeyCode::Up => select(state, selected.saturating_sub(1)),
        KeyCode::Down => select(state, selected + 1),
        KeyCode::PageUp => select(state, selected.saturating_sub(PAGE)),
        KeyCode::PageDown => select(state, selected + PAGE),
        KeyCode::Home => select(state, 0),
        KeyCode::End => select(state, last),
        KeyCode::Enter | KeyCode::Char(' ') => fold(state, id, |folded| !folded),
        KeyCode::Right => fold(state, id, |_| false),
        KeyCode::Left => fold(state, id, |_| true),
        KeyCode::Char(ALL_KEY) => fold_all(state),
        KeyCode::Char(DISMISS_ALL_KEY) => dismiss_all(state),
        KeyCode::Char(c) if c.to_ascii_lowercase() == PAUSE_KEY => {
            effects = id.map(|id| pause(state, id)).unwrap_or_default();
        }
        KeyCode::Char(c) if c.to_ascii_lowercase() == CANCEL_KEY => {
            effects = id.map(|id| cancel(state, id, now)).unwrap_or_default();
        }
        KeyCode::Char(c) if c.to_ascii_lowercase() == OPEN_KEY => {
            effects = id.map(|id| open_result(state, id)).unwrap_or_default();
        }
        KeyCode::Char(c) if c.to_ascii_lowercase() == ANSWER_KEY => {
            if let Some(id) = id {
                answer(state, id);
            }
        }
        KeyCode::Char(DISMISS_KEY) => {
            if let Some(id) = id {
                dismiss(state, id);
            }
        }
        KeyCode::Delete => match id {
            Some(id) if state.jobs.iter().any(|job| job.id == id && job.running()) => {
                effects = cancel(state, id, now);
            }
            Some(id) => dismiss(state, id),
            None => {}
        },
        _ => {}
    }
    if let Some(Overlay::Jobs(list)) = state.overlay {
        select(state, list.selected);
    }
    effects
}

/// A click on row `row` of the panel: on the selected job it folds or unfolds it, on another
/// it selects it.
pub(crate) fn on_row(state: &mut AppState, list: JobList, row: usize) {
    if row == list.selected {
        let id = shown(&state.jobs).get(row).map(|job| job.id);
        fold(state, id, |folded| !folded);
    }
    select(state, row);
}

/// Shows what finished job `id` made: its folder, its results, its hits.
pub(crate) fn open_result(state: &mut AppState, id: u64) -> Vec<Effect> {
    let Some(reveal) = state.jobs.iter().find(|job| job.id == id && !job.running()).and_then(|job| job.reveal.clone())
    else {
        return Vec::new();
    };
    state.overlay = None;
    match reveal {
        Reveal::Folder { side, dir, focus } => {
            crate::screens::close_all(state);
            state.aside = None;
            state.active = side;
            vec![state.navigate(side, dir, focus)]
        }
        Reveal::Results { side, request } => {
            bring_back_results(state, side, &request);
            Vec::new()
        }
        Reveal::EntropyHits => crate::activity::show_entropy_hits(state),
        Reveal::PortScan(record) => {
            state.overlay = Some(Overlay::PortScanResult(Box::new(crate::PortScanView { record: *record, top: 0 })));
            Vec::new()
        }
    }
}

/// Shows the results of `request` in the panel on `side`: where they are, or brought back from
/// the results the panel left.
fn bring_back_results(state: &mut AppState, side: Side, request: &crate::SearchRequest) {
    let holds = |panel: &crate::Panel| {
        matches!(panel.results.as_ref().map(|results| &results.source),
            Some(crate::ResultSource::Search { request: shown, .. }) if super::watched::same_search(shown, request))
    };
    let index = side.index();
    if !holds(state.panel(side)) {
        let Some(results) = state.last_results[index].take_if(|panel| holds(panel)) else {
            state.notice = Some(tr!(job_results_gone).to_owned());
            return;
        };
        state.settle();
        let left = std::mem::replace(&mut state.panels[index], results);
        if left.is_results() {
            state.last_results[index] = Some(left);
        } else {
            state.result_history[index] = Some(left);
        }
    }
    crate::screens::close_all(state);
    state.active = side;
    state.refit();
}

/// Alt-G: opens the result of the job that finished last with one.
pub(crate) fn open_newest_result(state: &mut AppState) -> Vec<Effect> {
    let newest = shown(&state.jobs).into_iter().find(|job| !job.running() && job.reveal.is_some()).map(|job| job.id);
    match newest {
        Some(id) => open_result(state, id),
        None => {
            state.notice = Some(tr!(no_job_result).to_owned());
            Vec::new()
        }
    }
}
