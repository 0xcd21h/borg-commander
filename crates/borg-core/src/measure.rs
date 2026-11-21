use crate::{AppState, Effect, How, Msg, Panel, Row, SearchProgress, Side};
use borg_i18n::tr;
use borg_vfs::{Entry, EntryKind, FolderUsage};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::Instant,
};

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

/// The folder being measured in the background, for the size count job `job`.
#[derive(Debug, Clone)]
pub struct SizeScan {
    pub id: u64,
    /// The job that stands for the count.
    pub job: u64,
    pub root: PathBuf,
    pub side: Side,
    pub progress: SearchProgress,
    /// The job's folders still to measure once `root` is done.
    pub rest: Vec<PathBuf>,
    /// What the job's folders measured before `root` came to.
    pub before: SearchProgress,
}

impl SizeScan {
    /// How many items the job read, in every folder it measured so far.
    pub fn visited(&self) -> usize {
        self.so_far().visited
    }

    fn so_far(&self) -> SearchProgress {
        SearchProgress {
            visited: self.before.visited + self.progress.visited,
            errors: self.before.errors + self.progress.errors,
            skipped: self.before.skipped + self.progress.skipped,
            ..Default::default()
        }
    }

    fn roots(&self) -> impl Iterator<Item = &PathBuf> {
        std::iter::once(&self.root).chain(&self.rest)
    }
}

/// A size count that waits for the one being measured to end.
#[derive(Debug, Clone)]
pub struct WaitingCount {
    job: u64,
    side: Side,
    root: PathBuf,
    rest: Vec<PathBuf>,
}

impl WaitingCount {
    fn roots(&self) -> impl Iterator<Item = &PathBuf> {
        std::iter::once(&self.root).chain(&self.rest)
    }
}

/// Stops every size count, the one being measured and those waiting. Totals measured so far
/// stay.
pub(crate) fn cancel(state: &mut AppState, now: Instant) -> Vec<Effect> {
    for waiting in std::mem::take(&mut state.waiting_sizes) {
        end_stopped(state, waiting.job, now);
    }
    let Some(scan) = state.sizing.take() else { return Vec::new() };
    end_stopped(state, scan.job, now);
    vec![Effect::CancelMeasure]
}

/// Stops the size count of job `job`. When it was being measured, the next count waiting starts.
pub(crate) fn cancel_job(state: &mut AppState, job: u64, now: Instant) -> Vec<Effect> {
    if let Some(at) = state.waiting_sizes.iter().position(|waiting| waiting.job == job) {
        state.waiting_sizes.remove(at);
        end_stopped(state, job, now);
        return Vec::new();
    }
    if state.sizing.take_if(|scan| scan.job == job).is_none() {
        return Vec::new();
    }
    end_stopped(state, job, now);
    let mut effects = vec![Effect::CancelMeasure];
    effects.extend(next(state));
    effects
}

fn end_stopped(state: &mut AppState, job: u64, now: Instant) {
    crate::jobs::end_sizes(state, job, How::Cancelled, tr!(background_sizes_stopped).to_owned(), now);
}

/// Alt-I: counts the sizes below the active panel's folder, or stops every count under way.
pub(crate) fn toggle(state: &mut AppState, now: Instant) -> Vec<Effect> {
    if state.sizing.is_some() {
        return cancel(state, now);
    }
    if state.active_panel().is_results() || state.active_panel().foreign() || state.monitor_open() {
        state.notice = Some(tr!(background_sizes_directory_required).into());
        return Vec::new();
    }
    let root = state.active_panel().dir().to_path_buf();
    count(state, root, Vec::new(), now)
}

/// The folders F3 counts in `panel`. On a folder or `..` it counts the marked folders, or with
/// none marked the folder under the cursor, and on `..` the folder the panel shows. Empty where
/// F3 views a file instead. Links to folders are not counted, as size scans never follow them.
pub fn folders_to_count(panel: &Panel) -> Vec<PathBuf> {
    if panel.is_results() || panel.foreign() {
        return Vec::new();
    }
    let hovered = match panel.hovered() {
        Some(Row::Parent) => panel.dir().to_path_buf(),
        Some(Row::Entry(entry)) if entry.kind == EntryKind::Dir => entry.path(panel.dir()),
        Some(Row::Entry(_) | Row::Group { .. }) | None => return Vec::new(),
    };
    let marked: Vec<PathBuf> =
        panel.marked().filter(|entry| entry.kind == EntryKind::Dir).map(|entry| entry.path(panel.dir())).collect();
    if marked.is_empty() { vec![hovered] } else { marked }
}

