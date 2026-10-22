//! When background work started, so a spinner appears only once work takes a while.

use std::time::{Duration, Instant};

use crate::{AppState, Listing, Side};

/// Work that finishes sooner never shows a spinner, so quick refreshes do not flicker.
pub const SPINNER_DELAY: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Task {
    /// A panel reading a folder or searching.
    Listing(Side),
    /// A job, shell operation, or size scan reported in a panel footer.
    Footer(Side),
    /// The activity monitor waiting for a sample, memory map, capture, or search.
    Activity,
    /// The Docker view loading its first list, or running a start, stop, or restart.
    Docker,
    /// The quick view or Info pane preparing its contents.
    Preview,
}

#[derive(Debug, Clone, Default)]
pub struct Busy {
    since: Vec<(Task, Instant)>,
    reveal: Option<Instant>,
}

impl Busy {
    pub fn since(&self, task: Task) -> Option<Instant> {
        self.since.iter().find(|(t, _)| *t == task).map(|(_, start)| *start)
    }

    /// When the task's spinner appeared, once it has run for [`SPINNER_DELAY`].
    pub fn shown_since(&self, task: Task, now: Instant) -> Option<Instant> {
        self.since(task).map(|start| start + SPINNER_DELAY).filter(|&shown| now >= shown)
    }

    pub fn any_shown(&self, now: Instant) -> bool {
        self.since.iter().any(|&(_, start)| now >= start + SPINNER_DELAY)
    }

    /// The next moment a spinner becomes due, so the screen can be redrawn then.
    pub fn reveal_at(&self) -> Option<Instant> {
        self.reveal
    }
}

pub(crate) fn sync(state: &mut AppState, now: Instant) {
    let mut active = Vec::new();
    for side in Side::ALL {
        if state.panel(side).listing() == &Listing::Loading {
            active.push(Task::Listing(side));
        }
        let job = state.jobs.iter().any(|job| job.shown_on == side);
        let operation = state.operation.as_ref().is_some_and(|op| state.shell.running && op.side == side);
        let sizes = state.sizing.as_ref().is_some_and(|scan| scan.side == side);
        if job || operation || sizes {
            active.push(Task::Footer(side));
        }
    }
    if state.activity.side.is_some() && state.activity.pending.is_some() {
        active.push(Task::Activity);
    }
    let docker = &state.docker;
    if docker.is_open() && (!docker.loaded && docker.pending.is_some() || !docker.working.is_empty()) {
        active.push(Task::Docker);
    }
    if state.aside.is_some_and(|aside| state.peeked.as_ref().is_none_or(|peeked| peeked.aside != aside)) {
        active.push(Task::Preview);
    }
    let busy = &mut state.busy;
    busy.since.retain(|(task, _)| active.contains(task));
    for task in active {
        if busy.since(task).is_none() {
            busy.since.push((task, now));
        }
    }
    busy.reveal = busy.since.iter().map(|&(_, start)| start + SPINNER_DELAY).filter(|&at| at > now).min();
}
