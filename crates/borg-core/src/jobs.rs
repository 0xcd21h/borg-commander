//! Background jobs: what each one works on, how far it got, how it ended, and what opening its
//! result shows. Every kind fills the same gauge and detail rows, so the jobs panel, the toasts,
//! the panel footers and the shortcut bar draw any job the same way.

use std::{
    path::{Path, PathBuf},
    time::Instant,
};

use borg_i18n::{Message, tr};
use borg_jobs::Ask;

use crate::{AppState, Effect, Overlay, Side};

mod files;
mod ports;
mod sheet;
mod toasts;
mod watched;

pub(crate) use files::{
    begin, begin_on, begin_view, cancel_opening, on_event, on_question_key, reveal_folder, rewrite_subject, start,
    subject,
};
pub(crate) use ports::{Scanning, begin_port_scan, end_port_scan, port_scan_progress};
pub(crate) use sheet::{on_list_key, on_row, open_list, open_newest_result};
pub use toasts::{MOST as MOST_TOASTS, Toast, ToastEvent};
pub(crate) use watched::{begin_index, begin_sizes, end_index, end_sizes, reveal_results, sizes_progress};

/// Finished jobs the panel keeps before the oldest of them go.
const KEPT: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobKind {
    Copy,
    Move,
    Delete,
    /// Files copied out of an archive.
    Unpack,
    /// A file unpacked from an archive for the viewer or the editor.
    View,
    /// Files copied into an archive.
    Pack,
    /// A folder made in an archive.
    ArchiveFolder,
    /// A file edited from an archive, written back into it.
    Save,
    /// Text replaced in the files a contents search found.
    Replace,
    /// Folder sizes counted in the background.
    Sizes,
    /// A search that fills a results panel.
    Search,
    /// The entropy scan of every process.
    Entropy,
    /// The first build of a folder's search index.
    Index,
    /// A host's TCP ports tried one by one.
    PortScan,
    /// Broadcast and multicast traffic heard on an interface for the Neighbors map.
    Listen,
}

impl JobKind {
    pub fn doing(self) -> &'static str {
        match self {
            Self::Copy => tr!(copying),
            Self::Move => tr!(moving),
            Self::Delete => tr!(deleting),
            Self::Unpack | Self::View => tr!(unpacking),
            Self::Pack => tr!(packing),
            Self::ArchiveFolder => tr!(making_archive_folder),
            Self::Save => tr!(saving_into_archive),
            Self::Replace => tr!(replacing_text),
            Self::Sizes => tr!(counting_sizes),
            Self::Search => tr!(searching),
            Self::Entropy => tr!(scanning_processes),
            Self::Index => tr!(indexing),
            Self::PortScan => tr!(scanning_ports),
            Self::Listen => tr!(listening),
        }
    }

    /// The job's name in the jobs panel, before what it works on.
    pub fn title(self) -> &'static str {
        match self {
            Self::Copy => tr!(job_copy),
            Self::Move => tr!(job_move),
            Self::Delete => tr!(job_delete),
            Self::Unpack | Self::View => tr!(job_unpack),
            Self::Pack => tr!(job_pack),
            Self::ArchiveFolder => tr!(job_archive_folder),
            Self::Save => tr!(job_save),
            Self::Replace => tr!(job_replace),
            Self::Sizes => tr!(job_sizes),
            Self::Search => tr!(job_search),
            Self::Entropy => tr!(job_entropy),
            Self::Index => tr!(job_index),
            Self::PortScan => tr!(job_port_scan),
            Self::Listen => tr!(job_listen),
        }
    }

    /// Whether finishing says nothing unless something went wrong: the panel or the editor
    /// already shows what changed.
    fn quiet(self) -> bool {
        matches!(self, Self::ArchiveFolder | Self::Save)
    }

    /// Whether the job's end leaves a notice while toasts are off. A search and the entropy scan
    /// show how they ended where their results are, and a port scan in its own dialog or notice.
    fn notices(self) -> bool {
        !matches!(self, Self::Search | Self::Entropy | Self::PortScan)
    }

    /// Whether the job is part of opening a file, which neither toasts nor stays once done.
    fn hidden(self) -> bool {
        self == Self::View
    }

    /// Whether the panel the job works in shows its progress in the footer. The other kinds
    /// show theirs where they run: the results, the folder sizes, the monitor.
    pub fn in_footer(self) -> bool {
        !matches!(self, Self::Sizes | Self::Search | Self::Entropy | Self::Index)
    }

    /// Whether the job can wait and go on again. Listening hears only what is said while it
    /// runs, so it stops instead.
    pub fn pauses(self) -> bool {
        self.in_footer() && self != Self::Listen
    }

    /// Whether the job can be stopped before it ends.
    pub fn cancels(self) -> bool {
        self != Self::Index
    }

    /// The panel that shows the job's progress: the one it writes into.
    fn shown_on(self, origin: Side) -> Side {
        match self {
            Self::Copy | Self::Move | Self::Unpack | Self::Pack => origin.other(),
            _ => origin,
        }
    }
}

