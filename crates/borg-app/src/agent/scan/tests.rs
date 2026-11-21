use super::*;
use crate::rule_scan::FileHit;
use borg_rules::{Evidence, INLINE, Level};
use serde_json::json;
use std::path::PathBuf;

fn marker(level: Level, evidence: Option<&[u8]>) -> RuleMatch {
    RuleMatch {
        rule: "marker".into(),
        namespace: INLINE.into(),
        level,
        description: Some("A marker".into()),
        false_positives: Some("Notes about markers".into()),
        evidence: evidence.map(|bytes| Evidence {
            string: "$a".into(),
            offset: 8,
            length: bytes.len() as u64 + 100,
            bytes: bytes.to_vec(),
        }),
    }
}

fn region(start: u64, name: &str, permissions: &str) -> MemoryRegion {
    MemoryRegion {
        start,
        end: start + 0x1000,
        permissions: permissions.into(),
        name: name.into(),
        ..Default::default()
    }
}

#[test]
fn a_scan_call_takes_a_location_or_a_process_and_defaults_to_what_fits_each() {
    let folder = scan_call(&json!({ "path": " ~/work " })).unwrap();
    assert_eq!(
        folder,
        ScanCall {
            target: ScanTarget::Location("~/work".into()),
            checks: ScanChecks::Rules,
            rules: None,
            limit: SHOWN,
            admin: false
        }
    );
    let process = scan_call(&json!({ "pid": 4100, "rules": "rule a { condition: true }", "admin": true })).unwrap();
    assert_eq!((process.target, process.checks, process.admin), (ScanTarget::Process(4100), ScanChecks::Both, true));
    assert_eq!(process.rules.as_deref(), Some("rule a { condition: true }"));
    assert_eq!(scan_call(&json!({ "pid": 1, "check": "integrity" })).unwrap().checks, ScanChecks::Integrity);
    assert_eq!(scan_call(&json!({ "pid": 1, "limit": 1_000_000 })).unwrap().limit, FINDING_LIMIT);
    assert_eq!(scan_call(&json!({ "path": ".", "limit": 0 })).unwrap().limit, 1);
}

#[test]
fn a_scan_call_says_what_is_wrong_with_it() {
    let long = "x".repeat(INLINE_LIMIT + 1);
    for (args, says) in [
        (json!({}), "needs a path"),
        (json!({ "path": "  " }), "needs a path"),
        (json!({ "path": ".", "pid": 1 }), "either a path or a pid"),
        (json!({ "pid": 0 }), "PID of a running process"),
        (json!({ "pid": "1" }), "PID of a running process"),
        (json!({ "pid": 1, "check": "memory" }), "no check \"memory\""),
        (json!({ "path": ".", "check": "both" }), "integrity checks a running process"),
        (json!({ "pid": 1, "check": "integrity", "rules": "rule a { condition: true }" }), "only with check"),
        (json!({ "path": ".", "rules": 7 }), "YARA rule text"),
        (json!({ "path": ".", "rules": long }), "longer than 64 KiB"),
        (json!({ "path": ".", "admin": true }), "needs a pid"),
    ] {
        let refused = scan_call(&args).unwrap_err();
        assert!(refused.contains(says), "{args}: {refused}");
    }
}

