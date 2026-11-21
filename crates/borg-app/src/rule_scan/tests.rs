use super::*;
use crate::integrity::PageUse;
use anyhow::anyhow;
use borg_core::Severity;
use std::fs;

const MARKER_RULE: &str = "rule marker { meta: severity = \"alert\" strings: $a = \"borg-marker\" condition: $a }";
const MARKER: &[u8] = b"borg-marker";

/// A process's memory held in memory: what each region holds, and the regions it cannot read.
struct Model {
    memory: Vec<(u64, Vec<u8>)>,
    reads: std::cell::Cell<u64>,
}
impl Access for Model {
    fn read(&self, address: u64, len: usize) -> anyhow::Result<Vec<u8>> {
        let (start, bytes) = self
            .memory
            .iter()
            .find(|(start, bytes)| *start <= address && address + len as u64 <= start + bytes.len() as u64)
            .ok_or_else(|| anyhow!("unreadable"))?;
        self.reads.set(self.reads.get() + len as u64);
        let from = (address - start) as usize;
        Ok(bytes[from..from + len].to_vec())
    }
    fn pages(&self, _: u64, _: usize) -> Option<Vec<PageUse>> {
        None
    }
    fn read_file(&self, _: &str, _: u64, _: usize) -> Option<Vec<u8>> {
        None
    }
    fn exists(&self, _: &str) -> Option<bool> {
        None
    }
}

fn region(start: u64, end: u64, permissions: &str, path: Option<&str>) -> MemoryRegion {
    MemoryRegion { start, end, permissions: permissions.into(), path: path.map(Into::into), ..Default::default() }
}

fn rules_with(files: &[(&str, &str)]) -> (tempfile::TempDir, Rules) {
    let folder = tempfile::tempdir().expect("folder");
    for (name, text) in files {
        fs::write(folder.path().join(name), text).expect("rule file");
    }
    let rules = compile(Some(folder.path()));
    (folder, rules)
}

#[test]
fn rule_files_are_those_named_yar_or_yara_in_the_folder_and_below_in_order() {
    let folder = tempfile::tempdir().expect("folder");
    fs::create_dir_all(folder.path().join("vendor/deep")).expect("folders");
    for name in ["b.yara", "a.yar", "notes.txt", "vendor/deep/c.YAR", "vendor/README.md"] {
        fs::write(folder.path().join(name), "").expect("file");
    }
    let found: Vec<_> =
        files(folder.path()).iter().map(|path| path.strip_prefix(folder.path()).unwrap().to_owned()).collect();
    assert_eq!(found, [PathBuf::from("a.yar"), "b.yara".into(), "vendor/deep/c.YAR".into()]);
    assert!(files(&folder.path().join("missing")).is_empty());
}

#[test]
fn a_file_is_scanned_whole_and_one_over_the_contents_limit_stays_unread() {
    let (folder, rules) = rules_with(&[("marker.yar", MARKER_RULE)]);
    let file = folder.path().join("sample.bin");
    fs::write(&file, [&b"\0\x01 head "[..], MARKER].concat()).expect("sample");
    let mut buffer = Vec::new();
    let hits = scan_file(&rules, &file, &mut buffer).expect("read").expect("within the limit");
    let [hit] = &hits[..] else { panic!("{hits:?}") };
    assert_eq!((hit.rule.as_str(), hit.evidence.as_ref().map(|evidence| evidence.offset)), ("marker", Some(8)));

    let huge = folder.path().join("huge.bin");
    fs::File::create(&huge).expect("huge").set_len(borg_core::CONTENTS_SIZE_LIMIT + 1).expect("size");
    assert_eq!(scan_file(&rules, &huge, &mut buffer).expect("read"), None);
}

