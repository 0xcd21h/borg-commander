use std::{
    collections::BTreeMap,
    fs,
    io::{Cursor, Write},
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
};

use borg_drives::{Drive, DriveKind, Drives, Rates};
use borg_jobs::{Ask, Control, Decision, Event, Outcome};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

use super::*;
use crate::{format::sniff, index::Item as RawItem, name::clean};

mod write;

const HELLO: &[u8] = b"hello archive\n";
const SECRET: &str = "open sesame";

fn index(path: &Path) -> Index {
    Archive { path, password: None, tool: None }.index(&mut |_, _| true).unwrap()
}

/// Drives of one kind at the given speeds.
fn on(kind: DriveKind, rates: Rates) -> Drives {
    Drives::uniform(Drive { id: "test".into(), kind, filesystem: "test".into(), clones: false }, rates)
}

/// Drives fast enough that no job asks how long it takes.
fn quick() -> Drives {
    on(DriveKind::Memory, DriveKind::Memory.typical())
}

/// What a folder holds: each path below it with its content, `/` for a folder, `-> target` for a link.
fn tree(root: &Path) -> BTreeMap<String, String> {
    let mut found = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            let key = path.strip_prefix(root).unwrap().to_string_lossy().into_owned();
            let meta = fs::symlink_metadata(&path).unwrap();
            let value = if meta.is_symlink() {
                format!("-> {}", fs::read_link(&path).unwrap().display())
            } else if meta.is_dir() {
                pending.push(path);
                "/".into()
            } else {
                String::from_utf8_lossy(&fs::read(&path).unwrap()).into_owned()
            };
            found.insert(key, value);
        }
    }
    found
}

/// Unpacks `paths` of the archive, answering each question with the next of `answers`.
fn unpack(
    archive: &Path,
    index: &Index,
    paths: &[&str],
    base: &str,
    target: &Path,
    answers: &[Decision],
) -> (Outcome, Vec<Ask>) {
    let (report, asked) = unpack_report(archive, index, paths, base, target, answers);
    (report.outcome, asked)
}

fn unpack_report(
    archive: &Path,
    index: &Index,
    paths: &[&str],
    base: &str,
    target: &Path,
    answers: &[Decision],
) -> (Report<String>, Vec<Ask>) {
    let control = Control::default();
    let mut answers = answers.iter();
    let mut asked = Vec::new();
    let paths: Vec<String> = paths.iter().map(|path| path.to_string()).collect();
    let job = Unpack { archive: Archive { path: archive, password: None, tool: None }, paths: &paths, base, target };
    let outcome = job.run(index, &quick(), &control, &mut |event| {
        if let Event::Asking(ask) = event {
            asked.push(ask);
            control.answer(*answers.next().unwrap_or(&Decision::Cancel));
        }
    });
    (outcome, asked)
}

fn zip_archive(path: &Path, build: impl FnOnce(&mut ZipWriter<fs::File>)) {
    let mut zip = ZipWriter::new(fs::File::create(path).unwrap());
    build(&mut zip);
    zip.finish().unwrap();
}

fn deflated() -> SimpleFileOptions {
    SimpleFileOptions::default().compression_method(CompressionMethod::Deflated)
}

fn sample_zip(path: &Path) {
    zip_archive(path, |zip| {
        zip.add_directory("docs/", deflated()).unwrap();
        zip.start_file("docs/guide.md", deflated().unix_permissions(0o640)).unwrap();
        zip.write_all(HELLO).unwrap();
        zip.start_file("tools/run.sh", deflated().unix_permissions(0o755)).unwrap();
        zip.write_all(b"#!/bin/sh\n").unwrap();
        zip.add_symlink("latest", "docs/guide.md", deflated()).unwrap();
    });
}

/// A tar archive whose entries are written with exactly the names given, unsafe ones included.
fn raw_tar(entries: &[(&str, tar::EntryType, &[u8], &str)]) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    for (name, kind, data, link) in entries {
        let mut header = tar::Header::new_gnu();
        header.as_old_mut().name[..name.len()].copy_from_slice(name.as_bytes());
        header.set_entry_type(*kind);
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        if !link.is_empty() {
            header.as_old_mut().linkname[..link.len()].copy_from_slice(link.as_bytes());
        }
        header.set_cksum();
        builder.append(&header, *data).unwrap();
    }
    builder.into_inner().unwrap()
}

