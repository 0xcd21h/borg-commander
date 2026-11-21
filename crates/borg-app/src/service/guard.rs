//! The way back from an update that does not come up. The previous program file, which the
//! update kept beside the new one, watches the service start again as the newer Borg. When the
//! service does not answer as that version in time, or can no longer be reached through borg
//! cloud while it could before, the guard puts the previous program back and starts it.

use std::{
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use rustix::process::{Pid, Signal, kill_process};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{ON_DEMAND, RUN, SERVICE_COMMAND, server::Lifetime};

/// The argument that makes a Borg a guard, followed by what it watches for.
pub const GUARD: &str = "--guard-update";
/// How long the newer Borg has to come up.
const WAIT: Duration = Duration::from_secs(60);
const POLL: Duration = Duration::from_secs(1);

/// What the guard watches for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Watch {
    /// The version the service moves to.
    version: String,
    /// The version it ran, which only the guard of that same version puts back.
    previous: String,
    process: u32,
    lifetime: Lifetime,
    /// Whether paired computers reached the service through borg cloud before the move.
    reachable: bool,
    program: PathBuf,
}

/// Starts the previous program file, when the update kept one, to watch this service move to
/// `version`.
pub(super) fn start(version: &str, lifetime: Lifetime, reachable: bool) -> Option<Child> {
    let program = crate::program::path().ok()?.canonicalize().ok()?;
    let previous = borg_update::previous(&program);
    let watch = Watch {
        version: version.to_owned(),
        previous: crate::update::VERSION.to_owned(),
        process: std::process::id(),
        lifetime,
        reachable,
        program,
    };
    Command::new(previous)
        .arg(GUARD)
        .arg(serde_json::to_string(&watch).ok()?)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .ok()
}

/// The guard: waits for the service to come up as the newer Borg, and otherwise puts this
/// program back in its place and starts the service on it.
pub async fn run(args: &[String]) -> Result<()> {
    let [watch] = args else { anyhow::bail!("usage: borg {GUARD} WATCH") };
    let watch: Watch = serde_json::from_str(watch).context("reading what to watch")?;
    if watch.previous != crate::update::VERSION || came_up(&watch).await {
        return Ok(());
    }
    borg_update::restore_previous(&watch.program).context("putting the previous Borg back")?;
    if let Some(process) = i32::try_from(watch.process).ok().and_then(Pid::from_raw) {
        let _ = kill_process(process, Signal::KILL);
    }
    if watch.lifetime == Lifetime::OnDemand {
        restart_on_demand(&watch.program)?;
    }
    Ok(())
}

/// Whether the service answers as the newer Borg within the wait, reachable through borg
/// cloud when it was before.
async fn came_up(watch: &Watch) -> bool {
    let deadline = Instant::now() + WAIT;
    while Instant::now() < deadline {
        if let Ok(Ok(status)) = tokio::time::timeout(POLL, super::request_json(json!({"mode": "status"}))).await
            && arrived(watch, &status)
        {
            return true;
        }
        tokio::time::sleep(POLL).await;
    }
    false
}

fn arrived(watch: &Watch, status: &Value) -> bool {
    status["version"] == watch.version.as_str() && (!watch.reachable || status["reachable"] == true)
}

/// Starts the service the way a Borg does on demand. An installed one starts again by itself.
fn restart_on_demand(program: &Path) -> Result<()> {
    let mut service = Command::new(program)
        .args([SERVICE_COMMAND, RUN, ON_DEMAND])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .context("starting the service again")?;
    std::thread::spawn(move || service.wait());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn watch(reachable: bool) -> Watch {
        Watch {
            version: "0.12.0".into(),
            previous: "0.11.0".into(),
            process: 42,
            lifetime: Lifetime::Kept,
            reachable,
            program: PathBuf::from("/usr/local/bin/borg"),
        }
    }

    #[test]
    fn the_newer_borg_has_arrived_once_it_answers_as_that_version_and_is_reachable_as_before() {
        let reachable = json!({"version": "0.12.0", "reachable": true});
        let unreachable = json!({"version": "0.12.0", "reachable": false});
        assert!(arrived(&watch(true), &reachable));
        assert!(!arrived(&watch(true), &unreachable), "paired computers must reach it again");
        assert!(arrived(&watch(false), &unreachable), "one nobody reached before need not be reachable");
        assert!(!arrived(&watch(false), &json!({"version": "0.11.0", "reachable": true})));
        assert!(!arrived(&watch(false), &json!({"ok": true})), "an older service says no version");
    }

    #[test]
    fn what_the_guard_watches_for_survives_its_arguments() {
        let watch = watch(true);
        let carried: Watch = serde_json::from_str(&serde_json::to_string(&watch).unwrap()).unwrap();
        assert_eq!(carried, watch);
    }
}
