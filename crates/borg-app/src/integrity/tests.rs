use super::*;
use crate::integrity::compare::image_format;
use crate::integrity::compare::slice_offset;
use anyhow::{Result, anyhow};
use borg_core::{Finding, FindingKind, MemoryRegion, ProcessKey, Severity};
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;

/// A process's memory, and the files it sees, held in memory.
#[derive(Default)]
struct Model {
    memory: Vec<(u64, Vec<u8>)>,
    files: HashMap<&'static str, Vec<u8>>,
    /// Pages the page tables call the file's own.
    clean: Vec<u64>,
    page_tables: bool,
    reads: std::cell::Cell<u64>,
}
impl Access for Model {
    fn read(&self, address: u64, len: usize) -> Result<Vec<u8>> {
        self.reads.set(self.reads.get() + len as u64);
        let (start, bytes) = self
            .memory
            .iter()
            .find(|(start, bytes)| *start <= address && address + len as u64 <= start + bytes.len() as u64)
            .ok_or_else(|| anyhow!("unmapped"))?;
        let from = (address - start) as usize;
        Ok(bytes[from..from + len].to_vec())
    }
    fn pages(&self, address: u64, count: usize) -> Option<Vec<PageUse>> {
        self.page_tables.then(|| {
            (0..count as u64)
                .map(|i| {
                    let clean = self.clean.contains(&(address + i * PAGE));
                    PageUse { present: true, swapped: false, file: clean }
                })
                .collect()
        })
    }
    fn read_file(&self, path: &str, offset: u64, len: usize) -> Option<Vec<u8>> {
        let bytes = self.files.get(path)?;
        let from = (offset as usize).min(bytes.len());
        Some(bytes[from..(from + len).min(bytes.len())].to_vec())
    }
    fn exists(&self, path: &str) -> Option<bool> {
        Some(self.files.contains_key(path))
    }
}

fn region(start: u64, end: u64, permissions: &str, name: &str) -> MemoryRegion {
    MemoryRegion {
        start,
        end,
        permissions: permissions.into(),
        name: name.into(),
        path: name.starts_with('/').then(|| name.into()),
        offset: name.starts_with('/').then_some(0),
        ..Default::default()
    }
}
fn kinds(findings: &[Finding]) -> Vec<(FindingKind, Severity)> {
    findings.iter().map(|f| (f.kind, f.severity)).collect()
}
fn elf() -> Vec<u8> {
    let mut page = vec![0u8; PAGE as usize];
    page[..6].copy_from_slice(&[0x7f, b'E', b'L', b'F', 2, 1]);
    page[16] = 3;
    page
}

#[test]
fn the_memory_map_shows_code_no_file_backs_and_code_from_deleted_or_temporary_files() {
    let regions = [
        region(0x1000, 0x2000, "r-xp", "[vdso]"),
        region(0x1000, 0x2000, "r-x", "__TEXT · dyld shared cache combined __TEXT"),
        region(0x2000, 0x3000, "r-xp", ""),
        region(0x3000, 0x4000, "rwxp", "[anon:payload]"),
        region(0x4000, 0x5000, "r-xp", "/usr/lib/libssl.so.3 (deleted)"),
        region(0x5000, 0x6000, "r-xp", "/usr/lib/libgone.so (deleted)"),
        region(0x6000, 0x7000, "r-xp", "/memfd:libffi (deleted)"),
        region(0x7000, 0x8000, "r-xp", "/memfd:x (deleted)"),
        region(0x8000, 0x9000, "r-xp", "/tmp/stage/lib.so"),
        region(0x9000, 0xa000, "rwxp", "/opt/shop/libold.so"),
        region(0xa000, 0xb000, "rw-p", "/tmp/data"),
    ];
    let exists = |path: &str| Some(path == "/usr/lib/libssl.so.3");
    assert_eq!(
        kinds(&map_findings(&regions, false, exists)),
        [
            (FindingKind::UnbackedCode, Severity::Warning),
            (FindingKind::UnbackedCode, Severity::Alert),
            (FindingKind::ReplacedFile, Severity::Notice),
            (FindingKind::DeletedCode, Severity::Alert),
            (FindingKind::DeletedCode, Severity::Notice),
            (FindingKind::DeletedCode, Severity::Alert),
            (FindingKind::TemporaryCode, Severity::Warning),
            (FindingKind::WritableCode, Severity::Warning),
        ]
    );
    // Files that cannot be seen leave a deletion unexplained, not proven.
    assert_eq!(kinds(&map_findings(&regions[5..6], false, |_| None)), [(FindingKind::DeletedCode, Severity::Warning)]);
    // A program that compiles code as it runs makes code no file backs all the time.
    let jit = map_findings(&regions[2..4], true, exists);
    assert_eq!(kinds(&jit), [(FindingKind::UnbackedCode, Severity::Notice); 2]);
    assert!(jit[1].detail.contains("rwxp in [anon:payload]"), "{}", jit[1].detail);
    let facts = Facts { name: "dbeaver".into(), ..Default::default() };
    assert!(compiles_code(&facts, &[region(0, 1, "r-xp", "/opt/dbeaver/jre/lib/server/libjvm.so")]));
    assert!(!compiles_code(&facts, &regions));
    // Assembled here, so the public-content check, which looks for real ones, stays quiet.
    let user = |rest: &str| ["/private/var/folders/", "xy/", rest].concat();
    assert!(temporary(&user("abc123/T/lib.dylib")) && !temporary(&user("abc123/C/cache")));
}