#[test]
fn names_open_archives_by_their_ending_in_any_case() {
    for name in ["a.zip", "A.TAR.GZ", "b.tgz", "c.7z", "d.rar", "e.tar.zst", "notes.txt.gz", "f.xz", "g.bz2"] {
        assert!(named(name), "{name}");
    }
    for name in ["notes.txt", ".zip", "zip", "archive.zipx"] {
        assert!(!named(name), "{name}");
    }
    assert_eq!(suffix("x.Tar.Gz"), Some(".tar.gz"));
}

#[test]
fn entry_names_never_lead_outside_the_archive() {
    assert_eq!(clean("./docs//guide.md", false).as_deref(), Some("docs/guide.md"));
    assert_eq!(clean("docs/", false).as_deref(), Some("docs"));
    assert_eq!(clean("docs\\guide.md", true).as_deref(), Some("docs/guide.md"));
    for unsafe_name in ["../etc/passwd", "/etc/passwd", "a/../../b", "", ".", "a\0b"] {
        assert_eq!(clean(unsafe_name, false), None, "{unsafe_name:?}");
    }
    for unsafe_name in ["\\windows\\system32", "C:\\boot.ini", "..\\up"] {
        assert_eq!(clean(unsafe_name, true), None, "{unsafe_name:?}");
    }
}

#[test]
fn the_first_bytes_name_the_format() {
    assert_eq!(sniff(b"PK\x03\x04rest"), Some(format::Container::Zip));
    assert_eq!(sniff(b"\x1F\x8B\x08"), Some(format::Container::Stream(Compression::Gzip)));
    assert_eq!(sniff(&raw_tar(&[("a", tar::EntryType::Regular, b"x", "")])), Some(format::Container::Tar));
    assert_eq!(sniff(b"plain text"), None);
}

#[test]
fn an_index_implies_folders_and_adds_up_what_each_holds() {
    let file =
        |path: &str, size: u64, slot| RawItem { size, ..RawItem::new(path.into(), ItemKind::File, path.into(), slot) };
    let index = Index::new(
        Format::Zip,
        100,
        vec![file("a/b/one", 3, 0), file("a/two", 4, 1), file("top", 5, 2), file("a/two", 6, 3)],
        1,
    );
    let names = |folder| index.list(folder).unwrap().map(|item| item.path.clone()).collect::<Vec<_>>();
    assert_eq!(names(""), ["a", "top"]);
    assert_eq!(names("a"), ["a/b", "a/two"]);
    let a = index.item("a").unwrap();
    assert_eq!((a.kind, a.size, a.inside.files, a.inside.folders), (ItemKind::Folder, 9, 2, 1), "the later a/two wins");
    assert_eq!(index.totals(), Totals { files: 3, folders: 2, bytes: 14 });
    assert!(index.list("top").is_none() && index.list("missing").is_none());
    let chosen: Vec<&str> = index.selection(&["a".into()]).iter().map(|item| item.path.as_str()).collect();
    assert_eq!(chosen, ["a", "a/b", "a/b/one", "a/two"]);
}

#[test]
fn a_zip_lists_and_unpacks_with_modes_and_links() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sample.zip");
    sample_zip(&path);
    let index = index(&path);
    assert_eq!(index.format, Format::Zip);
    let guide = index.item("docs/guide.md").unwrap();
    assert_eq!((guide.size, guide.mode), (HELLO.len() as u64, 0o640));
    assert_eq!(index.item("latest").unwrap().kind, ItemKind::Symlink);

    let target = dir.path().join("out");
    fs::create_dir(&target).unwrap();
    let (outcome, asked) = unpack(&path, &index, &["docs", "tools", "latest"], "", &target, &[]);
    assert!(matches!(outcome, Outcome::Done { skipped: 0, .. }), "{outcome:?}");
    assert!(asked.is_empty());
    let found = tree(&target);
    assert_eq!(found["docs/guide.md"], String::from_utf8_lossy(HELLO));
    assert_eq!(found["latest"], "-> docs/guide.md");
    let mode = fs::metadata(target.join("tools/run.sh")).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode & 0o100, 0o100, "the script stays executable");
}

