use super::*;
use borg_core::CaptureScope;
use std::{
    net::SocketAddr,
    os::unix::{fs::PermissionsExt, process::ExitStatusExt},
};
use tokio::io::AsyncReadExt;

fn tcpdump(filter: String) -> Tcpdump {
    Tcpdump { interface: "en0".into(), headers_only: false, processes: false, filter }
}

#[test]
fn the_helper_takes_back_every_filter_the_screen_makes() {
    let connection = |local: &str, remote: &str| CaptureScope::Connection {
        transport: borg_core::Transport::Tcp,
        local: local.parse::<SocketAddr>().unwrap(),
        remote: remote.parse::<SocketAddr>().unwrap(),
    };
    let scopes = [
        CaptureScope::Port { transport: borg_core::Transport::Udp, port: 5353 },
        connection("192.0.2.10:50123", "203.0.113.9:443"),
        connection("[2001:db8::1]:50124", "[2001:db8:ffff::2]:443"),
        CaptureScope::Process { pid: 8239 },
        CaptureScope::Everything,
    ];
    for scope in scopes {
        let asked = Tcpdump { headers_only: true, ..tcpdump(scope.filter()) };
        assert_eq!(Tcpdump::parse(&asked.helper_args()), Some(asked.clone()), "{scope:?}");
    }
}

#[test]
fn the_helper_refuses_anything_else() {
    let args = |words: &[&str]| words.iter().map(|word| (*word).to_owned()).collect::<Vec<_>>();
    for refused in [
        args(&["-zrm", "0", "0"]),
        args(&["pktap,en0", "0", "0"]),
        args(&["en0;id", "0", "0"]),
        args(&["en0", "2", "0"]),
        args(&["en0", "0", "0", "-z /tmp/x"]),
        args(&["en0", "0", "0", "port 1 `id`"]),
        args(&["en0", "0", "0", "port 1", "more"]),
        args(&["en0", "0"]),
    ] {
        assert_eq!(Tcpdump::parse(&refused), None, "{refused:?}");
    }
    let tagged = args(&["en0", "0", "1"]);
    assert_eq!(Tcpdump::parse(&tagged).is_some(), cfg!(target_os = "macos"), "pktap is macOS's");
}

#[test]
fn a_cancelled_dialog_is_quiet_and_a_refusal_gives_the_systems_reason() {
    let failed = Some(ExitStatus::from_raw(1 << 8));
    let osascript = "0:178: execution error: User canceled. (-128)\n";
    assert_eq!(refused(failed, osascript), CaptureFailure::Cancelled);
    assert_eq!(refused(Some(ExitStatus::from_raw(126 << 8)), ""), CaptureFailure::Cancelled);
    let headless = "0:178: execution error: No user interaction allowed. (-1713)\n";
    let reason = tr!(capture_admin_unavailable, error = "No user interaction allowed. (-1713)");
    assert_eq!(refused(failed, headless), CaptureFailure::Failed(reason));
    let sudo = "sudo: a password is required\n";
    let reason = tr!(capture_admin_unavailable, error = "sudo: a password is required");
    assert_eq!(outcome("", failed, sudo), Err(CaptureFailure::Failed(reason)));
    let listening = "{\"Ok\":\"listening on en0\"}\n";
    assert_eq!(outcome(listening, Some(ExitStatus::from_raw(0)), ""), Ok("listening on en0".into()));
    assert_eq!(outcome("{\"Err\":\"no tcpdump\"}", None, ""), Err(CaptureFailure::Failed("no tcpdump".into())));
}

/// A stand-in for tcpdump that writes a header, then waits for Ctrl-C and writes its last bytes.
const STAND_IN: &str = "#!/bin/sh\ntrap 'printf LAST; exit 0' INT\nprintf HEAD\nwhile :; do sleep 0.05; done\n";
const OWNER_ONLY: u32 = 0o700;

#[tokio::test]
async fn closing_borgs_side_interrupts_tcpdump_and_its_last_bytes_still_arrive() {
    let folder = tempfile::tempdir().unwrap();
    let program = folder.path().join(Tool::Tcpdump.program());
    std::fs::write(&program, STAND_IN).unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(OWNER_ONLY)).unwrap();
    let socket = folder.path().join(SOCKET);
    let listener = UnixListener::bind(&socket).unwrap();
    let helper = tokio::spawn({
        let socket = socket.clone();
        async move { run(&socket, &tcpdump(String::new()), &program).await.map_err(|error| error.to_string()) }
    });
    let (stream, _) = listener.accept().await.unwrap();
    let (mut reader, writer) = stream.into_split();
    let mut head = [0; 4];
    reader.read_exact(&mut head).await.unwrap();
    assert_eq!(&head, b"HEAD");
    drop(writer);
    let mut rest = Vec::new();
    tokio::time::timeout(STOP_GRACE, reader.read_to_end(&mut rest)).await.unwrap().unwrap();
    assert_eq!(rest, b"LAST", "tcpdump finished what it was writing, and the stream ended");
    assert_eq!(helper.await.unwrap(), Ok(String::new()));
}

#[test]
fn the_helper_takes_what_listening_asks_for() {
    let listening = Tcpdump::listening("en0");
    assert_eq!(Tcpdump::parse(&listening.helper_args()), Some(listening));
}
