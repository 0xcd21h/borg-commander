//! What the monitor reads of each process and of the system.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
}
