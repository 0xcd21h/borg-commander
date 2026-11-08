//! Explicit boot-service lifecycle. The installed unit runs Borg as the current user, even when
//! the operating system service manager itself starts it before that user logs in.

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, bail};
use borg_core::{ServiceCheck, ServiceStatus};

use crate::{agent, session::StateDir};

const SUDO: &str = "sudo";
#[cfg(target_os = "macos")]
const LABEL: &str = "com.borg-commander.service";
#[cfg(target_os = "macos")]
const DAEMONS: &str = "/Library/LaunchDaemons";
#[cfg(target_os = "linux")]
const SYSTEMD: &str = "/run/systemd/system";
#[cfg(target_os = "linux")]
const UNITS: &str = "/etc/systemd/system";

fn root() -> bool {
    rustix::process::geteuid().is_root()
}

/// `program` with root's rights: under sudo, unless Borg is root already, so a root shell
/// without sudo sets up the service too.
fn elevated(program: &str) -> Command {
    command_as(program, root())
}

fn command_as(program: &str, root: bool) -> Command {
    if root {
        Command::new(program)
    } else {
        let mut command = Command::new(SUDO);
        command.arg(program);
        command
    }
}

fn user() -> Result<String> {
    let user = uzers::get_user_by_uid(uzers::get_effective_uid())
        .map(|account| account.name().to_string_lossy().into_owned())
        .or_else(|| env::var("USER").ok())
        .context("current user is not in the account database and USER is not set")?;
    if user.is_empty() || !user.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-') {
        bail!("current user contains characters that cannot be used in a service name");
    }
    Ok(user)
}

fn home() -> Result<PathBuf> {
    env::var_os("HOME").map(PathBuf::from).context("HOME is not set")
}

fn state() -> Result<StateDir> {
    StateDir::locate().context("set HOME or BORG_STATE_DIR")
}

