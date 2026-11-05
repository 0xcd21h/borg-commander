use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::Arc,
};

use borg_drives::{DriveKind, Drives, Rates, Remember};
use borg_jobs::{Ask, Control, Decision, Event, Outcome};

use super::{
    HELLO, SECRET, bzip, deflated, gzip, index, on, quick, sample_zip, seven_zip_source, tree, xz, zip_archive,
};
use crate::{Archive, Change, Compression, Error, Format, Index, Report, Rewrite, Unpack, Watch};

const NOTES: &str = "new notes";
const INNER: &str = "inner";
const NOTES_MODE: u32 = 0o600;
const ZSTD_LEVEL: i32 = 3;
const LONG_NAME_PARTS: usize = 30;
/// A drive this slow makes any change take longer than Borg waits before asking.
const CRAWLING: Rates = Rates { read: 1, write: 1 };

fn listed(archive: Archive<'_>, watch: Watch) -> Result<Arc<Index>, Error> {
    archive.index(watch).map(Arc::new)
}

/// Changes the archive on `drives`, answering each question with the next of `answers`.
fn change_on(
    drives: &Drives,
    path: &Path,
    password: Option<&str>,
    change: Change,
    answers: &[Decision],
) -> (Outcome, Vec<Ask>) {
    let (report, asked) = report_on(drives, path, password, change, answers);
    (report.outcome, asked)
}

fn report_on(
    drives: &Drives,
    path: &Path,
    password: Option<&str>,
    change: Change,
    answers: &[Decision],
) -> (Report<PathBuf>, Vec<Ask>) {
    let control = Control::default();
    let mut answers = answers.iter();
    let mut asked = Vec::new();
    let job = Rewrite { archive: Archive { path, password, tool: None }, change: &change };
    let outcome = job.run(&listed, drives, &control, &mut |event| {
        if let Event::Asking(ask) = event {
            asked.push(ask);
            control.answer(*answers.next().unwrap_or(&Decision::Cancel));
        }
    });
    (outcome, asked)
}

fn change(path: &Path, password: Option<&str>, change: Change, answers: &[Decision]) -> (Outcome, Vec<Ask>) {
    change_on(&quick(), path, password, change, answers)
}

fn done(outcome: &Outcome) {
    assert!(matches!(outcome, Outcome::Done { .. }), "{outcome:?}");
}

/// Everything the archive holds, unpacked: each path with its content.
fn contents(path: &Path, password: Option<&str>) -> BTreeMap<String, String> {
    let archive = Archive { path, password, tool: None };
    let index = archive.index(&mut |_, _| true).unwrap();
    let target = tempfile::tempdir().unwrap();
    let everything = vec![String::new()];
    let job = Unpack { archive, paths: &everything, base: "", target: target.path() };
    done(&job.run(&index, &quick(), &Control::default(), &mut |_| {}).outcome);
    tree(target.path())
}

/// Files of this machine to add: `notes.txt` and the folder `more` with `inner.txt`.
fn local_files(dir: &Path) -> PathBuf {
    let local = dir.join("local");
    fs::create_dir_all(local.join("more")).unwrap();
    fs::write(local.join("notes.txt"), NOTES).unwrap();
    fs::set_permissions(local.join("notes.txt"), fs::Permissions::from_mode(NOTES_MODE)).unwrap();
    fs::write(local.join("more/inner.txt"), INNER).unwrap();
    local
}

fn add(local: &Path, into: &str) -> Change {
    Change::Add { sources: vec![local.join("notes.txt"), local.join("more")], into: into.into() }
}

fn paths(list: &[&str]) -> Vec<String> {
    list.iter().map(|path| path.to_string()).collect()
}

/// No new archive is left half written beside the original.
fn no_partials(dir: &Path) {
    let partials: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".borg-partial"))
        .collect();
    assert!(partials.is_empty(), "{partials:?}");
}

fn sample_tar(long_name: &str) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    let mut file = |path: &str, data: &[u8]| {
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        builder.append_data(&mut header, path, data).unwrap();
    };
    file("a.txt", b"alpha");
    file("docs/b.txt", b"beta");
    file(long_name, HELLO);
    builder.into_inner().unwrap()
}

fn long_name() -> String {
    vec!["folder"; LONG_NAME_PARTS].join("/") + "/deep.txt"
}