#[test]
fn entries_chosen_in_a_folder_land_without_that_folder() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sample.zip");
    sample_zip(&path);
    let (outcome, _) = unpack(&path, &index(&path), &["docs/guide.md"], "docs", dir.path(), &[]);
    assert!(matches!(outcome, Outcome::Done { .. }));
    assert_eq!(fs::read(dir.path().join("guide.md")).unwrap(), HELLO);
}

#[test]
fn a_zip_whose_entries_share_data_does_not_open() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bomb.zip");
    zip_archive(&path, |zip| {
        zip.start_file("one", deflated()).unwrap();
        zip.write_all(&[0; 4096]).unwrap();
        for copy in 0..8 {
            zip.shallow_copy_file("one", &format!("copy-{copy}")).unwrap();
        }
    });
    let error = Archive { path: &path, password: None, tool: None }.index(&mut |_, _| true).unwrap_err();
    assert!(matches!(error, Error::Overlapping), "{error:?}");
}

#[test]
fn unsafe_names_and_devices_are_left_out_and_counted() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("evil.tar");
    let entries: [(&str, tar::EntryType, &[u8], &str); 4] = [
        ("../escaped", tar::EntryType::Regular, b"x", ""),
        ("/etc/owned", tar::EntryType::Regular, b"x", ""),
        ("null", tar::EntryType::Char, b"", ""),
        ("fine", tar::EntryType::Regular, b"ok", ""),
    ];
    fs::write(&path, raw_tar(&entries)).unwrap();
    let index = index(&path);
    assert_eq!(index.left_out, 3);
    let names: Vec<&str> = index.list("").unwrap().map(|item| item.path.as_str()).collect();
    assert_eq!(names, ["fine"]);
}

#[test]
fn a_link_in_the_archive_cannot_redirect_later_entries() {
    let dir = tempfile::tempdir().unwrap();
    let outside = dir.path().join("outside");
    let target = dir.path().join("target");
    fs::create_dir_all(&outside).unwrap();
    fs::create_dir_all(&target).unwrap();
    let path = dir.path().join("trap.tar");
    let link = outside.to_string_lossy().into_owned();
    let entries: [(&str, tar::EntryType, &[u8], &str); 2] =
        [("escape", tar::EntryType::Symlink, b"", &link), ("escape/planted", tar::EntryType::Regular, b"gotcha", "")];
    fs::write(&path, raw_tar(&entries)).unwrap();
    let (_, asked) = unpack(&path, &index(&path), &["escape"], "", &target, &[Decision::Skip]);
    assert!(fs::read_dir(&outside).unwrap().next().is_none(), "nothing lands outside");
    assert!(matches!(asked.as_slice(), [Ask::Conflict { .. }]), "the link meets the folder it would replace");

    let other = dir.path().join("other");
    fs::create_dir_all(&other).unwrap();
    symlink(&outside, other.join("escape")).unwrap();
    let (_, asked) = unpack(&path, &index(&path), &["escape/planted"], "", &other, &[Decision::Skip]);
    assert!(fs::read_dir(&outside).unwrap().next().is_none(), "a link already there is not followed");
    assert!(matches!(asked.as_slice(), [Ask::Failure { .. }]), "{asked:?}");
}

#[test]
fn an_entry_holding_more_than_its_header_says_stops_at_that_size() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lying.tar");
    fs::write(&path, raw_tar(&[("big", tar::EntryType::Regular, &[7; 9000], "")])).unwrap();
    let real = index(&path);
    let mut lying = real.item("big").unwrap().clone();
    lying.size = 100;
    let index = Index::new(real.format, real.bytes, vec![lying], 0);
    let (outcome, asked) = unpack(&path, &index, &["big"], "", dir.path(), &[Decision::Skip]);
    assert!(matches!(outcome, Outcome::Done { skipped: 1, .. }), "{outcome:?}");
    assert!(matches!(&asked[..], [Ask::Failure { message, .. }] if message.contains("big")));
    assert!(!dir.path().join("big").exists(), "the partial file is removed");
}