fn run(command: &mut Command) -> Result<()> {
    let display = format!("{command:?}");
    let status = command.status().with_context(|| format!("running {display}"))?;
    if !status.success() {
        bail!("{display} exited with {status}");
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn managed() -> bool {
    Path::new(SYSTEMD).is_dir()
}

#[cfg(target_os = "linux")]
fn label(user: &str) -> String {
    format!("borg-commander-{user}.service")
}

#[cfg(target_os = "linux")]
fn installed_at(label: &str) -> PathBuf {
    Path::new(UNITS).join(label)
}

#[cfg(target_os = "linux")]
fn unit() -> Result<(String, String, PathBuf)> {
    if !managed() {
        bail!("systemd is not running here; run `borg service run` under this container's supervisor instead");
    }
    let user = user()?;
    let home = home()?;
    let state = state()?;
    let label = label(&user);
    let source = state.path().join(&label);
    let executable = agent::program_path();
    let text = format!(
        "[Unit]\nDescription=Borg Commander sessions for {user}\nAfter=network-online.target\nWants=network-online.target\n\n[Service]\nType=simple\nUser={user}\nEnvironment=HOME={}\nEnvironment=BORG_STATE_DIR={}\nExecStart=\"{}\" service run\nRestart=on-failure\nRestartSec=2\n\n[Install]\nWantedBy=multi-user.target\n",
        home.display(),
        state.path().display(),
        executable.replace('"', "\\\"")
    );
    Ok((label, text, source))
}

#[cfg(target_os = "macos")]
fn managed() -> bool {
    true
}

#[cfg(target_os = "macos")]
fn label(user: &str) -> String {
    format!("{LABEL}.{user}")
}

#[cfg(target_os = "macos")]
fn installed_at(label: &str) -> PathBuf {
    Path::new(DAEMONS).join(format!("{label}.plist"))
}

#[cfg(target_os = "macos")]
fn unit() -> Result<(String, String, PathBuf)> {
    let user = user()?;
    let home = home()?;
    let state = state()?;
    let label = label(&user);
    let source = state.path().join(format!("{label}.plist"));
    let executable = agent::program_path();
    let xml = |value: &str| {
        value
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&apos;")
    };
    let text = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict>\n<key>Label</key><string>{}</string>\n<key>UserName</key><string>{}</string>\n<key>ProgramArguments</key><array><string>{}</string><string>service</string><string>run</string></array>\n<key>EnvironmentVariables</key><dict><key>HOME</key><string>{}</string><key>BORG_STATE_DIR</key><string>{}</string></dict>\n<key>RunAtLoad</key><true/>\n<key>KeepAlive</key><true/>\n</dict></plist>\n",
        xml(&label),
        xml(&user),
        xml(&executable),
        xml(&home.display().to_string()),
        xml(&state.path().display().to_string())
    );
    Ok((label, text, source))
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn unit() -> Result<(String, String, PathBuf)> {
    bail!("Borg service installation supports Linux and macOS")
}

/// Whether the service is installed, or `None` where nothing starts a service Borg installs.
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn installed() -> Option<bool> {
    managed().then(|| user().is_ok_and(|user| installed_at(&label(&user)).is_file()))
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn installed() -> Option<bool> {
    None
}

/// How the background service stands here, for the Remote menu's dialog.
pub async fn check() -> ServiceCheck {
    let status = if crate::service::available().await {
        ServiceStatus::Running
    } else {
        match installed() {
            None => ServiceStatus::Unmanaged,
            Some(true) => ServiceStatus::Stopped,
            Some(false) => ServiceStatus::Missing,
        }
    };
    ServiceCheck { status, program: agent::program_path(), root: root() }
}

pub fn install() -> Result<()> {
    let (label, text, source) = unit()?;
    fs::create_dir_all(source.parent().context("no service state directory")?)?;
    fs::write(&source, text)?;
    #[cfg(target_os = "linux")]
    {
        run(elevated("install").args(["-m", "0644"]).arg(&source).arg(installed_at(&label)))?;
        run(elevated("systemctl").arg("daemon-reload"))?;
    }
    #[cfg(target_os = "macos")]
    {
        run(elevated("launchctl").args(["disable", &format!("system/{label}")]))?;
        run(elevated("install")
            .args(["-o", "root", "-g", "wheel", "-m", "0644"])
            .arg(&source)
            .arg(installed_at(&label)))?;
    }
    println!("Installed {label}. Run `borg service enable` for boot startup and `borg service start` to start it now.");
    Ok(())
}

pub fn enable() -> Result<()> {
    let (label, _, _) = unit()?;
    #[cfg(target_os = "linux")]
    run(elevated("systemctl").args(["enable", &label]))?;
    #[cfg(target_os = "macos")]
    run(elevated("launchctl").args(["enable", &format!("system/{label}")]))?;
    println!("Enabled {label} at boot.");
    Ok(())
}

pub fn disable() -> Result<()> {
    let (label, _, _) = unit()?;
    #[cfg(target_os = "linux")]
    run(elevated("systemctl").args(["disable", &label]))?;
    #[cfg(target_os = "macos")]
    run(elevated("launchctl").args(["disable", &format!("system/{label}")]))?;
    println!("Disabled {label} at boot.");
    Ok(())
}

pub fn start() -> Result<()> {
    let (label, _, _) = unit()?;
    #[cfg(target_os = "linux")]
    run(elevated("systemctl").args(["start", &label]))?;
    #[cfg(target_os = "macos")]
    run(elevated("launchctl").args(["bootstrap", "system"]).arg(installed_at(&label)))?;
    println!("Started {label}.");
    Ok(())
}

pub fn stop() -> Result<()> {
    let (label, _, _) = unit()?;
    #[cfg(target_os = "linux")]
    run(elevated("systemctl").args(["stop", &label]))?;
    #[cfg(target_os = "macos")]
    run(elevated("launchctl").args(["bootout", &format!("system/{label}")]))?;
    println!("Stopped {label}; its shells closed and its named workspaces remain saved.");
    Ok(())
}

pub fn uninstall() -> Result<()> {
    let (label, _, source) = unit()?;
    let _ = stop();
    let _ = disable();
    #[cfg(target_os = "linux")]
    {
        run(elevated("rm").arg("--").arg(installed_at(&label)))?;
        run(elevated("systemctl").arg("daemon-reload"))?;
    }
    #[cfg(target_os = "macos")]
    run(elevated("rm").arg("--").arg(installed_at(&label)))?;
    let _ = fs::remove_file(source);
    println!("Uninstalled {label}; named workspaces and pairing keys remain saved.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(command: &Command) -> Vec<String> {
        std::iter::once(command.get_program())
            .chain(command.get_args())
            .map(|word| word.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn root_runs_the_program_itself_and_anyone_else_runs_it_under_sudo() {
        let mut as_root = command_as("systemctl", true);
        as_root.args(["enable", "borg-commander-root.service"]);
        assert_eq!(words(&as_root), ["systemctl", "enable", "borg-commander-root.service"]);
        let mut as_user = command_as("systemctl", false);
        as_user.arg("daemon-reload");
        assert_eq!(words(&as_user), ["sudo", "systemctl", "daemon-reload"]);
    }
}
