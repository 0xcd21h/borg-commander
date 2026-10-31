use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use borg_jobs::{Ask, Control, Decision, Event, Outcome, Totals};
use serde_json::json;
use std::{
    fs::{self, File},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::{Duration, UNIX_EPOCH},
};

fn run(source: PathBuf, target: PathBuf, control: &Control, emit: &mut dyn FnMut(Event)) -> Outcome {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    CopyJob { from: None, to: None, sources: vec![source], target }.run(runtime.handle(), control, emit)
}

#[test]
fn recursive_copy_preserves_empty_folders_links_raw_names_modes_and_times() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let target = temp.path().join("target");
    fs::create_dir_all(source.join("empty")).unwrap();
    fs::create_dir(&target).unwrap();
    #[cfg(target_os = "linux")]
    let raw = std::ffi::OsString::from_vec(vec![b'x', 0xff]);
    #[cfg(not(target_os = "linux"))]
    let raw = std::ffi::OsString::from("資料");
    let file = source.join(&raw);
    fs::write(&file, b"contents").unwrap();
    let meta = File::open(&file).unwrap();
    meta.set_permissions(fs::Permissions::from_mode(0o751)).unwrap();
    meta.set_times(fs::FileTimes::new().set_modified(UNIX_EPOCH + Duration::new(123456789, 123456))).unwrap();
    std::os::unix::fs::symlink(&raw, source.join("link")).unwrap();
    let outcome = run(source.clone(), target.clone(), &Control::default(), &mut |_| {});
    assert!(matches!(outcome, Outcome::Done { done: Totals { files: 2, bytes: 8 }, .. }), "{outcome:?}");
    let copied = target.join("source");
    assert!(copied.join("empty").is_dir());
    assert_eq!(fs::read(copied.join(&raw)).unwrap(), b"contents");
    assert_eq!(fs::read_link(copied.join("link")).unwrap(), PathBuf::from(raw));
    let copied = node(&copied.join(file.file_name().unwrap())).unwrap().unwrap();
    let original = node(&file).unwrap().unwrap();
    assert_eq!(copied.mode, original.mode);
    assert_eq!(copied.modified, original.modified);
}

#[test]
fn conflicts_keep_both_skip_overwrite_and_cancel_without_partial_destinations() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let target = temp.path().join("target");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&target).unwrap();
    let source = source.join("data");
    let bytes = vec![0x93; CHUNK * 3 + 7];
    fs::write(&source, &bytes).unwrap();
    fs::write(target.join("data"), b"original").unwrap();
    let control = Control::default();
    let outcome = run(source.clone(), target.clone(), &control, &mut |event| match event {
        Event::Asking(Ask::Conflict { .. }) => control.answer(Decision::Overwrite),
        Event::Progress(progress) if progress.done.bytes > 0 => control.cancel(),
        _ => {}
    });
    assert!(matches!(outcome, Outcome::Cancelled { .. }), "{outcome:?}");
    assert_eq!(fs::read(target.join("data")).unwrap(), b"original");
    assert_eq!(fs::read_dir(&target).unwrap().count(), 1);
    for decision in [Decision::Skip, Decision::KeepBoth, Decision::OverwriteAll] {
        let control = Control::default();
        let outcome = run(source.clone(), target.clone(), &control, &mut |event| {
            if matches!(event, Event::Asking(Ask::Conflict { .. })) {
                control.answer(decision);
            }
        });
        assert!(matches!(outcome, Outcome::Done { .. }), "{outcome:?}");
        if decision == Decision::KeepBoth {
            assert_eq!(fs::read(target.join("data (2)")).unwrap(), bytes);
        }
    }
    assert_eq!(fs::read(target.join("data")).unwrap(), bytes);
    assert_eq!(fs::read_dir(&target).unwrap().count(), 2);
}

#[test]
fn copy_refuses_descendants_and_destination_symlink_directories() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    fs::create_dir_all(source.join("nested")).unwrap();
    let outcome = run(source.clone(), source.join("nested/.."), &Control::default(), &mut |_| {});
    assert!(matches!(outcome, Outcome::Failed(_)), "{outcome:?}");
    let target = temp.path().join("target");
    fs::create_dir(&target).unwrap();
    std::os::unix::fs::symlink(temp.path(), target.join("source")).unwrap();
    let control = Control::default();
    let outcome = run(source, target, &control, &mut |event| {
        if matches!(event, Event::Asking(Ask::Failure { .. })) {
            control.answer(Decision::Skip);
        }
    });
    assert!(matches!(outcome, Outcome::Done { skipped: 1, .. }), "{outcome:?}");
    assert!(!temp.path().join("nested").exists());
}

#[tokio::test]
async fn server_drops_staged_uploads_rejects_oversize_chunks_and_never_clobbers_races() {
    let temp = tempfile::tempdir().unwrap();
    let target = temp.path().join("data");
    let mut server = Server::default();
    let begin = || Op::Begin { path: wire(&target), size: 3, overwrite: false };
    server.apply(begin()).unwrap();
    assert!(server.apply(Op::Write { data: STANDARD.encode(b"toolong") }).is_err());
    server.apply(Op::Abort).unwrap();
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
    server.apply(begin()).unwrap();
    server.apply(Op::Write { data: STANDARD.encode(b"new") }).unwrap();
    fs::write(&target, b"raced").unwrap();
    let meta = node(&target).unwrap().unwrap();
    assert!(server.apply(Op::Finish { node: meta }).is_err());
    assert_eq!(fs::read(&target).unwrap(), b"raced");
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
    server.apply(begin()).unwrap();
    drop(server);
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
    let server = Server::default();
    let (_, reply) = server.handle(json!({"op":"stat","path":wire(Path::new("relative"))})).await;
    assert!(!reply.ok);
}
