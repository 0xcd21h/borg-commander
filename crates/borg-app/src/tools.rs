//! Programs Borg runs from the system: where they are installed, and how this machine installs
//! one that is missing.

use borg_core::{Installer, Platform};
use std::path::PathBuf;

/// Where programs are installed when the folder is not on `PATH`, as `/usr/sbin` often is not.
pub(crate) const SYSTEM_DIRS: [&str; 4] = ["/usr/sbin", "/usr/bin", "/sbin", "/bin"];
/// Where Homebrew links the system programs it installs, which `PATH` often leaves out.
const HOMEBREW_DIRS: [&str; 3] = ["/opt/homebrew/sbin", "/usr/local/sbin", "/home/linuxbrew/.linuxbrew/sbin"];
const PLATFORM: Platform = if cfg!(target_os = "macos") { Platform::MacOs } else { Platform::Linux };

/// The installed program: the one on `PATH`, or else in a system or Homebrew folder. Each call
/// looks again, so a program installed while Borg runs is found.
pub(crate) fn find(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .filter(|dir| dir.is_absolute())
        .chain(SYSTEM_DIRS.into_iter().chain(HOMEBREW_DIRS).map(PathBuf::from))
        .map(|dir| dir.join(program))
        .find(|candidate| candidate.is_file())
}

/// How this machine installs a missing program, when Borg knows its package manager.
pub(crate) fn installer() -> Option<Installer> {
    let root = rustix::process::geteuid().is_root();
    Installer::detect(PLATFORM, root, |program| find(program).is_some())
}