#[test]
fn adding_to_a_zip_keeps_what_it_held() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sample.zip");
    sample_zip(&path);
    let before = contents(&path, None);
    let local = local_files(dir.path());
    let (outcome, asked) = change(&path, None, add(&local, "docs"), &[]);
    done(&outcome);
    assert!(asked.is_empty(), "{asked:?}");
    let after = contents(&path, None);
    for (key, value) in &before {
        assert_eq!(after.get(key), Some(value), "{key}");
    }
    assert_eq!(after["docs/notes.txt"], NOTES);
    assert_eq!(after["docs/more/inner.txt"], INNER);
    assert_eq!(index(&path).item("docs/notes.txt").unwrap().mode, NOTES_MODE);
    no_partials(dir.path());
}

#[test]
fn removing_renaming_and_new_folders_change_a_zip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sample.zip");
    sample_zip(&path);
    done(&change(&path, None, Change::Remove { paths: paths(&["tools"]) }, &[]).0);
    assert!(index(&path).item("tools/run.sh").is_none());
    done(&change(&path, None, Change::Move { paths: paths(&["docs"]), to: "manual".into() }, &[]).0);
    let after = contents(&path, None);
    assert_eq!(after["manual/guide.md"].as_bytes(), HELLO);
    assert_eq!(after["latest"], "-> docs/guide.md");
    assert!(!after.contains_key("docs"), "{after:?}");
    done(&change(&path, None, Change::Folder { path: "empty".into() }, &[]).0);
    assert!(index(&path).item("empty").unwrap().is_folder());
    no_partials(dir.path());
}

#[test]
fn a_replaced_file_keeps_its_mode() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sample.zip");
    sample_zip(&path);
    let edited = dir.path().join("edited.md");
    fs::write(&edited, "edited").unwrap();
    done(&change(&path, None, Change::Replace { path: "docs/guide.md".into(), source: edited }, &[]).0);
    assert_eq!(contents(&path, None)["docs/guide.md"], "edited");
    let index = index(&path);
    assert_eq!(index.item("docs/guide.md").unwrap().mode, 0o640);
    assert_eq!(index.item("tools/run.sh").unwrap().mode, 0o755);
}

#[test]
fn names_already_in_the_archive_are_asked_about() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sample.zip");
    sample_zip(&path);
    let other = dir.path().join("guide.md");
    fs::write(&other, "other").unwrap();
    let adding = || Change::Add { sources: vec![other.clone()], into: "docs".into() };
    let bytes = fs::read(&path).unwrap();

    let (outcome, asked) = change(&path, None, adding(), &[Decision::Skip]);
    assert_eq!(outcome, Outcome::Done { done: Default::default(), skipped: 1 });
    assert!(matches!(&asked[..], [Ask::Conflict { target, .. }] if *target == path.join("docs/guide.md")), "{asked:?}");
    assert_eq!(fs::read(&path).unwrap(), bytes);

    done(&change(&path, None, adding(), &[Decision::KeepBoth]).0);
    let after = contents(&path, None);
    assert_eq!(after["docs/guide (2).md"], "other");
    assert_eq!(after["docs/guide.md"].as_bytes(), HELLO);

    done(&change(&path, None, adding(), &[Decision::Overwrite]).0);
    assert_eq!(contents(&path, None)["docs/guide.md"], "other");
    let guides = index(&path).list("docs").unwrap().filter(|item| item.name() == "guide.md").count();
    assert_eq!(guides, 1);
}

#[test]
fn sources_left_out_stay_in_the_report() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sample.zip");
    sample_zip(&path);
    let local = local_files(dir.path());
    let other = dir.path().join("guide.md");
    fs::write(&other, "other").unwrap();
    let sources = vec![other.clone(), local.join("notes.txt")];

    let (report, _) = report_on(&quick(), &path, None, Change::Add { sources, into: "docs".into() }, &[Decision::Skip]);
    assert!(matches!(report.outcome, Outcome::Done { skipped: 1, .. }), "{report:?}");
    assert_eq!(report.kept, vec![other]);
    assert_eq!(contents(&path, None)["docs/notes.txt"], NOTES);
}

