//! Telling a host that limits the scan from one that has nothing to say. A port that answered
//! once is the canary: the scan asks it again every few seconds, and when it falls silent twice
//! in a row the host, or a firewall in front of it, has stopped answering this scanner. A sudden
//! run of silent ports after a stretch of answers makes the scan ask the canary at once.

use std::collections::VecDeque;

use super::{PortAnswer, PortRange, ranges};

/// How many silent ports in a row, right after a stretch that answered, count as sudden.
pub const SILENT_RUN: usize = 40;
/// How many of the ports just before the run must have answered.
const ANSWERED_BEFORE: usize = SILENT_RUN / 2;
/// How many silent canary answers in a row stop the scan: the miss and its retry.
const CANARY_MISSES: u8 = 2;

/// What the scan does after the canary answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Canary {
    Fine,
    /// Silent once: ask again at once.
    Retry,
    /// Silent twice: stop the scan.
    Stop {
        port: u16,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Watch {
    canary: Option<(u16, PortAnswer)>,
    misses: u8,
    recent: VecDeque<bool>,
}

impl Watch {
    /// The port to ask again, once one answered: an open one when there is one.
    pub fn canary(&self) -> Option<u16> {
        self.canary.map(|(port, _)| port)
    }

    /// Notes a port's answer, and says whether a sudden run of silence calls for the canary now.
    pub fn record(&mut self, port: u16, answer: PortAnswer) -> bool {
        match (self.canary, answer) {
            (_, PortAnswer::Filtered) => {}
            (None, _) | (Some((_, PortAnswer::Closed)), PortAnswer::Open) => self.canary = Some((port, answer)),
            _ => {}
        }
        self.recent.push_back(answer != PortAnswer::Filtered);
        if self.recent.len() > SILENT_RUN * 2 {
            self.recent.pop_front();
        }
        self.sudden()
    }

    fn sudden(&self) -> bool {
        if self.recent.len() < SILENT_RUN * 2 || self.canary.is_none() {
            return false;
        }
        let (before, run) = self.recent.iter().enumerate().fold((0, 0), |(before, run), (index, &answered)| {
            match (index < SILENT_RUN, answered) {
                (true, true) => (before + 1, run),
                (false, true) => (before, run + 1),
                _ => (before, run),
            }
        });
        before >= ANSWERED_BEFORE && run == 0
    }

    /// Notes the canary's answer. A silent run is forgotten once it answers, so the same run
    /// asks it only once.
    pub fn canary_answered(&mut self, answer: PortAnswer) -> Canary {
        let Some((port, _)) = self.canary else { return Canary::Fine };
        if answer != PortAnswer::Filtered {
            self.misses = 0;
            self.recent.clear();
            return Canary::Fine;
        }
        self.misses += 1;
        if self.misses >= CANARY_MISSES { Canary::Stop { port } } else { Canary::Retry }
    }
}

/// What a scan found so far, port by port in the order it asks them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tally {
    ports: Vec<u16>,
    answers: Vec<Option<PortAnswer>>,
}

impl Tally {
    pub fn new(ports: Vec<u16>) -> Self {
        let answers = vec![None; ports.len()];
        Self { ports, answers }
    }

    pub fn ports(&self) -> &[u16] {
        &self.ports
    }

    pub fn record(&mut self, index: usize, answer: PortAnswer) {
        if let Some(slot) = self.answers.get_mut(index) {
            *slot = Some(answer);
        }
    }

    pub fn done(&self) -> usize {
        self.answers.iter().flatten().count()
    }

    pub fn open(&self) -> Vec<u16> {
        self.found(PortAnswer::Open)
    }

    fn found(&self, wanted: PortAnswer) -> Vec<u16> {
        let answered = self.ports.iter().zip(&self.answers);
        let mut ports: Vec<u16> =
            answered.filter(|(_, answer)| **answer == Some(wanted)).map(|(port, _)| *port).collect();
        ports.sort_unstable();
        ports
    }

    fn count(&self, wanted: PortAnswer) -> usize {
        self.answers.iter().filter(|answer| **answer == Some(wanted)).count()
    }

    /// The ports the scan never got an answer for.
    pub fn not_scanned(&self) -> Vec<PortRange> {
        let mut ports: Vec<u16> = self
            .ports
            .iter()
            .zip(&self.answers)
            .filter(|(_, answer)| answer.is_none())
            .map(|(port, _)| *port)
            .collect();
        ports.sort_unstable();
        ranges(&ports)
    }

    pub fn result(&self, ending: super::Ending) -> super::PortScanResult {
        super::PortScanResult {
            open: self.open(),
            closed: self.count(PortAnswer::Closed),
            filtered: self.count(PortAnswer::Filtered),
            not_scanned: self.not_scanned(),
            ending,
        }
    }
}
