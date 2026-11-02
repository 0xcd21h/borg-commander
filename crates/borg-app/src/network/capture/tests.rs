use super::*;

fn request(filter: &str) -> CaptureRequest {
    CaptureRequest {
        interface: "en0".into(),
        seconds: Some(5),
        headers_only: true,
        filter: filter.into(),
        folder: PathBuf::from("/tmp"),
        administrator: false,
        processes: false,
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
}

#[test]
fn a_capture_with_processes_reads_pktap_and_saves_pcapng() {
    let tagged = CaptureRequest { processes: true, ..request("") };
    assert_eq!(&Tcpdump::of(&tagged).args()[..2], ["-i", "pktap,en0"]);
    assert!(file_name(&tagged).starts_with(FILE_PREFIX) && file_name(&tagged).ends_with(".pcapng"));
    assert!(file_name(&request("")).ends_with(".pcap"));
}