#[test]
fn the_process_shows_its_program_environment_tracer_and_name() {
    let facts = Facts {
        name: "kworker/0:1".into(),
        program: Some(("/home/me/.cache/run".into(), true)),
        environment: vec![
            "LD_PRELOAD=/home/me/hook.so".into(),
            "DYLD_LIBRARY_PATH=/opt/shop/lib".into(),
            "LD_AUDIT=".into(),
            "PATH=/usr/bin".into(),
        ],
        preloaded: vec!["/lib/libsnoop.so".into()],
        tracer: Some((812, "gdb".into())),
    };
    let findings = process_findings(&facts, |_| Some(false));
    assert_eq!(
        kinds(&findings),
        [
            (FindingKind::ProgramGone, Severity::Alert),
            (FindingKind::Preload, Severity::Warning),
            (FindingKind::LibraryPath, Severity::Notice),
            (FindingKind::Preload, Severity::Warning),
            (FindingKind::Traced, Severity::Warning),
            (FindingKind::NameMismatch, Severity::Warning),
        ]
    );
    assert_eq!(findings[1].detail, "LD_PRELOAD=/home/me/hook.so");
    assert!(findings[4].detail.contains("812") && findings[4].detail.contains("gdb"));
    // An update replaces a running program's file; scripts and long names are named as they are.
    let named = |name: &str, path: &str| {
        let facts = Facts { name: name.into(), program: Some((path.into(), false)), ..Default::default() };
        kinds(&process_findings(&facts, |_| Some(true)))
    };
    assert_eq!(named("backup", "/usr/bin/python3.12"), []);
    assert_eq!(named("shop-inventory-", "/opt/shop/shop-inventory-sync"), []);
    assert_eq!(named("sshd", "/home/me/miner"), [(FindingKind::NameMismatch, Severity::Notice)]);
    let replaced = Facts { program: Some(("/usr/sbin/cron".into(), true)), name: "cron".into(), ..Default::default() };
    assert_eq!(kinds(&process_findings(&replaced, |_| Some(true))), [(FindingKind::ReplacedFile, Severity::Notice)]);
}

#[test]
fn executable_headers_are_told_from_lookalikes() {
    assert_eq!(image_format(&elf()), Some("ELF"));
    let mut macho = vec![0u8; 64];
    macho[..4].copy_from_slice(&0xfeed_facf_u32.to_le_bytes());
    macho[12] = 6;
    macho[16] = 20;
    assert_eq!(image_format(&macho), Some("Mach-O"));
    macho[27] = 0x80;
    assert_eq!(image_format(&macho), None, "in the shared cache");
    let mut pe = vec![0u8; 512];
    pe[..2].copy_from_slice(b"MZ");
    pe[0x3c] = 0x80;
    pe[0x80..0x84].copy_from_slice(b"PE\0\0");
    assert_eq!(image_format(&pe), Some("PE"));
    pe[0x80] = b'X';
    assert_eq!(image_format(&pe), None);
    // A Java class starts like a universal Mach-O file.
    assert_eq!(image_format(&[0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 0x34]), None);
    assert_eq!(image_format(b"\x7fELF but text"), None);
}

