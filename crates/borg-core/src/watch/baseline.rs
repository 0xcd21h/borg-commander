//! What starting a program normally looks like on this machine: which parent starts which
//! program. Borg learns it for a while before it acts on anything, and the user can allow a
//! pair by hand.

use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    time::{Duration, SystemTime},
};

use super::StartedProcess;

/// Days Borg learns before it freezes anything.
pub const LEARNING_DAYS: u64 = 7;
/// Pairs the baseline holds; a machine that starts more than this has stopped being a baseline.
pub const PAIRS_LIMIT: usize = 20_000;

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

/// How a program is written in the baseline: its path, or the name it calls itself.
fn written(program: Option<&std::path::Path>, name: &str) -> String {
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
}

impl Default for Baseline {
    fn default() -> Self {
        Self::started(SystemTime::UNIX_EPOCH, Duration::from_secs(LEARNING_DAYS * 24 * 60 * 60))
    }
}

impl Baseline {
    pub fn started(since: SystemTime, period: Duration) -> Self {
        Self { since, period, pairs: BTreeSet::new(), allowed: Vec::new() }
    }

    /// When the learning period ends.
    pub fn learned_at(&self) -> SystemTime {
        self.since + self.period
    }

    pub fn learning(&self, now: SystemTime) -> bool {
        now < self.learned_at()
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

    /// Remembers that this parent starts this program here.
    pub fn note(&mut self, process: &StartedProcess) {
        if self.pairs.len() >= PAIRS_LIMIT {
            return;
        }
        if let Some(pair) = Self::pair(process) {
            self.pairs.insert(pair);
        }
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
