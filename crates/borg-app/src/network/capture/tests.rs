use super::*;
use std::os::unix::fs::PermissionsExt;

fn request(filter: &str) -> CaptureRequest {
    CaptureRequest {
        interface: "en0".into(),
        seconds: Some(5),
        keep: Keep::Headers,
        filter: filter.into(),
        folder: PathBuf::from("/tmp"),
        administrator: false,
        processes: false,
        tls_keys: borg_core::TlsKeys::Off,
    }
}

#[test]
fn a_denied_interface_says_so() {
    let macos = "tcpdump: en0: You don't have permission to capture on that device\n\
                 ((cannot open BPF device) /dev/bpf0: Permission denied)\n";
    let linux = "tcpdump: eth0: You don't have permission to perform this capture on that device\n\
                 (socket: Operation not permitted)\n";
    assert_eq!(explain(macos), CaptureFailure::Denied);
    assert_eq!(explain(linux), CaptureFailure::Denied);
}

#[test]
fn an_agents_refused_capture_says_there_is_no_permission() {
    let request = request("");
    assert_eq!(refused(CaptureFailure::Denied, &request), tr!(capture_denied, interface = "en0"));
    assert_eq!(refused(CaptureFailure::Failed("tcpdump stopped".into()), &request), "tcpdump stopped");
    assert_eq!(refused(CaptureFailure::NoRoom, &request), capture_no_room(Path::new("/tmp")));
}

#[test]
fn another_failure_gives_tcpdump_reason() {
    let errors = "tcpdump: listening on en0, link-type EN10MB (Ethernet)\ntcpdump: syntax error in filter\n";
    assert_eq!(explain(errors), CaptureFailure::Failed(tr!(capture_failed, error = "syntax error in filter")));
}

#[test]
fn the_command_keeps_headers_only_when_asked_and_ends_the_options_before_the_filter() {
    let command = command(Path::new("/usr/sbin/tcpdump"), &Tcpdump::of(&request("tcp port 443")));
    let args: Vec<String> = command.as_std().get_args().map(|arg| arg.to_string_lossy().into_owned()).collect();
    let snap = HEADER_BYTES.to_string();
    let limit = PACKET_LIMIT.to_string();
    assert_eq!(args, ["-i", "en0", "-U", "-n", "-s", &snap, "-c", &limit, "-w", "-", "--", "tcp port 443"]);
    for keep in [Keep::Whole, Keep::ConnectionStart] {
        let whole = Tcpdump::of(&CaptureRequest { keep, ..request("") }).args();
        assert_eq!(whole[4..6], ["-s", "0"], "{keep:?} takes whole packets from tcpdump");
    }
}

#[test]
fn the_file_gets_what_tcpdump_wrote_while_each_connection_is_within_its_budget() {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("capture-20260101-120000.pcap");
    let pcap = borg_testkit::capture::bytes();
    let packets = Capture::read(&pcap).unwrap().packets.len();
    for keep in borg_core::KEEP_CHOICES {
        let mut saving = Saving::new(File::create(&path).unwrap(), keep);
        pcap.chunks(100).for_each(|chunk| saving.write(chunk).unwrap());
        saving.finish().unwrap();
        assert_eq!((saving.written, saving.counter.packets), (pcap.len() as u64, packets), "{keep:?}");
        assert_eq!(std::fs::read(&path).unwrap(), pcap, "{keep:?}");
    }
}

#[test]
fn the_room_left_is_what_the_disk_has_free_beyond_the_reserve() {
    let folder = tempfile::tempdir().unwrap();
    let free = || {
        let stats = rustix::fs::statvfs(folder.path()).unwrap();
        stats.f_bavail * stats.f_frsize
    };
    let before = free();
    let left = room(folder.path()).unwrap();
    let most = before.max(free()).saturating_sub(CAPTURE_DISK_RESERVE);
    assert!(left <= most, "{left} left of {before} free");
    assert_eq!(room(&folder.path().join("missing")), None);
}

