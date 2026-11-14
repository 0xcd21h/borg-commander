//! Jobs the app's runners do: copies, moves, deletes, archive changes and replacing text. Their
//! events fill the shared gauge, and their questions open as dialogs.

use std::{
    path::{Path, PathBuf},
    time::Instant,
};

use borg_drives::Remember;
use borg_i18n::{Count, tr};
use borg_jobs::{Ask, Decision, Event, Outcome, Progress, Spec, Totals};
use crossterm::event::{KeyCode, KeyEvent};

use super::{Detail, Gauge, How, Job, JobCommand, JobKind, Link, Phase, Question, Reveal, Unit, Value, find_mut, name};
use crate::{AppState, Effect, Overlay, Side};

const CONFLICT_CHOICES: [(Decision, char, borg_i18n::Message); 6] = [
    (Decision::Overwrite, 'o', tr!(@ overwrite)),
    (Decision::Skip, 's', tr!(@ skip)),
    (Decision::KeepBoth, 'k', tr!(@ keep_both)),
    (Decision::OverwriteAll, 'a', tr!(@ overwrite_all)),
    (Decision::SkipAll, 'n', tr!(@ skip_all)),
    (Decision::Cancel, 'c', tr!(@ cancel)),
];
const FAILURE_CHOICES: [(Decision, char, borg_i18n::Message); 4] = [
    (Decision::Retry, 'r', tr!(@ retry)),
    (Decision::Skip, 's', tr!(@ skip)),
    (Decision::SkipAll, 'a', tr!(@ skip_all)),
    (Decision::Cancel, 'c', tr!(@ cancel)),
];
const PROCEED_CHOICES: [(Decision, char, borg_i18n::Message); 2] =
    [(Decision::Proceed, 'u', tr!(@ unpack_anyway)), (Decision::Cancel, 'c', tr!(@ cancel))];
/// For long work on one file, such as an archive: an answer may stop Borg asking about it.
const SLOW_FILE_CHOICES: [(Decision, char, borg_i18n::Message); 4] = [
    (Decision::Proceed, 's', tr!(@ slow_start)),
    (Decision::Always(Remember::File), 'f', tr!(@ slow_always_file)),
    (Decision::Always(Remember::Kind), 'd', tr!(@ slow_always_kind)),
    (Decision::Cancel, 'c', tr!(@ cancel)),
];
const SLOW_CHOICES: [(Decision, char, borg_i18n::Message); 3] = [
    (Decision::Proceed, 's', tr!(@ slow_start)),
    (Decision::Always(Remember::Kind), 'd', tr!(@ slow_always_kind)),
    (Decision::Cancel, 'c', tr!(@ cancel)),
];
const NO_CHOICES: [(Decision, char, borg_i18n::Message); 0] = [];

impl Job {
    pub fn choices(&self) -> &'static [(Decision, char, borg_i18n::Message)] {
        match &self.phase {
            Phase::Asking(Ask::Conflict { .. }) => &CONFLICT_CHOICES,
            Phase::Asking(Ask::Failure { .. }) => &FAILURE_CHOICES,
            Phase::Asking(Ask::Expand { .. } | Ask::Room { .. }) => &PROCEED_CHOICES,
            Phase::Asking(Ask::Slow { file: true, .. }) => &SLOW_FILE_CHOICES,
            Phase::Asking(Ask::Slow { file: false, .. }) => &SLOW_CHOICES,
            Phase::Preparing | Phase::Running | Phase::Ended(_) => &NO_CHOICES,
        }
    }
}

/// What a file job works on: one file's name or how many, and where they go.
pub(crate) fn subject(sources: &[PathBuf], target: Option<&Path>) -> String {
    let what = match sources {
        [one] => name(one),
        many => borg_i18n::count(many.len(), Count::Item),
    };
    match target {
        Some(target) => tr!(job_subject_to, what = what, target = target.display()),
        None => what,
    }
}

/// What a change to the archive at `archive` works on.
pub(crate) fn rewrite_subject(archive: &Path) -> String {
    name(archive)
}

pub(crate) fn start(state: &mut AppState, kind: JobKind, spec: Spec, now: Instant) -> Vec<Effect> {
    let target = match &spec {
        Spec::Copy { target, .. } | Spec::Move { target, .. } => Some(target.clone()),
        Spec::Delete { .. } => None,
    };
    let (id, mut effects) = begin(state, kind, subject(spec.sources(), target.as_deref()), now);
    if let Some(target) = target {
        let shown_on = kind.shown_on(state.active);
        let (dir, focus) = landing(state.panel(shown_on).dir(), spec.sources(), &target);
        reveal_folder(state, id, shown_on, dir, focus);
    }
    effects.push(Effect::StartJob { id, spec });
    effects
}

/// Where copies of `sources` into `target` show: in `target` when it is the folder the panel
/// shows or several files go there, and beside a single copy under a new name otherwise.
fn landing(shown: &Path, sources: &[PathBuf], target: &Path) -> (PathBuf, Option<String>) {
    let first = sources.first().map(|source| name(source));
    match (sources, target.parent()) {
        ([_], Some(parent)) if target != shown => (parent.to_path_buf(), Some(name(target))),
        _ => (target.to_path_buf(), first),
    }
}

