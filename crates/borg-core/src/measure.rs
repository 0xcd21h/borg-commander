use crate::{AppState, Effect, Msg, SearchProgress, Side};
use borg_i18n::tr;
use borg_vfs::Entry;
use std::path::PathBuf;

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
    if state.active_panel().is_results() || state.activity.side.is_some() {
        state.notice = Some(tr!(background_sizes_directory_required).into());
        return Vec::new();
    }
    state.notice = None;
    state.next_size += 1;
    let id = state.next_size;
    let root = state.active_panel().dir().to_path_buf();
    state.sizing = Some(SizeScan { id, root: root.clone(), side: state.active, progress: Default::default() });
    vec![Effect::MeasureSizes { id, root }]
}

pub(crate) fn batch(state: &mut AppState, id: u64, entries: Vec<Entry>) {
    if state.sizing.as_ref().is_none_or(|scan| scan.id != id) {
        return;
    }
    let totals = entries
        .into_iter()
        .filter_map(|entry| {
            let path = entry.identity().to_path_buf();
            entry.usage.map(|usage| (path, (entry.size, usage)))
        })
        .collect();
    for panel in &mut state.panels {
        panel.measure_sizes(&totals);
    }
}

pub(crate) fn finish(state: &mut AppState, id: u64, error: Option<String>) -> Vec<Effect> {
    if state.sizing.as_ref().is_none_or(|scan| scan.id != id) {
        return Vec::new();
    }
    let scan = state.sizing.take().unwrap();
    state.notice = Some(error.clone().unwrap_or_else(|| {
        format!(
            tr!(background_sizes_done),
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