#[test]
fn renames_that_cannot_work_fail_before_writing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sample.zip");
    sample_zip(&path);
    let bytes = fs::read(&path).unwrap();
    for to in ["docs/inner", "latest", "../outside"] {
        let (outcome, _) = change(&path, None, Change::Move { paths: paths(&["docs"]), to: to.into() }, &[]);
        assert!(matches!(outcome, Outcome::Failed(_)), "{to}: {outcome:?}");
    }
    assert_eq!(fs::read(&path).unwrap(), bytes);
    no_partials(dir.path());
}

#[test]
fn an_encrypted_zip_takes_new_entries_with_its_password() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("locked.zip");
    zip_archive(&path, |zip| {
        zip.start_file("secret.txt", deflated().with_aes_encryption(zip::AesMode::Aes256, SECRET)).unwrap();
        std::io::Write::write_all(zip, HELLO).unwrap();
    });
    let local = local_files(dir.path());
    let bytes = fs::read(&path).unwrap();
    for password in [None, Some("wrong")] {
        let (outcome, _) = change(&path, password, add(&local, ""), &[]);
        assert!(matches!(outcome, Outcome::Failed(_)), "{password:?}: {outcome:?}");
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
    done(&change(&path, Some(SECRET), add(&local, ""), &[]).0);
    assert!(index(&path).item("notes.txt").unwrap().encrypted);
    let after = contents(&path, Some(SECRET));
    assert_eq!(after["secret.txt"].as_bytes(), HELLO);
    assert_eq!(after["notes.txt"], NOTES);
}

#[test]
fn a_7z_keeps_untouched_blocks_and_packs_the_rest_again() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sample.7z");
    sevenz_rust2::compress_to_path(seven_zip_source(dir.path()), &path).unwrap();
    let local = local_files(dir.path());
    done(&change(&path, None, add(&local, "docs"), &[]).0);
    done(&change(&path, None, Change::Remove { paths: paths(&["top.txt"]) }, &[]).0);
    done(&change(&path, None, Change::Move { paths: paths(&["docs"]), to: "manual".into() }, &[]).0);
    let after = contents(&path, None);
    assert_eq!(after["manual/guide.md"].as_bytes(), HELLO);
    assert_eq!(after["manual/notes.txt"], NOTES);
    assert_eq!(after["manual/more/inner.txt"], INNER);
    assert!(!after.contains_key("top.txt"), "{after:?}");
    assert_eq!(index(&path).format, Format::SevenZip);
    no_partials(dir.path());
}

#[test]
fn an_encrypted_7z_stays_encrypted() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("locked.7z");
    sevenz_rust2::compress_to_path_encrypted(seven_zip_source(dir.path()), &path, SECRET.into()).unwrap();
    let local = local_files(dir.path());
    done(&change(&path, Some(SECRET), add(&local, ""), &[]).0);
    let archive = Archive { path: &path, password: Some(SECRET), tool: None };
    let index = archive.index(&mut |_, _| true).unwrap();
    assert!(index.item("notes.txt").unwrap().encrypted);
    let after = contents(&path, Some(SECRET));
    assert_eq!(after["notes.txt"], NOTES);
    assert_eq!(after["docs/guide.md"].as_bytes(), HELLO);
}

#[test]
fn a_plain_tar_gains_entries_and_keeps_long_names() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sample.tar");
    let long = long_name();
    fs::write(&path, sample_tar(&long)).unwrap();
    let local = local_files(dir.path());
    done(&change(&path, None, add(&local, "docs"), &[]).0);
    done(&change(&path, None, Change::Remove { paths: paths(&["a.txt"]) }, &[]).0);
    let after = contents(&path, None);
    assert_eq!(after[&long].as_bytes(), HELLO);
    assert_eq!(after["docs/b.txt"], "beta");
    assert_eq!(after["docs/notes.txt"], NOTES);
    assert!(!after.contains_key("a.txt"), "{after:?}");
}

