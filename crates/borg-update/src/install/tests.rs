use crate::{Release, UpdateError, Version, release::fetch};
use flate2::{Compression, write::GzEncoder};
use ring::signature::{Ed25519KeyPair, KeyPair};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

use super::*;

fn served(body: Vec<u8>, read: impl FnOnce(&str)) {
    use std::io::{BufRead, BufReader, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/asset", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(&mut stream);
        let mut line = String::new();
        while reader.read_line(&mut line).unwrap() != 0 {
            if line == "\r\n" {
                break;
            }
            line.clear();
        }
        let header = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
        let _ = stream.write_all(header.as_bytes()).and_then(|_| stream.write_all(&body));
    });
    read(&url);
    server.join().unwrap();
}

#[test]
fn downloads_archives_larger_than_the_http_library_default_limit() {
    const LARGE_ARCHIVE_BYTES: usize = 11 * 1024 * 1024;
    let payload = vec![0x5a; LARGE_ARCHIVE_BYTES];
    served(payload.clone(), |url| {
        let version = Version { major: 0, minor: 8, patch: 2 };
        let name = asset_name(version, "x86_64-unknown-linux-musl");
        let release = Release { version, assets: vec![crate::Asset { name: name.clone(), url: url.into() }] };
        assert_eq!(download(&release, &name, ARCHIVE_LIMIT).unwrap(), payload);
    });
}

#[test]
fn checksum_downloads_remain_bounded() {
    served(vec![0; CHECKSUM_LIMIT as usize + 1], |url| {
        assert!(matches!(fetch(url, CHECKSUM_LIMIT), Err(UpdateError::Network(_))));
    });
}

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
            matches!(install_signed(&release, target, &exe, &[public_key(OWNER)]), Err(UpdateError::MissingAsset(name)) if name == asset_name(release.version, expected))
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

const OWNER: [u8; 32] = [7; 32];
const STRANGER: [u8; 32] = [9; 32];
const TARGET: &str = "aarch64-apple-darwin";

fn key_pair(seed: [u8; 32]) -> Ed25519KeyPair {
    Ed25519KeyPair::from_seed_unchecked(&seed).unwrap_or_else(|e| panic!("{e}"))
}

fn public_key(seed: [u8; 32]) -> PublicKey {
    key_pair(seed).public_key().as_ref().try_into().unwrap_or_else(|e| panic!("{e}"))
}

fn sign(seed: [u8; 32], signed: &[u8]) -> Vec<u8> {
    key_pair(seed).sign(signed).as_ref().to_vec()
}

fn checksum_file(bundle: &[u8], name: &str) -> Vec<u8> {
    format!("{}  {name}\n", hex(&Sha256::digest(bundle))).into_bytes()
}

/// Serves each asset at `/<name>`, answers 404 for anything else, and returns the address.
fn serve(assets: Vec<(String, Vec<u8>)>) -> String {
    use std::io::{BufRead, BufReader, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap_or_else(|e| panic!("{e}"));
    let base = format!("http://{}", listener.local_addr().unwrap_or_else(|e| panic!("{e}")));
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(&mut stream);
            let mut request = String::new();
            let mut line = String::new();
            while reader.read_line(&mut line).is_ok_and(|read| read != 0) && line != "\r\n" {
                if request.is_empty() {
                    request.clone_from(&line);
                }
                line.clear();
            }
            let path = request.split_whitespace().nth(1).unwrap_or_default().trim_start_matches('/');
            let (status, body) = match assets.iter().find(|(name, _)| name == path) {
                Some((_, body)) => ("200 OK", body.as_slice()),
                None => ("404 Not Found", &[][..]),
            };
            let header = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
            let _ = stream.write_all(header.as_bytes()).and_then(|()| stream.write_all(body));
        }
    });
    base
}

const V1: Version = Version { major: 1, minor: 0, patch: 0 };

struct Published {
    version: Version,
    bundle: Vec<u8>,
    checksum: Vec<u8>,
    signature: Option<Vec<u8>>,
}

impl Published {
    fn signed_by(seed: [u8; 32]) -> Self {
        Self::version(V1, seed, b"#!updated")
    }

    fn version(version: Version, seed: [u8; 32], binary: &[u8]) -> Self {
        let bundle = archive(&[("borg/borg", binary)]);
        let checksum = checksum_file(&bundle, &asset_name(version, TARGET));
        let signature = Some(sign(seed, &checksum));
        Self { version, bundle, checksum, signature }
    }

