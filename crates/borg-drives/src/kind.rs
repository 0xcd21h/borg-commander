use serde::{Deserialize, Serialize};

const MEGABYTE: u64 = 1_000_000;

const HARD_DRIVE: Rates = Rates { read: 120 * MEGABYTE, write: 100 * MEGABYTE };
const SOLID_STATE: Rates = Rates { read: 900 * MEGABYTE, write: 700 * MEGABYTE };
const MEMORY: Rates = Rates { read: 4000 * MEGABYTE, write: 3000 * MEGABYTE };
const NETWORK: Rates = Rates { read: 60 * MEGABYTE, write: 40 * MEGABYTE };
const REMOTE: Rates = Rates { read: 10 * MEGABYTE, write: 8 * MEGABYTE };
const UNKNOWN: Rates = Rates { read: 150 * MEGABYTE, write: 100 * MEGABYTE };

/// The kinds of drive Borg tells apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DriveKind {
    /// A spinning disk.
    HardDrive,
    SolidState,
    /// A filesystem kept in memory, such as `tmpfs` or a RAM disk.
    Memory,
    /// A share another machine serves, such as NFS, SMB or sshfs.
    Network,
    /// A machine a remote panel shows.
    Remote,
    /// A drive the system says too little about.
    Unknown,
}

/// How many bytes a drive reads and writes each second.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rates {
    pub read: u64,
    pub write: u64,
}

impl DriveKind {
    /// How fast a drive of this kind usually is, until Borg measures the drive itself.
    pub fn typical(self) -> Rates {
        match self {
            Self::HardDrive => HARD_DRIVE,
            Self::SolidState => SOLID_STATE,
            Self::Memory => MEMORY,
            Self::Network => NETWORK,
            Self::Remote => REMOTE,
            Self::Unknown => UNKNOWN,
        }
    }
}
