//! What a macOS executable's code signature says about who may read its process's memory.
//! macOS lets no other program, administrators included, read the memory of its own programs
//! (System Integrity Protection) or of programs built with the Hardened Runtime that do not
//! allow debugging. Both show in the signature, so Borg tells why before a read fails.

use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    path::Path,
    sync::OnceLock,
};

use borg_core::MemoryProtection;

const FAT_MAGIC: u32 = 0xcafe_babe;
const FAT_MAGIC_64: u32 = 0xcafe_babf;
const MH_MAGIC_64: u32 = 0xfeed_facf;
const LC_CODE_SIGNATURE: u32 = 0x1d;
const SUPERBLOB_MAGIC: u32 = 0xfade_0cc0;
const CODE_DIRECTORY_MAGIC: u32 = 0xfade_0c02;
const ENTITLEMENTS_MAGIC: u32 = 0xfade_7171;
const CODE_DIRECTORY_SLOT: u32 = 0;
const ENTITLEMENTS_SLOT: u32 = 5;
const CS_RUNTIME: u32 = 0x1_0000;
#[cfg(target_arch = "aarch64")]
const NATIVE_CPU: u32 = 0x0100_000c;
#[cfg(not(target_arch = "aarch64"))]
const NATIVE_CPU: u32 = 0x0100_0007;
/// Larger headers than any real executable carries mean the file is not one.
const COMMANDS_LIMIT: u32 = 1 << 20;
const BLOBS_LIMIT: u32 = 64;
const ENTITLEMENTS_LIMIT: u32 = 1 << 20;
const GET_TASK_ALLOW: &str = "<key>com.apple.security.get-task-allow</key>";
const CRYPTEXES: [&str; 2] = ["/System/Cryptexes", "/System/Volumes/Preboot/Cryptexes"];

/// Why macOS keeps every other program out of the memory of a process running `executable`, or
/// `None` when nothing in its signature does or the system leaves debugging unrestricted.
pub(crate) fn protection(executable: &Path) -> Option<MemoryProtection> {
    if !cfg!(target_os = "macos") || !debugging_restricted() {
        return None;
    }
    // Programs macOS ships in cryptexes, such as Safari's, carry no platform mark in their own
    // signature; the system trusts them as its own all the same.
    if CRYPTEXES.iter().any(|root| executable.starts_with(root)) {
        return Some(MemoryProtection::System);
    }
    read(&mut File::open(executable).ok()?).ok().flatten()
}

/// Whether System Integrity Protection still restricts debugging, as it does unless someone
/// turned it off; the setting holds until the next restart, so it is read once.
fn debugging_restricted() -> bool {
    static RESTRICTED: OnceLock<bool> = OnceLock::new();
    *RESTRICTED.get_or_init(|| {
        let Ok(output) = std::process::Command::new("/usr/bin/csrutil").arg("status").output() else {
            return true;
        };
        let status = String::from_utf8_lossy(&output.stdout);
        !(status.contains("status: disabled") || status.contains("Debugging Restrictions: disabled"))
    })
}

