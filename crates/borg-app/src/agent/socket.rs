//! Where sessions listen for `borg mcp`: sockets in owner-only folders, and how Claude is told to connect.

use anyhow::{Result, bail};
use std::{
    os::unix::fs::{DirBuilderExt, MetadataExt},
    path::{Path, PathBuf},
};

pub(super) const SOCKET_PREFIX: &str = "session-";

pub(super) const SOCKET_SUFFIX: &str = ".sock";

pub(super) const OWNER_ONLY: u32 = 0o700;

pub(super) const OTHERS: u32 = 0o077;

/// Unix socket paths are short (104 bytes on macOS), so sockets live in a short folder of
/// their own. With `XDG_RUNTIME_DIR` set, as on Linux desktops and in tests, the folder is
/// named after the runtime folder, so `borg mcp` finds only its sessions; without it, as on
/// macOS, every program the user starts shares one, whatever its temporary folder.
pub(crate) fn socket_dir(runtime_dir: &Path) -> Result<PathBuf> {
    let uid = rustix::process::getuid().as_raw();
    let name = if std::env::var_os("XDG_RUNTIME_DIR").is_some() {
        // FNV-1a, which stays the same from one build of borg to the next.
        let hash = runtime_dir
            .as_os_str()
            .as_encoded_bytes()
            .iter()
            .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3));
        format!("{hash:016x}")
    } else {
        "default".to_owned()
    };
    owner_only(&PathBuf::from(format!("/tmp/borg-{uid}-{name}")))
}

/// Creates a folder only its owner can enter. One that someone else owns, or that others
/// can enter, is refused, so no other user can reach a session through it.
pub(super) fn owner_only(dir: &Path) -> Result<PathBuf> {
    let dir = dir.to_path_buf();
    std::fs::DirBuilder::new().recursive(true).mode(OWNER_ONLY).create(&dir)?;
    let meta = std::fs::metadata(&dir)?;
    if meta.uid() != rustix::process::getuid().as_raw() || meta.mode() & OTHERS != 0 {
        bail!("{} must belong to you and be closed to others", dir.display());
    }
    Ok(dir)
}

pub fn hostname() -> String {
    rustix::system::uname().nodename().to_string_lossy().into_owned()
}

/// How Claude reaches this borg, for the Connect dialog: the program by a path that stays
/// put across upgrades, and over SSH, the address and user the user came in with.
pub fn connect_info() -> borg_core::ConnectInfo {
    let ssh = std::env::var("SSH_CONNECTION").ok().and_then(|connection| {
        // "client-address client-port server-address server-port"
        let parts: Vec<&str> = connection.split_whitespace().collect();
        let (address, port) = (*parts.get(2)?, *parts.get(3)?);
        let user = std::env::var("USER").or_else(|_| std::env::var("LOGNAME")).ok().filter(|user| !user.is_empty());
        let user = user.map_or_else(String::new, |user| format!("{user}@"));
        Some(if port == "22" {
            format!("{user}{address}")
        } else if address.contains(':') {
            format!("ssh://{user}[{address}]:{port}")
        } else {
            format!("ssh://{user}{address}:{port}")
        })
    });
    let name = match &ssh {
        Some(_) => {
            let host = hostname();
            let short: String = host
                .split('.')
                .next()
                .unwrap_or_default()
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
                .collect();
            if short.is_empty() { "borg".to_owned() } else { format!("borg-{short}") }
        }
        None => "borg".to_owned(),
    };
    borg_core::ConnectInfo { name, program: program_path(), ssh, relay: String::new() }
}

/// This program's path, preferring the one on `PATH` that leads to it, since a package
/// manager's versioned folder changes with each upgrade.
pub fn program_path() -> String {
    let exe = std::env::current_exe().ok();
    let real = exe.as_ref().and_then(|exe| std::fs::canonicalize(exe).ok());
    if let (Some(path), Some(real)) = (std::env::var_os("PATH"), &real) {
        for dir in std::env::split_paths(&path).filter(|dir| dir.is_absolute()) {
            let candidate = dir.join("borg");
            if std::fs::canonicalize(&candidate).ok().as_ref() == Some(real) {
                return candidate.display().to_string();
            }
        }
    }
    exe.map_or_else(|| "borg".to_owned(), |exe| exe.display().to_string())
}
