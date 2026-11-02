use super::*;
use crate::agent::server::Router;
use serde_json::json;
use std::{collections::HashMap, os::unix::fs::MetadataExt, path::Path};

#[test]
fn external_calls_cannot_claim_an_origin_or_prior_approval() {
    let forged: Call = serde_json::from_value(json!({
        "tool": "run", "args": { "command": "true" }, "origin": "trusted", "approved": true
    }))
    .unwrap();
    let checked = forged.external().unwrap();
    assert!(checked.origin.is_none());
    assert!(!checked.approved);
}

#[test]
fn router_resolves_named_local_sessions_without_redirecting_other_connections() {
    let temp = tempfile::tempdir().unwrap();
    let state = crate::session::StateDir::at(temp.path().join("state"));
    let _ = crate::sessions::ensure_default(&state).unwrap();
    let prod = crate::sessions::create(&state, "prod").unwrap();
    let router = Router {
        runtime_dir: temp.path().into(),
        state: Some(state.path().into()),
        selected: "local:default".into(),
        locals: HashMap::new(),
        remotes: HashMap::new(),
    };
    assert_eq!(router.resolve("prod"), Ok(format!("local:{}", prod.id)));
    assert_eq!(router.resolve(&format!("local:{}", prod.id)), Ok(format!("local:{}", prod.id)));
    assert!(router.resolve("missing").is_err());
    assert_eq!(router.selected, "local:default");
}

#[test]
fn setup_connects_over_ssh_with_the_installers_folder_on_the_path() {
    let text = setup(Some("prod"));
    assert!(
        text.contains(
            r#"claude mcp add borg-prod -- ssh -o BatchMode=yes prod 'PATH="$HOME/.local/bin:$PATH" borg mcp'"#
        ),
        "{text}"
    );
    assert!(text.contains(r#""command": "ssh""#) && text.contains(r#""borg-prod""#), "{text}");
}

#[test]
fn shell_output_runs_from_the_command_start_to_its_end() {
    let bash = b"\x15ls -l\r\n\x1b]133;C\x07total 0\r\n\x1b[1;34mbin\x1b[0m\r\n\x1b]133;D;0\x07\x1b]7;file://box/\x07\x1b]133;A\x07$ ";
    assert_eq!(shell_output(bash, "ls -l"), "total 0\nbin\n");
    let sh = b"\x1b[7m web \x1b[0m $ \x1b]133;B\x07\r\n\x1b[7m web \x1b[0m $ echo hi\r\nhi\r\n\x1b]133;D;0\x07\x1b]133;A\x07/ # ";
    assert_eq!(shell_output(sh, "echo hi"), "hi\n", "without a command-start mark, the echoed command is left out");
    assert_eq!(shell_output(b"true\r\n\x1b]133;D;0\x07", "true"), "");
}

#[test]
fn containers_are_found_by_name_service_or_id() {
    let container = |id: &str, name: &str, service: Option<&str>| borg_docker::Container {
        id: id.into(),
        name: name.into(),
        image: String::new(),
        command: String::new(),
        created: String::new(),
        state: borg_docker::State::Running,
        status: String::new(),
        ports: String::new(),
        networks: String::new(),
        project: None,
        service: service.map(str::to_owned),
        project_dir: None,
        config_files: Vec::new(),
    };
    let all = [container("a1b2", "shop-web-1", Some("web")), container("c3d4", "blog-web-1", Some("web"))];
    assert_eq!(find_container(&all, "shop-web-1").map(|c| c.id.as_str()), Some("a1b2"));
    assert_eq!(find_container(&all, "c3"), Some(&all[1]));
    assert!(find_container(&all, "web").is_none(), "two services named web are ambiguous");
}

#[test]
fn socket_folders_are_short_and_owner_only() {
    let one = socket_dir(Path::new("/tmp/borg-test-a")).unwrap();
    assert!(one.join("session-4294967295.sock").as_os_str().len() < 100);
    assert_eq!(std::fs::metadata(&one).unwrap().mode() & 0o777, 0o700);
}

#[test]
fn a_capture_call_names_one_scope_and_a_bounded_length() {
    use borg_core::{CaptureScope, LONGEST_CAPTURE, SHORTEST_CAPTURE, Transport};
    let everything = capture_call(&json!({})).unwrap();
    assert_eq!(
        everything,
        capture::CaptureCall {
            scope: CaptureScope::Everything,
            interface: None,
            seconds: SHORTEST_CAPTURE,
            headers_only: false,
            limit: capture::SUMMARY_ROWS,
        }
    );
    let port = capture_call(&json!({ "port": 53, "transport": "udp", "interface": "en0", "seconds": 30 })).unwrap();
    assert_eq!(port.scope, CaptureScope::Port { transport: Transport::Udp, port: 53 });
    assert_eq!((port.interface.as_deref(), port.seconds), (Some("en0"), 30));
    let connection = capture_call(&json!({ "local": "192.0.2.10:50123", "remote": "203.0.113.9:443" })).unwrap();
    assert_eq!(
        connection.scope,
        CaptureScope::Connection {
            transport: Transport::Tcp,
            local: "192.0.2.10:50123".parse().unwrap(),
            remote: "203.0.113.9:443".parse().unwrap(),
        }
    );
    let process = capture_call(&json!({ "pid": 4100, "headers_only": true, "limit": 5 })).unwrap();
    assert_eq!((process.scope, process.headers_only, process.limit), (CaptureScope::Process { pid: 4100 }, true, 5));

    for (args, error) in [
        (json!({ "port": 53, "pid": 4100 }), "one scope"),
        (json!({ "port": 70000 }), "from 1 to 65535"),
        (json!({ "transport": "sctp" }), "no transport sctp"),
        (json!({ "local": "192.0.2.10:50123" }), "both be addresses"),
        (json!({ "pid": 0 }), "PID of a running process"),
        (json!({ "seconds": LONGEST_CAPTURE + 1 }), "from 1 to 60"),
        (json!({ "seconds": 0 }), "from 1 to 60"),
    ] {
        let refused = capture_call(&args).unwrap_err();
        assert!(refused.contains(error), "{args}: {refused}");
    }
}

#[test]
fn a_read_names_a_packet_from_1() {
    assert_eq!(packet_number(&json!({})), Ok(None));
    assert_eq!(packet_number(&json!({ "packet": 12 })), Ok(Some(12)));
    assert!(packet_number(&json!({ "packet": 0 })).is_err());
    assert!(packet_number(&json!({ "packet": "first" })).is_err());
}