/// What a gauge counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Bytes,
    Files,
    Folders,
    Processes,
    Ports,
    Seconds,
    Hosts,
}

/// How far a job got: what it did, and of how much when that is known.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Gauge {
    pub done: u64,
    pub total: Option<u64>,
    pub unit: Option<Unit>,
}

impl Gauge {
    pub fn of(done: u64, total: u64, unit: Unit) -> Self {
        Self { done, total: Some(total), unit: Some(unit) }
    }

    pub fn counting(done: u64, unit: Unit) -> Self {
        Self { done, total: None, unit: Some(unit) }
    }

    pub fn fraction(&self) -> Option<f32> {
        let total = self.total?;
        Some((self.done as f64 / total.max(1) as f64).clamp(0.0, 1.0) as f32)
    }
}

/// What a row of a job's details says, which the screen formats.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Text(String),
    Path(PathBuf),
    Number(u64),
    /// How much of a total, in `unit`.
    Of {
        done: u64,
        total: u64,
        unit: Unit,
    },
}

/// One row of a job's details: a name and what it says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detail {
    pub label: Message,
    pub value: Value,
}

impl Detail {
    pub fn new(label: Message, value: Value) -> Self {
        Self { label, value }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum How {
    Done,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobEnding {
    pub how: How,
    pub at: Instant,
    /// What the job did or why it stopped, in a line.
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase {
    /// Finding out how much there is to do.
    Preparing,
    Running,
    Asking(Ask),
    Ended(JobEnding),
}

/// What opening a finished job shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reveal {
    /// A folder in the panel on `side`, with the cursor on `focus`.
    Folder { side: Side, dir: PathBuf, focus: Option<String> },
    /// A search's results, in the panel on `side` or brought back to it.
    Results { side: Side, request: Box<crate::SearchRequest> },
    /// The hits of the entropy scan in the activity monitor.
    EntropyHits,
    /// The open ports a port scan found.
    PortScan(Box<crate::PortScanRecord>),
    /// The hosts listening heard, on the Neighbors page.
    Neighbors,
}

/// How a job is steered: what its pause and cancel turn into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Link {
    /// The app's runner, through `Effect::JobControl`.
    Runner,
    /// A search filling the results panel on `side` as listing `generation`.
    Search { side: Side, generation: u64 },
    /// The folder sizes being counted.
    Sizes,
    /// The entropy scan of every process.
    Entropy,
    /// The index of a folder being built.
    Index(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub id: u64,
    pub kind: JobKind,
    /// What the job works on: the files and where they go, a folder, a search.
    pub subject: String,
    pub origin: Side,
    pub shown_on: Side,
    pub started: Instant,
    pub paused: bool,
    pub folded: bool,
    pub phase: Phase,
    pub gauge: Gauge,
    pub details: Vec<Detail>,
    /// The file the viewer waits for, which the job unpacks.
    pub opens: Option<PathBuf>,
    pub reveal: Option<Reveal>,
    /// Its question waits for Alt-J or A in the panel, because you closed the note that asked.
    pub put_off: bool,
    pub(crate) link: Link,
}

impl Job {
    pub fn fraction(&self) -> Option<f32> {
        self.gauge.fraction()
    }

    pub fn running(&self) -> bool {
        !matches!(self.phase, Phase::Ended(_))
    }

    pub fn asking(&self) -> bool {
        matches!(self.phase, Phase::Asking(_))
    }

    pub fn ending(&self) -> Option<&JobEnding> {
        match &self.phase {
            Phase::Ended(ending) => Some(ending),
            _ => None,
        }
    }

    /// When the job stopped working, or `now` while it still works.
    pub fn until(&self, now: Instant) -> Instant {
        self.ending().map_or(now, |ending| ending.at)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobCommand {
    Pause(bool),
    Cancel,
    Answer(borg_jobs::Decision),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Question {
    pub id: u64,
    pub choice: usize,
}

/// The jobs panel and the job its cursor is on, counted in the order `shown` gives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobList {
    pub selected: usize,
}

/// The jobs in the order the panel lists them: those still working, oldest first, then the
/// finished ones, newest first.
pub fn shown(jobs: &[Job]) -> Vec<&Job> {
    let mut ended: Vec<&Job> = jobs.iter().filter(|job| !job.running()).collect();
    ended.sort_by_key(|job| std::cmp::Reverse(job.ending().map(|ending| ending.at)));
    jobs.iter().filter(|job| job.running()).chain(ended).collect()
}

/// Adds a job whose progress shows in the panel on `shown_on`, and returns its id. A started
/// toast follows unless it ends first.
pub(crate) fn add(
    state: &mut AppState,
    kind: JobKind,
    subject: String,
    shown_on: Side,
    link: Link,
    now: Instant,
) -> u64 {
    let id = state.next_job;
    state.next_job += 1;
    state.jobs.push(Job {
        id,
        kind,
        subject,
        origin: state.active,
        shown_on,
        started: now,
        paused: false,
        folded: true,
        phase: Phase::Preparing,
        gauge: Gauge::default(),
        details: Vec::new(),
        opens: None,
        reveal: None,
        put_off: false,
        link,
    });
    if !kind.hidden() && !kind.quiet() {
        toasts::started(state, id, now);
    }
    id
}

pub(crate) fn find_mut(state: &mut AppState, id: u64) -> Option<&mut Job> {
    state.jobs.iter_mut().find(|job| job.id == id)
}

/// Takes in how far job `id` got.
pub(crate) fn advance(state: &mut AppState, id: u64, gauge: Gauge, details: Vec<Detail>) {
    let Some(job) = find_mut(state, id).filter(|job| job.running()) else { return };
    job.gauge = gauge;
    job.details = details;
    if job.phase == Phase::Preparing {
        job.phase = Phase::Running;
    }
}

/// Ends job `id`. A finished job stays in the panel with what it did and a toast says so; with
/// the toasts off, a notice does. A job that is part of opening a file just goes.
pub(crate) fn end(state: &mut AppState, id: u64, how: How, summary: String, now: Instant) {
    let Some(index) = state.jobs.iter().position(|job| job.id == id && job.running()) else { return };
    if matches!(state.overlay, Some(Overlay::Question(question)) if question.id == id) {
        state.overlay = None;
    }
    let job = &mut state.jobs[index];
    if job.kind.hidden() {
        state.jobs.remove(index);
        toasts::forget(state, id);
        return;
    }
    job.paused = false;
    job.folded = how != How::Failed;
    job.phase = Phase::Ended(JobEnding { how, at: now, summary: summary.clone() });
    let notices = job.kind.notices();
    toasts::ended(state, id, how, now);
    if !state.settings.preferences.job_notifications && notices {
        state.notice = Some(summary);
    }
    keep_newest(state);
    close_empty_list(state);
}

/// Drops a job that ended with nothing to say, such as a quiet archive change.
pub(crate) fn drop_job(state: &mut AppState, id: u64) {
    state.jobs.retain(|job| job.id != id);
    toasts::forget(state, id);
    close_empty_list(state);
}

fn keep_newest(state: &mut AppState) {
    let mut ended: Vec<(Instant, u64)> =
        state.jobs.iter().filter_map(|job| job.ending().map(|ending| (ending.at, job.id))).collect();
    if ended.len() <= KEPT {
        return;
    }
    ended.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
    let gone: Vec<u64> = ended.split_off(KEPT).into_iter().map(|(_, id)| id).collect();
    state.jobs.retain(|job| !gone.contains(&job.id));
    state.toasts.retain(|toast| !gone.contains(&toast.job));
}

fn close_empty_list(state: &mut AppState) {
    if let Some(Overlay::Jobs(list)) = state.overlay {
        let last = shown(&state.jobs).len().saturating_sub(1);
        state.overlay = Some(Overlay::Jobs(JobList { selected: list.selected.min(last) }));
    }
}

/// Stops job `id` the way its kind stops: the runner's control, or the search, size count or
/// scan it stands for.
pub(crate) fn cancel(state: &mut AppState, id: u64, now: Instant) -> Vec<Effect> {
    let Some(job) = state.jobs.iter().find(|job| job.id == id && job.running()) else { return Vec::new() };
    match job.link.clone() {
        Link::Runner => vec![Effect::JobControl { id, command: JobCommand::Cancel }],
        Link::Search { side, generation } => watched::cancel_search(state, side, generation),
        Link::Sizes => crate::measure::cancel(state, now),
        Link::Entropy => vec![Effect::StopEntropyScan],
        Link::Index(_) => Vec::new(),
    }
}

pub(crate) fn cancel_all(state: &mut AppState, now: Instant) -> Vec<Effect> {
    let running: Vec<u64> = state.jobs.iter().filter(|job| job.running()).map(|job| job.id).collect();
    running.into_iter().flat_map(|id| cancel(state, id, now)).collect()
}

/// Keeps the jobs that follow other state up with it, opens a waiting question when it may
/// show, and lets toasts go once their time is up.
pub(crate) fn sync(state: &mut AppState, now: Instant) {
    watched::sync(state, now);
    files::surface_questions(state, now);
    toasts::sync(state, now);
    state.jobs_wake = wake_at(state, now);
}

fn wake_at(state: &AppState, now: Instant) -> Option<Instant> {
    [files::question_wakes_at(state), toasts::wake_at(state, now)].into_iter().flatten().min()
}

pub(crate) fn animating(state: &AppState, now: Instant) -> bool {
    toasts::animating(state, now)
}

/// The name a path shows in a job: its last part.
fn name(path: &Path) -> String {
    path.file_name().map_or_else(|| path.display().to_string(), |name| name.to_string_lossy().into_owned())
}

/// The jobs still working.
pub fn running(jobs: &[Job]) -> impl Iterator<Item = &Job> {
    jobs.iter().filter(|job| job.running())
}

/// Whether the keys a toast names reach the panels: the full-screen console, the viewer and
/// the editors keep Alt-J and Alt-G for themselves.
pub fn toast_keys_work(state: &AppState) -> bool {
    state.effective_view() == crate::View::Panels && state.viewer.is_none() && crate::editor::focused(state).is_none()
}

/// Toasts that show now, oldest first.
pub fn visible_toasts(state: &AppState, now: Instant) -> Vec<&Toast> {
    toasts::visible(state, now)
}

/// Closes the toast of job `id`. The job stays in the panel, and a question it asks waits there.
pub(crate) fn close_toast(state: &mut AppState, id: u64) {
    toasts::close(state, id);
}

/// Alt-X: closes every toast that shows.
pub(crate) fn close_toasts(state: &mut AppState, now: Instant) {
    toasts::close_all(state, now);
}
