use std::{fs, io::Write, os::unix::fs::PermissionsExt};

use borg_core::{ArchiveMount, Password, UnpackTarget};
use borg_jobs::Decision;
use tokio::sync::mpsc::unbounded_channel;
use zip::{AesMode, ZipWriter, write::SimpleFileOptions};

use super::*;

const GUIDE: &[u8] = b"archive guide\n";
const SECRET: &str = "open sesame";

fn site_zip(path: &Path, password: Option<&str>) {
    let mut zip = ZipWriter::new(fs::File::create(path).unwrap());
    let plain = SimpleFileOptions::default();
    let options = password.map_or(plain, |password| plain.with_aes_encryption(AesMode::Aes256, password));
    zip.add_directory("docs/", plain).unwrap();
    zip.start_file("docs/guide.md", options).unwrap();
    zip.write_all(GUIDE).unwrap();
    zip.start_file("readme.md", options).unwrap();
    zip.write_all(b"read me\n").unwrap();
    zip.finish().unwrap();
}

fn mount(archive: &Path, password: Option<&str>) -> ArchiveMount {
    ArchiveMount {
        archive: archive.to_path_buf(),
        host: archive.parent().unwrap().to_path_buf(),
        password: password.map(Password::new),
        verified: false,
        reading: None,
        format: None,
    }
}

/// What listing `dir` of the archive sends the panel.
fn listed(indexes: &Indexes, archive: &Path, dir: &Path, password: Option<&str>) -> Vec<Msg> {
    let (messages, mut inbox) = unbounded_channel();
    let listing = Listing {
        side: Side::Left,
        generation: 1,
        dir: dir.to_path_buf(),
        mount: mount(archive, password),
        messages,
        stopped: Arc::new(AtomicBool::new(false)),
    };
    listing.run(indexes);
    let mut sent = Vec::new();
    while let Ok(msg) = inbox.try_recv() {
        sent.push(msg);
    }
    sent
}

#[test]
fn a_listing_fills_the_panel_with_folders_and_their_sizes() {
    let dir = tempfile::tempdir().unwrap();
    let archive = dir.path().join("site.zip");
    site_zip(&archive, None);
    let indexes = Indexes::default();

    let sent = listed(&indexes, &archive, &archive, None);
    let [Msg::ArchiveOpened { left_out: 0, .. }, Msg::Batch { entries, .. }, Msg::Listed { error: None, .. }] =
        &sent[..]
    else {
        panic!("opened, one batch, listed: {sent:?}")
    };
    let docs = entries.iter().find(|entry| entry.name == "docs").unwrap();
    assert_eq!(docs.kind, EntryKind::Dir);
    assert_eq!(docs.size, GUIDE.len() as u64);
    assert_eq!(docs.usage.as_ref().map(|usage| usage.files), Some(1));
    assert!(entries.iter().any(|entry| entry.name == "readme.md" && entry.kind == EntryKind::File));

    let sent = listed(&indexes, &archive, &archive.join("docs"), None);
    assert!(matches!(&sent[1], Msg::Batch { entries, .. } if entries.len() == 1 && entries[0].name == "guide.md"));

    let sent = listed(&indexes, &archive, &archive.join("missing"), None);
    assert!(matches!(sent.last(), Some(Msg::Listed { error: Some(error), .. }) if error.contains("missing")));
}

#[test]
fn an_encrypted_archive_lists_once_the_password_opens_it() {
    let dir = tempfile::tempdir().unwrap();
    let archive = dir.path().join("site.zip");
    site_zip(&archive, Some(SECRET));
    let indexes = Indexes::default();

    let locked = |password| listed(&indexes, &archive, &archive, password);
    assert!(matches!(&locked(None)[..], [Msg::ArchiveLocked { wrong: false, .. }]));
    assert!(matches!(&locked(Some("guess"))[..], [Msg::ArchiveLocked { wrong: true, .. }]));
    assert!(matches!(locked(Some(SECRET)).last(), Some(Msg::Listed { error: None, .. })));
}

#[test]
fn a_file_unpacked_for_the_viewer_goes_away_with_it() {
    let dir = tempfile::tempdir().unwrap();
    let archive = dir.path().join("site.zip");
    site_zip(&archive, None);
    let scratch = dir.path().join("archive-view");
    let request = UnpackRequest {
        archive: archive.clone(),
        password: None,
        paths: vec!["docs/guide.md".into()],
        base: "docs".into(),
        target: UnpackTarget::Viewer { shown: archive.join("docs/guide.md") },
        remove: false,
    };
    let control = Control::default();
    let (outcome, unpacked) =
        unpack_for_viewer(&Indexes::default(), &request, &scratch, &Drives::default(), &control, &mut |_| {});
    assert!(matches!(outcome, Outcome::Done { .. }), "{outcome:?}");
    let unpacked = unpacked.expect("the file is unpacked");
    assert_eq!(fs::read(&unpacked.path).unwrap(), GUIDE);
    assert_eq!(fs::metadata(&scratch).unwrap().permissions().mode() & 0o777, PRIVATE_FOLDER);
    drop(unpacked);
    assert_eq!(fs::read_dir(&scratch).unwrap().count(), 0);
}

