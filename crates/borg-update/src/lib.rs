mod channel;
mod install;
mod release;
mod signature;
mod version;

pub use channel::{Channel, channel};
pub use install::{asset_name, install, install_trusted, previous, restore_previous, verify};
pub use release::{Asset, LATEST_RELEASE_URL, RELEASES_URL, Release, latest, parse_release};
pub use signature::RELEASE_PUBLIC_KEYS;
pub use version::Version;

#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error("network: {0}")]
    Network(String),
    #[error("no release has been published yet")]
    NoRelease,
    #[error("unexpected release data: {0}")]
    Release(String),
    #[error("version {0:?} is not a version number")]
    Version(String),
    #[error("no download for {0} in this release")]
    MissingAsset(String),
    #[error("checksum mismatch for {0}")]
    Checksum(String),
    #[error("{0} has no signature in this release, so it was not installed")]
    Unsigned(String),
    #[error(
        "the signature of {0} is not from a release key this copy of borg trusts, so it was not installed; install the new version with the installer instead"
    )]
    Signature(String),
    #[error("this build of borg has no release key and cannot check updates; install the new version instead")]
    NoReleaseKey,
    #[error("the release archive has no {0} inside")]
    MissingBinary(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
