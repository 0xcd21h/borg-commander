//! Sending a signal or a new priority to a process, after checking it is still the one chosen.

use super::running;
use anyhow::{Context, Result, bail};
use borg_core::{ControlOutcome, ControlReport, LEAST_NICE, NICEST, ProcessControl, ProcessKey, ProcessSignal};
use borg_i18n::tr;
use rustix::{
    io::Errno,
    process::{Pid, Signal, kill_process, setpriority_process},
};

fn signal(signal: ProcessSignal) -> Signal {
    match signal {
        ProcessSignal::Terminate => Signal::TERM,
        ProcessSignal::Interrupt => Signal::INT,
        ProcessSignal::HangUp => Signal::HUP,
        ProcessSignal::Kill => Signal::KILL,
        ProcessSignal::Stop => Signal::STOP,
        ProcessSignal::Continue => Signal::CONT,
    }
}

/// What the system's answer means for the monitor: refusals and a vanished process are outcomes
/// the monitor explains, anything else is an error.
fn outcome(result: rustix::io::Result<()>) -> Result<ControlOutcome> {
    match result {
        Ok(()) => Ok(ControlOutcome::Done),
        Err(Errno::PERM | Errno::ACCESS) => Ok(ControlOutcome::Denied),
        Err(Errno::SRCH) => Ok(ControlOutcome::Gone),
        Err(error) => Err(error.into()),
    }
}

pub(super) fn control(process: ProcessKey, control: ProcessControl) -> Result<ControlReport> {
    if process.pid == std::process::id() {
        bail!(tr!(process_control_own));
    }
    if let ProcessControl::Priority(nice) = control
        && !(LEAST_NICE..=NICEST).contains(&nice)
    {
        bail!(tr!(activity_request_invalid));
    }
    let pid = i32::try_from(process.pid).ok().and_then(Pid::from_raw).context(tr!(activity_request_invalid))?;
    let report = |outcome| ControlReport { process, control, outcome };
    if !running(process) {
        return Ok(report(ControlOutcome::Gone));
    }
    let result = match control {
        ProcessControl::Signal(chosen) => kill_process(pid, signal(chosen)),
        ProcessControl::Priority(nice) => setpriority_process(Some(pid), nice),
    };
    outcome(result).map(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{process::Command, thread, time::Duration};

    const SETTLE: Duration = Duration::from_millis(200);

    fn key(pid: u32) -> ProcessKey {
        super::super::identify(pid).expect("the child is running").0
    }

    #[test]
    fn signals_and_priority_reach_the_chosen_process_and_a_reused_pid_is_left_alone() {
        let mut child = Command::new("sleep").arg("30").spawn().unwrap();
        let process = key(child.id());
        let done = |control| control_outcome(process, control);
        assert_eq!(done(ProcessControl::Signal(ProcessSignal::Stop)), ControlOutcome::Done);
        assert_eq!(done(ProcessControl::Signal(ProcessSignal::Continue)), ControlOutcome::Done);
        assert_eq!(done(ProcessControl::Priority(NICEST)), ControlOutcome::Done);
        assert_eq!(rustix::process::getpriority_process(Pid::from_raw(child.id() as i32)).unwrap(), NICEST);
        let impostor = ProcessKey { start: process.start + 1, ..process };
        assert_eq!(control_outcome(impostor, ProcessControl::Signal(ProcessSignal::Kill)), ControlOutcome::Gone);
        thread::sleep(SETTLE);
        assert!(child.try_wait().unwrap().is_none(), "a different start time sends nothing");
        assert_eq!(done(ProcessControl::Signal(ProcessSignal::Terminate)), ControlOutcome::Done);
        assert!(!child.wait().unwrap().success());
        assert_eq!(done(ProcessControl::Signal(ProcessSignal::Terminate)), ControlOutcome::Gone);
    }

    #[test]
    fn borg_never_signals_itself_and_priorities_stay_in_range() {
        let own = key(std::process::id());
        assert!(control(own, ProcessControl::Signal(ProcessSignal::Kill)).is_err());
        let mut child = Command::new("sleep").arg("30").spawn().unwrap();
        let process = key(child.id());
        assert!(control(process, ProcessControl::Priority(NICEST + 1)).is_err());
        child.kill().unwrap();
        child.wait().unwrap();
    }

    #[test]
    fn refusals_and_vanished_processes_are_outcomes() {
        assert_eq!(outcome(Err(Errno::PERM)).unwrap(), ControlOutcome::Denied);
        assert_eq!(outcome(Err(Errno::ACCESS)).unwrap(), ControlOutcome::Denied);
        assert_eq!(outcome(Err(Errno::SRCH)).unwrap(), ControlOutcome::Gone);
        assert!(outcome(Err(Errno::INVAL)).is_err());
    }

    fn control_outcome(process: ProcessKey, control: ProcessControl) -> ControlOutcome {
        super::control(process, control).unwrap().outcome
    }
}
