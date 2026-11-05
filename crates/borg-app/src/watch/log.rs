//! Where the watcher keeps what it has found and what it has learned: a bounded log of alerts
//! and the baseline, both owner-only files in the state folder, which every Borg on the machine
//! reads and the watcher alone writes.

use anyhow::{Context, Result};
use borg_core::{ALERT_LOG, ALERT_LOG_ASIDE, AlertLog, Allowance, Baseline, WATCH_ALLOWED, WATCH_BASELINE};
use std::{
    fs,
    io::{ErrorKind, Read},
    path::{Path, PathBuf},
    time::SystemTime,
};

use crate::session::StateDir;

/// Bytes of the log Borg reads and writes; [`borg_core::ALERTS_KEPT`] alerts with their evidence
/// and commands of [`borg_core::COMMAND_KEPT`] bytes fit well inside this.
const LOG_BYTES: usize = 8 * 1024 * 1024;
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

/// The log the file holds, if Borg can read it, still naming an earlier log it kept aside only
/// while that file is there.
fn parsed_log(path: &Path) -> Option<AlertLog> {
    let mut log: AlertLog =
        read_bounded(path, LOG_BYTES as u64).and_then(|bytes| serde_json::from_slice(&bytes).ok())?;
    log.kept_aside = log.kept_aside.filter(|aside| fs::symlink_metadata(aside).is_ok());
    Some(log)
}

/// The alerts on this machine. A log that is not there yet, or that Borg cannot read, counts as
/// empty, so an unreadable file never stops the page from opening.
pub fn read_log(state: &Path) -> AlertLog {
    parsed_log(&log_path(state)).unwrap_or_default()
}

/// The log as the state folder holds it, to change and write back. A log that is there but
/// that Borg cannot read is renamed aside first, so writing a new one never overwrites alerts
/// Borg could not show. The new log, written at once, names the file it went to, and so does
/// the second half, only when it moved just now.
pub fn load_log(state: &Path) -> Result<(AlertLog, Option<PathBuf>)> {
    let path = log_path(state);
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok((AlertLog::default(), None)),
        _ => {}
    }
    if let Some(log) = parsed_log(&path) {
        return Ok((log, None));
    }
    let taken = |name: &Path| fs::symlink_metadata(name).is_ok();
    let aside = state.join(ALERT_LOG_ASIDE);
    let aside = if taken(&aside) { borg_jobs::unique_by(&aside, taken) } else { aside };
    match fs::rename(&path, &aside) {
        Ok(()) => {
            let mut log = AlertLog::beside(aside.clone());
            write_log(state, &mut log)?;
            Ok((log, Some(aside)))
        }
        Err(error) if error.kind() == ErrorKind::NotFound => Ok((read_log(state), None)),
        Err(error) => Err(error).with_context(|| format!("keeping {} as {}", path.display(), aside.display())),
    }
}

