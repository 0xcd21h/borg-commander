use std::{
    fs,
    io::Read,
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
const EXECUTABLE: u32 = 0o755;

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

fn download(release: &Release, name: &str) -> Result<Vec<u8>, UpdateError> {
    let asset = release
        .assets
        .iter()
        .find(|asset| asset.name == name)
        .ok_or_else(|| UpdateError::MissingAsset(name.to_owned()))?;
    fetch(&asset.url)
}

pub fn install(release: &Release, target: &str, exe: &Path) -> Result<PathBuf, UpdateError> {
    let name = release_asset_name(release, target);
    let archive = download(release, &name)?;
    let checksum = download(release, &format!("{name}{CHECKSUM_SUFFIX}"))?;
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
mod tests {
    use flate2::{Compression, write::GzEncoder};

    use super::*;

    fn release(targets: &[&str]) -> Release {
        let version = Version { major: 0, minor: 8, patch: 1 };
        Release {
            version,
            assets: targets
                .iter()
                .map(|target| crate::Asset { name: asset_name(version, target), url: String::new() })
                .collect(),
        }
    }

    #[test]
    fn gnu_linux_builds_use_static_releases_of_the_same_architecture() {
        let release = release(&["x86_64-unknown-linux-musl", "aarch64-unknown-linux-musl"]);
        for arch in ["x86_64", "aarch64"] {
            assert_eq!(
                release_asset_name(&release, &format!("{arch}-unknown-linux-gnu")),
                asset_name(release.version, &format!("{arch}-unknown-linux-musl"))
            );
        }
    }

    #[test]
    fn exact_release_targets_take_precedence() {
        let release = release(&["x86_64-unknown-linux-gnu", "x86_64-unknown-linux-musl", "aarch64-apple-darwin"]);
        for target in ["x86_64-unknown-linux-gnu", "x86_64-unknown-linux-musl", "aarch64-apple-darwin"] {
            assert_eq!(release_asset_name(&release, target), asset_name(release.version, target));
        }
    }

    #[test]
    fn missing_targets_do_not_install_another_architecture() {
        let release = release(&["aarch64-unknown-linux-musl"]);
        let directory = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let exe = directory.path().join(BINARY);
        fs::write(&exe, b"original").unwrap_or_else(|e| panic!("{e}"));
        for (target, expected) in [
            ("x86_64-unknown-linux-gnu", "x86_64-unknown-linux-musl"),
            ("armv7-unknown-linux-gnueabihf", "armv7-unknown-linux-gnueabihf"),
            ("aarch64-apple-darwin", "aarch64-apple-darwin"),
        ] {
            assert!(
                matches!(install(&release, target, &exe), Err(UpdateError::MissingAsset(name)) if name == asset_name(release.version, expected))
            );
            assert_eq!(fs::read(&exe).unwrap_or_else(|e| panic!("{e}")), b"original");
        }
    }

    fn archive(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut builder = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::fast()));
        for (path, body) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(body.len() as u64);
            header.set_mode(EXECUTABLE);
            header.set_cksum();
            builder.append_data(&mut header, path, *body).unwrap_or_else(|e| panic!("{e}"));
        }
        builder.into_inner().and_then(|gz| gz.finish()).unwrap_or_else(|e| panic!("{e}"))
    }

    #[test]
    fn names_assets_by_version_and_target() {
        let version = Version { major: 1, minor: 4, patch: 0 };
        assert_eq!(asset_name(version, "aarch64-apple-darwin"), "borg-1.4.0-aarch64-apple-darwin.tar.gz");
    }

    #[test]
    fn verifies_checksums_and_extracts_the_binary() {
        let bundle = archive(&[("borg-1.0.0/README.md", b"hi"), ("borg-1.0.0/borg", b"#!binary")]);
        let sum = format!("{}  borg.tar.gz\n", hex(&Sha256::digest(&bundle)));
        assert!(verify(&bundle, &sum, "borg.tar.gz").is_ok());
        assert!(matches!(verify(b"tampered", &sum, "borg.tar.gz"), Err(UpdateError::Checksum(_))));
        assert_eq!(extract(&bundle).unwrap_or_else(|e| panic!("{e}")), b"#!binary");
        assert!(matches!(extract(&archive(&[("README.md", b"x")])), Err(UpdateError::MissingBinary(_))));
    }
}
