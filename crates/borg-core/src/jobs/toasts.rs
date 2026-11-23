//! Toasts: small notes in a corner that a job started, ended or waits for an answer, or that a
//! panel entered a vault's folder. They stack, slide in and fade out, and never take keys. Alt-X
//! or a click on a toast's close mark closes it early.

use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use super::How;
use crate::{AppState, Motion, Overlay, Side};

/// A job that ends sooner than this gets no started toast.
const START_DELAY: Duration = Duration::from_secs(1);
const STARTED_STAYS: Duration = Duration::from_secs(3);
const DONE_STAYS: Duration = Duration::from_secs(6);
const CANCELLED_STAYS: Duration = Duration::from_secs(4);
const FAILED_STAYS: Duration = Duration::from_secs(10);
const VAULT_STAYS: Duration = Duration::from_secs(8);
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToastTopic {
    Job { id: u64, event: ToastEvent },
    Vault { side: Side, cipher: PathBuf, open: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastId {
    Job(u64),
    Vault,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toast {
    pub topic: ToastTopic,
    /// When it shows.
    pub at: Instant,
}

impl Toast {
    pub fn id(&self) -> ToastId {
        match self.topic {
            ToastTopic::Job { id, .. } => ToastId::Job(id),
            ToastTopic::Vault { .. } => ToastId::Vault,
        }
    }

    pub fn job(&self) -> Option<u64> {
        match self.topic {
            ToastTopic::Job { id, .. } => Some(id),
            ToastTopic::Vault { .. } => None,
        }
    }

    fn event(&self) -> Option<ToastEvent> {
        match self.topic {
            ToastTopic::Job { event, .. } => Some(event),
            ToastTopic::Vault { .. } => None,
        }
    }

    fn stays(&self) -> Option<Duration> {
        match self.event() {
            Some(ToastEvent::Started) => Some(STARTED_STAYS),
            Some(ToastEvent::Asking) => None,
            Some(ToastEvent::Ended(How::Done)) => Some(DONE_STAYS),
            Some(ToastEvent::Ended(How::Cancelled)) => Some(CANCELLED_STAYS),
            Some(ToastEvent::Ended(How::Failed)) => Some(FAILED_STAYS),
            None => Some(VAULT_STAYS),
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

fn job_toast(id: u64, event: ToastEvent, at: Instant) -> Toast {
    Toast { topic: ToastTopic::Job { id, event }, at }
}

pub(super) fn started(state: &mut AppState, job: u64, now: Instant) {
    if on(state) {
        state.toasts.push(job_toast(job, ToastEvent::Started, now + START_DELAY));
    }
}

pub(super) fn ended(state: &mut AppState, job: u64, how: How, now: Instant) {
    forget(state, job);
    if on(state) {
        state.toasts.push(job_toast(job, ToastEvent::Ended(how), now));
        keep_most(state, now);
    }
}

/// Shows that the panel on `side` entered the vault folder `cipher`, in place of an earlier
/// such toast, unless the toasts are off.
pub(super) fn vault(state: &mut AppState, side: Side, cipher: PathBuf, open: bool, now: Instant) -> bool {
    if !on(state) {
        return false;
    }
    close(state, ToastId::Vault);
    state.toasts.push(Toast { topic: ToastTopic::Vault { side, cipher, open }, at: now });
    keep_most(state, now);
    true
}

pub(super) fn forget(state: &mut AppState, job: u64) {
    state.toasts.retain(|toast| toast.job() != Some(job));
}

/// Closes the toast `id`. A question its job asked waits in the jobs panel.
pub(super) fn close(state: &mut AppState, id: ToastId) {
    state.toasts.retain(|toast| toast.id() != id);
    if let ToastId::Job(job) = id
        && let Some(asking) = super::find_mut(state, job).filter(|found| found.asking())
    {
        asking.put_off = true;
    }
}

/// Closes every toast that shows now. One still to come shows when its time comes.
pub(super) fn close_all(state: &mut AppState, now: Instant) {
    let shown: Vec<ToastId> = visible(state, now).iter().map(|toast| toast.id()).collect();
    shown.into_iter().for_each(|id| close(state, id));
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
    let asking: Vec<u64> = state
        .jobs
        .iter()
        .filter(|job| job.asking() && !job.put_off && !question_open(state, job.id))
        .map(|job| job.id)
        .collect();
    let jobs: Vec<u64> = state.jobs.iter().map(|job| job.id).collect();
    let shown: Vec<PathBuf> = Side::ALL.into_iter().map(|side| state.panel(side).destination().to_path_buf()).collect();
    state.toasts.retain(|toast| {
        let current = toast.ends().is_none_or(|ends| now < ends);
        current
            && match &toast.topic {
                ToastTopic::Job { id, event } => {
                    jobs.contains(id) && (*event != ToastEvent::Asking || asking.contains(id))
                }
                ToastTopic::Vault { side, cipher, .. } => shown[side.index()] == *cipher,
            }
    });
    for job in asking {
        let waits = ToastTopic::Job { id: job, event: ToastEvent::Asking };
        if !state.toasts.iter().any(|toast| toast.topic == waits) {
            forget(state, job);
            state.toasts.push(Toast { topic: waits, at: now });
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
