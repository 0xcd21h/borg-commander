use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
    thread,
    time::{Duration, UNIX_EPOCH},
};

use borg_jobs::{Ask, Control, Decision, Event, Outcome, Spec, Totals, run};
use tempfile::TempDir;

const OLD: Duration = Duration::from_secs(1_600_000_000);
const BIG: usize = 24 * 1024 * 1024;
const MANY_FILES: usize = 4100;
const STREAMED_BYTES: usize = 2 * 1024 * 1024;

fn sandbox() -> (TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let (from, to) = (dir.path().join("from"), dir.path().join("to"));
    fs::create_dir_all(&from).unwrap_or_else(|e| panic!("{e}"));
    fs::create_dir_all(&to).unwrap_or_else(|e| panic!("{e}"));
    (dir, from, to)
}

fn write(path: &Path, body: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap_or_else(|e| panic!("{e}"));
    }
    fs::write(path, body).unwrap_or_else(|e| panic!("{e}"));
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn tree(root: &Path) {
    write(&root.join("project/src/main.rs"), "fn main() {}");
    write(&root.join("project/README.md"), "hello");
    let script = root.join("project/run.sh");
    write(&script, "#!/bin/sh");
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap_or_else(|e| panic!("{e}"));
    let old = fs::File::open(&script).unwrap_or_else(|e| panic!("{e}"));
    old.set_times(fs::FileTimes::new().set_modified(UNIX_EPOCH + OLD)).unwrap_or_else(|e| panic!("{e}"));
    symlink("README.md", root.join("project/readme-link")).unwrap_or_else(|e| panic!("{e}"));
}

fn answering(spec: &Spec, answers: &[Decision]) -> (Outcome, Vec<Event>) {
    let control = Control::default();
    let mut answers = answers.iter().copied();
    let mut events = Vec::new();
    let control_ref = &control;
    let outcome = thread::scope(|scope| {
        let (tx, rx) = mpsc::channel::<Event>();
        let runner = scope.spawn(move || run(spec, control_ref, &mut |event| drop(tx.send(event))));
        for event in rx {
            if matches!(event, Event::Asking(_)) {
                control_ref.answer(answers.next().unwrap_or(Decision::Cancel));
            }
            events.push(event);
        }
        runner.join().unwrap_or_else(|_| panic!("job panicked"))
    });
    (outcome, events)
}

#[test]
fn copies_trees_with_permissions_times_and_links() {
    for optimize_hdd in [false, true] {
        let (_dir, from, to) = sandbox();
        tree(&from);
        let spec = Spec::Copy { sources: vec![from.join("project")], target: to.clone(), optimize_hdd };
        let (outcome, events) = answering(&spec, &[]);

        assert_eq!(outcome, Outcome::Done { done: Totals { files: 4, bytes: 26 }, skipped: 0 });
        assert_eq!(read(&to.join("project/src/main.rs")), "fn main() {}");
        let script = fs::metadata(to.join("project/run.sh")).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(script.permissions().mode() & 0o777, 0o755);
        assert_eq!(script.modified().ok(), Some(UNIX_EPOCH + OLD));
        assert_eq!(fs::read_link(to.join("project/readme-link")).ok(), Some(PathBuf::from("README.md")));
        assert!(read(&from.join("project/README.md")) == "hello", "copy keeps the source");
        assert!(events.iter().any(|event| matches!(event, Event::Scanning(_))));
        let last = events.iter().rev().find_map(|event| match event {
            Event::Progress(progress) => Some(progress.done),
            _ => None,
        });
        assert_eq!(last, Some(Totals { files: 4, bytes: 26 }));
    }
}

#[test]
fn conflicts_ask_and_remember_all_answers() {
    for optimize_hdd in [false, true] {
        let (_dir, from, to) = sandbox();
        for name in ["a.txt", "b.txt", "c.txt"] {
            write(&from.join(name), "new");
            write(&to.join(name), "old");
        }
        let sources = ["a.txt", "b.txt", "c.txt"].map(|name| from.join(name)).to_vec();
        let (outcome, events) = answering(
            &Spec::Copy { sources, target: to.clone(), optimize_hdd },
            &[Decision::Skip, Decision::OverwriteAll],
        );

        let asked = events.iter().filter(|event| matches!(event, Event::Asking(Ask::Conflict { .. }))).count();
        assert_eq!(asked, 2);
        assert_eq!([read(&to.join("a.txt")), read(&to.join("b.txt")), read(&to.join("c.txt"))], ["old", "new", "new"]);
        assert!(matches!(outcome, Outcome::Done { skipped: 1, .. }));
    }
}

