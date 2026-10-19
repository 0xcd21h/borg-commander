use std::{
    env,
    time::{Duration, SystemTime},
};

use anyhow::{Context, Result};
use borg_core::Msg;
use borg_update::{LATEST_RELEASE_URL, Version, channel, install, latest};
use tokio::sync::mpsc::UnboundedSender;

use crate::session::StateDir;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const TARGET: &str = env!("BORG_TARGET");
pub const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);

fn current() -> Result<Version> {
    VERSION.parse().context("reading the built-in version")
}

pub fn run() -> Result<()> {
    let exe = env::current_exe()?.canonicalize()?;
    if let Some(hint) = channel(&exe).upgrade_hint() {
        println!("borg was installed with a package manager. Update it with:\n  {hint}");
        return Ok(());
    }
    let current = current()?;
    match latest(LATEST_RELEASE_URL, current)? {
        None => println!("borg {current} is up to date."),
        Some(release) => {
            println!("Updating borg {current} → {}…", release.version);
            install(&release, TARGET, &exe)?;
            println!("Updated. Restart borg to use {}.", release.version);
        }
    }
    Ok(())
}

pub fn check_in_background(state_dir: StateDir, messages: UnboundedSender<Msg>) {
    let now = SystemTime::now();
    if !state_dir.update_check_due(now, CHECK_EVERY) {
        return;
    }
    tokio::task::spawn_blocking(move || {
        let _ = state_dir.record_update_check(now);
        let Ok(current) = current() else { return };
        let Ok(Some(release)) = latest(LATEST_RELEASE_URL, current) else { return };
        let how = env::current_exe()
            .ok()
            .and_then(|exe| channel(&exe).upgrade_hint())
            .map_or_else(|| "borg update".to_owned(), str::to_owned);
        let _ = messages.send(Msg::Notice(format!("borg {} is available · run `{how}`", release.version)));
    });
}
