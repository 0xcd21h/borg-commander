//! What the monitor reads of each process and of the system.

use serde::{Deserialize, Serialize};
use std::{path::PathBuf, time::Duration};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProcessKey {
    pub pid: u32,
    pub start: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessInfo {
    pub key: ProcessKey,
    pub parent: Option<u32>,
    pub name: String,
    pub executable: Option<PathBuf>,
    pub cpu: Option<u32>,
    pub resident: Option<u64>,
    pub virtual_bytes: Option<u64>,
    pub read: Option<u64>,
    pub written: Option<u64>,
    /// Why the system keeps every other program, administrators included, out of its memory.
    #[serde(default)]
    pub protection: Option<MemoryProtection>,
    /// How nice the process is to others, from -20 to 19, when the system shows it.
    #[serde(default)]
    pub nice: Option<i32>,
    /// Paused by a stop signal until it is sent Continue.
    #[serde(default)]
    pub stopped: bool,
}

/// Why macOS lets no other program, administrators included, read a process's memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemoryProtection {
    /// Part of macOS, which System Integrity Protection guards.
    System,
    /// Built with the Hardened Runtime, without allowing debugging.
    HardenedRuntime,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemSample {
    pub processes: Vec<ProcessInfo>,
    pub total: u64,
    pub used: u64,
    pub swap_total: u64,
    pub swap_used: u64,
    pub cpu: Vec<u32>,
    /// Borg itself runs as administrator, so asking to be one again reads nothing more.
    #[serde(default)]
    pub administrator: bool,
    /// Borg's own process, which the monitor sends no signals to.
    #[serde(default)]
    pub own: Option<u32>,
    /// What the disks read and wrote, from the second sample on.
    #[serde(default)]
    pub disk: Option<DiskActivity>,
    #[serde(default)]
    pub load: Option<LoadAverage>,
    #[serde(default)]
    pub uptime: Option<Duration>,
    /// The system's threads, where it counts them.
    #[serde(default)]
    pub threads: Option<Threads>,
}

/// Bytes the disks read and wrote per second, and how busy the busiest disk was, in
/// hundredths of a percent, where the system reports it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiskActivity {
    pub read: u64,
    pub written: u64,
    pub busy: Option<u32>,
}

/// The number of processes waiting to run, averaged over one, five and fifteen minutes, in
/// hundredths.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoadAverage {
    pub one: u32,
    pub five: u32,
    pub fifteen: u32,
}

/// All of the system's threads, and those running or ready to run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Threads {
    pub total: u32,
    pub running: u32,
}
