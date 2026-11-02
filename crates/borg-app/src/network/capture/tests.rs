use super::*;

#[test]
fn a_denied_interface_says_so() {
    let macos = "tcpdump: en0: You don't have permission to capture on that device\n\
                 ((cannot open BPF device) /dev/bpf0: Permission denied)\n";
    let linux = "tcpdump: eth0: You don't have permission to perform this capture on that device\n\
                 (socket: Operation not permitted)\n";
    assert_eq!(explain(macos, "en0"), tr!(capture_denied, interface = "en0"));
    assert_eq!(explain(linux, "eth0"), tr!(capture_denied, interface = "eth0"));
}

#[test]
fn another_failure_gives_tcpdump_reason() {
    let errors = "tcpdump: listening on en0, link-type EN10MB (Ethernet)\ntcpdump: syntax error in filter\n";
    assert_eq!(explain(errors, "en0"), tr!(capture_failed, error = "syntax error in filter"));
}

#[test]
fn the_command_keeps_headers_only_when_asked() {
    let request = CaptureRequest {
        interface: "en0".into(),
        seconds: Some(5),
        headers_only: true,
        filter: "tcp port 443".into(),
        folder: PathBuf::from("/tmp"),
    };
    let command = command(Path::new("/usr/sbin/tcpdump"), &request);
    let args: Vec<String> = command.as_std().get_args().map(|arg| arg.to_string_lossy().into_owned()).collect();
    let snap = HEADER_BYTES.to_string();
    let limit = PACKET_LIMIT.to_string();
    assert_eq!(args, ["-i", "en0", "-U", "-n", "-s", &snap, "-c", &limit, "-w", "-", "tcp port 443"]);
}