fn read(file: &mut (impl Read + Seek)) -> io::Result<Option<MemoryProtection>> {
    let Some(base) = native_slice(file)? else { return Ok(None) };
    let header = read_at(file, base, 32)?;
    if le(&header, 0) != MH_MAGIC_64 {
        return Ok(None);
    }
    let (count, size) = (le(&header, 16), le(&header, 20));
    if size > COMMANDS_LIMIT {
        return Ok(None);
    }
    let commands = read_at(file, base + 32, size as usize)?;
    let mut at = 0;
    let mut signature = None;
    for _ in 0..count {
        let Some(command) = commands.get(at..at + 8) else { break };
        let length = le(command, 4) as usize;
        if le(command, 0) == LC_CODE_SIGNATURE {
            signature = commands.get(at..at + 16).map(|command| le(command, 8));
            break;
        }
        if length < 8 {
            break;
        }
        at += length;
    }
    let Some(offset) = signature else { return Ok(None) };
    let superblob = base + u64::from(offset);
    let index = read_at(file, superblob, 12)?;
    if be(&index, 0) != SUPERBLOB_MAGIC || be(&index, 8) > BLOBS_LIMIT {
        return Ok(None);
    }
    let slots = read_at(file, superblob + 12, be(&index, 8) as usize * 8)?;
    let blob = |slot: u32| {
        slots
            .as_chunks::<8>()
            .0
            .iter()
            .find(|entry| be(*entry, 0) == slot)
            .map(|entry| superblob + u64::from(be(entry, 4)))
    };
    let Some(directory) = blob(CODE_DIRECTORY_SLOT) else { return Ok(None) };
    let directory = read_at(file, directory, 40)?;
    if be(&directory, 0) != CODE_DIRECTORY_MAGIC {
        return Ok(None);
    }
    if directory[38] != 0 {
        return Ok(Some(MemoryProtection::System));
    }
    if be(&directory, 12) & CS_RUNTIME == 0 {
        return Ok(None);
    }
    let debuggable = match blob(ENTITLEMENTS_SLOT) {
        Some(at) => {
            let header = read_at(file, at, 8)?;
            let length = be(&header, 4);
            if be(&header, 0) != ENTITLEMENTS_MAGIC || !(8..=ENTITLEMENTS_LIMIT).contains(&length) {
                return Ok(None);
            }
            let plist = read_at(file, at + 8, length as usize - 8)?;
            allows_debugging(&String::from_utf8_lossy(&plist))
        }
        None => false,
    };
    Ok((!debuggable).then_some(MemoryProtection::HardenedRuntime))
}

/// Where the slice this computer runs starts: the whole file, or its slice of a universal binary,
/// else the first slice, as when Borg runs translated beside programs with no slice of its kind.
fn native_slice(file: &mut (impl Read + Seek)) -> io::Result<Option<u64>> {
    let header = read_at(file, 0, 8)?;
    let wide = match be(&header, 0) {
        FAT_MAGIC => false,
        FAT_MAGIC_64 => true,
        _ => return Ok(Some(0)),
    };
    let count = be(&header, 4).min(BLOBS_LIMIT);
    let size = if wide { 32 } else { 20 };
    let table = read_at(file, 8, count as usize * size)?;
    let offset = |entry: &[u8]| {
        if wide { (u64::from(be(entry, 8)) << 32) | u64::from(be(entry, 12)) } else { u64::from(be(entry, 8)) }
    };
    let mut slices = table.chunks_exact(size);
    Ok(slices.clone().find(|entry| be(entry, 0) == NATIVE_CPU).or_else(|| slices.next()).map(offset))
}

fn allows_debugging(plist: &str) -> bool {
    plist.split_once(GET_TASK_ALLOW).is_some_and(|(_, rest)| rest.trim_start().starts_with("<true/>"))
}

fn read_at(file: &mut (impl Read + Seek), offset: u64, len: usize) -> io::Result<Vec<u8>> {
    file.seek(SeekFrom::Start(offset))?;
    let mut bytes = vec![0; len];
    file.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn le(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap_or_default())
}