#[test]
fn unpacking_asks_about_archives_that_expand_far_or_into_many_files() {
    use crate::unpack::guard::{EXPANSION_FLOOR, EXPANSION_RATIO, FILE_CEILING, questions};
    let (archive, target) = (Path::new("/a.zip"), Path::new("/out"));
    let totals = |files, bytes| Totals { files, folders: 0, bytes };
    let packed = EXPANSION_FLOOR / EXPANSION_RATIO;

    assert!(questions(archive, target, packed, totals(1, EXPANSION_FLOOR), None).is_empty());
    assert!(matches!(
        &questions(archive, target, packed, totals(1, EXPANSION_FLOOR * 2), None)[..],
        [Ask::Expand { unpacked, files: 1, .. }] if *unpacked == EXPANSION_FLOOR * 2
    ));
    assert!(questions(archive, target, EXPANSION_FLOOR, totals(1, EXPANSION_FLOOR * 2), None).is_empty());
    assert!(matches!(
        &questions(archive, target, packed, totals(FILE_CEILING + 1, 1), None)[..],
        [Ask::Expand { files, .. }] if *files == FILE_CEILING + 1
    ));
    assert!(matches!(
        &questions(archive, target, packed, totals(1, 500), Some(100))[..],
        [Ask::Room { needed: 500, free: 100, .. }]
    ));
    assert!(questions(archive, target, packed, totals(1, 500), Some(500)).is_empty());
}

#[test]
fn a_refused_question_cancels_before_anything_is_written() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sample.zip");
    sample_zip(&path);
    let real = index(&path);
    let mut huge = real.item("docs/guide.md").unwrap().clone();
    huge.size = real.bytes.saturating_mul(1 << 20).max(1 << 31);
    let index = Index::new(real.format, real.bytes, vec![huge], 0);
    let target = dir.path().join("out");
    fs::create_dir(&target).unwrap();

    let (outcome, asked) = unpack(&path, &index, &[""], "", &target, &[Decision::Cancel]);
    assert!(matches!(outcome, Outcome::Cancelled { .. }), "{outcome:?}");
    assert!(matches!(&asked[..], [Ask::Expand { .. }]), "{asked:?}");
    assert!(tree(&target).is_empty());
}

#[test]
fn names_already_there_are_asked_about() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sample.zip");
    sample_zip(&path);
    let index = index(&path);
    let target = dir.path().join("out");
    fs::create_dir_all(target.join("docs")).unwrap();
    fs::write(target.join("docs/guide.md"), "mine").unwrap();

    let (_, asked) = unpack(&path, &index, &["docs"], "", &target, &[Decision::Skip]);
    assert!(matches!(&asked[..], [Ask::Conflict { sizes, .. }] if *sizes == (HELLO.len() as u64, 4)));
    assert_eq!(fs::read_to_string(target.join("docs/guide.md")).unwrap(), "mine");

    unpack(&path, &index, &["docs"], "", &target, &[Decision::KeepBoth]);
    assert_eq!(fs::read_to_string(target.join("docs/guide.md")).unwrap(), "mine");
    assert_eq!(fs::read(target.join("docs/guide (2).md")).unwrap(), HELLO);

    unpack(&path, &index, &["docs"], "", &target, &[Decision::Overwrite]);
    assert_eq!(fs::read(target.join("docs/guide.md")).unwrap(), HELLO);
    assert!(!tree(&target).keys().any(|name| name.contains("borg-partial")));
}

#[test]
fn entries_left_out_stay_in_the_report() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sample.zip");
    sample_zip(&path);
    let index = index(&path);
    let target = dir.path().join("out");
    fs::create_dir_all(target.join("docs")).unwrap();
    fs::write(target.join("docs/guide.md"), "mine").unwrap();

    let (report, _) = unpack_report(&path, &index, &["docs", "tools"], "", &target, &[Decision::Skip]);
    assert!(matches!(report.outcome, Outcome::Done { skipped: 1, .. }), "{report:?}");
    assert_eq!(report.kept, vec!["docs".to_owned()]);
    assert_eq!(fs::read(target.join("tools/run.sh")).unwrap(), b"#!/bin/sh\n");
}

#[test]
fn hard_links_point_at_entries_unpacked_before_them() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("links.tar");
    let entries: [(&str, tar::EntryType, &[u8], &str); 2] =
        [("first", tar::EntryType::Regular, b"shared", ""), ("second", tar::EntryType::Link, b"", "first")];
    fs::write(&path, raw_tar(&entries)).unwrap();
    let index = index(&path);
    let target = dir.path().join("out");
    fs::create_dir(&target).unwrap();
    unpack(&path, &index, &["first", "second"], "", &target, &[]);
    assert_eq!(fs::read(target.join("second")).unwrap(), b"shared");

    let alone = dir.path().join("alone");
    fs::create_dir(&alone).unwrap();
    let (_, asked) = unpack(&path, &index, &["second"], "", &alone, &[Decision::Skip]);
    assert!(matches!(&asked[..], [Ask::Failure { message, .. }] if message.contains("first")));
}

