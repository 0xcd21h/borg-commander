use borg_i18n::tr;
use std::time::Instant;

use borg_jobs::{Ask, Decision, Event, Outcome, Progress, Spec, Totals};
use crossterm::event::{KeyCode, KeyEvent};

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
const PAUSE_KEY: char = 'p';
const CANCEL_KEY: char = 'c';

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobKind {
    Copy,
    Move,
    Delete,
}

impl JobKind {
    pub fn doing(self) -> &'static str {
        match self {
            Self::Copy => tr!(copying),
            Self::Move => tr!(moving),
            Self::Delete => tr!(deleting),
        }
    }

    pub fn done(self) -> &'static str {
        match self {
            Self::Copy => tr!(copied),
            Self::Move => tr!(moved),
            Self::Delete => tr!(deleted),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobState {
    Scanning(Totals),
    Running(Progress),
    Asking(Ask),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub id: u64,
    pub kind: JobKind,
    pub origin: Side,
    pub shown_on: Side,
    pub state: JobState,
    pub progress: Option<Progress>,
    pub paused: bool,
    pub started: Instant,
}

impl Job {
    pub fn fraction(&self) -> Option<f32> {
        let progress = self.progress.as_ref()?;
        let total = progress.total;
        let (done, all) = if total.bytes > 0 {
            (progress.done.bytes as f64, total.bytes as f64)
        } else {
            (progress.done.files as f64, total.files.max(1) as f64)
        };
        Some((done / all).clamp(0.0, 1.0) as f32)
    }

    pub fn choices(&self) -> &'static [(Decision, char, borg_i18n::Message)] {
        match self.state {
            JobState::Asking(Ask::Failure { .. }) => &FAILURE_CHOICES,
            JobState::Asking(Ask::Conflict { .. }) | JobState::Scanning(_) | JobState::Running(_) => &CONFLICT_CHOICES,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobCommand {
    Pause(bool),
    Cancel,
    Answer(Decision),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Question {
    pub id: u64,
    pub choice: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobList {
    pub selected: usize,
}

pub(crate) fn start(state: &mut AppState, kind: JobKind, spec: Spec, now: Instant) -> Vec<Effect> {
    let (id, mut effects) = begin(state, kind, now);
    effects.push(Effect::StartJob { id, spec });
    effects
}

/// Adds a job to the list and returns its id, for a job that some effect then runs.
pub(crate) fn begin(state: &mut AppState, kind: JobKind, now: Instant) -> (u64, Vec<Effect>) {
    let id = state.next_job;
    state.next_job += 1;
    let origin = state.active;
    let shown_on = match kind {
        JobKind::Copy | JobKind::Move => origin.other(),
        JobKind::Delete => origin,
    };
    state.jobs.push(Job {
        id,
        kind,
        origin,
        shown_on,
        state: JobState::Scanning(Totals::default()),
        progress: None,
        paused: false,
        started: now,
    });
    let effects = crate::measure::cancel(state);
    crate::measure::forget(state);
    (id, effects)
}

pub(crate) fn on_event(state: &mut AppState, id: u64, event: Event) -> Vec<Effect> {
    let Some(index) = state.jobs.iter().position(|job| job.id == id) else { return Vec::new() };
    match event {
        Event::Scanning(found) => state.jobs[index].state = JobState::Scanning(found),
        Event::Progress(progress) => {
            let job = &mut state.jobs[index];
            job.progress = Some(progress.clone());
            job.state = JobState::Running(progress);
        }
        Event::Asking(ask) => state.jobs[index].state = JobState::Asking(ask),
        Event::Finished(outcome) => return finish(state, index, outcome),
    }
    Vec::new()
}

fn finish(state: &mut AppState, index: usize, outcome: Outcome) -> Vec<Effect> {
    let job = state.jobs.remove(index);
    if matches!(state.overlay, Some(Overlay::Question(question)) if question.id == job.id) {
        state.overlay = None;
    }
    state.notice = Some(match outcome {
        Outcome::Done { done, skipped } => {
            if skipped == 0 {
                state.panel_mut(job.origin).clear_marks();
            }
            let skipped = if skipped > 0 { tr!(skipped, skipped = skipped) } else { String::new() };
            format!("{} {}{skipped}", job.kind.done(), summary(done))
        }
        Outcome::Cancelled { done } => tr!(cancelled_after, job.kind.doing(), summary(done)),
        Outcome::Failed(message) => tr!(job_failed, job.kind.doing(), message = message),
    });
    state.reload_all()
}

fn summary(totals: Totals) -> String {
    if totals.files == 1 { tr!(jobs_1_file).to_owned() } else { tr!(files, totals.files) }
}

pub(crate) fn surface_questions(state: &mut AppState) {
    if state.overlay.is_some() {
        return;
    }
    let asking = state.jobs.iter().find(|job| matches!(job.state, JobState::Asking(_)));
    if let Some(job) = asking {
        state.overlay = Some(Overlay::Question(Question { id: job.id, choice: 0 }));
    }
}

pub(crate) fn on_question_key(state: &mut AppState, question: Question, key: &KeyEvent) -> Vec<Effect> {
    let Some(job) = state.jobs.iter().find(|job| job.id == question.id) else {
        state.overlay = None;
        return Vec::new();
    };
    let choices = job.choices();
    let last = choices.len().saturating_sub(1);
    let decision = match key.code {
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
    if let Some(job) = state.jobs.iter_mut().find(|job| job.id == question.id) {
        job.state = job.progress.clone().map_or(JobState::Scanning(Totals::default()), JobState::Running);
    }
    vec![Effect::JobControl { id: question.id, command: JobCommand::Answer(decision) }]
}

pub(crate) fn open_list(state: &mut AppState) -> Vec<Effect> {
    if state.jobs.is_empty() {
        state.notice = Some(tr!(no_jobs_running).to_owned());
        return Vec::new();
    }
    state.overlay = Some(Overlay::Jobs(JobList { selected: 0 }));
    Vec::new()
}

pub(crate) fn on_list_key(state: &mut AppState, list: JobList, key: &KeyEvent) -> Vec<Effect> {
    let last = state.jobs.len().saturating_sub(1);
    let selected = list.selected.min(last);
    let mut effects = Vec::new();
    match key.code {
        KeyCode::Esc | KeyCode::Enter => {
            state.overlay = None;
            return effects;
        }
        KeyCode::Up => state.overlay = Some(Overlay::Jobs(JobList { selected: selected.saturating_sub(1) })),
        KeyCode::Down => state.overlay = Some(Overlay::Jobs(JobList { selected: (selected + 1).min(last) })),
        KeyCode::Char(PAUSE_KEY) | KeyCode::Char(' ') => {
            if let Some(job) = state.jobs.get_mut(selected) {
                job.paused = !job.paused;
                effects.push(Effect::JobControl { id: job.id, command: JobCommand::Pause(job.paused) });
            }
        }
        KeyCode::Char(CANCEL_KEY) | KeyCode::Delete => {
            if let Some(job) = state.jobs.get(selected) {
                effects.push(Effect::JobControl { id: job.id, command: JobCommand::Cancel });
            }
        }
        _ => {}
    }
    if state.jobs.is_empty() {
        state.overlay = None;
    }
    effects
}

pub(crate) fn cancel_all(state: &AppState) -> Vec<Effect> {
    state.jobs.iter().map(|job| Effect::JobControl { id: job.id, command: JobCommand::Cancel }).collect()
}