/// Writes the log, without the oldest alerts it takes for the file to fit [`LOG_BYTES`], so a
/// log Borg wrote always reads back whole.
pub fn write_log(state: &Path, log: &mut AlertLog) -> Result<()> {
    let mut bytes = serde_json::to_vec(log)?;
    while bytes.len() > LOG_BYTES && !log.alerts.is_empty() {
        let mut over = bytes.len() - LOG_BYTES;
        while over > 0
            && let Some(oldest) = log.alerts.pop()
        {
            over = over.saturating_sub(serde_json::to_vec(&oldest)?.len());
        }
        bytes = serde_json::to_vec(log)?;
    }
    fs::create_dir_all(state)?;
    crate::store::write_privately(&log_path(state), &bytes)
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
    use borg_core::{ALERTS_KEPT, Ancestor, Response, StartedProcess, WatchFinding, WatchStatus};
    use std::time::Duration;

    /// Libraries on the command line of a large build's Java daemon.
    const LIBRARIES: usize = 3000;

    fn alert_log() -> AlertLog {
        let mut log = AlertLog::watching(WatchStatus::default());
        let process = StartedProcess { name: "sh".into(), ..StartedProcess::default() };
        log.add(SystemTime::UNIX_EPOCH, WatchFinding::ShellOnSocket { shell: "sh".into() }, Response::Frozen, process);
        log
    }

    /// A log of as many alerts as it keeps, each one second after the one before.
    fn full_log(process: impl Fn(u64) -> StartedProcess) -> AlertLog {
        let mut log = AlertLog::watching(WatchStatus::default());
        for second in 0..ALERTS_KEPT as u64 {
            let at = SystemTime::UNIX_EPOCH + Duration::from_secs(second);
            log.add(at, WatchFinding::ShellOnSocket { shell: "sh".into() }, Response::Alerted, process(second));
        }
        log
    }

    /// A log that holds twice the bytes Borg reads, in the connections of its alerts.
    fn oversized_log() -> AlertLog {
        let mut log = full_log(|_| StartedProcess { name: "sh".into(), ..StartedProcess::default() });
        let connection = "x".repeat(2 * LOG_BYTES / ALERTS_KEPT);
        for alert in &mut log.alerts {
            alert.evidence.connections = vec![connection.clone()];
        }
        log
    }

    #[test]
    fn the_log_and_the_baseline_come_back_as_they_were_written() {
        let folder = tempfile::tempdir().unwrap();
        let state = folder.path();
        assert_eq!(read_log(state), AlertLog::default(), "no log yet is an empty log");
        assert_eq!(read_baseline(state), None);
        let mut log = alert_log();
        write_log(state, &mut log).unwrap();
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
        write_log(folder.path(), &mut alert_log()).unwrap();
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

    /// A build's Java daemon has every library on its command line, and each alert about a
    /// script it starts carries that line as the command of its parent. A full log of them used
    /// to pass the bytes Borg reads, so the page showed no alerts and the next write lost them.
    #[test]
    fn a_full_log_of_long_commands_reads_back_whole() {
        let folder = tempfile::tempdir().unwrap();
        let state = folder.path();
        let classpath = format!("java -cp {} org.gradle.launcher.daemon.Daemon", "/opt/lib/x.jar:".repeat(LIBRARIES));
        let java = Ancestor {
            pid: 10,
            name: "java".into(),
            program: Some("/usr/bin/java".into()),
            command: classpath,
            ..Ancestor::default()
        };
        let mut log = full_log(|second| StartedProcess {
            name: "build.sh".into(),
            command: vec!["/tmp/ci/build.sh".into(), second.to_string()],
            ancestors: vec![java.clone()],
            ..StartedProcess::default()
        });
        write_log(state, &mut log).unwrap();
        assert_eq!(log.alerts.len(), ALERTS_KEPT, "nothing had to go");
        assert_eq!(read_log(state), log);
    }

    #[test]
    fn a_log_too_long_to_read_back_loses_its_oldest_alerts_as_it_is_written() {
        let folder = tempfile::tempdir().unwrap();
        let state = folder.path();
        let mut log = oversized_log();
        let newest = log.alerts[0].id;
        let one = serde_json::to_vec(&log.alerts[0]).unwrap().len();
        write_log(state, &mut log).unwrap();
        let written = fs::metadata(log_path(state)).unwrap().len() as usize;
        assert!(written <= LOG_BYTES, "{written} bytes");
        assert!(LOG_BYTES - written < 2 * one, "only the alerts it takes go: {} left", log.alerts.len());
        assert_eq!(log.alerts[0].id, newest, "the newest alert stays");
        assert_eq!(read_log(state), log, "what is written reads back, and the log in hand is the same");
    }

    #[test]
    fn a_log_borg_cannot_read_is_kept_aside_before_a_new_one_is_written() {
        let folder = tempfile::tempdir().unwrap();
        let state = folder.path();
        assert_eq!(load_log(state).unwrap(), (AlertLog::default(), None), "no log yet is an empty log");
        let mut log = alert_log();
        write_log(state, &mut log).unwrap();
        assert_eq!(load_log(state).unwrap(), (log, None), "a log Borg reads stays where it is");

        let nonsense = b"not json".to_vec();
        let too_long = serde_json::to_vec(&oversized_log()).unwrap();
        let first = state.join(ALERT_LOG_ASIDE);
        let second = borg_jobs::unique_by(&first, |_| false);
        for (contents, aside) in [(&nonsense, &first), (&too_long, &second)] {
            fs::write(log_path(state), contents).unwrap();
            assert_eq!(read_log(state), AlertLog::default(), "the page shows nothing it cannot read");
            let kept = AlertLog::beside(aside.clone());
            assert_eq!(load_log(state).unwrap(), (kept.clone(), Some(aside.clone())));
            assert_eq!(read_log(state), kept, "a new log takes its place and says where it went");
            assert_eq!(&fs::read(aside).unwrap(), contents, "and the old one is kept whole");
        }
    }
}
