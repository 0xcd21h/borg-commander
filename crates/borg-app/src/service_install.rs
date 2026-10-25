//! Explicit boot-service lifecycle. The installed unit runs Borg as the current user, even when
//! the operating system service manager itself starts it before that user logs in.

use std::{env, fs, path::PathBuf, process::Command};

use anyhow::{Context, Result, bail};

use crate::{agent, session::StateDir};

#[cfg(target_os = "macos")]
const LABEL: &str = "com.borg-commander.service";

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
fn unit() -> Result<(String, String, PathBuf)> {
    if !std::path::Path::new("/run/systemd/system").is_dir() {
        bail!("systemd is not running here; run `borg service run` under this container's supervisor instead");
    }
    let user = user()?;
    let home = home()?;
    let state = state()?;
    let label = format!("borg-commander-{user}.service");
    let destination = PathBuf::from("/etc/systemd/system").join(&label);
    let source = state.path().join(&label);
    let executable = agent::program_path();
    let text = format!(
        "[Unit]\nDescription=Borg Commander sessions for {user}\nAfter=network-online.target\nWants=network-online.target\n\n[Service]\nType=simple\nUser={user}\nEnvironment=HOME={}\nEnvironment=BORG_STATE_DIR={}\nExecStart=\"{}\" service run\nRestart=on-failure\nRestartSec=2\n\n[Install]\nWantedBy=multi-user.target\n",
        home.display(),
        state.path().display(),
        executable.replace('"', "\\\"")
    );
    Ok((label, text, source.with_file_name(destination.file_name().unwrap_or_default())))
}

#[cfg(target_os = "macos")]
fn unit() -> Result<(String, String, PathBuf)> {
    let user = user()?;
    let home = home()?;
    let state = state()?;
    let label = format!("{LABEL}.{user}");
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

pub fn install() -> Result<()> {
    let (label, text, source) = unit()?;
    fs::create_dir_all(source.parent().context("no service state directory")?)?;
    fs::write(&source, text)?;
    #[cfg(target_os = "linux")]
    {
        run(Command::new("sudo")
            .args(["install", "-m", "0644"])
            .arg(&source)
            .arg(PathBuf::from("/etc/systemd/system").join(&label)))?;
        run(Command::new("sudo").args(["systemctl", "daemon-reload"]))?;
    }
    #[cfg(target_os = "macos")]
    {
        run(Command::new("sudo").args(["launchctl", "disable", &format!("system/{label}")]))?;
        run(Command::new("sudo")
            .args(["install", "-o", "root", "-g", "wheel", "-m", "0644"])
            .arg(&source)
            .arg(PathBuf::from("/Library/LaunchDaemons").join(format!("{label}.plist"))))?;
    }
    println!("Installed {label}. Run `borg service enable` for boot startup and `borg service start` to start it now.");
    Ok(())
}

pub fn enable() -> Result<()> {
    let (label, _, _) = unit()?;
    #[cfg(target_os = "linux")]
    run(Command::new("sudo").args(["systemctl", "enable", &label]))?;
    #[cfg(target_os = "macos")]
    run(Command::new("sudo").args(["launchctl", "enable", &format!("system/{label}")]))?;
    println!("Enabled {label} at boot.");
    Ok(())
}

pub fn disable() -> Result<()> {
    let (label, _, _) = unit()?;
    #[cfg(target_os = "linux")]
    run(Command::new("sudo").args(["systemctl", "disable", &label]))?;
    #[cfg(target_os = "macos")]
    run(Command::new("sudo").args(["launchctl", "disable", &format!("system/{label}")]))?;
    println!("Disabled {label} at boot.");
    Ok(())
}

pub fn start() -> Result<()> {
    let (label, _, _) = unit()?;
    #[cfg(target_os = "linux")]
    run(Command::new("sudo").args(["systemctl", "start", &label]))?;
    #[cfg(target_os = "macos")]
    {
        let destination = PathBuf::from("/Library/LaunchDaemons").join(format!("{label}.plist"));
        run(Command::new("sudo").args(["launchctl", "bootstrap", "system"]).arg(destination))?;
    }
    println!("Started {label}.");
    Ok(())
}

pub fn stop() -> Result<()> {
    let (label, _, _) = unit()?;
    #[cfg(target_os = "linux")]
    run(Command::new("sudo").args(["systemctl", "stop", &label]))?;
    #[cfg(target_os = "macos")]
    run(Command::new("sudo").args(["launchctl", "bootout", &format!("system/{label}")]))?;
    println!("Stopped {label}; its shells closed and its named workspaces remain saved.");
    Ok(())
}

pub fn uninstall() -> Result<()> {
    let (label, _, source) = unit()?;
    let _ = stop();
    let _ = disable();
    #[cfg(target_os = "linux")]
    {
        run(Command::new("sudo").args(["rm", "--"]).arg(PathBuf::from("/etc/systemd/system").join(&label)))?;
        run(Command::new("sudo").args(["systemctl", "daemon-reload"]))?;
    }
    #[cfg(target_os = "macos")]
    run(Command::new("sudo")
        .args(["rm", "--"])
        .arg(PathBuf::from("/Library/LaunchDaemons").join(format!("{label}.plist"))))?;
    let _ = fs::remove_file(source);
    println!("Uninstalled {label}; named workspaces and pairing keys remain saved.");
    Ok(())
}