#[test]
fn compressed_tars_are_written_back_with_their_compression() {
    let dir = tempfile::tempdir().unwrap();
    let long = long_name();
    let plain = sample_tar(&long);
    let zstd = |data: &[u8]| zstd::encode_all(data, ZSTD_LEVEL).unwrap();
    let packed: [(&str, Compression, Vec<u8>); 4] = [
        ("sample.tar.gz", Compression::Gzip, gzip(&plain)),
        ("sample.tar.bz2", Compression::Bzip2, bzip(&plain)),
        ("sample.tar.xz", Compression::Xz, xz(&plain)),
        ("sample.tar.zst", Compression::Zstd, zstd(&plain)),
    ];
    for (name, compression, bytes) in packed {
        let path = dir.path().join(name);
        fs::write(&path, bytes).unwrap();
        done(&change(&path, None, Change::Move { paths: paths(&["a.txt"]), to: "renamed.txt".into() }, &[]).0);
        assert_eq!(index(&path).format, Format::Tar(Some(compression)), "{name}");
        let after = contents(&path, None);
        assert_eq!(after["renamed.txt"], "alpha", "{name}");
        assert_eq!(after[&long].as_bytes(), HELLO, "{name}");
    }
    no_partials(dir.path());
}

#[test]
fn entries_borg_leaves_out_stay_in_the_archive() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("odd.tar");
    let entries: [(&str, tar::EntryType, &[u8], &str); 4] = [
        ("../evil", tar::EntryType::Regular, b"x", ""),
        ("keep.txt", tar::EntryType::Regular, b"keep", ""),
        ("drop.txt", tar::EntryType::Regular, b"drop", ""),
        ("device", tar::EntryType::Char, b"", ""),
    ];
    fs::write(&path, super::raw_tar(&entries)).unwrap();
    assert_eq!(index(&path).left_out, 2);
    done(&change(&path, None, Change::Remove { paths: paths(&["drop.txt"]) }, &[]).0);
    let mut archive = tar::Archive::new(fs::File::open(&path).unwrap());
    let names: Vec<String> = archive
        .entries()
        .unwrap()
        .map(|entry| String::from_utf8_lossy(&entry.unwrap().path_bytes()).into_owned())
        .collect();
    assert_eq!(names, ["../evil", "keep.txt", "device"]);
}

#[test]
fn a_change_that_would_take_long_asks_first() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sample.zip");
    sample_zip(&path);
    let bytes = fs::read(&path).unwrap();
    let slow = on(DriveKind::HardDrive, CRAWLING);
    let remove = |path: &str| Change::Remove { paths: paths(&[path]) };

    let (outcome, asked) = change_on(&slow, &path, None, remove("tools"), &[Decision::Cancel]);
    assert!(matches!(outcome, Outcome::Cancelled { .. }), "{outcome:?}");
    let slow_question =
        |ask: &Ask| matches!(ask, Ask::Slow { path: on, file: true, kind: DriveKind::HardDrive, .. } if *on == path);
    assert!(matches!(&asked[..], [ask] if slow_question(ask)), "{asked:?}");
    assert_eq!(fs::read(&path).unwrap(), bytes);
    no_partials(dir.path());

    let (outcome, _) = change_on(&slow, &path, None, remove("tools"), &[Decision::Always(Remember::File)]);
    done(&outcome);
    let (outcome, asked) = change_on(&slow, &path, None, remove("docs"), &[]);
    done(&outcome);
    assert!(asked.is_empty(), "{asked:?}");
}

#[test]
fn unpacking_that_would_take_long_asks_first() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sample.zip");
    sample_zip(&path);
    let index = index(&path);
    let target = dir.path().join("out");
    fs::create_dir(&target).unwrap();
    let everything = vec![String::new()];
    let job = Unpack {
        archive: Archive { path: &path, password: None, tool: None },
        paths: &everything,
        base: "",
        target: &target,
    };
    let control = Control::default();
    let mut asked = Vec::new();
    let outcome = job.run(&index, &on(DriveKind::Remote, CRAWLING), &control, &mut |event| {
        if let Event::Asking(ask) = event {
            asked.push(ask);
            control.answer(Decision::Cancel);
        }
    });
    assert!(matches!(outcome.outcome, Outcome::Cancelled { .. }), "{outcome:?}");
    assert!(matches!(&asked[..], [Ask::Slow { kind: DriveKind::Remote, .. }]), "{asked:?}");
    assert!(tree(&target).is_empty());
}

