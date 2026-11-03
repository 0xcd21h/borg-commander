//! Asking before work that would take long, unless an earlier answer says not to, and learning
//! from the time the work took.

use std::{
    path::Path,
    time::{Duration, Instant},
};

use borg_drives::{Drives, Work};

use crate::{Ask, Control, Decision, Event};

/// What a job is about to do, for the question it asks when that would take long.
#[derive(Debug, Clone)]
pub struct Pace<'a> {
    pub drives: &'a Drives,
    pub work: Work,
    /// The path the question names.
    pub path: &'a Path,
    /// Whether an answer may name `path` itself.
    pub file: bool,
    /// How long the work may take before Borg asks.
    pub patience: Duration,
}

impl Pace<'_> {
    /// Asks first when the work would take longer than the job's patience, and says whether to
    /// go ahead; the answer may keep Borg from asking again.
    pub fn ask(&self, control: &Control, emit: &mut dyn FnMut(Event)) -> bool {
        let Some(slowest) = self.drives.slowest(&self.work) else { return true };
        let estimate = self.drives.estimate(&self.work);
        let file = self.file.then_some(self.path);
        if estimate <= self.patience || !self.drives.asks(file, slowest.kind) {
            return true;
        }
        emit(Event::Asking(Ask::Slow {
            path: self.path.to_path_buf(),
            file: self.file,
            bytes: self.work.bytes(),
            estimate,
            kind: slowest.kind,
        }));
        match control.decide() {
            Decision::Proceed => true,
            Decision::Always(remember) => {
                self.drives.remember(remember, file, slowest.kind);
                true
            }
            _ => false,
        }
    }

    /// Learns from work that began at `started` and is done.
    pub fn learn(&self, started: Instant) {
        self.drives.learn(&self.work, started.elapsed());
    }
}
