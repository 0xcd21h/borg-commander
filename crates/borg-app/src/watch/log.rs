//! Where the watcher keeps what it has found and what it has learned: a bounded log of alerts
//! and the baseline, both owner-only files in the state folder, which every Borg on the machine
//! reads and the watcher alone writes.

use anyhow::{Context, Result};
use borg_core::{ALERT_LOG, AlertLog, Allowance, Baseline, WATCH_ALLOWED, WATCH_BASELINE};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::SystemTime,
};

use crate::session::StateDir;

/// Bytes of the log Borg reads; [`borg_core::ALERTS_KEPT`] alerts with their evidence fit well
/// inside this.
const LOG_BYTES: u64 = 8 * 1024 * 1024;
/// Bytes of the baseline Borg reads; [`borg_core::PAIRS_LIMIT`] pairs fit inside this.
const BASELINE_BYTES: u64 = 16 * 1024 * 1024;

/// The state folder the watcher writes in.
pub fn folder() -> Result<PathBuf> {
    Ok(StateDir::locate().context("set HOME or BORG_STATE_DIR")?.path().to_path_buf())
}

pub fn log_path(state: &Path) -> PathBuf {
    state.join(ALERT_LOG)
}

pub fn baseline_path(state: &Path) -> PathBuf {
    state.join(WATCH_BASELINE)
}

pub fn allowed_path(state: &Path) -> PathBuf {
    state.join(WATCH_ALLOWED)
}

fn read_bounded(path: &Path, limit: u64) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    let read = fs::File::open(path).and_then(|file| file.take(limit).read_to_end(&mut bytes));
    read.ok().map(|_| bytes)
}

/// The alerts on this machine. A log that is not there yet, or that Borg cannot read, counts as
/// empty, so an unreadable file never stops the page from opening.
pub fn read_log(state: &Path) -> AlertLog {
    read_bounded(&log_path(state), LOG_BYTES).and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

pub fn write_log(state: &Path, log: &AlertLog) -> Result<()> {
    fs::create_dir_all(state)?;
    crate::store::write_privately(&log_path(state), &serde_json::to_vec(log)?)
}

pub fn read_baseline(state: &Path) -> Option<Baseline> {
    read_bounded(&baseline_path(state), BASELINE_BYTES).and_then(|bytes| serde_json::from_slice(&bytes).ok())
}

pub fn write_baseline(state: &Path, baseline: &Baseline) -> Result<()> {
    fs::create_dir_all(state)?;
    crate::store::write_privately(&baseline_path(state), &serde_json::to_vec(baseline)?)
}

/// The starts the user has allowed. The Alerts page writes this file and the watcher reads it,
/// so an allowance survives the watcher writing its baseline.
pub fn read_allowances(state: &Path) -> Vec<Allowance> {
    read_bounded(&allowed_path(state), BASELINE_BYTES)
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Adds an allowance to the file, once.
pub fn allow(state: &Path, allowance: Allowance) -> Result<()> {
    let mut allowed = read_allowances(state);
    if !allowed.contains(&allowance) {
        allowed.push(allowance);
    }
    fs::create_dir_all(state)?;
    crate::store::write_privately(&allowed_path(state), &serde_json::to_vec(&allowed)?)
}

/// When the log last changed, so a Borg that is only watching the page can tell.
pub fn log_changed(state: &Path) -> Option<SystemTime> {
    fs::metadata(log_path(state)).ok().and_then(|metadata| metadata.modified().ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use borg_core::{Response, StartedProcess, WatchFinding, WatchStatus};

    fn alert_log() -> AlertLog {
        let mut log = AlertLog::watching(WatchStatus::default());
        let process = StartedProcess { name: "sh".into(), ..StartedProcess::default() };
        log.add(SystemTime::UNIX_EPOCH, WatchFinding::ShellOnSocket { shell: "sh".into() }, Response::Frozen, process);
        log
    }

    #[test]
    fn the_log_and_the_baseline_come_back_as_they_were_written() {
        let folder = tempfile::tempdir().unwrap();
        let state = folder.path();
        assert_eq!(read_log(state), AlertLog::default(), "no log yet is an empty log");
        assert_eq!(read_baseline(state), None);
        let log = alert_log();
        write_log(state, &log).unwrap();
        assert_eq!(read_log(state), log);
        let baseline = Baseline::started(SystemTime::UNIX_EPOCH, std::time::Duration::from_secs(60));
        write_baseline(state, &baseline).unwrap();
        assert_eq!(read_baseline(state).as_ref(), Some(&baseline));
        assert!(log_changed(state).is_some());
    }

    #[test]
    fn the_log_belongs_to_its_owner_alone() {
        use std::os::unix::fs::PermissionsExt;
        let folder = tempfile::tempdir().unwrap();
        write_log(folder.path(), &alert_log()).unwrap();
        let mode = fs::metadata(log_path(folder.path())).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn an_allowance_is_kept_once_and_the_watcher_reads_it_back() {
        let folder = tempfile::tempdir().unwrap();
        let state = folder.path();
        assert!(read_allowances(state).is_empty());
        let allowance = Allowance { program: Some("/bin/sh".into()), ..Allowance::default() };
        allow(state, allowance.clone()).unwrap();
        allow(state, allowance.clone()).unwrap();
        assert_eq!(read_allowances(state), [allowance]);
        let other = Allowance { parent: Some("nginx".into()), ..Allowance::default() };
        allow(state, other.clone()).unwrap();
        assert_eq!(read_allowances(state).len(), 2);
        write_baseline(state, &Baseline::default()).unwrap();
        assert_eq!(read_allowances(state).len(), 2, "writing the baseline cannot lose them");
    }

    #[test]
    fn a_log_full_of_nonsense_reads_as_empty_instead_of_failing() {
        let folder = tempfile::tempdir().unwrap();
        fs::write(log_path(folder.path()), b"not json").unwrap();
        assert_eq!(read_log(folder.path()), AlertLog::default());
        fs::write(baseline_path(folder.path()), b"{\"since\":").unwrap();
        assert_eq!(read_baseline(folder.path()), None);
    }
}
