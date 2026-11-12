use std::{
    fs,
    io::{self, Read},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};

use crate::{Release, UpdateError, Version, release::fetch};

const BINARY: &str = "borg";
const ARCHIVE_SUFFIX: &str = ".tar.gz";
const CHECKSUM_SUFFIX: &str = ".sha256";
const STAGING_SUFFIX: &str = ".borg-update";
/// Ends the name of the program file an update replaced, kept hidden beside the new one.
const PREVIOUS_SUFFIX: &str = ".previous";
const HIDDEN: &str = ".";
const EXECUTABLE: u32 = 0o755;
const ARCHIVE_LIMIT: u64 = 128 * 1024 * 1024;
const CHECKSUM_LIMIT: u64 = 16 * 1024;
/// The largest program Borg unpacks from a release, well above what any release holds.
const BINARY_LIMIT: u64 = 256 * 1024 * 1024;

pub fn asset_name(version: Version, target: &str) -> String {
    format!("{BINARY}-{version}-{target}{ARCHIVE_SUFFIX}")
}

fn release_asset_name(release: &Release, target: &str) -> String {
    let exact = asset_name(release.version, target);
    if release.assets.iter().any(|asset| asset.name == exact) {
        return exact;
    }
    let compatible = match target {
        "x86_64-unknown-linux-gnu" => "x86_64-unknown-linux-musl",
        "aarch64-unknown-linux-gnu" => "aarch64-unknown-linux-musl",
        _ => return exact,
    };
    asset_name(release.version, compatible)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn verify(archive: &[u8], checksum_file: &str, name: &str) -> Result<(), UpdateError> {
    let expected = checksum_file.split_whitespace().next().unwrap_or_default().to_ascii_lowercase();
    if hex(&Sha256::digest(archive)) == expected { Ok(()) } else { Err(UpdateError::Checksum(name.to_owned())) }
}

fn extract(archive: &[u8]) -> Result<Vec<u8>, UpdateError> {
    let mut entries = tar::Archive::new(GzDecoder::new(archive));
    for entry in entries.entries()? {
        let entry = entry?;
        let is_binary =
            entry.header().entry_type().is_file() && entry.path()?.file_name().is_some_and(|name| name == BINARY);
        if is_binary {
            let mut bytes = Vec::new();
            entry.take(BINARY_LIMIT + 1).read_to_end(&mut bytes)?;
            if bytes.len() as u64 > BINARY_LIMIT {
                return Err(UpdateError::TooLarge(BINARY.to_owned()));
            }
            return Ok(bytes);
        }
    }
    Err(UpdateError::MissingBinary(BINARY.to_owned()))
}

fn download(release: &Release, name: &str, limit: u64) -> Result<Vec<u8>, UpdateError> {
    let asset = release
        .assets
        .iter()
        .find(|asset| asset.name == name)
        .ok_or_else(|| UpdateError::MissingAsset(name.to_owned()))?;
    fetch(&asset.url, limit)
}

pub fn install(release: &Release, target: &str, exe: &Path) -> Result<PathBuf, UpdateError> {
    let name = release_asset_name(release, target);
    let archive = download(release, &name, ARCHIVE_LIMIT)?;
    let checksum = download(release, &format!("{name}{CHECKSUM_SUFFIX}"), CHECKSUM_LIMIT)?;
    verify(&archive, &String::from_utf8_lossy(&checksum), &name)?;
    let binary = extract(&archive)?;

    let staging = exe.with_extension(STAGING_SUFFIX.trim_start_matches('.'));
    fs::write(&staging, binary)?;
    fs::set_permissions(&staging, fs::Permissions::from_mode(EXECUTABLE))?;
    let _ = keep_previous(exe);
    fs::rename(&staging, exe).inspect_err(|_| {
        let _ = fs::remove_file(&staging);
    })?;
    Ok(exe.to_path_buf())
}

/// Where the program file an update replaced stays, hidden beside the new one.
pub fn previous(exe: &Path) -> PathBuf {
    let name = exe.file_name().unwrap_or_default().to_string_lossy();
    exe.with_file_name(format!("{HIDDEN}{name}{PREVIOUS_SUFFIX}"))
}

/// Keeps the program file as it is now under [`previous`], in place of an older one there.
fn keep_previous(exe: &Path) -> io::Result<()> {
    let kept = previous(exe);
    let staged = kept.with_extension(STAGING_SUFFIX.trim_start_matches('.'));
    let _ = fs::remove_file(&staged);
    if fs::hard_link(exe, &staged).is_err() {
        fs::copy(exe, &staged)?;
    }
    fs::rename(&staged, &kept).inspect_err(|_| {
        let _ = fs::remove_file(&staged);
    })
}

/// Puts the program file an update replaced back in place of the new one.
pub fn restore_previous(exe: &Path) -> io::Result<()> {
    fs::rename(previous(exe), exe)
}

#[cfg(test)]
mod tests;