    fn assets(self) -> Vec<(String, Vec<u8>)> {
        let name = asset_name(self.version, TARGET);
        let mut assets = vec![(name.clone(), self.bundle), (format!("{name}{CHECKSUM_SUFFIX}"), self.checksum)];
        assets
            .extend(self.signature.map(|signature| (format!("{name}{CHECKSUM_SUFFIX}{SIGNATURE_SUFFIX}"), signature)));
        assets
    }

    /// Installs over an executable holding `original` and returns the result with what the file holds afterwards.
    fn install(self) -> (Result<PathBuf, UpdateError>, Vec<u8>) {
        let (releases, exe) = publish(vec![self]);
        let result = install_signed(&releases[0], TARGET, exe.path(), &[public_key(OWNER)]);
        (result, exe.read())
    }
}

struct Installed {
    _directory: tempfile::TempDir,
    path: PathBuf,
}

impl Installed {
    fn path(&self) -> &Path {
        &self.path
    }

    fn read(&self) -> Vec<u8> {
        fs::read(&self.path).unwrap_or_else(|e| panic!("{e}"))
    }
}

/// Serves the releases from one address and makes an executable holding `original`.
fn publish(published: Vec<Published>) -> (Vec<Release>, Installed) {
    let versions: Vec<Version> = published.iter().map(|release| release.version).collect();
    let assets: Vec<Vec<(String, Vec<u8>)>> = published.into_iter().map(Published::assets).collect();
    let names: Vec<Vec<String>> =
        assets.iter().map(|list| list.iter().map(|(name, _)| name.clone()).collect()).collect();
    let base = serve(assets.into_iter().flatten().collect());
    let releases = versions
        .into_iter()
        .zip(names)
        .map(|(version, names)| Release {
            version,
            assets: names.into_iter().map(|name| crate::Asset { url: format!("{base}/{name}"), name }).collect(),
        })
        .collect();
    let directory = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let path = directory.path().join(BINARY);
    fs::write(&path, b"original").unwrap_or_else(|e| panic!("{e}"));
    (releases, Installed { _directory: directory, path })
}

fn name() -> String {
    asset_name(V1, TARGET)
}

fn minor(minor: u64) -> Version {
    Version { major: 1, minor, patch: 0 }
}

#[test]
fn a_copy_that_does_not_know_the_new_key_steps_to_the_newest_release_it_can_check() {
    let (releases, exe) = publish(vec![
        Published::version(minor(4), STRANGER, b"#!1.4"),
        Published::version(minor(1), OWNER, b"#!1.1"),
        Published::version(minor(3), STRANGER, b"#!1.3"),
        Published::version(minor(2), OWNER, b"#!1.2 adds the new key"),
        Published::version(V1, OWNER, b"#!1.0"),
    ]);
    let stepped = install_newest_trusted(releases, V1, TARGET, exe.path(), &[public_key(OWNER)]);
    assert_eq!(stepped.unwrap_or_else(|e| panic!("{e}")), Some(minor(2)));
    assert_eq!(exe.read(), b"#!1.2 adds the new key");
}

#[test]
fn a_copy_that_knows_both_keys_installs_releases_signed_with_either() {
    for seed in [OWNER, STRANGER] {
        let (releases, exe) = publish(vec![Published::signed_by(seed)]);
        let keys = [public_key(OWNER), public_key(STRANGER)];
        assert!(install_signed(&releases[0], TARGET, exe.path(), &keys).is_ok());
        assert_eq!(exe.read(), b"#!updated");
    }
}

#[test]
fn nothing_is_installed_when_no_newer_release_can_be_checked() {
    let (releases, exe) = publish(vec![
        Published::version(minor(2), STRANGER, b"#!1.2"),
        Published::version(minor(1), STRANGER, b"#!1.1"),
        Published::version(V1, OWNER, b"#!1.0"),
    ]);
    let stepped = install_newest_trusted(releases, V1, TARGET, exe.path(), &[public_key(OWNER)]);
    assert_eq!(stepped.unwrap_or_else(|e| panic!("{e}")), None);
    assert_eq!(exe.read(), b"original");
}

#[test]
fn a_release_signed_by_the_owner_installs() {
    let (result, exe) = Published::signed_by(OWNER).install();
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(exe, b"#!updated");
}

#[test]
fn a_release_signed_by_another_key_is_refused() {
    let (result, exe) = Published::signed_by(STRANGER).install();
    assert!(matches!(result, Err(UpdateError::Signature(ref file)) if *file == name()), "{result:?}");
    assert_eq!(exe, b"original");
}