/// Lets opening the finished job show `dir` in the panel on `side`, when that panel shows this
/// machine's folders.
pub(crate) fn reveal_folder(state: &mut AppState, id: u64, side: Side, dir: PathBuf, focus: Option<String>) {
    let panel = state.panel(side);
    if panel.foreign() || panel.is_results() || panel.archive.is_some() {
        return;
    }
    if let Some(job) = find_mut(state, id) {
        job.reveal = Some(Reveal::Folder { side, dir, focus });
    }
}

/// Adds a job to the list and returns its id, for a job that some effect then runs.
pub(crate) fn begin(state: &mut AppState, kind: JobKind, subject: String, now: Instant) -> (u64, Vec<Effect>) {
    let shown_on = kind.shown_on(state.active);
    begin_on(state, kind, subject, shown_on, now)
}

/// Adds a job whose progress shows in the panel on `shown_on`.
pub(crate) fn begin_on(
    state: &mut AppState,
    kind: JobKind,
    subject: String,
    shown_on: Side,
    now: Instant,
) -> (u64, Vec<Effect>) {
    let id = super::add(state, kind, subject, shown_on, Link::Runner, now);
    if kind == JobKind::View {
        return (id, Vec::new());
    }
    let effects = crate::measure::cancel(state, now);
    crate::measure::forget(state);
    (id, effects)
}

/// Adds a job that unpacks the file the viewer waits for as `opens`.
pub(crate) fn begin_view(state: &mut AppState, opens: PathBuf, now: Instant) -> u64 {
    let (id, _) = begin(state, JobKind::View, name(&opens), now);
    if let Some(job) = find_mut(state, id) {
        job.opens = Some(opens);
    }
    id
}

pub(crate) fn on_event(state: &mut AppState, id: u64, event: Event, now: Instant) -> Vec<Effect> {
    let Some(job) = find_mut(state, id).filter(|job| job.running()) else { return Vec::new() };
    match event {
        Event::Scanning(found) => {
            job.phase = Phase::Preparing;
            job.gauge = Gauge::counting(found.files, Unit::Files);
        }
        Event::Progress(progress) => {
            let (gauge, details) = measure(job.kind, &progress);
            super::advance(state, id, gauge, details);
            if let Some(job) = find_mut(state, id).filter(|job| job.asking()) {
                job.phase = Phase::Running;
            }
        }
        Event::Asking(ask) => {
            job.folded = false;
            job.phase = Phase::Asking(ask);
        }
        Event::Finished(outcome) => return finish(state, id, outcome, now),
    }
    Vec::new()
}

/// A file job's gauge, in bytes when it knows how many, and its detail rows.
fn measure(kind: JobKind, progress: &Progress) -> (Gauge, Vec<Detail>) {
    let Progress { done, total, current } = progress;
    let gauge = if total.bytes > 0 {
        Gauge::of(done.bytes, total.bytes, Unit::Bytes)
    } else {
        Gauge::of(done.files, total.files, Unit::Files)
    };
    let mut details = vec![
        Detail::new(tr!(@ job_detail_now), Value::Path(current.clone())),
        Detail::new(tr!(@ job_detail_files), Value::Of { done: done.files, total: total.files, unit: Unit::Files }),
    ];
    if total.bytes > 0 && kind != JobKind::Delete {
        details.push(Detail::new(
            tr!(@ job_detail_size),
            Value::Of { done: done.bytes, total: total.bytes, unit: Unit::Bytes },
        ));
    }
    (gauge, details)
}

fn finish(state: &mut AppState, id: u64, outcome: Outcome, now: Instant) -> Vec<Effect> {
    let Some(job) = state.jobs.iter().find(|job| job.id == id).cloned() else { return Vec::new() };
    if let Some(path) = job.opens {
        let notice = match outcome {
            Outcome::Done { .. } => None,
            Outcome::Cancelled { done } => Some(tr!(cancelled_after, job.kind.doing(), summary(done))),
            Outcome::Failed(message) => Some(tr!(job_failed, job.kind.doing(), message = message)),
        };
        super::drop_job(state, id);
        if let Some(notice) = notice {
            crate::viewer::failed(state, path, notice);
        }
        return Vec::new();
    }
    let (how, line) = match outcome {
        Outcome::Done { skipped: 0, .. } if job.kind.quiet() => {
            super::drop_job(state, id);
            return state.reload_all();
        }
        Outcome::Done { done, skipped } => {
            if skipped == 0 {
                state.panel_mut(job.origin).clear_marks();
            }
            if let Some(job) = find_mut(state, id) {
                job.details.retain(|detail| detail.label != tr!(@ job_detail_now));
                if skipped > 0 {
                    job.details.push(Detail::new(tr!(@ job_detail_skipped), Value::Number(skipped)));
                }
            }
            let skipped = if skipped > 0 { tr!(skipped, skipped = skipped) } else { String::new() };
            let words: Vec<String> = job.kind.done().map(str::to_owned).into_iter().chain([summary(done)]).collect();
            (How::Done, format!("{}{skipped}", words.join(" ")))
        }
        Outcome::Cancelled { done } => (How::Cancelled, tr!(cancelled_after, job.kind.doing(), summary(done))),
        Outcome::Failed(message) => (How::Failed, tr!(job_failed, job.kind.doing(), message = message)),
    };
    super::end(state, id, how, line, now);
    state.reload_all()
}