#[test]
fn keep_both_and_copies_into_the_same_folder_get_numbered_names() {
    for optimize_hdd in [false, true] {
        let (_dir, from, to) = sandbox();
        write(&from.join("notes.txt"), "mine");
        write(&to.join("notes.txt"), "theirs");
        answering(
            &Spec::Copy { sources: vec![from.join("notes.txt")], target: to.clone(), optimize_hdd },
            &[Decision::KeepBoth],
        );
        assert_eq!(read(&to.join("notes (2).txt")), "mine");

        answering(&Spec::Copy { sources: vec![from.join("notes.txt")], target: from.clone(), optimize_hdd }, &[]);
        assert_eq!(read(&from.join("notes (2).txt")), "mine");
    }
}

#[test]
fn refuses_to_copy_a_folder_into_itself() {
    for optimize_hdd in [false, true] {
        let (_dir, from, _to) = sandbox();
        tree(&from);
        let inside = from.join("project/src");
        let (outcome, _) =
            answering(&Spec::Copy { sources: vec![from.join("project")], target: inside, optimize_hdd }, &[]);
        assert!(matches!(outcome, Outcome::Failed(message) if message.contains("into itself")));
    }
}

#[test]
fn cancelling_mid_file_leaves_no_partial_files() {
    let (_dir, from, to) = sandbox();
    fs::write(from.join("big.bin"), vec![7u8; BIG]).unwrap_or_else(|e| panic!("{e}"));
    let control = Arc::new(Control::default());
    control.set_paused(true);
    let spec = Spec::Copy { sources: vec![from.join("big.bin")], target: to.clone(), optimize_hdd: false };
    let worker = {
        let control = Arc::clone(&control);
        thread::spawn(move || run(&spec, &control, &mut |_| {}))
    };
    thread::sleep(Duration::from_millis(50));
    control.cancel();
    let outcome = worker.join().unwrap_or_else(|_| panic!("job panicked"));
    assert!(matches!(outcome, Outcome::Cancelled { .. }));
    let left: Vec<_> = fs::read_dir(&to).unwrap_or_else(|e| panic!("{e}")).collect();
    assert!(left.is_empty(), "nothing may remain after cancelling");
}

#[test]
fn moves_rename_and_deletes_remove_trees() {
    let (_dir, from, to) = sandbox();
    tree(&from);
    let (moved, _) = answering(&Spec::Move { sources: vec![from.join("project")], target: to.clone() }, &[]);
    assert!(matches!(moved, Outcome::Done { skipped: 0, .. }));
    assert!(!from.join("project").exists());
    assert_eq!(read(&to.join("project/README.md")), "hello");

    let (deleted, _) = answering(&Spec::Delete { sources: vec![to.join("project")] }, &[]);
    assert_eq!(deleted, Outcome::Done { done: Totals { files: 4, bytes: 26 }, skipped: 0 });
    assert!(!to.join("project").exists());
}

#[test]
fn single_sources_can_be_copied_to_a_new_name() {
    for optimize_hdd in [false, true] {
        let (_dir, from, to) = sandbox();
        write(&from.join("a.txt"), "x");
        answering(&Spec::Copy { sources: vec![from.join("a.txt")], target: to.join("renamed.txt"), optimize_hdd }, &[]);
        assert_eq!(read(&to.join("renamed.txt")), "x");
    }
}

#[test]
fn unreadable_files_can_be_skipped() {
    for optimize_hdd in [false, true] {
        let (_dir, from, to) = sandbox();
        write(&from.join("ok.txt"), "fine");
        let missing = from.join("gone.txt");
        let (outcome, events) = answering(
            &Spec::Copy { sources: vec![missing, from.join("ok.txt")], target: to.clone(), optimize_hdd },
            &[Decision::Skip],
        );
        assert!(events.iter().any(|event| matches!(event, Event::Asking(Ask::Failure { .. }))));
        assert!(matches!(outcome, Outcome::Done { .. }));
        assert_eq!(read(&to.join("ok.txt")), "fine");
    }
}