#[test]
fn single_compressed_files_stay_read_only() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("notes.txt.gz");
    fs::write(&path, gzip(HELLO)).unwrap();
    let (outcome, _) = change(&path, None, Change::Remove { paths: paths(&["notes.txt"]) }, &[]);
    assert_eq!(outcome, Outcome::Failed(borg_i18n::tr!(archive_read_only, format = "gz")));
    assert!(!crate::writable(Format::Rar));
    assert!(crate::writable(Format::Tar(Some(Compression::Zstd))));
}

#[test]
fn moves_go_into_a_folder_the_archive_holds() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sample.zip");
    sample_zip(&path);
    done(&change(&path, None, Change::Move { paths: paths(&["tools/run.sh", "latest"]), to: "docs".into() }, &[]).0);
    done(&change(&path, None, Change::Move { paths: paths(&["docs/guide.md"]), to: "readme.md".into() }, &[]).0);
    let after = contents(&path, None);
    assert_eq!(after["docs/run.sh"], "#!/bin/sh\n");
    assert_eq!(after["docs/latest"], "-> docs/guide.md");
    assert_eq!(after["readme.md"].as_bytes(), HELLO);
    let local = local_files(dir.path());
    let (outcome, _) = change(&path, None, add(&local, "../up"), &[]);
    assert!(matches!(outcome, Outcome::Failed(_)), "{outcome:?}");
}

const FIRST_SAVE: &str = "alpha, first save";
const LONGER_SAVE: &str = "alpha, second save with a fixed typo and a longer line";
const SHORTER_SAVE: &str = "alpha";
/// A save longer than one tar block, which used to spill into where the next header goes.
const SAVE_PAST_A_BLOCK: usize = 2000;
const GROWTH: usize = 700;

/// Changes the archive on a drive slow enough that Borg asks first, runs `meanwhile` while the
/// question waits, and then goes ahead.
fn change_while_asked(path: &Path, change: Change, meanwhile: impl FnOnce()) -> (Report<PathBuf>, Vec<Ask>) {
    let control = Control::default();
    let mut meanwhile = Some(meanwhile);
    let mut asked = Vec::new();
    let job = Rewrite { archive: Archive { path, password: None, tool: None }, change: &change };
    let report = job.run(&listed, &on(DriveKind::HardDrive, CRAWLING), &control, &mut |event| {
        if let Event::Asking(ask) = event {
            asked.push(ask);
            if let Some(meanwhile) = meanwhile.take() {
                meanwhile();
            }
            control.answer(Decision::Proceed);
        }
    });
    (report, asked)
}

fn slow_question(asked: &[Ask]) -> bool {
    matches!(asked, [Ask::Slow { .. }])
}

#[test]
fn a_file_saved_again_while_a_tar_change_waits_goes_in_as_it_is_then() {
    let plain = sample_tar(&long_name());
    let past_a_block = "y".repeat(SAVE_PAST_A_BLOCK);
    for (name, bytes) in [("backup.tar", plain.clone()), ("backup.tar.gz", gzip(&plain))] {
        for second in [LONGER_SAVE, SHORTER_SAVE, past_a_block.as_str()] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join(name);
            fs::write(&path, &bytes).unwrap();
            let edit = dir.path().join("a.txt");
            fs::write(&edit, FIRST_SAVE).unwrap();
            let replace = Change::Replace { path: "a.txt".into(), source: edit.clone() };
            let (report, asked) = change_while_asked(&path, replace, || fs::write(&edit, second).unwrap());
            done(&report.outcome);
            assert!(slow_question(&asked), "{asked:?}");
            let after = contents(&path, None);
            assert_eq!(after["a.txt"], second, "{name}");
            assert_eq!(after["docs/b.txt"], "beta", "{name}");
            assert_eq!(index(&path).item("a.txt").map(|item| item.size), Some(second.len() as u64), "{name}");
            no_partials(dir.path());
        }
    }
}

#[test]
fn a_file_that_grows_while_a_plain_tar_waits_goes_in_whole() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("plain.tar");
    fs::write(&path, sample_tar(&long_name())).unwrap();
    let log = dir.path().join("log.txt");
    fs::write(&log, NOTES).unwrap();
    let grown = format!("{NOTES}{}", "x".repeat(GROWTH));
    let adding = Change::Add { sources: vec![log.clone()], into: String::new() };
    let (report, asked) = change_while_asked(&path, adding, || fs::write(&log, &grown).unwrap());
    done(&report.outcome);
    assert!(slow_question(&asked), "{asked:?}");
    let after = contents(&path, None);
    assert_eq!(after["log.txt"], grown);
    assert_eq!(after["a.txt"], "alpha");
}

