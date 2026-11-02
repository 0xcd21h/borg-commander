use crate::{AppState, Effect, Msg, SearchProgress, Side};
use borg_i18n::tr;
use borg_vfs::{Entry, FolderUsage};
use std::{collections::HashMap, path::PathBuf};

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
    pub root: PathBuf,
    pub side: Side,
    pub progress: SearchProgress,
}

pub(crate) fn cancel(state: &mut AppState) -> Vec<Effect> {
    if state.sizing.take().is_some() { vec![Effect::CancelMeasure] } else { Vec::new() }
}

pub(crate) fn toggle(state: &mut AppState) -> Vec<Effect> {
    if state.sizing.is_some() {
        state.notice = Some(tr!(background_sizes_stopped).into());
        return cancel(state);
    }
    if state.active_panel().is_results()
        || state.active_panel().mount.is_some()
        || state.active_panel().remote_mount.is_some()
        || state.monitor_open()
    {
        state.notice = Some(tr!(background_sizes_directory_required).into());
        return Vec::new();
    }
    state.notice = None;
    state.next_size += 1;
    let id = state.next_size;
    let root = state.active_panel().dir().to_path_buf();
    state.sizing = Some(SizeScan { id, root: root.clone(), side: state.active, progress: Default::default() });
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

pub(crate) fn finish(state: &mut AppState, id: u64, error: Option<String>) -> Vec<Effect> {
    if state.sizing.as_ref().is_none_or(|scan| scan.id != id) {
        return Vec::new();
    }
    let scan = state.sizing.take().unwrap();
    if error.is_none() {
        forget(state);
    }
    state.notice = Some(error.clone().unwrap_or_else(|| {
        tr!(
            background_sizes_done,
            count = scan.progress.visited,
            errors = scan.progress.errors,
            skipped = scan.progress.skipped
        )
    }));
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
    if let Some(scan) = state.sizing.as_mut().filter(|scan| scan.id == id) {
        scan.progress = progress;
    }
}

pub(crate) fn receive(state: &mut AppState, message: Msg) -> Vec<Effect> {
    match message {
        Msg::Sized { id, entries } => batch(state, id, entries),
        Msg::SizeProgress { id, progress: update } => progress(state, id, update),
        Msg::SizesFinished { id, error } => return finish(state, id, error),
        _ => unreachable!(),
    }
    Vec::new()
}
