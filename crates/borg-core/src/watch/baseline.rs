//! What starting a program normally looks like on this machine: which parent starts which
//! program. Borg learns it for a while before it acts on anything, and the user can allow a
//! pair by hand.

use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    time::{Duration, SystemTime},
};

use super::{
    StartedProcess,
    rules::{Surveyor, looks_around},
};

/// Days Borg learns before it freezes anything.
pub const LEARNING_DAYS: u64 = 7;
/// The longest Borg learns, whatever the configuration asks for.
pub const LEARNING_DAYS_LIMIT: u64 = 365;
/// Days after learning ends that freezing waits for the user to see what Borg would have frozen
/// meanwhile. After that it starts anyway, so a machine nobody looks at is still protected.
pub const REVIEW_DAYS: u64 = 3;
const DAY_SECONDS: u64 = 24 * 60 * 60;
/// Pairs the baseline holds; a machine that starts more than this has stopped being a baseline.
pub const PAIRS_LIMIT: usize = 20_000;
/// Commands that look around the machine the baseline remembers, each with the program it came
/// from.
pub const SURVEYS_LIMIT: usize = 5_000;

/// A parent and a program it started, each as the program's path, or its name where Borg could
/// not read the path.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ParentChild {
    pub parent: String,
    pub child: String,
}

/// A start the user is content with, as the configuration or the Alerts page states it. Every
/// field that is set must match, and one with nothing set matches nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Allowance {
    pub program: Option<String>,
    pub parent: Option<String>,
    pub user: Option<String>,
}

impl Allowance {
    pub fn covers(&self, process: &StartedProcess) -> bool {
        let fields = [&self.program, &self.parent, &self.user];
        if fields.iter().all(|field| field.is_none()) {
            return false;
        }
        let matches_program = |pattern: &String| names_of(process).contains(pattern);
        let matches_parent = |pattern: &String| {
            process.parent().is_some_and(|parent| {
                parent.program.as_ref().map(|path| path.display().to_string()).as_ref() == Some(pattern)
                    || parent.program_name() == pattern
                    || parent.name == *pattern
            })
        };
        self.program.as_ref().is_none_or(matches_program)
            && self.parent.as_ref().is_none_or(matches_parent)
            && self.user.as_ref().is_none_or(|user| process.user == *user)
    }
}

/// The ways of naming a process's program, so an allowance may give either.
fn names_of(process: &StartedProcess) -> Vec<String> {
    let mut names = vec![process.program_name().to_owned(), process.name.clone()];
    if let Some(program) = &process.program {
        names.push(program.display().to_string());
    }
    names
}

/// How long Borg learns when the configuration asks for `days`.
pub fn learning_period(days: u64) -> Duration {
    Duration::from_secs(days.min(LEARNING_DAYS_LIMIT) * DAY_SECONDS)
}

/// When freezing starts on its own for a machine that finished learning at `learned_at`.
pub fn review_ends(learned_at: SystemTime) -> SystemTime {
    learned_at + Duration::from_secs(REVIEW_DAYS * DAY_SECONDS)
}

/// How a program is written in the baseline: its path, or the name it calls itself.
pub(super) fn written(program: Option<&std::path::Path>, name: &str) -> String {
    program.map_or_else(|| name.to_owned(), |path| path.display().to_string())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Baseline {
    /// When Borg started learning on this machine.
    pub since: SystemTime,
    pub period: Duration,
    pub pairs: BTreeSet<ParentChild>,
    #[serde(default)]
    pub allowed: Vec<Allowance>,
    /// The commands that look around the machine each program ran while Borg learned.
    #[serde(default)]
    pub surveys: BTreeSet<(Surveyor, String)>,
}

impl Default for Baseline {
    fn default() -> Self {
        Self::started(SystemTime::UNIX_EPOCH, learning_period(LEARNING_DAYS))
    }
}

impl Baseline {
    pub fn started(since: SystemTime, period: Duration) -> Self {
        Self { since, period, pairs: BTreeSet::new(), allowed: Vec::new(), surveys: BTreeSet::new() }
    }

    /// When the learning period ends.
    pub fn learned_at(&self) -> SystemTime {
        self.since + self.period
    }

    pub fn learning(&self, now: SystemTime) -> bool {
        now < self.learned_at()
    }

    /// When freezing starts even if nobody has seen what Borg would have frozen while it learned.
    pub fn review_ends(&self) -> SystemTime {
        review_ends(self.learned_at())
    }

    /// How much of the learning period is left.
    pub fn remaining(&self, now: SystemTime) -> Duration {
        self.learned_at().duration_since(now).unwrap_or_default()
    }

    pub fn pair(process: &StartedProcess) -> Option<ParentChild> {
        let parent = process.parent()?;
        Some(ParentChild {
            parent: written(parent.program.as_deref(), &parent.name),
            child: written(process.program.as_deref(), &process.name),
        })
    }

    /// Remembers that this parent starts this program here, and which program a command that
    /// looks around the machine came from.
    pub fn note(&mut self, process: &StartedProcess) {
        let program = process.program_name();
        if looks_around(program)
            && self.surveys.len() < SURVEYS_LIMIT
            && let Some(surveyor) = Surveyor::of(process)
        {
            self.surveys.insert((surveyor, program.to_owned()));
        }
        if self.pairs.len() >= PAIRS_LIMIT {
            return;
        }
        if let Some(pair) = Self::pair(process) {
            self.pairs.insert(pair);
        }
    }

    /// Whether this program ran this command that looks around the machine while Borg learned.
    pub fn surveyed(&self, surveyor: &Surveyor, command: &str) -> bool {
        self.surveys.contains(&(surveyor.clone(), command.to_owned()))
    }

    pub fn allows(&self, process: &StartedProcess) -> bool {
        self.allowed.iter().any(|allowance| allowance.covers(process))
    }

    /// Whether this start is one the machine does anyway, or one the user allowed.
    pub fn knows(&self, process: &StartedProcess) -> bool {
        self.allows(process) || Self::pair(process).is_some_and(|pair| self.pairs.contains(&pair))
    }

    /// Allows a start like this one from now on, once.
    pub fn allow(&mut self, allowance: Allowance) {
        if !self.allowed.contains(&allowance) {
            self.allowed.push(allowance);
        }
    }
}