fn be(bytes: &[u8], at: usize) -> u32 {
    u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// A 64-bit executable for this computer with a signature holding a code directory with
    /// `platform` and `flags`, and `entitlements` when given.
    fn executable(platform: u8, flags: u32, entitlements: Option<&str>) -> Vec<u8> {
        let mut directory = vec![0; 44];
        directory[0..4].copy_from_slice(&CODE_DIRECTORY_MAGIC.to_be_bytes());
        directory[4..8].copy_from_slice(&44u32.to_be_bytes());
        directory[12..16].copy_from_slice(&flags.to_be_bytes());
        directory[38] = platform;
        let mut blobs = vec![(CODE_DIRECTORY_SLOT, directory)];
        if let Some(plist) = entitlements {
            let mut blob = ENTITLEMENTS_MAGIC.to_be_bytes().to_vec();
            blob.extend_from_slice(&(plist.len() as u32 + 8).to_be_bytes());
            blob.extend_from_slice(plist.as_bytes());
            blobs.push((ENTITLEMENTS_SLOT, blob));
        }
        let mut superblob = SUPERBLOB_MAGIC.to_be_bytes().to_vec();
        superblob.extend_from_slice(&0u32.to_be_bytes());
        superblob.extend_from_slice(&(blobs.len() as u32).to_be_bytes());
        let mut offset = 12 + blobs.len() as u32 * 8;
        for (slot, blob) in &blobs {
            superblob.extend_from_slice(&slot.to_be_bytes());
            superblob.extend_from_slice(&offset.to_be_bytes());
            offset += blob.len() as u32;
        }
        for (_, blob) in blobs {
            superblob.extend_from_slice(&blob);
        }
        // A segment command the reader steps over, then the signature's.
        let segment = [0x19u32, 24, 0, 0, 0, 0];
        let signature_at = 32 + 24 + 16;
        let signature = [LC_CODE_SIGNATURE, 16, signature_at, superblob.len() as u32];
        let mut file = Vec::new();
        for word in [MH_MAGIC_64, NATIVE_CPU, 0, 2, 2, 40, 0, 0].into_iter().chain(segment).chain(signature) {
            file.extend_from_slice(&word.to_le_bytes());
        }
        file.extend_from_slice(&superblob);
        file
    }

    /// A universal binary with `slice` at 4096 for each CPU in `cpus`, or at the start of the file,
    /// where no program is, for a CPU of 0.
    fn universal(cpus: &[u32], slice: &[u8]) -> Vec<u8> {
        let mut file = Vec::new();
        file.extend_from_slice(&FAT_MAGIC.to_be_bytes());
        file.extend_from_slice(&(cpus.len() as u32).to_be_bytes());
        for &cpu in cpus {
            let at = if cpu == 0 { 0 } else { 4096 };
            for word in [cpu, 0, at, slice.len() as u32, 12] {
                file.extend_from_slice(&word.to_be_bytes());
            }
        }
        file.resize(4096, 0);
        file.extend_from_slice(slice);
        file
    }

    fn protection_of(file: Vec<u8>) -> Option<MemoryProtection> {
        read(&mut Cursor::new(file)).unwrap()
    }

    #[test]
    fn platform_code_and_the_hardened_runtime_protect_memory_unless_debugging_is_allowed() {
        assert_eq!(protection_of(executable(26, 0x2000, None)), Some(MemoryProtection::System));
        assert_eq!(protection_of(executable(0, CS_RUNTIME, None)), Some(MemoryProtection::HardenedRuntime));
        let closed = "<plist><dict><key>com.apple.security.cs.allow-jit</key><true/></dict></plist>";
        assert_eq!(protection_of(executable(0, CS_RUNTIME, Some(closed))), Some(MemoryProtection::HardenedRuntime));
        let open = format!("<plist><dict>{GET_TASK_ALLOW}\n\t<true/></dict></plist>");
        assert_eq!(protection_of(executable(0, CS_RUNTIME, Some(&open))), None);
        let refused = format!("<plist><dict>{GET_TASK_ALLOW}<false/></dict></plist>");
        assert_eq!(protection_of(executable(0, CS_RUNTIME, Some(&refused))), Some(MemoryProtection::HardenedRuntime));
        assert_eq!(protection_of(executable(0, 0x2_0002, None)), None, "an ad-hoc signature protects nothing");
    }

    #[test]
    fn a_universal_binary_is_read_from_this_computers_slice_and_other_files_protect_nothing() {
        let platform = executable(26, 0, None);
        assert_eq!(protection_of(universal(&[0, NATIVE_CPU], &platform)), Some(MemoryProtection::System));
        assert_eq!(protection_of(universal(&[0x0100_0042], &platform)), Some(MemoryProtection::System));
        assert_eq!(protection_of(universal(&[0, 0x0100_0042], &platform)), None, "the first slice holds no program");
        let script = b"#!/bin/sh\necho a script with no Mach-O header at all\n".to_vec();
        assert_eq!(protection_of(script), None);
        let mut unsigned = executable(26, 0, None);
        unsigned[32 + 24..32 + 24 + 4].copy_from_slice(&0x1bu32.to_le_bytes());
        assert_eq!(protection_of(unsigned), None);
        assert!(read(&mut Cursor::new(executable(26, 0, None)[..60].to_vec())).is_err());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn system_programs_are_protected_unless_debugging_is_unrestricted_and_this_test_is_not() {
        let system = debugging_restricted().then_some(MemoryProtection::System);
        assert_eq!(protection(Path::new("/bin/sleep")), system);
        assert_eq!(protection(Path::new("/System/Cryptexes/App/usr/libexec/agent")), system);
        assert_eq!(protection(&std::env::current_exe().unwrap()), None);
    }
}
