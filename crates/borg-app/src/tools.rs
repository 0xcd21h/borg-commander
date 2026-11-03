//! Programs Borg runs from the system: where they are installed, and how this machine installs
//! one that is missing.

use borg_core::{Installer, Platform};
use std::{ffi::OsStr, path::PathBuf};

/// Where programs are installed when the folder is not on `PATH`, as `/usr/sbin` often is not.
pub(crate) const SYSTEM_DIRS: [&str; 4] = ["/usr/sbin", "/usr/bin", "/sbin", "/bin"];
/// Where Homebrew links the system programs it installs, which `PATH` often leaves out.
const HOMEBREW_DIRS: [&str; 3] = ["/opt/homebrew/sbin", "/usr/local/sbin", "/home/linuxbrew/.linuxbrew/sbin"];
/// Where Homebrew keeps the programs of formulas it leaves unlinked, such as libarchive's
/// `bsdtar`, which Borg offers to install for RAR archives.
const HOMEBREW_KEGS: [&str; 3] = [
    "/opt/homebrew/opt/libarchive/bin",
    "/usr/local/opt/libarchive/bin",
    "/home/linuxbrew/.linuxbrew/opt/libarchive/bin",
];
const PLATFORM: Platform = if cfg!(target_os = "macos") { Platform::MacOs } else { Platform::Linux };

/// The installed program: the one on `PATH`, or else in a system or Homebrew folder. Each call
/// looks again, so a program installed while Borg runs is found.
pub(crate) fn find(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    folders(&path).into_iter().map(|dir| dir.join(program)).find(|candidate| candidate.is_file())
}

/// The folders `find` looks in, in order: the absolute ones on `path`, then the system's and
/// Homebrew's.
fn folders(path: &OsStr) -> Vec<PathBuf> {
    let known = SYSTEM_DIRS.into_iter().chain(HOMEBREW_DIRS).chain(HOMEBREW_KEGS).map(PathBuf::from);
    std::env::split_paths(path).filter(|dir| dir.is_absolute()).chain(known).collect()
}

/// How this machine installs a missing program, when Borg knows its package manager.
pub(crate) fn installer() -> Option<Installer> {
    let root = rustix::process::geteuid().is_root();
    Installer::detect(PLATFORM, root, |program| find(program).is_some())
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::*;

    #[test]
    fn programs_are_looked_for_on_the_path_first_then_where_homebrew_keeps_bsdtar() {
        let path = OsString::from("/home/me/bin:relative:/usr/local/bin");
        let folders = folders(&path);
        assert_eq!(folders[..2], [PathBuf::from("/home/me/bin"), PathBuf::from("/usr/local/bin")]);
        assert!(folders.contains(&PathBuf::from("/opt/homebrew/opt/libarchive/bin")));
        assert!(folders.contains(&PathBuf::from("/home/linuxbrew/.linuxbrew/opt/libarchive/bin")));
    }
}
