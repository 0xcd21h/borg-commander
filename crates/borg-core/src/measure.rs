use crate::{AppState, Effect, Msg, SearchProgress, Side};
use borg_i18n::tr;
use borg_vfs::{Entry, FolderUsage};
use std::{collections::HashMap, path::PathBuf, time::Instant};

/// Folders remembered from a scan before only shallow ones are kept.
const KEPT_TOTALS: usize = 50_000;
/// Folders this close to the scan root are always remembered.
const KEPT_DEPTH: usize = 2;

/// Totals from the latest background scan, so a folder listed again during the scan, or
/// after it was stopped, still shows them. A finished scan's cache replaces them.
#[derive(Debug, Clone, Default)]
pub struct Measured {
    root: PathBuf,
    totals: HashMap<PathBuf, (u64, FolderUsage)>,
}

#[derive(Debug, Clone)]
pub struct SizeScan {
    pub id: u64,
    /// The job that stands for the scan.
    pub job: u64,
    pub root: PathBuf,
    pub side: Side,
    pub progress: SearchProgress,
}

pub(crate) fn cancel(state: &mut AppState, now: Instant) -> Vec<Effect> {
    let Some(scan) = state.sizing.take() else { return Vec::new() };
    let summary = tr!(background_sizes_stopped).to_owned();
    crate::jobs::end_sizes(state, scan.job, crate::How::Cancelled, summary, now);
    vec![Effect::CancelMeasure]
}

pub(crate) fn toggle(state: &mut AppState, now: Instant) -> Vec<Effect> {
    if state.sizing.is_some() {
        return cancel(state, now);
    }
    if state.active_panel().is_results() || state.active_panel().foreign() || state.monitor_open() {
        state.notice = Some(tr!(background_sizes_directory_required).into());
        return Vec::new();
    }
    state.notice = None;
    state.next_size += 1;
    let id = state.next_size;
    let root = state.active_panel().dir().to_path_buf();
    let job = crate::jobs::begin_sizes(state, root.clone(), state.active, now);
    state.sizing = Some(SizeScan { id, job, root: root.clone(), side: state.active, progress: Default::default() });
    state.measured = Measured { root: root.clone(), totals: HashMap::new() };
    vec![Effect::MeasureSizes { id, root }]
}

pub(crate) fn batch(state: &mut AppState, id: u64, entries: Vec<Entry>) {
    if state.sizing.as_ref().is_none_or(|scan| scan.id != id) {
        return;
    }
    let totals: HashMap<_, _> = entries
        .into_iter()
        .filter_map(|entry| {
            let path = entry.identity().to_path_buf();
            entry.usage.map(|usage| (path, (entry.size, usage)))
        })
        .collect();
    for panel in &mut state.panels {
        panel.measure_sizes(&totals);
    }
    let measured = &mut state.measured;
    for (path, total) in totals {
        let shallow = path.strip_prefix(&measured.root).is_ok_and(|rest| rest.components().count() <= KEPT_DEPTH);
        if measured.totals.len() < KEPT_TOTALS || shallow || measured.totals.contains_key(&path) {
            measured.totals.insert(path, total);
        }
    }
}

/// Shows remembered totals in a folder that was just listed again.
pub(crate) fn reapply(state: &mut AppState, side: Side) {
    if !state.measured.totals.is_empty() {
        state.panels[side.index()].measure_sizes(&state.measured.totals);
    }
}

/// Forgets remembered totals, for example before a job changes the files they describe.
pub(crate) fn forget(state: &mut AppState) {
    state.measured = Measured::default();
}

pub(crate) fn finish(state: &mut AppState, id: u64, error: Option<String>, now: Instant) -> Vec<Effect> {
    if state.sizing.as_ref().is_none_or(|scan| scan.id != id) {
        return Vec::new();
    }
    let scan = state.sizing.take().unwrap();
    if error.is_none() {
        forget(state);
    }
    let how = if error.is_some() { crate::How::Failed } else { crate::How::Done };
    let summary = error.clone().unwrap_or_else(|| {
        tr!(
            background_sizes_done,
            count = scan.progress.visited,
            errors = scan.progress.errors,
            skipped = scan.progress.skipped
        )
    });
    crate::jobs::end_sizes(state, scan.job, how, summary, now);
    if error.is_some() {
        return Vec::new();
    }
    let mut effects = Vec::new();
    for side in Side::ALL {
        let panel = state.panel(side);
        if !panel.is_results() && (panel.dir().starts_with(&scan.root) || scan.root.parent() == Some(panel.dir())) {
            effects.push(state.reload(side));
        }
    }
    effects
}

pub(crate) fn progress(state: &mut AppState, id: u64, progress: SearchProgress) {
    let Some(scan) = state.sizing.as_mut().filter(|scan| scan.id == id) else { return };
    scan.progress = progress;
    let (job, root, progress) = (scan.job, scan.root.clone(), scan.progress.clone());
    crate::jobs::sizes_progress(state, job, &root, &progress);
}

pub(crate) fn receive(state: &mut AppState, message: Msg, now: Instant) -> Vec<Effect> {
    match message {
        Msg::Sized { id, entries } => batch(state, id, entries),
        Msg::SizeProgress { id, progress: update } => progress(state, id, update),
        Msg::SizesFinished { id, error } => return finish(state, id, error, now),
        _ => unreachable!(),
    }
    Vec::new()
}