#[test]
fn a_universal_file_is_compared_from_the_slice_that_matches_memory() {
    let key: Vec<u8> = (0..24).collect();
    let mut head = vec![0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 2];
    for offset in [0x4000u32, 0x8000] {
        head.extend([0; 8]);
        head.extend(offset.to_be_bytes());
        head.extend([0; 8]);
    }
    let file = |offset: u64| Some(if offset == 0x8000 { key.clone() } else { vec![9; 24] });
    assert_eq!(slice_offset(&head, &key, file), Some(0x8000));
    assert_eq!(slice_offset(&key, &key, file), Some(0), "thin");
    assert_eq!(slice_offset(&[0; 32], &key, file), None);
}

#[test]
fn code_is_compared_with_its_file_and_unbacked_memory_searched_for_images() {
    let code: Vec<u8> = (0..3 * PAGE).map(|i| (i % 251) as u8).collect();
    let mut patched = code.clone();
    patched[PAGE as usize + 0x10..PAGE as usize + 0x15].copy_from_slice(&[0xe9, 0xde, 0xad, 0xbe, 0xef]);
    let mut hidden = vec![0u8; 2 * PAGE as usize];
    hidden[PAGE as usize..2 * PAGE as usize].copy_from_slice(&elf());
    let model = Model {
        memory: vec![(0x10000, patched), (0x20000, hidden), (0x22000, vec![0xc3; PAGE as usize])],
        files: HashMap::from([("/opt/shop/bin/shop", code)]),
        ..Default::default()
    };
    let regions = [
        region(0x10000, 0x13000, "r-xp", "/opt/shop/bin/shop"),
        region(0x20000, 0x22000, "rw-p", ""),
        region(0x22000, 0x23000, "r-xp", ""),
        region(0x30000, 0x31000, "rw-p", "[heap]"),
    ];
    let facts =
        Facts { name: "shop".into(), program: Some(("/opt/shop/bin/shop".into(), false)), ..Default::default() };
    let report =
        check(ProcessKey { pid: 7, start: 1 }, &facts, &regions, &model, &AtomicBool::new(false), |_, _, _| {});
    assert_eq!(
        kinds(&report.findings),
        [
            (FindingKind::CodeChanged, Severity::Alert),
            (FindingKind::HiddenImage, Severity::Alert),
            (FindingKind::UnbackedCode, Severity::Warning),
        ]
    );
    let changed = &report.findings[0];
    assert_eq!((changed.address, changed.length), (Some(0x11010), SHOWN));
    assert!(
        changed.detail.contains("1 of 3 pages") && changed.detail.contains("+0x1010: e9 de ad be ef"),
        "{}",
        changed.detail
    );
    assert_eq!(report.findings[1].address, Some(0x21000));
    assert_eq!((report.compared, report.uncompared), (3 * PAGE, 0));
    assert_eq!((report.unbacked, report.scanned, report.unreadable), (4 * PAGE, 3 * PAGE, PAGE));
    assert!(report.complete);

    // Pages the page tables call the file's own hold what the file does, so are not read.
    let model = Model { page_tables: true, clean: vec![0x10000, 0x11000, 0x12000], reads: Default::default(), ..model };
    let report =
        check(ProcessKey { pid: 7, start: 1 }, &facts, &regions[..1], &model, &AtomicBool::new(false), |_, _, _| {});
    assert_eq!((kinds(&report.findings), report.compared, model.reads.get()), (vec![], 3 * PAGE, 0));
    // So are code the memory map calls clean and memory it says nothing of is in use.
    let model = Model { page_tables: false, ..model };
    let quiet = [
        MemoryRegion { dirty: Some(0), ..regions[0].clone() },
        MemoryRegion { resident: Some(0), swap: Some(0), ..regions[1].clone() },
    ];
    let report = check(ProcessKey { pid: 7, start: 1 }, &facts, &quiet, &model, &AtomicBool::new(false), |_, _, _| {});
    assert_eq!((report.compared, report.scanned, model.reads.get()), (3 * PAGE, 2 * PAGE, 0));

    let report = check(ProcessKey { pid: 7, start: 1 }, &facts, &regions, &model, &AtomicBool::new(true), |_, _, _| {});
    assert!(!report.complete && report.compared == 0 && report.uncompared == 3 * PAGE);
}