impl JobKind {
    /// The word a runner's finished job starts its summary with.
    fn done(self) -> Option<&'static str> {
        match self {
            Self::Copy => Some(tr!(copied)),
            Self::Move => Some(tr!(moved)),
            Self::Delete => Some(tr!(deleted)),
            Self::Unpack | Self::View => Some(tr!(unpacked)),
            Self::Pack => Some(tr!(packed)),
            Self::ArchiveFolder | Self::Save => Some(tr!(archive_changed)),
            Self::Replace => Some(tr!(replaced_text)),
            Self::Sizes | Self::Search | Self::Entropy | Self::Index | Self::PortScan => None,
        }
    }
}

fn summary(totals: Totals) -> String {
    if totals.files == 1 { tr!(jobs_1_file).to_owned() } else { tr!(files, totals.files) }
}

/// Whether keys come in where a question would cut in: an editor, the console, or a command
/// being typed.
fn typing(state: &AppState) -> bool {
    crate::editor::focused(state).is_some() || state.effective_view() == crate::View::Shell || !state.shell.line_empty()
}

/// The asking job whose question opens next, unless keys are coming in where it would cut in.
fn waiting_question(state: &AppState) -> Option<&Job> {
    if state.overlay.is_some() {
        return None;
    }
    state.jobs.iter().find(|job| job.asking())
}

pub(crate) fn surface_questions(state: &mut AppState, now: Instant) {
    let Some(id) = waiting_question(state).map(|job| job.id) else { return };
    if typing(state) && now < state.idle_since + crate::update_offer::QUIET {
        return;
    }
    state.overlay = Some(Overlay::Question(Question { id, choice: 0 }));
}

/// Opens the question of the first asking job, as Alt-J does when one waits.
pub(crate) fn open_question(state: &mut AppState) -> bool {
    let Some(id) = state.jobs.iter().find(|job| job.asking()).map(|job| job.id) else { return false };
    state.overlay = Some(Overlay::Question(Question { id, choice: 0 }));
    true
}

pub(crate) fn question_wakes_at(state: &AppState) -> Option<Instant> {
    waiting_question(state).filter(|_| typing(state)).map(|_| state.idle_since + crate::update_offer::QUIET)
}

pub(crate) fn on_question_key(state: &mut AppState, question: Question, key: &KeyEvent) -> Vec<Effect> {
    let Some(job) = state.jobs.iter().find(|job| job.id == question.id && job.asking()) else {
        state.overlay = None;
        return Vec::new();
    };
    let choices = job.choices();
    let last = choices.len().saturating_sub(1);
    let decision = match crate::keyboard::latin(key).code {
        KeyCode::Esc => Some(Decision::Cancel),
        KeyCode::Enter => choices.get(question.choice).map(|(decision, _, _)| *decision),
        KeyCode::Char(c) => choices.iter().find(|(_, hotkey, _)| *hotkey == c).map(|(decision, _, _)| *decision),
        KeyCode::Left | KeyCode::Up | KeyCode::BackTab => {
            let choice = question.choice.checked_sub(1).unwrap_or(last);
            state.overlay = Some(Overlay::Question(Question { choice, ..question }));
            None
        }
        KeyCode::Right | KeyCode::Down | KeyCode::Tab => {
            let choice = if question.choice >= last { 0 } else { question.choice + 1 };
            state.overlay = Some(Overlay::Question(Question { choice, ..question }));
            None
        }
        _ => None,
    };
    let Some(decision) = decision else { return Vec::new() };
    state.overlay = None;
    if let Some(job) = find_mut(state, question.id) {
        job.phase = if job.gauge.total.is_some() { Phase::Running } else { Phase::Preparing };
    }
    vec![Effect::JobControl { id: question.id, command: JobCommand::Answer(decision) }]
}

/// Cancels the job that unpacks `path` for the viewer, which no longer waits for it.
pub(crate) fn cancel_opening(state: &AppState, path: &Path) -> Vec<Effect> {
    let opening = state.jobs.iter().filter(|job| job.running() && job.opens.as_deref() == Some(path));
    opening.map(|job| Effect::JobControl { id: job.id, command: JobCommand::Cancel }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_land_in_the_folder_or_beside_a_renamed_file() {
        let shown = Path::new("/var/log");
        let one = [PathBuf::from("/home/me/build.sh")];
        let two = [PathBuf::from("/home/me/a"), PathBuf::from("/home/me/b")];
        assert_eq!(landing(shown, &one, shown), (shown.to_path_buf(), Some("build.sh".into())));
        assert_eq!(landing(shown, &one, Path::new("/var/log/new.sh")), (shown.to_path_buf(), Some("new.sh".into())));
        assert_eq!(landing(shown, &two, Path::new("/tmp")), (PathBuf::from("/tmp"), Some("a".into())));
    }
}
