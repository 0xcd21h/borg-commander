mod channel;
mod install;
mod release;
mod version;

pub use channel::{Channel, channel};
pub use install::{asset_name, install, verify};
pub use release::{Asset, LATEST_RELEASE_URL, Release, latest, parse_release};
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
    #[error("the release archive has no {0} inside")]
    MissingBinary(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