#[test]
fn compressed_tars_and_single_files_read_through_their_compression() {
    let dir = tempfile::tempdir().unwrap();
    let tar = {
        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_size(HELLO.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append_data(&mut header, "inside/hello.txt", HELLO).unwrap();
        builder.into_inner().unwrap()
    };
    let compressed: [(&str, Vec<u8>, Compression); 4] = [
        ("t.tar.gz", gzip(&tar), Compression::Gzip),
        ("t.tar.bz2", bzip(&tar), Compression::Bzip2),
        ("t.tar.xz", xz(&tar), Compression::Xz),
        ("t.tar.zst", zstd::encode_all(Cursor::new(&tar), 3).unwrap(), Compression::Zstd),
    ];
    for (name, bytes, compression) in compressed {
        let path = dir.path().join(name);
        fs::write(&path, bytes).unwrap();
        let index = index(&path);
        assert_eq!(index.format, Format::Tar(Some(compression)), "{name}");
        let archive = Archive { path: &path, password: None, tool: None };
        let mut out = Vec::new();
        assert_eq!(archive.copy_out(&index, "inside/hello.txt", &mut out).unwrap(), HELLO.len() as u64);
        assert_eq!(out, HELLO, "{name}");
    }
    let single = dir.path().join("notes.txt.gz");
    fs::write(&single, gzip(HELLO)).unwrap();
    let index = index(&single);
    assert_eq!(index.format, Format::Compressed(Compression::Gzip));
    let item = index.item("notes.txt").unwrap();
    assert_eq!(item.size, HELLO.len() as u64);
}

fn gzip(data: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(data).unwrap();
    encoder.finish().unwrap()
}

fn bzip(data: &[u8]) -> Vec<u8> {
    let mut encoder = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::default());
    encoder.write_all(data).unwrap();
    encoder.finish().unwrap()
}

fn xz(data: &[u8]) -> Vec<u8> {
    let mut encoder = lzma_rust2::XzWriter::new(Vec::new(), lzma_rust2::XzOptions::with_preset(1)).unwrap();
    encoder.write_all(data).unwrap();
    encoder.finish().unwrap()
}

fn seven_zip_source(dir: &Path) -> PathBuf {
    let source = dir.join("source");
    fs::create_dir_all(source.join("docs")).unwrap();
    fs::write(source.join("docs/guide.md"), HELLO).unwrap();
    fs::write(source.join("top.txt"), "top").unwrap();
    source
}

#[test]
fn a_7z_archive_lists_and_unpacks() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sample.7z");
    sevenz_rust2::compress_to_path(seven_zip_source(dir.path()), &path).unwrap();
    let index = index(&path);
    assert_eq!(index.format, Format::SevenZip);
    assert_eq!(index.item("docs/guide.md").unwrap().size, HELLO.len() as u64);
    let target = dir.path().join("out");
    fs::create_dir(&target).unwrap();
    let (outcome, _) = unpack(&path, &index, &[""], "", &target, &[]);
    assert!(matches!(outcome, Outcome::Done { .. }), "{outcome:?}");
    assert_eq!(fs::read(target.join("docs/guide.md")).unwrap(), HELLO);
    assert_eq!(fs::read_to_string(target.join("top.txt")).unwrap(), "top");
}

