use super::*;
use std::os::unix::net::UnixListener;

const OLD: Duration = Duration::from_secs(4 * 24 * 60 * 60);

fn age(path: &Path, by: Duration) {
    let file = fs::File::open(path).unwrap();
    file.set_modified(SystemTime::now() - by).unwrap();
}

fn write(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, b"borg").unwrap();
}

#[test]
fn the_newest_capture_comes_first_once_and_the_list_keeps_its_limit() {
    let at = UNIX_EPOCH + Duration::from_secs(1_000);
    let records: Vec<CaptureRecord> = (0..CAPTURE_RECORDS)
        .map(|n| CaptureRecord { path: PathBuf::from(format!("/work/capture-{n}.pcap")), saved: 1 })
        .collect();
    let records = remembered(records, Path::new("/work/capture-5.pcap"), at);
    assert_eq!(records.len(), CAPTURE_RECORDS);
    assert_eq!(records[0], CaptureRecord { path: "/work/capture-5.pcap".into(), saved: 1_000 });
    assert_eq!(records.iter().filter(|record| record.path == Path::new("/work/capture-5.pcap")).count(), 1);

    let records = remembered(records, Path::new("/work/capture-new.pcap"), at);
    assert_eq!(records.len(), CAPTURE_RECORDS, "the oldest drops off the list");
    assert_eq!(records[0].path, Path::new("/work/capture-new.pcap"));
    assert!(
        !records.iter().any(|record| record.path == Path::new(&format!("/work/capture-{}.pcap", CAPTURE_RECORDS - 1)))
    );
}

#[test]
fn a_capture_that_took_in_its_keys_is_found_as_pcapng_with_its_key_log_and_a_deleted_one_is_not() {
    let temp = tempfile::tempdir().unwrap();
    let pcapng = temp.path().join("capture-1.pcapng");
    let keys = temp.path().join("capture-1.keylog");
    write(&pcapng);
    write(&keys);
    let record = CaptureRecord { path: temp.path().join("capture-1.pcap"), saved: 7 };
    let found = resolve(&record).unwrap();
    assert_eq!(found.path, pcapng);
    assert_eq!(found.keys.as_deref(), Some(keys.as_path()));
    assert_eq!(found.bytes, 8);
    assert!(resolve(&CaptureRecord { path: temp.path().join("capture-2.pcap"), saved: 7 }).is_none());
}

#[test]
fn cleaning_removes_files_and_folders_and_counts_one_already_gone_as_nothing() {
    let temp = tempfile::tempdir().unwrap();
    let folder = temp.path().join("1.1.0");
    write(&folder.join("integration.zsh"));
    let file = temp.path().join("cd-200");
    write(&file);
    let leftovers = [
        Leftover { path: folder.clone(), bytes: 4 },
        Leftover { path: file.clone(), bytes: 4 },
        Leftover { path: temp.path().join("gone"), bytes: 9 },
    ];
    let cleanup = clean(&leftovers, SystemTime::now());
    assert_eq!((cleanup.removed, cleanup.bytes, cleanup.failed), (2, 8, 0));
    assert!(!folder.exists() && !file.exists());
}

#[test]
fn a_socket_answers_while_its_process_listens() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("mirror-ab.sock");
    let listener = UnixListener::bind(&path).unwrap();
    assert!(answers(&path));
    drop(listener);
    assert!(!answers(&path), "the socket file stays, but nobody listens");

    let ours = temp.path().join(format!(
        "{}x-{}{}",
        crate::agent::SOCKET_PREFIX,
        std::process::id(),
        borg_core::SOCKET_SUFFIX
    ));
    write(&ours);
    assert!(answers(&ours), "a session's socket names its process, which runs");
}

#[test]
fn old_files_of_ended_borgs_are_left_over_on_disk_and_this_borgs_are_not() {
    let temp = tempfile::tempdir().unwrap();
    let runtime = temp.path().join("run");
    let old_version = runtime.join("0.1.0");
    write(&old_version.join("integration.zsh"));
    age(&old_version.join("integration.zsh"), OLD);
    age(&old_version, OLD);
    let ours = runtime.join(crate::update::VERSION);
    write(&ours.join("integration.zsh"));
    write(&ours.join(format!("{}{}", borg_term::CD_PREFIX, std::process::id())));
    age(&ours.join(format!("{}{}", borg_term::CD_PREFIX, std::process::id())), OLD);
    let ended = ours.join(format!("{}{}", borg_term::CD_PREFIX, u32::MAX - 1));
    write(&ended);
    age(&ended, OLD);
    let cache = temp.path().join("cache");
    let old_sizes = cache.join("sizes-ab.json");
    write(&old_sizes);
    age(&old_sizes, OLD);

    let places = Places { runtime: runtime.clone(), state: None, cache: Some(cache), config: None };
    let mut found: Vec<PathBuf> = leftovers(&places, false, &[], SystemTime::now())
        .into_iter()
        .map(|leftover| leftover.path)
        .filter(|path| path.starts_with(temp.path()))
        .collect();
    found.sort();
    let mut expected = vec![old_version, ended, old_sizes];
    expected.sort();
    assert_eq!(found, expected);
}

#[test]
fn the_report_measures_each_place_and_leaves_the_service_runtime_out_of_the_state() {
    let temp = tempfile::tempdir().unwrap();
    let state = temp.path().join("state");
    write(&state.join("session.toml"));
    write(&state.join(crate::service::SERVICE_RUNTIME).join("TTL"));
    let runtime = temp.path().join("run");
    write(&runtime.join("TTL"));
    let places = Places { runtime, state: Some(state), cache: None, config: None };
    let report = report(&places, &[], None);
    let usage = |place| report.places.iter().find(|usage| usage.place == place).unwrap();
    assert_eq!(report.places.len(), Place::ALL.len());
    assert_eq!((usage(Place::State).bytes, usage(Place::State).items), (4, 1));
    assert_eq!(usage(Place::Temporary).bytes, 8, "the service's runtime folder counts as temporary");
    assert_eq!(usage(Place::Caches).bytes, 0);
}
