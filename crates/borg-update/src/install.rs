use std::{
    fs,
    io::Read,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};

use crate::{
    Release, UpdateError, Version,
    release::{fetch, releases},
    signature::{PublicKey, SIGNATURE_LIMIT, SIGNATURE_SUFFIX, release_keys, verify_signature},
};

const BINARY: &str = "borg";
const ARCHIVE_SUFFIX: &str = ".tar.gz";
const CHECKSUM_SUFFIX: &str = ".sha256";
const STAGING_SUFFIX: &str = ".borg-update";
const EXECUTABLE: u32 = 0o755;
const ARCHIVE_LIMIT: u64 = 128 * 1024 * 1024;
const CHECKSUM_LIMIT: u64 = 16 * 1024;

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

/// The signature covers the checksum file, which names the archive it belongs to. Checking that
/// name keeps a signed checksum of another version or target from being accepted for this one.
fn verify_signed(checksum_file: &[u8], signature: &[u8], name: &str, keys: &[PublicKey]) -> Result<(), UpdateError> {
    verify_signature(keys, checksum_file, signature, name)?;
    let text = String::from_utf8_lossy(checksum_file);
    let named = text.split_whitespace().nth(1).map(|file| file.trim_start_matches('*'));
    if named == Some(name) { Ok(()) } else { Err(UpdateError::Checksum(name.to_owned())) }
}

fn extract(archive: &[u8]) -> Result<Vec<u8>, UpdateError> {
    let mut entries = tar::Archive::new(GzDecoder::new(archive));
    for entry in entries.entries()? {
        let mut entry = entry?;
        let is_binary = entry.path()?.file_name().is_some_and(|name| name == BINARY);
        if is_binary {
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes)?;
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
    install_signed(release, target, exe, &release_keys()?)
}

/// Installs `latest`, or the newest release between `current` and it that this copy can check
/// when `latest` is signed with a key this copy does not know. Returns the version installed.
/// The copy installed that way knows the newer keys and can take the next step itself.
pub fn install_trusted(
    latest: &Release,
    releases_url: &str,
    current: Version,
    target: &str,
    exe: &Path,
) -> Result<Version, UpdateError> {
    let keys = release_keys()?;
    match install_signed(latest, target, exe, &keys) {
        Ok(_) => Ok(latest.version),
        Err(UpdateError::Signature(name)) => {
            let earlier = releases(releases_url)?.into_iter().filter(|release| release.version < latest.version);
            install_newest_trusted(earlier.collect(), current, target, exe, &keys)?.ok_or(UpdateError::Signature(name))
        }
        Err(error) => Err(error),
    }
}

fn install_newest_trusted(
    mut releases: Vec<Release>,
    current: Version,
    target: &str,
    exe: &Path,
    keys: &[PublicKey],
) -> Result<Option<Version>, UpdateError> {
    releases.retain(|release| release.version > current);
    releases.sort_by_key(|release| std::cmp::Reverse(release.version));
    for release in releases {
        match install_signed(&release, target, exe, keys) {
            Ok(_) => return Ok(Some(release.version)),
            Err(
                UpdateError::Signature(_)
                | UpdateError::Unsigned(_)
                | UpdateError::MissingAsset(_)
                | UpdateError::Checksum(_),
            ) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(None)
}

fn install_signed(release: &Release, target: &str, exe: &Path, keys: &[PublicKey]) -> Result<PathBuf, UpdateError> {
    let name = release_asset_name(release, target);
    if !release.assets.iter().any(|asset| asset.name == name) {
        return Err(UpdateError::MissingAsset(name));
    }
    let checksum = download(release, &format!("{name}{CHECKSUM_SUFFIX}"), CHECKSUM_LIMIT)?;
    let signature =
        download(release, &format!("{name}{CHECKSUM_SUFFIX}{SIGNATURE_SUFFIX}"), SIGNATURE_LIMIT).map_err(|error| {
            match error {
                UpdateError::MissingAsset(_) | UpdateError::NoRelease => UpdateError::Unsigned(name.clone()),
                other => other,
            }
        })?;
    verify_signed(&checksum, &signature, &name, keys)?;
    let archive = download(release, &name, ARCHIVE_LIMIT)?;
    verify(&archive, &String::from_utf8_lossy(&checksum), &name)?;
    let binary = extract(&archive)?;

    let staging = exe.with_extension(STAGING_SUFFIX.trim_start_matches('.'));
    fs::write(&staging, binary)?;
    fs::set_permissions(&staging, fs::Permissions::from_mode(EXECUTABLE))?;
    fs::rename(&staging, exe).inspect_err(|_| {
        let _ = fs::remove_file(&staging);
    })?;
    Ok(exe.to_path_buf())
}

#[cfg(test)]
mod tests;