#[test]
fn encrypted_archives_need_the_right_password() {
    let dir = tempfile::tempdir().unwrap();
    let zip_path = dir.path().join("locked.zip");
    zip_archive(&zip_path, |zip| {
        zip.start_file("secret.txt", deflated().with_aes_encryption(zip::AesMode::Aes256, SECRET)).unwrap();
        zip.write_all(HELLO).unwrap();
    });
    let seven_path = dir.path().join("locked.7z");
    sevenz_rust2::compress_to_path_encrypted(seven_zip_source(dir.path()), &seven_path, SECRET.into()).unwrap();

    let zip_index = index(&zip_path);
    assert!(zip_index.encrypted());
    let open = |path, password| Archive { path, password, tool: None };
    let mut out = Vec::new();
    assert!(matches!(open(&zip_path, None).copy_out(&zip_index, "secret.txt", &mut out), Err(Error::NeedsPassword)));
    assert!(matches!(open(&zip_path, Some("wrong")).check_password(&zip_index), Err(Error::WrongPassword)));
    open(&zip_path, Some(SECRET)).check_password(&zip_index).unwrap();
    open(&zip_path, Some(SECRET)).copy_out(&zip_index, "secret.txt", &mut out).unwrap();
    assert_eq!(out, HELLO);

    let listed = open(&seven_path, None).index(&mut |_, _| true);
    let seven_index = match listed {
        Err(Error::NeedsPassword) => open(&seven_path, Some(SECRET)).index(&mut |_, _| true).unwrap(),
        other => other.unwrap(),
    };
    assert!(seven_index.encrypted());
    assert!(open(&seven_path, Some("wrong")).check_password(&seven_index).is_err());
    let mut out = Vec::new();
    open(&seven_path, Some(SECRET)).copy_out(&seven_index, "docs/guide.md", &mut out).unwrap();
    assert_eq!(out, HELLO);
}

#[test]
fn a_reader_stops_when_its_watch_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("long.tar.gz");
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    let noise: Vec<u8> = (0..6_000_000)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        })
        .collect();
    fs::write(&path, gzip(&raw_tar(&[("noise", tar::EntryType::Regular, &noise, "")]))).unwrap();
    let error = Archive { path: &path, password: None, tool: None }.index(&mut |_, _| false).unwrap_err();
    assert!(matches!(error, Error::Stopped), "{error:?}");
}

#[test]
fn tool_listings_give_entries_with_their_kinds_and_sizes() {
    let seven_zip = "Path = docs\nFolder = +\nSize = 0\nModified = 2026-11-03 00:40:38\nAttributes = D drwxr-xr-x\nEncrypted = -\n\n\
                     Path = docs/guide.md\nFolder = -\nSize = 14\nModified = 2026-11-03 00:40:38\nAttributes =  -rw-r-----\nEncrypted = +\n\n\
                     Path = latest\nFolder = -\nSize = 13\nAttributes =  lrwxr-xr-x\nEncrypted = -\n\n\
                     Path = ../up\nFolder = -\nSize = 1\n";
    let (items, left_out) = read::tool::seven_zip_listing(seven_zip).unwrap();
    assert_eq!(left_out, 1);
    let guide = &items[1];
    assert_eq!((guide.path.as_str(), guide.size, guide.mode, guide.encrypted), ("docs/guide.md", 14, 0o640, true));
    assert!(guide.modified.is_some());
    assert_eq!((items[0].kind, items[2].kind), (ItemKind::Folder, ItemKind::Symlink));

    let bsdtar = "drwxr-xr-x  0 501    0           0 Nov  3 00:40 dir one/\n\
                  -rw-r--r--  0 501    0           6 Nov  3 00:40 dir one/a b.txt\n\
                  lrwxr-xr-x  0 501    0           0 Nov  3 00:40 link -> dir one/a b.txt\n";
    let (items, _) = read::tool::bsdtar_listing(bsdtar).unwrap();
    assert_eq!(items[1].path, "dir one/a b.txt");
    assert_eq!(items[1].size, 6);
    assert_eq!((items[2].kind, items[2].link.as_deref()), (ItemKind::Symlink, Some("dir one/a b.txt")));
}

fn installed(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path).map(|dir| dir.join(program)).find(|candidate| candidate.is_file())
}

#[test]
fn installed_tools_list_and_stream_entries() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sample.zip");
    sample_zip(&path);
    let bytes = fs::metadata(&path).unwrap().len();
    let tools = RAR_PROGRAMS
        .iter()
        .filter_map(|(program, kind)| installed(program).map(|program| Tool { kind: *kind, program }));
    for tool in tools {
        let index = read::tool::index(&tool, &path, bytes, None).unwrap();
        let guide = index.item("docs/guide.md").unwrap_or_else(|| panic!("{} lists the guide", tool.name()));
        assert_eq!(guide.size, HELLO.len() as u64, "{}", tool.name());
        let mut out = Vec::new();
        read::tool::each(&tool, &path, None, &[guide], &mut |_, reader| {
            std::io::copy(reader, &mut out).map_err(Error::from)?;
            Ok(Flow::Next)
        })
        .unwrap();
        assert_eq!(out, HELLO, "{}", tool.name());
    }
}

