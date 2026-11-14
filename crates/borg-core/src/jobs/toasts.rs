//! Toasts: small notes in a corner that a job started, ended or waits for an answer. They
//! stack, slide in and fade out, and never take keys.

use std::time::{Duration, Instant};

use super::How;
use crate::{AppState, Motion, Overlay};

/// A job that ends sooner than this gets no started toast.
const START_DELAY: Duration = Duration::from_secs(1);
const STARTED_STAYS: Duration = Duration::from_secs(3);
const DONE_STAYS: Duration = Duration::from_secs(6);
const CANCELLED_STAYS: Duration = Duration::from_secs(4);
const FAILED_STAYS: Duration = Duration::from_secs(10);
/// Toasts shown at once; a newer one pushes the oldest out.
pub const MOST: usize = 3;
/// How many of the motion's fades a toast takes to fade out.
const FADE_STEPS: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastEvent {
    Started,
    /// The job asks a question that cannot open yet.
    Asking,
    Ended(How),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Toast {
    pub job: u64,
    pub event: ToastEvent,
    /// When it shows.
    pub at: Instant,
}

impl Toast {
    fn stays(&self) -> Option<Duration> {
        match self.event {
            ToastEvent::Started => Some(STARTED_STAYS),
            ToastEvent::Asking => None,
            ToastEvent::Ended(How::Done) => Some(DONE_STAYS),
            ToastEvent::Ended(How::Cancelled) => Some(CANCELLED_STAYS),
            ToastEvent::Ended(How::Failed) => Some(FAILED_STAYS),
        }
    }

    /// When it is gone, or `None` while it waits for its job.
    pub fn ends(&self) -> Option<Instant> {
        self.stays().map(|stays| self.at + stays)
    }

    /// How far it slid in, from 0 to 1.
    pub fn arrival(&self, now: Instant, motion: Motion) -> f32 {
        share(now.saturating_duration_since(self.at), motion.fade)
    }

    /// How much of it still shows as it fades out, from 1 to 0.
    pub fn presence(&self, now: Instant, motion: Motion) -> f32 {
        let Some(ends) = self.ends() else { return 1.0 };
        share(ends.saturating_duration_since(now), motion.fade * FADE_STEPS)
    }

    fn shows(&self, now: Instant) -> bool {
        self.at <= now && self.ends().is_none_or(|ends| now < ends)
    }
}

fn share(elapsed: Duration, span: Duration) -> f32 {
    if span.is_zero() { 1.0 } else { (elapsed.as_secs_f32() / span.as_secs_f32()).clamp(0.0, 1.0) }
}

fn on(state: &AppState) -> bool {
    state.settings.preferences.job_notifications
}

pub(super) fn started(state: &mut AppState, job: u64, now: Instant) {
    if on(state) {
        state.toasts.push(Toast { job, event: ToastEvent::Started, at: now + START_DELAY });
    }
}

pub(super) fn ended(state: &mut AppState, job: u64, how: How, now: Instant) {
    forget(state, job);
    if on(state) {
        state.toasts.push(Toast { job, event: ToastEvent::Ended(how), at: now });
        keep_most(state, now);
    }
}

pub(super) fn forget(state: &mut AppState, job: u64) {
    state.toasts.retain(|toast| toast.job != job);
}

/// Whether job `id`'s question shows or may open at once, so no toast needs to ask.
fn question_open(state: &AppState, id: u64) -> bool {
    matches!(state.overlay, Some(Overlay::Question(question)) if question.id == id)
}

pub(super) fn sync(state: &mut AppState, now: Instant) {
    if !on(state) {
        state.toasts.clear();
        return;
    }
    let asking: Vec<u64> =
        state.jobs.iter().filter(|job| job.asking() && !question_open(state, job.id)).map(|job| job.id).collect();
    let jobs: Vec<u64> = state.jobs.iter().map(|job| job.id).collect();
    state.toasts.retain(|toast| {
        let alive = jobs.contains(&toast.job) && toast.ends().is_none_or(|ends| now < ends);
        alive && (toast.event != ToastEvent::Asking || asking.contains(&toast.job))
    });
    for job in asking {
        if !state.toasts.iter().any(|toast| toast.job == job && toast.event == ToastEvent::Asking) {
            forget(state, job);
            state.toasts.push(Toast { job, event: ToastEvent::Asking, at: now });
        }
    }
    keep_most(state, now);
}

/// Lets the oldest shown toasts go while more than `MOST` show.
fn keep_most(state: &mut AppState, now: Instant) {
    while state.toasts.iter().filter(|toast| toast.at <= now).count() > MOST {
        let Some(oldest) =
            state.toasts.iter().enumerate().filter(|(_, toast)| toast.at <= now).min_by_key(|(_, toast)| toast.at)
        else {
            return;
        };
        let index = oldest.0;
        state.toasts.remove(index);
    }
}

pub(super) fn visible(state: &AppState, now: Instant) -> Vec<&Toast> {
    let mut shown: Vec<&Toast> = state.toasts.iter().filter(|toast| toast.shows(now)).collect();
    shown.sort_by_key(|toast| toast.at);
    shown
}

pub(super) fn wake_at(state: &AppState, now: Instant) -> Option<Instant> {
    let fade = state.motion().fade * FADE_STEPS;
    let moments = state.toasts.iter().flat_map(|toast| {
        let ends = toast.ends();
        [Some(toast.at), ends.map(|ends| ends - fade), ends]
    });
    moments.flatten().filter(|moment| *moment > now).min()
}

pub(super) fn animating(state: &AppState, now: Instant) -> bool {
    let motion = state.motion();
    visible(state, now).iter().any(|toast| toast.arrival(now, motion) < 1.0 || toast.presence(now, motion) < 1.0)
}
