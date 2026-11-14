//! Jobs that stand for work other parts of the state run: searches into a results panel, the
//! folder sizes, the entropy scan of every process and the first build of an index. Each one's
//! gauge and details follow that work, and the job ends when it does.

use std::{path::PathBuf, time::Instant};

use borg_i18n::{Count, tr};

use super::{Detail, Gauge, How, JobKind, Link, Reveal, Unit, Value, add, advance, end};
use crate::{AppState, Effect, ResultSource, SearchProgress, SearchRequest, Side};

/// What a search looks for and where.
pub(crate) fn search_subject(request: &SearchRequest) -> String {
    let root = request.root.display();
    match &request.options.contents {
        Some(contents) => tr!(job_search_contents, text = contents.text, root = root),
        None if request.query.is_empty() => root.to_string(),
        None => tr!(job_search_in, query = request.query, root = root),
    }
}

/// Whether two requests are the same search, once again or refreshed.
pub(super) fn same_search(one: &SearchRequest, other: &SearchRequest) -> bool {
    one.root == other.root
        && one.query == other.query
        && one.options.kind == other.options.kind
        && one.options.contents == other.options.contents
}

/// A search the user started fills the panel on `side`: a job stands for it. Refreshes are
/// part of the results they refresh, not jobs of their own.
fn begin_search(state: &mut AppState, side: Side, now: Instant) {
    let Some((request, generation)) = shown_search(state, side)
        .filter(|(request, _, _, loading, _)| *loading && !request.options.refresh)
        .map(|(request, _, generation, _, _)| (request.clone(), generation))
    else {
        return;
    };
    if state.jobs.iter().any(|job| job.link == Link::Search { side, generation }) {
        return;
    }
    let request = &request;
    let id = add(state, JobKind::Search, search_subject(request), side, Link::Search { side, generation }, now);
    if let Some(job) = super::find_mut(state, id) {
        job.gauge = Gauge::counting(0, Unit::Folders);
        job.reveal = Some(Reveal::Results { side, request: Box::new(request.clone()) });
    }
}

/// Lets opening the finished job `id` bring back the search results the panel on `side` shows.
pub(crate) fn reveal_results(state: &mut AppState, id: u64, side: Side) {
    let Some(request) = shown_search(state, side).map(|(request, ..)| request.clone()) else { return };
    if let Some(job) = super::find_mut(state, id) {
        job.reveal = Some(Reveal::Results { side, request: Box::new(request) });
    }
}

/// The search the panel on `side` shows, its listing generation, whether it still runs, and
/// how far it got.
fn shown_search(state: &AppState, side: Side) -> Option<(&SearchRequest, &SearchProgress, u64, bool, usize)> {
    let panel = state.panel(side);
    let Some(ResultSource::Search { request, progress }) = panel.results.as_ref().map(|results| &results.source) else {
        return None;
    };
    Some((request, progress, panel.generation(), panel.loading(), panel.entries().len()))
}

fn search_details(request: &SearchRequest, progress: &SearchProgress, found: usize) -> Vec<Detail> {
    let looking =
        request.options.contents.as_ref().map_or_else(|| request.query.clone(), |contents| contents.text.clone());
    let mut details = vec![
        Detail::new(tr!(@ job_detail_in), Value::Path(request.root.clone())),
        Detail::new(tr!(@ job_detail_looking_for), Value::Text(looking)),
        Detail::new(tr!(@ job_detail_visited), Value::Number(progress.visited as u64)),
        Detail::new(tr!(@ job_detail_found), Value::Number(found as u64)),
    ];
    if progress.errors > 0 {
        details.push(Detail::new(tr!(@ job_detail_unreadable), Value::Number(progress.errors as u64)));
    }
    let left_out = progress.excluded + progress.skipped;
    if left_out > 0 {
        details.push(Detail::new(tr!(@ job_detail_left_out), Value::Number(left_out as u64)));
    }
    details
}

/// Follows the search job `id` linked to listing `generation` on `side`: a refresh of the same
/// search carries it on, and it ends once the panel stops searching for it.
fn follow_search(state: &mut AppState, id: u64, side: Side, generation: u64, now: Instant) {
    let Some(job) = state.jobs.iter().find(|job| job.id == id) else { return };
    let Some(Reveal::Results { request: wanted, .. }) = job.reveal.clone() else { return };
    let shown = shown_search(state, side).map(|(request, progress, at, loading, found)| {
        (same_search(request, &wanted), progress.clone(), at, loading, found, request.clone())
    });
    match shown {
        Some((true, progress, at, true, found, request)) => {
            if let Some(job) = super::find_mut(state, id) {
                job.link = Link::Search { side, generation: at };
            }
            let details = search_details(&request, &progress, found);
            advance(state, id, Gauge::counting(progress.visited as u64, Unit::Folders), details);
        }
        Some((true, progress, at, false, found, request)) if at == generation && !progress.cancelled => {
            let details = search_details(&request, &progress, found);
            advance(state, id, Gauge::counting(progress.visited as u64, Unit::Folders), details);
            let summary = tr!(job_search_done, found = borg_i18n::count(found, Count::Item));
            end(state, id, How::Done, summary, now);
        }
        _ => end(state, id, How::Cancelled, tr!(job_search_stopped).to_owned(), now),
    }
}