/// F3 on a folder: counts the folders [`folders_to_count`] names in the panel on `side`, leaving
/// out those a count under way or waiting already measures. `None` where F3 views a file instead.
pub(crate) fn count_folders(state: &mut AppState, side: Side, now: Instant) -> Option<Vec<Effect>> {
    let wanted = folders_to_count(state.panel(side));
    let first = wanted.first()?.clone();
    let fresh: Vec<PathBuf> = wanted.into_iter().filter(|folder| covering(state, folder).is_none()).collect();
    let Some((root, rest)) = fresh.split_first() else {
        let counted = covering(state, &first).map(|root| root.display().to_string()).unwrap_or_default();
        state.notice = Some(tr!(sizes_already_counting, folder = counted));
        return Some(Vec::new());
    };
    Some(count(state, root.clone(), rest.to_vec(), now))
}

/// The folder being counted, or waiting to be, that holds `folder`, so that its count measures
/// `folder` too.
fn covering<'a>(state: &'a AppState, folder: &Path) -> Option<&'a Path> {
    let measuring = state.sizing.iter().flat_map(SizeScan::roots);
    let waiting = state.waiting_sizes.iter().flat_map(WaitingCount::roots);
    measuring.chain(waiting).map(PathBuf::as_path).find(|root| folder.starts_with(root))
}

/// Adds a job that measures `root` and then `rest`, at once or after the counts before it.
fn count(state: &mut AppState, root: PathBuf, rest: Vec<PathBuf>, now: Instant) -> Vec<Effect> {
    state.notice = None;
    let side = state.active;
    let job = crate::jobs::begin_sizes(state, &root, &rest, side, now);
    if state.sizing.is_some() {
        state.waiting_sizes.push(WaitingCount { job, side, root, rest });
        return Vec::new();
    }
    start(state, WaitingCount { job, side, root, rest }, SearchProgress::default())
}

/// Starts measuring the count's first folder, after folders of the same job that came to
/// `before`.
fn start(state: &mut AppState, count: WaitingCount, before: SearchProgress) -> Vec<Effect> {
    state.next_size += 1;
    let WaitingCount { job, side, root, rest } = count;
    let scan =
        SizeScan { id: state.next_size, job, root: root.clone(), side, progress: Default::default(), rest, before };
    crate::jobs::sizes_progress(state, job, &root, &scan.so_far());
    let id = scan.id;
    state.sizing = Some(scan);
    state.measured = Measured { root: root.clone(), totals: HashMap::new() };
    vec![Effect::MeasureSizes { id, root }]
}

/// Starts the first count waiting, if any.
fn next(state: &mut AppState) -> Vec<Effect> {
    if state.waiting_sizes.is_empty() {
        return Vec::new();
    }
    let waiting = state.waiting_sizes.remove(0);
    start(state, waiting, SearchProgress::default())
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

/// A folder measured to its end: the panels showing it list it again from the saved scan, and
/// the job's next folder follows. The job ends after its last folder, or when one fails, and
/// the next count waiting starts.
pub(crate) fn finish(state: &mut AppState, id: u64, error: Option<String>, now: Instant) -> Vec<Effect> {
    let Some(scan) = state.sizing.take_if(|scan| scan.id == id) else { return Vec::new() };
    if let Some(error) = error {
        crate::jobs::end_sizes(state, scan.job, How::Failed, error, now);
        return next(state);
    }
    forget(state);
    let mut effects = relist(state, &scan.root);
    let so_far = scan.so_far();
    match scan.rest.split_first() {
        Some((root, rest)) => {
            let count = WaitingCount { job: scan.job, side: scan.side, root: root.clone(), rest: rest.to_vec() };
            effects.extend(start(state, count, so_far));
        }
        None => {
            let summary =
                tr!(background_sizes_done, count = so_far.visited, errors = so_far.errors, skipped = so_far.skipped);
            crate::jobs::end_sizes(state, scan.job, How::Done, summary, now);
            effects.extend(next(state));
        }
    }
    effects
}

/// Lists again the folders that show what measuring `root` changed: those inside it and the
/// one that holds it.
fn relist(state: &mut AppState, root: &Path) -> Vec<Effect> {
    let mut effects = Vec::new();
    for side in Side::ALL {
        let panel = state.panel(side);
        if !panel.is_results() && (panel.dir().starts_with(root) || root.parent() == Some(panel.dir())) {
            effects.push(state.reload(side));
        }
    }
    effects
}

pub(crate) fn progress(state: &mut AppState, id: u64, progress: SearchProgress) {
    let Some(scan) = state.sizing.as_mut().filter(|scan| scan.id == id) else { return };
    scan.progress = progress;
    let (job, root, so_far) = (scan.job, scan.root.clone(), scan.so_far());
    crate::jobs::sizes_progress(state, job, &root, &so_far);
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