/// A name with the characters bsdtar reads as a pattern still means that entry alone.
#[test]
fn bsdtar_is_given_an_entrys_name_and_not_a_pattern() {
    assert_eq!(read::tool::literal_pattern("docs/guide.md"), "docs/guide.md");
    assert_eq!(read::tool::literal_pattern("a*b[1]?.txt"), "a\\*b\\[1]\\?.txt");
    assert_eq!(read::tool::literal_pattern("back\\slash"), "back\\\\slash");
}

#[test]
fn installed_tools_stream_the_entry_named_even_when_its_name_reads_as_a_pattern() {
    const BRACKETS: &str = "notes[1].txt";
    const PLAIN: &str = "notes1.txt";
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("patterns.zip");
    zip_archive(&path, |zip| {
        zip.start_file(PLAIN, deflated()).unwrap();
        zip.write_all(b"plain").unwrap();
        zip.start_file(BRACKETS, deflated()).unwrap();
        zip.write_all(HELLO).unwrap();
    });
    let bytes = fs::metadata(&path).unwrap().len();
    let tools = RAR_PROGRAMS
        .iter()
        .filter_map(|(program, kind)| installed(program).map(|program| Tool { kind: *kind, program }));
    for tool in tools {
        let index = read::tool::index(&tool, &path, bytes, None).unwrap();
        let entry = index.item(BRACKETS).unwrap_or_else(|| panic!("{} lists it", tool.name()));
        let mut out = Vec::new();
        read::tool::each(&tool, &path, None, &[entry], &mut |_, reader| {
            std::io::copy(reader, &mut out).map_err(Error::from)?;
            Ok(Flow::Next)
        })
        .unwrap();
        assert_eq!(out, HELLO, "{}", tool.name());
    }
}

/// A program standing in for the one that reads RAR archives, which does what `script` says
/// whatever it is asked for.
fn scripted_tool(dir: &Path, script: &str) -> Tool {
    let program = dir.join("tool");
    fs::write(&program, format!("#!/bin/sh\n{script}\n")).unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
    Tool { kind: ToolKind::Bsdtar, program }
}

fn wanted_entry() -> Item {
    Item::new("big.bin".into(), ItemKind::File, "big.bin".into(), 0)
}

/// Leaving an entry before its end stops the program, where waiting for it would unpack the
/// rest of the entry first, however long that takes.
#[test]
fn stopping_before_an_entrys_end_does_not_wait_for_the_program_to_finish_it() {
    let dir = tempfile::tempdir().unwrap();
    let tool = scripted_tool(dir.path(), "exec yes");
    let entry = wanted_entry();
    for flow in [Flow::Stop, Flow::Next] {
        let mut first = [0; 1];
        read::tool::each(&tool, dir.path(), None, &[&entry], &mut |_, reader| {
            reader.read_exact(&mut first).map_err(Error::from)?;
            Ok(flow)
        })
        .unwrap();
        assert_eq!(&first, b"y");
    }
}

/// A program that has more to say about what is wrong than a pipe holds still gets to write
/// the entry.
#[test]
fn a_program_with_much_to_say_still_hands_over_the_entry() {
    let dir = tempfile::tempdir().unwrap();
    let tool = scripted_tool(dir.path(), "head -c 1000000 /dev/zero >&2\nprintf hello");
    let entry = wanted_entry();
    let mut out = Vec::new();
    read::tool::each(&tool, dir.path(), None, &[&entry], &mut |_, reader| {
        std::io::copy(reader, &mut out).map_err(Error::from)?;
        Ok(Flow::Next)
    })
    .unwrap();
    assert_eq!(out, b"hello");
}

#[test]
fn sizes_an_archive_claims_add_up_without_wrapping_around() {
    let mut huge = wanted_entry();
    huge.size = u64::MAX;
    let totals = Index::selected_totals(&[&huge, &huge]);
    assert_eq!((totals.files, totals.bytes), (2, u64::MAX));
}