#[test]
fn an_empty_file_saved_again_while_a_7z_change_waits_goes_in_as_it_is_then() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sample.7z");
    sevenz_rust2::compress_to_path(seven_zip_source(dir.path()), &path).unwrap();
    let edit = dir.path().join("top.txt");
    fs::write(&edit, "").unwrap();
    let replace = Change::Replace { path: "top.txt".into(), source: edit.clone() };
    let (report, asked) = change_while_asked(&path, replace, || fs::write(&edit, LONGER_SAVE).unwrap());
    done(&report.outcome);
    assert!(slow_question(&asked), "{asked:?}");
    assert_eq!(contents(&path, None)["top.txt"], LONGER_SAVE);
}

/// `a/config.json` and `b/config.json`, as a list of search results gives them.
fn two_of_one_name(dir: &Path) -> Vec<PathBuf> {
    ["a", "b"]
        .iter()
        .map(|folder| {
            let source = dir.join(folder).join("config.json");
            fs::create_dir_all(source.parent().unwrap()).unwrap();
            fs::write(&source, folder).unwrap();
            source
        })
        .collect()
}

fn small_archive(path: &Path) {
    if path.extension().is_some_and(|ending| ending == "zip") {
        sample_zip(path);
    } else {
        fs::write(path, sample_tar(&long_name())).unwrap();
    }
}

fn named(path: &Path, name: &str) -> usize {
    let index = index(path);
    index.entries().filter(|item| item.path == name).count() + index.shadowed().len()
}

#[test]
fn two_sources_of_one_name_are_asked_about_like_a_copy_into_a_folder() {
    for name in ["evidence.zip", "evidence.tar"] {
        for decision in [Decision::Skip, Decision::KeepBoth, Decision::Overwrite] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join(name);
            small_archive(&path);
            let sources = two_of_one_name(dir.path());
            let adding = Change::Add { sources: sources.clone(), into: String::new() };
            let (report, asked) = report_on(&quick(), &path, None, adding, &[decision]);
            let conflict = |ask: &Ask| matches!(ask, Ask::Conflict { source, target, .. } if *source == sources[1] && *target == path.join("config.json"));
            assert!(matches!(&asked[..], [ask] if conflict(ask)), "{name} {decision:?}: {asked:?}");
            done(&report.outcome);
            let after = contents(&path, None);
            assert_eq!(named(&path, "config.json"), 1, "{name} {decision:?}");
            match decision {
                Decision::Skip => {
                    assert_eq!(after["config.json"], "a", "{name}");
                    assert_eq!(report.kept, vec![sources[1].clone()], "{name}");
                }
                Decision::KeepBoth => {
                    assert_eq!(after["config.json"], "a", "{name}");
                    assert_eq!(after["config (2).json"], "b", "{name}");
                    assert!(report.kept.is_empty(), "{name}: {:?}", report.kept);
                }
                _ => {
                    assert_eq!(after["config.json"], "b", "{name}");
                    assert_eq!(report.kept, vec![sources[0].clone()], "{name}");
                }
            }
        }
    }
}

#[test]
fn two_folders_of_one_name_merge_and_ask_about_the_files_they_share() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("evidence.zip");
    sample_zip(&path);
    for (folder, file) in [("x", "one.txt"), ("y", "two.txt"), ("y", "one.txt")] {
        let source = dir.path().join(folder).join("logs").join(file);
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        fs::write(&source, folder).unwrap();
    }
    let sources = vec![dir.path().join("x/logs"), dir.path().join("y/logs")];
    let (report, asked) =
        report_on(&quick(), &path, None, Change::Add { sources, into: String::new() }, &[Decision::Skip]);
    assert!(matches!(&asked[..], [Ask::Conflict { target, .. }] if *target == path.join("logs/one.txt")), "{asked:?}");
    done(&report.outcome);
    let after = contents(&path, None);
    assert_eq!(after["logs/one.txt"], "x");
    assert_eq!(after["logs/two.txt"], "y");
    assert_eq!(report.kept, vec![dir.path().join("y/logs")]);
}