#[test]
fn memory_is_scanned_where_the_process_filled_it_once_per_rule_and_region() {
    let (_folder, rules) = rules_with(&[("marker.yar", MARKER_RULE), ("broken.yar", "rule broken {")]);
    let own = 0x10_0000;
    let mut filled = vec![0; (CHUNK + PAGE) as usize];
    let across = (CHUNK - 4) as usize;
    filled[across..across + MARKER.len()].copy_from_slice(MARKER);
    filled[64..64 + MARKER.len()].copy_from_slice(MARKER);
    let code = 0x5000_0000;
    let untouched_start = 0x6000_0000;
    let unreadable = 0x7000_0000;
    let regions = [
        region(own, own + filled.len() as u64, "rw-", None),
        region(code, code + PAGE, "r-x", Some("/usr/lib/libmarker.so")),
        MemoryRegion {
            resident: Some(0),
            swap: Some(0),
            ..region(untouched_start, untouched_start + PAGE, "rw-", None)
        },
        region(unreadable, unreadable + PAGE, "rw-", None),
    ];
    let model = Model {
        memory: vec![(own, filled.clone()), (code, MARKER.to_vec()), (untouched_start, MARKER.to_vec())],
        reads: Default::default(),
    };
    let process = ProcessKey { pid: 42, start: 1 };
    let mut shown = Vec::new();
    let scan = scan_memory(
        process,
        &rules,
        "/usr/bin/sample",
        &regions,
        &model,
        &AtomicBool::new(false),
        |scanned, total, _| shown.push((scanned, total)),
    );
    let [hit] = &scan.hits[..] else { panic!("{:?}", scan.hits) };
    assert_eq!((hit.found.rule.as_str(), hit.address(), hit.base), ("marker", own + 64, own));
    let report = scan.report();

    let matches: Vec<_> = report.findings.iter().filter(|finding| finding.kind == FindingKind::RuleMatch).collect();
    let [found] = &matches[..] else { panic!("{:?}", report.findings) };
    assert_eq!((found.severity, found.address, found.length), (Severity::Alert, Some(own + 64), MARKER.len()));
    assert_eq!(report.findings.last().map(|finding| finding.kind), Some(FindingKind::RuleProblem));
    assert_eq!(report.total, filled.len() as u64 + 2 * PAGE, "code from a file is not the process's own");
    assert_eq!((report.scanned, report.unreadable), (report.total, PAGE));
    assert!(report.complete);
    assert_eq!(model.reads.get(), filled.len() as u64 + OVERLAP, "the untouched page is never read");
    assert!(shown.iter().all(|&(scanned, total)| scanned <= total));

    let mut second = filled;
    second[64..64 + MARKER.len()].fill(0);
    let model = Model { memory: vec![(own, second)], reads: Default::default() };
    let scan = scan_memory(process, &rules, "", &regions[..1], &model, &AtomicBool::new(false), |_, _, _| {});
    let addresses: Vec<_> = scan.hits.iter().map(Hit::address).collect();
    assert_eq!(addresses, [own + across as u64], "a match across two reads is found");
}

#[test]
fn a_cancelled_scan_keeps_what_it_found_and_says_it_stopped_short() {
    let (_folder, rules) = rules_with(&[]);
    let regions = [region(0x1000, 0x2000, "rw-", None)];
    let model = Model { memory: vec![(0x1000, vec![0; PAGE as usize])], reads: Default::default() };
    let report = scan_memory(
        ProcessKey { pid: 7, start: 1 },
        &rules,
        "",
        &regions,
        &model,
        &AtomicBool::new(true),
        |_, _, _| {},
    );
    assert!(!report.complete);
    assert_eq!((report.scanned, report.total, model.reads.get()), (0, PAGE, 0));
    assert_eq!(report.rules, rules.count());
}

#[test]
fn a_folder_is_scanned_through_every_file_below_it_hidden_ones_included() {
    let (rules_folder, rules) = rules_with(&[("marker.yar", MARKER_RULE), ("broken.yar", "rule broken {")]);
    let root = rules_folder.path().join("tree");
    fs::create_dir_all(root.join(".hidden/deep")).expect("folders");
    fs::write(root.join(".hidden/deep/payload.sh"), [&b"#!/bin/sh\n"[..], MARKER].concat()).expect("payload");
    fs::write(root.join("notes.txt"), "nothing here").expect("notes");
    fs::write(root.join("again.bin"), [MARKER, b" and more"].concat()).expect("again");
    fs::File::create(root.join("huge.bin")).expect("huge").set_len(borg_core::CONTENTS_SIZE_LIMIT + 1).expect("size");

    let scan = scan_files(&rules, &root, &AtomicBool::new(false)).expect("scanned");
    let paths: Vec<_> = scan.hits.iter().map(|hit| hit.path.strip_prefix(&root).expect("below").to_owned()).collect();
    assert_eq!(paths, [PathBuf::from(".hidden/deep/payload.sh"), "again.bin".into()], "worst first, then by path");
    assert_eq!((scan.read, scan.too_large, scan.unreadable), (3, 1, 0));
    assert_eq!((scan.rules, scan.problems.len()), (rules.count(), 1));
    assert!(scan.complete);

    let one = scan_files(&rules, &root.join("again.bin"), &AtomicBool::new(false)).expect("one file");
    assert_eq!((one.read, one.hits.len()), (1, 1));
    assert!(scan_files(&rules, &root.join("missing"), &AtomicBool::new(false)).is_err());
}

#[test]
fn a_cancelled_folder_scan_says_it_stopped_short() {
    let (folder, rules) = rules_with(&[("marker.yar", MARKER_RULE)]);
    fs::write(folder.path().join("sample.bin"), MARKER).expect("sample");
    let scan = scan_files(&rules, folder.path(), &AtomicBool::new(true)).expect("scanned");
    assert!(!scan.complete);
    assert_eq!((scan.read, scan.hits.len()), (0, 0));
}

#[test]
fn installed_rules_come_from_the_folder_and_inline_rules_run_alone() {
    let (folder, _) = rules_with(&[("marker.yar", MARKER_RULE)]);
    let installed = RuleSet::Installed { folder: Some(folder.path().to_owned()) }.compile().expect("installed");
    let inline = RuleSet::Inline(MARKER_RULE.into()).compile().expect("inline");
    assert_eq!((installed.count(), inline.count()), (compile(None).count() + 1, 1));
    assert!(RuleSet::Inline("rule {".into()).compile().is_err());
}
