use super::*;

fn tree() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("tree");
    fs::create_dir_all(root.join("b")).unwrap();
    fs::write(root.join("a.txt"), "hello").unwrap();
    fs::write(root.join("b/c.txt"), "hi").unwrap();
    std::os::unix::fs::symlink("a.txt", root.join("link")).unwrap();
    dir
}

#[test]
fn counting_adds_files_and_links_without_following_them() {
    let dir = tree();
    let later = Instant::now() + Duration::from_secs(60);
    let size = tally(&[dir.path().join("tree")], later);
    assert_eq!(size, DeleteSize { bytes: 7, files: 3, complete: true });
    let single = tally(&[dir.path().join("tree/a.txt")], later);
    assert_eq!(single, DeleteSize { bytes: 5, files: 1, complete: true });
}

#[test]
fn counting_says_so_when_it_stops_early_or_misses_a_file() {
    let dir = tree();
    let size = tally(&[dir.path().join("tree")], Instant::now());
    assert!(!size.complete);
    let gone = tally(&[dir.path().join("gone")], Instant::now() + Duration::from_secs(60));
    assert_eq!(gone, DeleteSize { bytes: 0, files: 0, complete: false });
}

#[test]
fn lsof_fields_give_each_open_file_its_process() {
    let bytes = b"p12\0cpostgres\0\nfcwd\0n/\0\nf3\0n/srv/db/wal.log\0\np40\0ctail\0\nf4\0n/srv/db/wal.log\0\n";
    let files = parse_lsof(bytes);
    let rows: Vec<_> = files.iter().map(|file| (file.pid, file.name.as_str(), file.path.to_str().unwrap())).collect();
    assert_eq!(rows, [(12, "postgres", "/"), (12, "postgres", "/srv/db/wal.log"), (40, "tail", "/srv/db/wal.log")]);
}

#[test]
fn holders_are_the_programs_with_a_file_under_the_sources_once_each() {
    let dir = tree();
    let root = fs::canonicalize(dir.path()).unwrap().join("tree");
    let open = |pid, name: &str, path: PathBuf| OpenFile { pid, name: name.into(), path };
    let files = vec![
        open(40, "tail", root.join("a.txt")),
        open(12, "postgres", root.join("b/c.txt")),
        open(12, "postgres", root.join("a.txt")),
        open(77, "vim", root.with_file_name("tree-other/a.txt")),
        open(std::process::id(), "borg", root.join("a.txt")),
    ];
    let found = holders(&[dir.path().join("tree")], files);
    assert_eq!(found, [FileHolder { pid: 12, name: "postgres".into() }, FileHolder { pid: 40, name: "tail".into() }]);
    let link = holders(&[dir.path().join("tree/link")], vec![open(40, "tail", root.join("a.txt"))]);
    assert!(link.is_empty(), "a link is never open itself");
}

#[tokio::test]
async fn the_system_lists_a_file_this_test_holds_open() {
    let dir = tree();
    let path = dir.path().join("tree/a.txt");
    let _held = fs::File::open(&path).unwrap();
    let open = open_files().await.unwrap();
    let resolved = fs::canonicalize(&path).unwrap();
    assert!(open.iter().any(|file| file.pid == std::process::id() && file.path == resolved));
}