#[test]
fn evidence_is_quoted_text_when_it_reads_as_text_and_hex_otherwise() {
    assert_eq!(evidence_text(b"bash -i >& /dev/tcp/"), "\"bash -i >& /dev/tcp/\"");
    assert_eq!(evidence_text(b"a\tb\n\"c\""), r#""a\tb\n\"c\"""#);
    assert_eq!(evidence_text(&[0x7F, b'E', b'L', b'F', 0]), "7f 45 4c 46 00");
    assert_eq!(evidence_text(&[0xFF, b'a']), "ff 61");
}

#[test]
fn a_files_answer_lists_each_match_with_its_file_offset_and_evidence_worst_first() {
    let scan = FileScan {
        hits: vec![
            FileHit { path: "/work/run.sh".into(), found: marker(Level::Alert, Some(b"borg-marker")) },
            FileHit { path: "/work/blob.bin".into(), found: marker(Level::Notice, Some(&[0x48, 0xBB, 0x2F])) },
            FileHit { path: "/work/empty".into(), found: marker(Level::Notice, None) },
        ],
        rules: 1,
        problems: vec![Problem { file: "/rules/bad.yar".into(), message: "bad.yar:1:7: error: syntax".into() }],
        read: 12,
        too_large: 2,
        unreadable: 1,
        complete: false,
    };
    let reply = files_answer(&scan, &PathBuf::from("/work"), 2);
    let text = reply.text;
    assert!(reply.ok);
    assert!(
        text.starts_with(
            "Rules over /work · 1 rule · 12 files read · 2 too large to read · 1 unreadable · stopped before the end\n3 matches, worst first; showing 2:\n"
        ),
        "{text}"
    );
    assert!(text.contains("1. alert · rule_match · rule marker from inline\n   file: /work/run.sh\n   at: offset 8, 111 bytes\n   evidence $a (first bytes): \"borg-marker\"\n   looks for: A marker\n   could also be: Notes about markers\n"), "{text}");
    assert!(text.contains("2. notice · rule_match · rule marker from inline\n   file: /work/blob.bin\n"), "{text}");
    assert!(text.contains("evidence $a (first bytes): 48 bb 2f\n"), "{text}");
    assert!(!text.contains("/work/empty"), "the limit leaves out the rest: {text}");
    assert!(
        text.ends_with(
            "Rule files left out because they do not compile:\n  /rules/bad.yar: bad.yar:1:7: error: syntax\n"
        ),
        "{text}"
    );

    let quiet = files_answer(&FileScan { complete: true, ..Default::default() }, &PathBuf::from("/work"), 10).text;
    assert!(quiet.contains("· finished\nNo rule matched."), "{quiet}");
    let alone = FileScan {
        hits: vec![FileHit { path: "/work/empty".into(), found: marker(Level::Notice, None) }],
        ..Default::default()
    };
    assert!(
        files_answer(&alone, &PathBuf::from("/work"), 10)
            .text
            .contains("   at: the rule matched on its condition alone\n")
    );
}

#[test]
fn a_process_answer_merges_both_checks_worst_first_and_points_to_admin_when_memory_was_unreadable() {
    let process = borg_core::ProcessKey { pid: 4100, start: 1 };
    let changed = Finding {
        kind: FindingKind::CodeChanged,
        severity: Severity::Warning,
        detail: "2 of 40 pages differ from /usr/lib/libc.so".into(),
        region: Some(region(0x7000, "/usr/lib/libc.so", "r-x")),
        address: Some(0x7010),
        length: 8,
        note: None,
    };
    let preload = Finding {
        kind: FindingKind::Preload,
        severity: Severity::Notice,
        detail: "LD_PRELOAD=/tmp/x.so".into(),
        region: None,
        address: None,
        length: 0,
        note: None,
    };
    let integrity = IntegrityReport {
        process,
        findings: vec![changed, preload],
        compared: 4096,
        uncompared: 0,
        scanned: 0,
        unbacked: 0,
        unreadable: 4096,
        complete: true,
    };
    let rules = MemoryScan {
        process,
        hits: vec![Hit {
            found: marker(Level::Alert, Some(b"borg-marker")),
            region: region(0x10000, "heap", "rw-"),
            base: 0x10000,
        }],
        rules: 1,
        problems: Vec::new(),
        scanned: 4096,
        total: 8192,
        unreadable: 0,
        complete: false,
    };
    let scan = ProcessScan { integrity: Some(integrity), rules: Some(rules) };
    let text = process_answer(&scan, "sample", 4100, 10, false).text;
    let headings: Vec<_> =
        text.lines().filter(|line| line.split_once(". ").is_some_and(|(n, _)| n.parse::<usize>().is_ok())).collect();
    assert_eq!(
        headings,
        ["1. alert · rule_match · rule marker from inline", "2. warning · code_changed", "3. notice · preload"],
        "{text}"
    );
    let rules_line = format!(
        "\nRules: 1 rule · {} of {} the process filled scanned · {} unreadable · stopped before the end\n",
        size_text(4096),
        size_text(8192),
        size_text(0)
    );
    assert!(text.starts_with(&format!("sample (PID 4100)\nIntegrity: {} of code compared", size_text(4096))), "{text}");
    assert!(text.contains(&rules_line), "{text}");
    assert!(text.contains("   in: heap rw- 0x10000-0x11000\n   at: 0x10008, 111 bytes\n"), "{text}");
    assert!(
        text.contains("   at: 0x7010, 8 bytes, in /usr/lib/libc.so r-x 0x7000-0x8000\n   detail: 2 of 40 pages differ"),
        "{text}"
    );
    assert!(text.contains("3. notice · preload\n   detail: LD_PRELOAD=/tmp/x.so\n   could also be: "), "{text}");
    assert!(text.ends_with("admin true reads it as administrator.\n"), "{text}");
    assert!(!process_answer(&scan, "sample", 4100, 10, true).text.contains("admin true"));

    let nothing = ProcessScan { integrity: None, rules: None };
    assert!(process_answer(&nothing, "sample", 4100, 10, false).text.contains("Nothing found."));
}
