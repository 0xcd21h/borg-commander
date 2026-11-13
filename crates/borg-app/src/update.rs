use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use borg_core::{Msg, UpdateOffer};
use borg_update::{LATEST_RELEASE_URL, RELEASES_URL, Version, channel, install_trusted, latest};
use tokio::sync::mpsc::UnboundedSender;

use crate::{config::ConfigDir, session::StateDir};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// What `borg --version` prints before the version.
pub const VERSION_NAME: &str = "borg ";
pub const TARGET: &str = env!("BORG_TARGET");
pub const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);
/// How often an open Borg looks whether the day's check is due.
const CHECK_POLL: Duration = Duration::from_secs(60 * 60);

/// When Borg last looked for a release, and the release you skipped: a line each in the
/// state folder's `update-check`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UpdateCheck {
    pub checked: Option<SystemTime>,
    pub skipped: Option<Version>,
}

impl UpdateCheck {
    pub fn parse(text: &str) -> Self {
        let mut lines = text.lines().map(str::trim);
        let checked =
            lines.next().and_then(|line| line.parse().ok()).map(|secs| UNIX_EPOCH + Duration::from_secs(secs));
        let skipped = lines.next().and_then(|line| line.parse().ok());
        Self { checked, skipped }
    }

    pub fn text(&self) -> String {
        let secs = self.checked.and_then(|at| at.duration_since(UNIX_EPOCH).ok()).map_or(0, |age| age.as_secs());
        match self.skipped {
            Some(skipped) => format!("{secs}\n{skipped}\n"),
            None => format!("{secs}\n"),
        }
    }

    /// A day has passed since the last check, or the clock went back past it.
    pub fn due(&self, now: SystemTime) -> bool {
        self.checked.is_none_or(|last| now.duration_since(last).map_or(true, |age| age >= CHECK_EVERY))
    }

    /// Whether to offer `release`: any version but the one you skipped.
    pub fn offers(&self, release: Version) -> bool {
        self.skipped != Some(release)
    }
}

fn current() -> Result<Version> {
    VERSION.parse().context("reading the built-in version")
}

pub fn run() -> Result<()> {
    let exe = crate::program::path()?.canonicalize()?;
    if let Some(hint) = channel(&exe).upgrade_hint() {
        println!("borg was installed with a package manager. Update it with:\n  {hint}");
        return Ok(());
    }
    let current = current()?;
    match latest(LATEST_RELEASE_URL, current)? {
        None => println!("borg {current} is up to date."),
        Some(release) => {
            println!("Updating borg {current} → {}…", release.version);
            let installed = install_trusted(&release, RELEASES_URL, current, TARGET, &exe)?;
            if installed == release.version {
                println!("Updated. Restart borg to use {installed}.");
            } else {
                println!(
                    "Updated to {installed}, the newest release this copy could check. Run `borg update` again to reach {}.",
                    release.version
                );
            }
        }
    }
    Ok(())
}

/// Installs the latest release over the program file when it is newer than `current`, the
/// version the program file holds.
pub fn install_newer(current: &str) -> Result<()> {
    let exe = crate::program::path()?.canonicalize()?;
    if let Some(hint) = channel(&exe).upgrade_hint() {
        anyhow::bail!("{}", borg_i18n::tr!(remote_host_update_package_manager, hint = hint));
    }
    let current: Version = current.parse().context("reading the installed version")?;
    if let Some(release) = latest(LATEST_RELEASE_URL, current)? {
        install_trusted(&release, RELEASES_URL, current, TARGET, &exe)?;
    }
    Ok(())
}

/// Looks for a release once a day while Borg is open, and offers one you have not skipped.
/// Each look reads the config again, so that turning the check off or on takes effect in every
/// open Borg.
pub fn check_daily(state_dir: StateDir, config_dir: Option<ConfigDir>, messages: UnboundedSender<Msg>) {
    tokio::spawn(async move {
        while !messages.is_closed() {
            let (dir, config, sender) = (state_dir.clone(), config_dir.clone(), messages.clone());
            let _ = tokio::task::spawn_blocking(move || {
                if checks_on(config.as_ref()) {
                    check_once(&dir, &sender, SystemTime::now());
                }
            })
            .await;
            tokio::time::sleep(CHECK_POLL).await;
        }
    });
}

/// The config asks for the daily check, as it reads now.
fn checks_on(config_dir: Option<&ConfigDir>) -> bool {
    config_dir.and_then(|dir| dir.load().ok()).is_none_or(|config| config.update_check())
}

fn check_once(state_dir: &StateDir, messages: &UnboundedSender<Msg>, now: SystemTime) {
    let check = state_dir.update_check();
    if !check.due(now) {
        return;
    }
    let _ = state_dir.save_update_check(&UpdateCheck { checked: Some(now), ..check });
    let Ok(current) = current() else { return };
    let Ok(Some(release)) = latest(LATEST_RELEASE_URL, current) else { return };
    if check.offers(release.version) {
        let _ = messages.send(Msg::UpdateAvailable(offer(current, release.version)));
    }
}

/// The offer for `version`: `borg update`, or the command of the package manager that
/// installed Borg.
fn offer(current: Version, version: Version) -> UpdateOffer {
    let exe = crate::program::path().ok().and_then(|exe| exe.canonicalize().ok());
    let manager = exe.and_then(|exe| channel(&exe).upgrade_hint()).map(str::to_owned);
    UpdateOffer {
        version: version.to_string(),
        current: current.to_string(),
        program: crate::agent::program_path(),
        manager,
    }
}

/// Remembers `version` as skipped, keeping when Borg last looked.
pub fn skip(state_dir: &StateDir, version: &str) -> Result<()> {
    let skipped = version.parse().context("reading the skipped version")?;
    state_dir.save_update_check(&UpdateCheck { skipped: Some(skipped), ..state_dir.update_check() })
}

#[cfg(test)]
mod tests;
