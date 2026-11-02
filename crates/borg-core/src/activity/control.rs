//! Signals and priority changes the monitor sends to a process, and the dialog that picks one.

use super::{Activity, ActivityRequest, ProcessKey};
use crate::{AppState, Effect, Overlay};
use borg_i18n::tr;
use crossterm::event::{KeyCode, KeyEvent};
use serde::{Deserialize, Serialize};

/// The nicest a process can run, last in line for the CPU.
pub const NICEST: i32 = 19;
/// The least nice a process can run, first in line for the CPU.
pub const LEAST_NICE: i32 = -20;
/// The nice value processes start with.
pub const DEFAULT_NICE: i32 = 0;
/// How far one choice in the dialog moves a process's nice value.
pub const PRIORITY_STEP: i32 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProcessSignal {
    Terminate,
    Interrupt,
    HangUp,
    Kill,
    Stop,
    Continue,
}

impl ProcessSignal {
    /// The signal's name as `kill -l` lists it, the same in every language.
    pub fn name(self) -> &'static str {
        match self {
            Self::Terminate => "TERM",
            Self::Interrupt => "INT",
            Self::HangUp => "HUP",
            Self::Kill => "KILL",
            Self::Stop => "STOP",
            Self::Continue => "CONT",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProcessControl {
    Signal(ProcessSignal),
    /// Run at this nice value.
    Priority(i32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ControlOutcome {
    Done,
    /// The system lets only the process's owner or an administrator do it.
    Denied,
    /// The process ended, or its PID now belongs to another one.
    Gone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlReport {
    pub process: ProcessKey,
    pub control: ProcessControl,
    pub outcome: ControlOutcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlChoice {
    Signal(ProcessSignal),
    LowerPriority,
    RaisePriority,
}

pub const CONTROL_CHOICES: [ControlChoice; 8] = [
    ControlChoice::Signal(ProcessSignal::Terminate),
    ControlChoice::Signal(ProcessSignal::Interrupt),
    ControlChoice::Signal(ProcessSignal::HangUp),
    ControlChoice::Signal(ProcessSignal::Kill),
    ControlChoice::Signal(ProcessSignal::Stop),
    ControlChoice::Signal(ProcessSignal::Continue),
    ControlChoice::LowerPriority,
    ControlChoice::RaisePriority,
];

impl ControlChoice {
    /// The letter that picks this choice in the dialog.
    pub fn key(self) -> char {
        match self {
            Self::Signal(ProcessSignal::Terminate) => 't',
            Self::Signal(ProcessSignal::Interrupt) => 'i',
            Self::Signal(ProcessSignal::HangUp) => 'h',
            Self::Signal(ProcessSignal::Kill) => 'k',
            Self::Signal(ProcessSignal::Stop) => 's',
            Self::Signal(ProcessSignal::Continue) => 'c',
            Self::LowerPriority => 'l',
            Self::RaisePriority => 'r',
        }
    }
}

/// The dialog K opens on the selected process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlDialog {
    pub process: ProcessKey,
    pub name: String,
    /// The process's nice value, when the system shows it.
    pub nice: Option<i32>,
    pub selected: usize,
}

impl ControlDialog {
    /// What `choice` sends, or `None` when the priority is unknown or already at its limit.
    pub fn control(&self, choice: ControlChoice) -> Option<ProcessControl> {
        match choice {
            ControlChoice::Signal(signal) => Some(ProcessControl::Signal(signal)),
            ControlChoice::LowerPriority => self
                .nice
                .filter(|&nice| nice < NICEST)
                .map(|nice| ProcessControl::Priority((nice + PRIORITY_STEP).min(NICEST))),
            ControlChoice::RaisePriority => self
                .nice
                .filter(|&nice| nice > LEAST_NICE)
                .map(|nice| ProcessControl::Priority((nice - PRIORITY_STEP).max(LEAST_NICE))),
        }
    }
}

/// K opens the dialog on the selected process. Borg's own process is left alone.
pub(crate) fn open(state: &mut AppState) -> Vec<Effect> {
    let Some(process) = state.activity.process() else { return Vec::new() };
    if state.activity.sample.own == Some(process.key.pid) {
        state.activity.error = Some(tr!(process_control_own).into());
        return Vec::new();
    }
    state.overlay = Some(Overlay::ProcessControl(ControlDialog {
        process: process.key,
        name: process.name.clone(),
        nice: process.nice,
        selected: 0,
    }));
    Vec::new()
}

pub(crate) fn on_key(state: &mut AppState, mut dialog: ControlDialog, key: &KeyEvent) -> Vec<Effect> {
    let last = CONTROL_CHOICES.len() - 1;
    let choice = match key.code {
        KeyCode::Esc | KeyCode::F(10) => {
            state.overlay = None;
            return Vec::new();
        }
        KeyCode::Up => {
            dialog.selected = dialog.selected.saturating_sub(1);
            None
        }
        KeyCode::Down => {
            dialog.selected = (dialog.selected + 1).min(last);
            None
        }
        KeyCode::Home => {
            dialog.selected = 0;
            None
        }
        KeyCode::End => {
            dialog.selected = last;
            None
        }
        KeyCode::Enter => Some(CONTROL_CHOICES[dialog.selected]),
        KeyCode::Char(c) => CONTROL_CHOICES.iter().copied().find(|choice| choice.key() == c.to_ascii_lowercase()),
        _ => None,
    };
    let Some(control) = choice.and_then(|choice| dialog.control(choice)) else {
        state.overlay = Some(Overlay::ProcessControl(dialog));
        return Vec::new();
    };
    state.overlay = None;
    state.activity.request(ActivityRequest::Control { process: dialog.process, control, admin: false })
}

/// What the system said to a signal or priority change: a notice when it is done, and an error
/// that offers the administrator retry when only the owner or an administrator may do it.
pub(super) fn receive(activity: &mut Activity, request: ActivityRequest, report: ControlReport) -> Option<String> {
    let name = activity
        .sample
        .processes
        .iter()
        .find(|p| p.key == report.process)
        .map_or_else(|| report.process.pid.to_string(), |p| p.name.clone());
    let pid = report.process.pid;
    match report.outcome {
        ControlOutcome::Done => {
            activity.retry = None;
            activity.next = None;
            Some(match report.control {
                ProcessControl::Signal(signal) => {
                    tr!(process_signal_sent, signal = signal.name(), name = name, pid = pid)
                }
                ProcessControl::Priority(nice) => tr!(process_priority_set, name = name, pid = pid, nice = nice),
            })
        }
        ControlOutcome::Denied => {
            activity.error = Some(tr!(process_control_denied, name = name, pid = pid));
            activity.retry = (!activity.sample.administrator).then_some(request);
            None
        }
        ControlOutcome::Gone => {
            activity.error = Some(tr!(process_control_gone, name = name, pid = pid));
            activity.retry = None;
            None
        }
    }
}
