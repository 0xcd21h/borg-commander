//! What `borg update` asks of the background service that runs here: to move to the newer
//! Borg the program file holds now, with its sessions.

use std::time::Duration;

use borg_i18n::tr;
use serde_json::{Value, json};
use tokio::time::{Instant, sleep, timeout};

use super::{STATUS_WAIT, request_json, server};

/// How long the service has to check its program file and agree to move.
const RELAUNCH_WAIT: Duration = Duration::from_secs(10);
/// How long `borg update` waits for the service to come back as the newer Borg.
const MOVE_WAIT: Duration = Duration::from_secs(20);
const MOVE_POLL: Duration = Duration::from_millis(500);

/// What the service does about the Borg in its program file.
#[derive(Debug, PartialEq, Eq)]
enum Follow {
    /// It runs that version, or a newer one.
    Nothing,
    /// It runs a Borg too old to move by itself.
    Older,
    Move,
}

fn follow(status: &Value, installed: &str) -> Follow {
    match status["version"].as_str() {
        None => Follow::Older,
        Some(running) if server::newer(Some(installed), running).is_ok() => Follow::Move,
        Some(_) => Follow::Nothing,
    }
}

/// Moves the service that runs here to the newer Borg in the program file, and says how that
/// went. Nothing happens when no service runs.
pub async fn after_update() {
    let Some(status) = status().await else { return };
    let Some(installed) = crate::relaunch::installed_version().await else { return };
    match follow(&status, &installed) {
        Follow::Nothing => {}
        Follow::Older => println!("{}", tr!(update_service_older)),
        Follow::Move => println!("{}", move_to(&installed).await),
    }
}

async fn status() -> Option<Value> {
    timeout(STATUS_WAIT, request_json(json!({"mode": "status"}))).await.ok()?.ok()
}

async fn move_to(version: &str) -> String {
    match timeout(RELAUNCH_WAIT, request_json(json!({"mode": server::RELAUNCH}))).await {
        Ok(Ok(reply)) if reply["ok"] == true => {}
        Ok(Ok(reply)) => return tr!(update_service_stays, error = reply["error"].as_str().unwrap_or_default()),
        _ => return tr!(update_service_no_answer).to_owned(),
    }
    println!("{}", tr!(update_service_moving, version = version));
    if came_back(version).await {
        tr!(update_service_moved, version = version)
    } else {
        tr!(update_service_late, version = version)
    }
}

async fn came_back(version: &str) -> bool {
    let deadline = Instant::now() + MOVE_WAIT;
    while Instant::now() < deadline {
        sleep(MOVE_POLL).await;
        if status().await.is_some_and(|status| status["version"] == version) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_service_moves_only_when_the_program_file_holds_a_newer_borg() {
        let running = |version: &str| json!({"ok": true, "version": version});
        assert_eq!(follow(&running("0.11.0"), "0.12.0"), Follow::Move);
        assert_eq!(follow(&running("0.12.0"), "0.12.0"), Follow::Nothing);
        assert_eq!(follow(&running("0.13.0"), "0.12.0"), Follow::Nothing);
        assert_eq!(follow(&json!({"ok": true}), "0.12.0"), Follow::Older, "an older service says no version");
    }
}