/// Stops the search job's search, while its panel still runs it.
pub(super) fn cancel_search(state: &mut AppState, side: Side, generation: u64) -> Vec<Effect> {
    if state.panel(side).generation() != generation || !state.panel(side).loading() {
        return Vec::new();
    }
    crate::results::cancel_search(state, side)
}

pub(crate) fn begin_sizes(state: &mut AppState, root: PathBuf, side: Side, now: Instant) -> u64 {
    let id = add(state, JobKind::Sizes, root.display().to_string(), side, Link::Sizes, now);
    super::files::reveal_folder(state, id, side, root.clone(), None);
    sizes_progress(state, id, &root, &SearchProgress::default());
    id
}

pub(crate) fn sizes_progress(state: &mut AppState, id: u64, root: &std::path::Path, progress: &SearchProgress) {
    let mut details = vec![
        Detail::new(tr!(@ job_detail_folder), Value::Path(root.to_path_buf())),
        Detail::new(tr!(@ job_detail_visited), Value::Number(progress.visited as u64)),
    ];
    if progress.errors > 0 {
        details.push(Detail::new(tr!(@ job_detail_unreadable), Value::Number(progress.errors as u64)));
    }
    advance(state, id, Gauge::counting(progress.visited as u64, Unit::Folders), details);
}

pub(crate) fn end_sizes(state: &mut AppState, id: u64, how: How, summary: String, now: Instant) {
    end(state, id, how, summary, now);
}

pub(crate) fn begin_index(state: &mut AppState, root: PathBuf, now: Instant) {
    let id = add(state, JobKind::Index, root.display().to_string(), state.active, Link::Index(root.clone()), now);
    advance(state, id, Gauge::default(), vec![Detail::new(tr!(@ job_detail_folder), Value::Path(root))]);
}

/// Ends the job building the index of `root`, and says whether one was building it.
pub(crate) fn end_index(state: &mut AppState, root: &std::path::Path, how: How, summary: String, now: Instant) -> bool {
    let building = state.jobs.iter().find(|job| job.running() && job.link == Link::Index(root.to_path_buf()));
    let Some(id) = building.map(|job| job.id) else { return false };
    end(state, id, how, summary, now);
    true
}

/// Follows the entropy scan: a run that starts adds a job, and the job ends with the run.
fn follow_entropy(state: &mut AppState, now: Instant) {
    let running = state.jobs.iter().find(|job| job.running() && job.link == Link::Entropy).map(|job| job.id);
    let scan = state.activity.entropy.as_ref();
    let (scanned, total, hits, remaining) = scan.map_or((0, 0, 0, 0), |scan| {
        (scan.report.scanned as u64, scan.total as u64, scan.report.hits.len() as u64, scan.report.remaining.len())
    });
    let scanning = scan.is_some_and(|scan| scan.running);
    let id = match (running, scanning) {
        (Some(id), _) => id,
        (None, true) => {
            let subject = scan.map(|scan| scan.scope.label()).unwrap_or_default().to_owned();
            let id = add(state, JobKind::Entropy, subject, state.active, Link::Entropy, now);
            if let Some(job) = super::find_mut(state, id) {
                job.reveal = Some(Reveal::EntropyHits);
            }
            id
        }
        (None, false) => return,
    };
    let details = vec![
        Detail::new(tr!(@ job_detail_scanned), Value::Of { done: scanned, total, unit: Unit::Processes }),
        Detail::new(tr!(@ job_detail_hits), Value::Number(hits)),
    ];
    advance(state, id, Gauge::of(scanned, total, Unit::Processes), details);
    if !scanning {
        let (how, summary) = if remaining > 0 {
            (How::Cancelled, tr!(job_entropy_stopped, hits = hits))
        } else {
            (How::Done, tr!(job_entropy_done, hits = hits))
        };
        end(state, id, how, summary, now);
    }
}

pub(super) fn sync(state: &mut AppState, now: Instant) {
    for side in Side::ALL {
        begin_search(state, side, now);
    }
    let searches: Vec<(u64, Side, u64)> = state
        .jobs
        .iter()
        .filter(|job| job.running())
        .filter_map(|job| match job.link {
            Link::Search { side, generation } => Some((job.id, side, generation)),
            _ => None,
        })
        .collect();
    for (id, side, generation) in searches {
        follow_search(state, id, side, generation, now);
    }
    follow_entropy(state, now);
}
