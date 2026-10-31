use super::*;
use crate::activity::helper::bounded_output;
use crate::activity::helper::read_output;
use crate::activity::maps::parse_smaps;
use crate::activity::maps::parse_vmmap;
use crate::activity::reading::first_data;
use crate::memory_search::Query;
use anyhow::{Result, bail};
use borg_core::{MemorySnapshot, ProcessKey, SNAPSHOT_LIMIT};
use std::{
    fs::{self},
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};
use tokio::process::Command;
#[test]
fn linux_regions_keep_private_shared_swap_and_file_names_with_spaces() {
    let text = "1000-3000 rw-p 0002a000 08:01 1234 /tmp/file with spaces\nRss: 8 kB\nPrivate_Clean: 2 kB\nPrivate_Dirty: 3 kB\nShared_Clean: 1 kB\nShared_Dirty: 2 kB\nSwap: 4 kB\nVmFlags: rd wr\n3000-4000 ---p 00000000 00:00 0\n";
    let regions = parse_smaps(text);
    assert_eq!(regions.len(), 2);
    assert_eq!(regions[0].name, "/tmp/file with spaces");
    assert_eq!(regions[0].path.as_deref(), Some("/tmp/file with spaces"));
    assert_eq!((regions[0].offset, regions[1].offset), (Some(0x2a000), None));
    assert_eq!(regions[0].footprint(), Some(5120 + 4096));
    assert_eq!(regions[0].resident, Some(8192));
    assert_eq!(regions[0].private, Some(5120));
    assert_eq!(regions[0].shared, Some(3072));
    assert_eq!(regions[0].swap, Some(4096));
    assert!(!regions[1].readable());
}
#[test]
fn mac_regions_parse_real_vmmap_layout_and_ignore_summary_totals() {
    let text = "__TEXT 0000000100000000-0000000100004000 [ 16K 12K 0K 0K] r-x/r-x SM=COW /Applications/Test App\nMALLOC_SMALL 0000000200000000-0000000200100000 [ 1024K 64K 48K 2048] rw-/rwx SM=PRV\nStack 0000000300000000-0000000300004000 [ 16K 16K 16K 0K] rw-/rwx SM=PRV          thread 2\nTOTAL 4G 3G\n";
    let regions = parse_vmmap(text);
    assert_eq!(regions.len(), 3);
    assert_eq!(regions[0].start, 0x100000000);
    assert_eq!(regions[0].resident, Some(12 * 1024));
    assert_eq!(regions[0].name, "__TEXT · /Applications/Test App");
    assert_eq!(regions[0].path.as_deref(), Some("/Applications/Test App"));
    assert_eq!(regions[0].kind(), "__TEXT");
    assert_eq!(regions[0].footprint(), Some(0));
    assert_eq!(regions[1].end - regions[1].start, 1024 * 1024);
    assert_eq!(regions[1].footprint(), Some(48 * 1024 + 2048));
    assert!(regions[1].readable());
    assert_eq!((regions[2].name.as_str(), regions[2].path.as_deref()), ("Stack · thread 2", None));
    assert!(parse_vmmap("vmmap: process protected").is_empty());
}
#[test]
fn snapshot_exports_are_private_bounded_and_never_replace_existing_files() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("snapshot.bin");
    export(&path, b"sample bytes").unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"sample bytes");
    assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    assert!(export(&path, b"replace").is_err());
    assert!(export(&temp.path().join("large"), &vec![0; SNAPSHOT_LIMIT + 1]).is_err());
    assert!(!temp.path().join("large").exists());
}
#[test]
fn sampler_reports_its_own_process_and_rejects_reused_identity() {
    let mut sampler = Sampler::default();
    let sample = sampler.sample(None);
    let own = sample.processes.iter().find(|p| p.key.pid == std::process::id()).unwrap();
    assert!(own.resident.is_some());
    assert!(own.cpu.is_none() && own.read.is_none());
    assert!(verify(own.key).is_ok());
    assert!(verify(ProcessKey { start: own.key.start.saturating_add(1), ..own.key }).is_err());
}
#[test]
fn seeking_skips_zero_pages_to_the_page_holding_data_within_the_limit() {
    let mut memory = vec![0u8; 3 * SNAPSHOT_LIMIT];
    memory[2 * SNAPSHOT_LIMIT + 5000] = 7;
    let read = |start: u64, end: u64| Ok(memory[start as usize..end as usize].to_vec());
    let end = memory.len() as u64;
    assert_eq!(first_data(read, SNAPSHOT_LIMIT as u64, end, 0), Some(2 * SNAPSHOT_LIMIT as u64 + 4096));
    assert_eq!(first_data(read, 2 * SNAPSHOT_LIMIT as u64 + 6000, end, 0), None, "zeros to the end");
    let unreadable = |_: u64, _: u64| -> Result<Vec<u8>> { bail!("unreadable") };
    assert_eq!(first_data(unreadable, 0, end, 0), None);
}
#[test]
fn memory_reader_can_capture_known_bytes_from_its_own_address_space() {
    let bytes = b"Borg memory capture test\0";
    let memory = Memory::open(ProcessKey { pid: std::process::id(), start: 0 }).unwrap();
    assert_eq!(memory.read(bytes.as_ptr() as u64, bytes.len()).unwrap(), bytes);
}
#[tokio::test]
async fn integrity_finds_an_image_hidden_in_its_own_memory_and_runs_its_files_unchanged() {
    use borg_core::FindingKind;
    let own = Sampler::default().sample(None).processes.into_iter().find(|p| p.key.pid == std::process::id());
    // An executable header at the start of a page of memory no file backs.
    let mut buffer = vec![0u8; 64 * PAGE as usize];
    let at = (buffer.as_ptr() as usize).next_multiple_of(PAGE as usize) - buffer.as_ptr() as usize;
    buffer[at..at + 6].copy_from_slice(&[0x7f, b'E', b'L', b'F', 2, 1]);
    buffer[at + 16] = 3;
    let address = buffer.as_ptr() as u64 + at as u64;
    let report = integrity(own.unwrap().key, Arc::new(AtomicBool::new(false)), None).await.unwrap();
    std::hint::black_box(&buffer);
    let kinds: Vec<_> = report.findings.iter().map(|f| (f.kind, f.address, f.detail.as_str())).collect();
    assert!(kinds.iter().any(|&(kind, at, _)| kind == FindingKind::HiddenImage && at == Some(address)), "{kinds:?}");
    assert!(!kinds.iter().any(|&(kind, ..)| kind == FindingKind::CodeChanged), "{kinds:?}");
    // Tests running beside this one free memory as it reads, so some may be gone.
    assert!(report.compared > 0 && report.scanned > 0, "{report:?}");
}
#[tokio::test]
async fn command_output_is_bounded_and_inspection_has_a_deadline() {
    let mut command = Command::new("/bin/sh");
    command.args(["-c", "printf '1234567890'"]);
    assert_eq!(bounded_output(command, TIMEOUT).await.unwrap(), b"1234567890");
    assert!(read_output(&b"12345"[..], 4).await.is_err());
    let mut command = Command::new("/bin/sleep");
    command.arg("5");
    assert!(bounded_output(command, Duration::from_millis(10)).await.unwrap_err().to_string().contains("timed out"));
}
#[tokio::test]
async fn snapshot_search_needs_no_live_process_and_preserves_unicode_addresses() {
    let process = ProcessKey { pid: u32::MAX, start: 1 };
    let bytes: Vec<_> = "--𝐁𝐨𝐫𝐠--".encode_utf16().flat_map(u16::to_le_bytes).collect();
    let snapshot = MemorySnapshot { process, address: 0x4000, bytes, captured: 1 };
    let query = Query { text: "Borg".into(), mode: borg_core::MemorySearchMode::Unicode, match_case: true };
    let report =
        search_memory(process, query, Some(snapshot), None, Arc::new(AtomicBool::new(false)), None).await.unwrap();
    assert!(report.hits.iter().any(|h| h.address == 0x4004 && h.encoding == "UTF-16LE"));
    assert_eq!(report.skipped, 0);
    assert!(report.next.is_none());
}