#[test]
fn a_changed_checksum_file_is_refused() {
    let mut published = Published::signed_by(OWNER);
    published.bundle = archive(&[("borg-1.0.0/borg", b"#!malware")]);
    published.checksum = checksum_file(&published.bundle, &name());
    let (result, exe) = published.install();
    assert!(matches!(result, Err(UpdateError::Signature(_))), "{result:?}");
    assert_eq!(exe, b"original");
}

#[test]
fn a_changed_archive_is_refused() {
    let mut published = Published::signed_by(OWNER);
    published.bundle = archive(&[("borg-1.0.0/borg", b"#!malware")]);
    let (result, exe) = published.install();
    assert!(matches!(result, Err(UpdateError::Checksum(_))), "{result:?}");
    assert_eq!(exe, b"original");
}

#[test]
fn a_release_without_a_signature_is_refused() {
    let mut published = Published::signed_by(OWNER);
    published.signature = None;
    let (result, exe) = published.install();
    assert!(matches!(result, Err(UpdateError::Unsigned(ref file)) if *file == name()), "{result:?}");
    assert_eq!(exe, b"original");
}

#[test]
fn a_signed_checksum_of_another_archive_is_refused() {
    let mut published = Published::signed_by(OWNER);
    published.checksum = checksum_file(&published.bundle, "borg-0.9.0-aarch64-apple-darwin.tar.gz");
    published.signature = Some(sign(OWNER, &published.checksum));
    let (result, exe) = published.install();
    assert!(matches!(result, Err(UpdateError::Checksum(_))), "{result:?}");
    assert_eq!(exe, b"original");
}

#[test]
fn signatures_made_by_openssl_verify() {
    // A throwaway key made for this test: `openssl pkeyutl -sign -rawin` over the message below.
    const PUBLIC: &str = "07dca4a7032d69600cec57b429cc6c8ab8dc796e9a4c149b9f73d30a44051253";
    const SIGNATURE: &str = "f980bcc512db05d605c6ebca81e0d04fddeea30710268d28c2489afd116d3b58\
                             f6b8668b5805cccf3521a53a45e972ded1b92a5d2f5ea19123af43e4d2b2870e";
    let message = b"abc123  borg-1.0.0-test.tar.gz\n";
    let key = crate::signature::parse_key(PUBLIC).unwrap_or_else(|| panic!("key"));
    let signature: Vec<u8> =
        (0..SIGNATURE.len()).step_by(2).map(|at| u8::from_str_radix(&SIGNATURE[at..at + 2], 16).unwrap()).collect();
    assert!(verify_signature(&[key], message, &signature, "borg").is_ok());
    assert!(matches!(verify_signature(&[key], b"other", &signature, "borg"), Err(UpdateError::Signature(_))));
}

#[test]
fn release_keys_are_64_hex_digits() {
    assert_eq!(crate::signature::parse_key(""), None);
    assert_eq!(crate::signature::parse_key(&"zz".repeat(32)), None);
    assert_eq!(crate::signature::parse_key(&"0a".repeat(32)), Some([0x0a; 32]));
}

#[test]
fn only_a_regular_file_is_taken_as_the_program() {
    let mut builder = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::fast()));
    let mut link = tar::Header::new_gnu();
    link.set_entry_type(tar::EntryType::Symlink);
    link.set_size(0);
    builder.append_link(&mut link, "borg-1.0.0/borg", "/bin/sh").unwrap_or_else(|e| panic!("{e}"));
    let bundle = builder.into_inner().and_then(|gz| gz.finish()).unwrap_or_else(|e| panic!("{e}"));
    assert!(matches!(extract(&bundle), Err(UpdateError::MissingBinary(_))));
}

#[test]
fn an_update_keeps_the_program_it_replaces_hidden_beside_it() {
    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join("borg");
    assert_eq!(previous(&exe), dir.path().join(".borg.previous"));
    fs::write(&exe, b"old").unwrap();
    fs::write(previous(&exe), b"older").unwrap();
    keep_previous(&exe).unwrap();
    fs::write(exe.with_extension("next"), b"new").unwrap();
    fs::rename(exe.with_extension("next"), &exe).unwrap();
    assert_eq!(fs::read(previous(&exe)).unwrap(), b"old");
    restore_previous(&exe).unwrap();
    assert_eq!(fs::read(&exe).unwrap(), b"old");
    assert!(!previous(&exe).exists());
}
