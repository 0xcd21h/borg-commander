//! Freezing a process, letting it go on, and killing it. A frozen process has not exited, so
//! its service keeps running and nothing restarts it, and everything it was about to do waits
//! where it is.
//!
//! Every signal checks the process's identity first, so a number another process has taken in
//! the meantime is left alone, and nothing reaches the system's first process or Borg itself.

use borg_core::{ControlOutcome, ProcessKey};
use rustix::{
    io::Errno,
    process::{Pid, Signal, kill_process},
};

use super::facts::Processes;

/// The system's first process, which nothing here may signal.
const FIRST_PROCESS: u32 = 1;

/// What the system's answer means: a refusal and a process that has gone are outcomes the
/// Alerts page explains, and there is nothing else a signal can say.
fn outcome(result: rustix::io::Result<()>) -> ControlOutcome {
    match result {
        Ok(()) => ControlOutcome::Done,
        Err(Errno::PERM | Errno::ACCESS) => ControlOutcome::Denied,
        _ => ControlOutcome::Gone,
    }
}

/// Sends `signal` to one process, after reading its identity again.
fn signal_one(processes: &Processes, process: ProcessKey, signal: Signal) -> ControlOutcome {
    if process.pid == FIRST_PROCESS || process.pid == std::process::id() {
        return ControlOutcome::Denied;
    }
    let Some(pid) = i32::try_from(process.pid).ok().and_then(Pid::from_raw) else {
        return ControlOutcome::Gone;
    };
    if !processes.running(process) {
        return ControlOutcome::Gone;
    }
    outcome(kill_process(pid, signal))
}

/// Sends `signal` to a process and everything below it, the process first so that it cannot
/// start more children while its own are being signalled. What happens to the process itself
/// is the answer; a child that has gone meanwhile is no failure.
fn signal_tree(processes: &Processes, process: ProcessKey, signal: Signal) -> ControlOutcome {
    let answer = signal_one(processes, process, signal);
    if answer != ControlOutcome::Done {
        return answer;
    }
    for child in processes.tree(process.pid).into_iter().filter(|pid| *pid != process.pid) {
        if let Some(key) = processes.key(child) {
            let _ = signal_one(processes, key, signal);
        }
    }
    answer
}

/// Stops the process and its children where they are.
pub fn freeze(processes: &Processes, process: ProcessKey) -> ControlOutcome {
    signal_tree(processes, process, Signal::STOP)
}

/// Lets a frozen process and its children go on running.
pub fn resume(processes: &Processes, process: ProcessKey) -> ControlOutcome {
    signal_tree(processes, process, Signal::CONT)
}

/// Ends the process and its children. A frozen process cannot act on a polite signal until it
/// runs again, so this is the signal it cannot ignore.
pub fn kill(processes: &Processes, process: ProcessKey) -> ControlOutcome {
    signal_tree(processes, process, Signal::KILL)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "linux")]
    use std::{process::Command, thread, time::Duration};

    #[cfg(target_os = "linux")]
    const SETTLE: Duration = Duration::from_millis(200);

    #[cfg(target_os = "linux")]
    fn processes() -> Processes {
        Processes::default()
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn a_frozen_process_stops_where_it_is_runs_on_again_and_can_be_killed() {
        let processes = processes();
        let mut child = Command::new("sleep").arg("30").spawn().unwrap();
        let key = processes.key(child.id()).expect("the child is running");
        assert_eq!(freeze(&processes, key), ControlOutcome::Done);
        thread::sleep(SETTLE);
        assert!(child.try_wait().unwrap().is_none(), "a frozen process has not exited");
        assert_eq!(resume(&processes, key), ControlOutcome::Done);
        assert_eq!(kill(&processes, key), ControlOutcome::Done);
        assert!(!child.wait().unwrap().success());
        assert_eq!(freeze(&processes, key), ControlOutcome::Gone);
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn a_number_another_process_has_taken_is_left_alone() {
        let processes = processes();
        let mut child = Command::new("sleep").arg("30").spawn().unwrap();
        let key = processes.key(child.id()).expect("the child is running");
        let impostor = ProcessKey { start: key.start + 1, ..key };
        assert_eq!(freeze(&processes, impostor), ControlOutcome::Gone);
        thread::sleep(SETTLE);
        assert!(child.try_wait().unwrap().is_none(), "a different start time sends nothing");
        let _ = kill(&processes, key);
        let _ = child.wait();
    }

    #[test]
    fn borg_never_freezes_itself_or_the_systems_first_process() {
        let processes = Processes::default();
        let own = ProcessKey { pid: std::process::id(), start: 0 };
        assert_eq!(freeze(&processes, own), ControlOutcome::Denied);
        assert_eq!(kill(&processes, ProcessKey { pid: FIRST_PROCESS, start: 0 }), ControlOutcome::Denied);
    }
}