#[test]
fn buffered_copy_handles_multiple_batches_empty_files_and_streamed_files() {
    let (_dir, from, to) = sandbox();
    let root = from.join("tree");
    fs::create_dir(&root).unwrap();
    let mut bytes = STREAMED_BYTES as u64;
    for index in 0..MANY_FILES {
        let content = format!("payload-{index}");
        write(&root.join(format!("{index:05}.txt")), &content);
        bytes += content.len() as u64;
    }
    fs::write(root.join("large"), vec![7; STREAMED_BYTES]).unwrap();
    write(&root.join("empty"), "");
    let spec = Spec::Copy { sources: vec![root], target: to.clone(), optimize_hdd: true };
    let (outcome, _) = answering(&spec, &[]);
    assert_eq!(outcome, Outcome::Done { done: Totals { files: MANY_FILES as u64 + 2, bytes }, skipped: 0 });
    for index in 0..MANY_FILES {
        assert_eq!(read(&to.join(format!("tree/{index:05}.txt"))), format!("payload-{index}"));
    }
    assert_eq!(fs::read(to.join("tree/large")).unwrap(), vec![7; STREAMED_BYTES]);
    assert!(fs::read(to.join("tree/empty")).unwrap().is_empty());
    assert_eq!(fs::read_dir(to.join("tree")).unwrap().count(), MANY_FILES + 2);
}

#[test]
fn buffered_selected_files_use_captured_data_before_destination_writes() {
    let (_dir, from, to) = sandbox();
    let sources = ["a", "b", "c"].map(|name| from.join(name)).to_vec();
    for path in &sources {
        write(path, "original");
    }
    let spec = Spec::Copy { sources: sources.clone(), target: to.clone(), optimize_hdd: true };
    let mut observed_read = false;
    let outcome = run(&spec, &Control::default(), &mut |event| {
        if let Event::Progress(progress) = event
            && progress.done.files == 0
            && progress.done.bytes == 0
            && !observed_read
        {
            assert_eq!(fs::read_dir(&to).unwrap().count(), 0);
            fs::remove_file(&sources[0]).unwrap();
            observed_read = true;
        }
    });
    assert!(observed_read);
    assert_eq!(outcome, Outcome::Done { done: Totals { files: 3, bytes: 24 }, skipped: 0 });
    for name in ["a", "b", "c"] {
        assert_eq!(read(&to.join(name)), "original");
    }
}

#[test]
fn cancelling_a_buffered_read_batch_leaves_existing_destinations_untouched() {
    let (_dir, from, to) = sandbox();
    write(&from.join("a"), "new a");
    write(&from.join("b"), "new b");
    write(&to.join("a"), "old a");
    let spec = Spec::Copy { sources: vec![from.join("a"), from.join("b")], target: to.clone(), optimize_hdd: true };
    let control = Control::default();
    let outcome = run(&spec, &control, &mut |event| {
        if matches!(event, Event::Progress(_)) {
            control.cancel();
        }
    });
    assert_eq!(outcome, Outcome::Cancelled { done: Totals::default() });
    assert_eq!(read(&to.join("a")), "old a");
    assert_eq!(fs::read_dir(&to).unwrap().count(), 1);
}

#[test]
fn files_growing_after_scan_fall_back_to_streaming() {
    let (_dir, from, to) = sandbox();
    let source = from.join("growing");
    write(&source, "small");
    let spec = Spec::Copy { sources: vec![source.clone()], target: to.clone(), optimize_hdd: true };
    let content = vec![9; STREAMED_BYTES];
    let outcome = run(&spec, &Control::default(), &mut |event| {
        if matches!(event, Event::Scanning(_)) {
            fs::write(&source, &content).unwrap();
        }
    });
    assert_eq!(outcome, Outcome::Done { done: Totals { files: 1, bytes: STREAMED_BYTES as u64 }, skipped: 0 });
    assert_eq!(fs::read(to.join("growing")).unwrap(), content);
}