#[test]
fn extracting_an_encrypted_archive_says_how_to_give_the_password() {
    let dir = tempfile::tempdir().unwrap();
    let archive = dir.path().join("site.zip");
    site_zip(&archive, Some(SECRET));
    let target = dir.path().join("out");
    fs::create_dir(&target).unwrap();
    let request = UnpackRequest {
        archive,
        password: None,
        paths: vec![String::new()],
        base: String::new(),
        target: UnpackTarget::Folder(target.clone()),
        remove: false,
    };
    let outcome =
        unpack(&Indexes::default(), &request, &target, &Drives::default(), &Control::default(), &mut |_| {}).outcome;
    assert_eq!(outcome, Outcome::Failed(tr!(archive_extract_password).into()));
    assert_eq!(fs::read_dir(&target).unwrap().count(), 0);
}

/// Answers every question of a job with `decision`.
fn answering(control: &Control, decision: Decision) -> impl FnMut(Event) + '_ {
    move |event| {
        if let Event::Asking(_) = event {
            control.answer(decision);
        }
    }
}

fn entries_of(archive: &Path) -> Index {
    Archive { path: archive, password: None, tool: None }.index(&mut |_, _| true).unwrap()
}

#[test]
fn moving_out_takes_from_the_archive_only_what_unpacked_whole() {
    let dir = tempfile::tempdir().unwrap();
    let archive = dir.path().join("site.zip");
    site_zip(&archive, None);
    let target = dir.path().join("out");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("readme.md"), "mine").unwrap();
    let request = UnpackRequest {
        archive: archive.clone(),
        password: None,
        paths: vec!["docs".into(), "readme.md".into()],
        base: String::new(),
        target: UnpackTarget::Folder(target.clone()),
        remove: true,
    };
    let control = Control::default();
    let outcome = move_out(
        &Indexes::default(),
        &request,
        &target,
        &Drives::default(),
        &control,
        &mut answering(&control, Decision::Skip),
    );
    assert!(matches!(outcome, Outcome::Done { skipped: 1, .. }), "{outcome:?}");
    assert_eq!(fs::read(target.join("docs/guide.md")).unwrap(), GUIDE);
    assert_eq!(fs::read_to_string(target.join("readme.md")).unwrap(), "mine");
    let left = entries_of(&archive);
    assert!(left.item("docs").is_none() && left.item("docs/guide.md").is_none());
    assert!(left.item("readme.md").is_some(), "a skipped entry stays in the archive");
}

#[test]
fn moving_in_deletes_each_original_once_it_is_in_whole() {
    let dir = tempfile::tempdir().unwrap();
    let archive = dir.path().join("site.zip");
    site_zip(&archive, None);
    let (guide, notes) = (dir.path().join("guide.md"), dir.path().join("notes.txt"));
    fs::write(&guide, "other guide").unwrap();
    fs::write(&notes, "notes").unwrap();
    let sources = vec![guide.clone(), notes.clone()];
    let request = RewriteRequest {
        archive: archive.clone(),
        password: None,
        change: Change::Add { sources: sources.clone(), into: "docs".into() },
        originals: sources,
    };
    let control = Control::default();
    let outcome =
        rewrite(&Indexes::default(), &request, &Drives::default(), &control, &mut answering(&control, Decision::Skip));
    assert!(matches!(outcome, Outcome::Done { skipped: 1, .. }), "{outcome:?}");
    assert!(guide.exists(), "a file the archive did not take stays");
    assert!(!notes.exists());
    assert!(entries_of(&archive).item("docs/notes.txt").is_some());
}

#[test]
fn a_file_unpacked_for_the_editor_stays_while_it_is_open() {
    let dir = tempfile::tempdir().unwrap();
    let archive = dir.path().join("site.zip");
    site_zip(&archive, None);
    let scratch = dir.path().join("archive-view");
    let request = UnpackRequest {
        archive: archive.clone(),
        password: None,
        paths: vec!["docs/guide.md".into()],
        base: "docs".into(),
        target: UnpackTarget::Editor { shown: archive.join("docs/guide.md"), slot: borg_core::EditorSlot::Screen },
        remove: false,
    };
    let control = Control::default();
    let (outcome, unpacked) =
        unpack_for_editor(&Indexes::default(), &request, &scratch, &Drives::default(), &control, &mut |_| {});
    assert!(matches!(outcome, Outcome::Done { .. }), "{outcome:?}");
    let unpacked = unpacked.expect("the file is unpacked");
    assert!(unpacked.starts_with(&scratch));
    assert_eq!(fs::read(&unpacked).unwrap(), GUIDE);
}

#[test]
fn a_save_the_archive_does_not_take_says_where_the_edit_stays() {
    let dir = tempfile::tempdir().unwrap();
    let archive = dir.path().join("site.zip");
    site_zip(&archive, None);
    let edited = dir.path().join("guide.md");
    fs::write(&edited, "edited").unwrap();
    let request = RewriteRequest {
        archive,
        password: None,
        change: Change::Replace { path: "gone.md".into(), source: edited.clone() },
        originals: Vec::new(),
    };
    let outcome = rewrite(&Indexes::default(), &request, &Drives::default(), &Control::default(), &mut |_| {});
    let missing = tr!(archive_missing_entry, path = "gone.md");
    let kept = tr!(archive_edit_kept, error = missing, path = edited.display());
    assert_eq!(outcome, Outcome::Failed(kept));
}