#[test]
fn a_capture_with_processes_reads_pktap_and_saves_pcapng() {
    let tagged = CaptureRequest { processes: true, ..request("") };
    assert_eq!(&Tcpdump::of(&tagged).args()[..2], ["-i", "pktap,en0"]);
    assert!(file_name(&tagged).starts_with(FILE_PREFIX) && file_name(&tagged).ends_with(".pcapng"));
    assert!(file_name(&request("")).ends_with(".pcap"));
}

#[test]
fn a_capture_opens_with_the_key_log_named_after_it() {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("capture-20260101-120000.pcap");
    std::fs::write(&path, borg_testkit::capture::bytes()).unwrap();
    assert!(read_capture(&path).unwrap().keys.is_empty());
    let line = format!("CLIENT_RANDOM {} {}\n", "5a".repeat(32), "01".repeat(48));
    std::fs::write(path.with_extension(KEY_LOG_EXTENSION), line).unwrap();
    assert_eq!(read_capture(&path).unwrap().keys.len(), 1);
}

#[test]
fn a_capture_that_kept_tls_keys_holds_them_as_pcapng_and_keeps_the_key_log() {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("capture-20260101-120000.pcap");
    let (pcap, key_log) = borg_testkit::capture::shop_session();
    std::fs::write(&path, pcap).unwrap();
    let unrelated = format!("CLIENT_RANDOM {} {}\n", "5a".repeat(32), "01".repeat(48));
    let key_log_path = path.with_extension(KEY_LOG_EXTENSION);
    std::fs::write(&key_log_path, format!("{unrelated}{key_log}")).unwrap();

    let (saved, capture) = read_saved(path.clone(), true).unwrap();
    assert_eq!(saved, path.with_extension("pcapng"));
    assert!(!path.exists() && key_log_path.exists(), "the pcap is replaced and the key log stays");
    assert_eq!(std::fs::metadata(&saved).unwrap().permissions().mode() & 0o777, FILE_MODE);
    assert_eq!(capture.keys.len(), 2);

    std::fs::remove_file(&key_log_path).unwrap();
    let embedded = read_capture(&saved).unwrap();
    assert_eq!(embedded.keys.len(), 1, "only the capture's own connection goes in the file");
    assert_eq!(embedded.packets.len(), capture.packets.len());
}

#[test]
fn a_pcapng_capture_gets_its_keys_in_place_and_one_without_keys_stays_as_it_is() {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("capture-20260101-120000.pcapng");
    let (pcap, key_log) = borg_testkit::capture::shop_session();
    std::fs::write(&path, borg_core::with_secrets(&pcap, "").unwrap()).unwrap();
    std::fs::write(path.with_extension(KEY_LOG_EXTENSION), "").unwrap();
    let before = std::fs::read(&path).unwrap();
    assert_eq!(read_saved(path.clone(), true).unwrap().0, path);
    assert_eq!(std::fs::read(&path).unwrap(), before, "no key belongs to it");

    std::fs::write(path.with_extension(KEY_LOG_EXTENSION), key_log).unwrap();
    assert_eq!(read_saved(path.clone(), true).unwrap().0, path);
    std::fs::remove_file(path.with_extension(KEY_LOG_EXTENSION)).unwrap();
    assert_eq!(read_capture(&path).unwrap().keys.len(), 1);
    assert!(!path.with_added_extension(STAGED_EXTENSION).exists());
}

#[test]
fn a_capture_that_saved_nothing_takes_its_unused_key_log_along() {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("capture-20260101-120000.pcap");
    let (_, key_log) = create_files(&path, true).unwrap();
    let key_log = key_log.unwrap();
    assert_eq!(std::fs::metadata(&key_log).unwrap().permissions().mode() & 0o777, FILE_MODE);
    discard(&path, Some(&key_log));
    assert!(!path.exists() && !key_log.exists());

    let (_, written) = create_files(&path, true).unwrap();
    std::fs::write(written.as_deref().unwrap(), "CLIENT_RANDOM 00 00\n").unwrap();
    discard(&path, written.as_deref());
    assert!(written.unwrap().exists(), "a program wrote keys to it");
    assert!(create_files(&path, false).unwrap().1.is_none());
}
