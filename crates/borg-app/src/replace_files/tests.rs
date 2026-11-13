use super::*;
use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf};

fn request(dir: &Path, text: &str, with: &str, names: &[&str]) -> ReplaceRequest {
    ReplaceRequest {
        query: borg_view::Query { text: text.into(), ..Default::default() },
        with: with.into(),
        files: names.iter().map(|name| dir.join(name)).collect(),
    }
}

fn files(dir: &Path) -> Vec<PathBuf> {
    fs::write(dir.join("a.conf"), "port 80\r\nexport PORT=81\n").unwrap();
    fs::set_permissions(dir.join("a.conf"), fs::Permissions::from_mode(0o640)).unwrap();
    fs::write(dir.join("b.bin"), b"port\0\x01").unwrap();
    fs::write(dir.join("c.txt"), "nothing here\n").unwrap();
    vec![dir.join("a.conf"), dir.join("b.bin"), dir.join("c.txt")]
}

#[test]
fn text_files_change_in_place_and_others_are_skipped_untouched() {
    let dir = tempfile::tempdir().unwrap();
    files(dir.path());
    let mut events = Vec::new();
    let outcome = run(
        &request(dir.path(), "port", "listen", &["a.conf", "b.bin", "c.txt", "gone.txt"]),
        &Control::default(),
        &mut |event| events.push(event),
    );
    assert_eq!(outcome, Outcome::Done { done: Totals { files: 1, bytes: 0 }, skipped: 2 });
    assert_eq!(fs::read_to_string(dir.path().join("a.conf")).unwrap(), "listen 80\r\nexlisten listen=81\n");
    assert_eq!(fs::metadata(dir.path().join("a.conf")).unwrap().permissions().mode() & 0o777, 0o640);
    assert_eq!(fs::read(dir.path().join("b.bin")).unwrap(), b"port\0\x01");
    assert_eq!(fs::read_to_string(dir.path().join("c.txt")).unwrap(), "nothing here\n");
    let progress: Vec<u64> = events
        .iter()
        .map(|event| match event {
            Event::Progress(progress) => progress.done.files,
            other => panic!("only progress: {other:?}"),
        })
        .collect();
    assert_eq!(progress, [0, 1, 2, 3]);
    let leftovers = fs::read_dir(dir.path()).unwrap().count();
    assert_eq!(leftovers, 3, "no staging files stay behind");
}

#[test]
fn a_cancelled_job_changes_nothing_more_and_a_bad_expression_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    files(dir.path());
    let control = Control::default();
    control.cancel();
    let outcome = run(&request(dir.path(), "port", "listen", &["a.conf"]), &control, &mut |_| {});
    assert_eq!(outcome, Outcome::Cancelled { done: Totals::default() });

    let mut broken = request(dir.path(), "(port", "listen", &["a.conf"]);
    broken.query.regex = true;
    assert!(matches!(run(&broken, &Control::default(), &mut |_| {}), Outcome::Failed(_)));
    assert_eq!(fs::read_to_string(dir.path().join("a.conf")).unwrap(), "port 80\r\nexport PORT=81\n");
}
