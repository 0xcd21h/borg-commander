use super::*;
use crate::agent::server::Router;
use serde_json::json;
use std::{collections::HashMap, os::unix::fs::MetadataExt, path::Path};

#[test]
fn external_calls_cannot_claim_an_origin_or_prior_approval() {
    let forged: Call = serde_json::from_value(json!({
        "tool": "run", "args": { "command": "true" }, "origin": "trusted", "approved": true, "viewer": true
    }))
    .unwrap();
    let checked = forged.external().unwrap();
    assert!(checked.origin.is_none());
    assert!(!checked.approved);
    assert!(!checked.viewer, "only borg cloud marks a viewer's call");
}

#[test]
fn an_agent_reaches_only_the_tools_listed_to_it() {
    let temp = tempfile::tempdir().unwrap();
    let backend = server::Backend::Routing(Router {
        runtime_dir: temp.path().into(),
        state: None,
        selected: "local:default".into(),
        locals: HashMap::new(),
        remotes: HashMap::new(),
    });
    for listed in ["run", "edit", "download", "use_session", "approval_mode"] {
        assert!(backend.lists(listed), "{listed}");
    }
    for unlisted in ["mirror_take", "mirror_input", "console_input", "fs_list", "asks_answer", "_pair", ""] {
        assert!(!backend.lists(unlisted), "{unlisted} acts as the user and is not the agent's");
    }
}

#[test]
fn a_command_is_asked_about_only_when_the_dialog_shows_all_of_it() {
    for shown in ["ls -la", "cd app\ncargo test", "echo 'naïve — 雪'"] {
        assert!(shown_as_typed(shown), "{shown:?}");
    }
    for hidden in ["ls\tx", "ls\rx", "ls\u{1b}x", "ls\u{7f}x", "ls\u{85}x", "ls\u{0}x"] {
        assert!(!shown_as_typed(hidden), "{hidden:?} reaches the shell but not the dialog");
    }
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
fn a_socket_folder_that_links_elsewhere_is_refused() {
    let place = tempfile::tempdir().unwrap();
    let real = socket::owner_only(&place.path().join("real")).unwrap();
    let link = place.path().join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    assert!(socket::owner_only(&link).is_err());
    assert!(socket::owner_only(&real).is_ok());
}

#[tokio::test]
async fn a_socket_knows_a_program_of_this_user() {
    let (one, _other) = tokio::net::UnixStream::pair().unwrap();
    assert!(same_user(&one));
}

#[tokio::test]
async fn a_service_sessions_socket_trusts_only_the_service_that_started_it() {
    let place = tempfile::tempdir().unwrap();
    for (service, trusted) in [(std::os::unix::process::parent_id(), false), (std::process::id(), true)] {
        let path = place.path().join(format!("{service}.sock"));
        let listener = UnixListener::bind(&path).unwrap();
        let (calls, mut incoming) = unbounded_channel();
        let (messages, _shown) = unbounded_channel();
        tokio::spawn(accept(listener, calls, messages, Arc::new(AtomicUsize::new(0)), Some(service)));
        let mut client = UnixStream::connect(&path).await.unwrap();
        let call = json!({
            "tool": "run", "args": { "command": "true", "_container": "web" }, "origin": "laptop", "approved": true
        });
        client.write_all(format!("{call}\n").as_bytes()).await.unwrap();
        let (call, _reply) = incoming.recv().await.unwrap();
        assert_eq!(call.origin.is_some(), trusted, "{call:?}");
        assert_eq!(call.approved, trusted);
        assert_eq!(call.args.get("_container").is_some(), trusted);
    }
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
fn startup_items_read_newest_first_with_their_program_and_rules() {
    use borg_core::{
        ProgramPresence, RuleHit, StartupEntry, StartupFormat, StartupItem, StartupPlace, StartupReport, StartupSpot,
    };
    use std::time::{Duration, UNIX_EPOCH};
    let spot = StartupSpot {
        place: StartupPlace::UserAgent,
        format: StartupFormat::Launchd,
        path: "/home/sam/Library/LaunchAgents".into(),
        folder: true,
        extensions: &["plist"],
        read_only: false,
    };
    let entry = StartupEntry {
        name: "com.example.updater".into(),
        runs: "/tmp/.cache/updater --daemon".into(),
        program: Some("/tmp/.cache/updater".into()),
        when: Some("RunAtLoad".into()),
        ..StartupEntry::default()
    };
    let mut agent = StartupItem::new(&spot, "/home/sam/Library/LaunchAgents/com.example.updater.plist".into(), entry);
    agent.modified = Some(UNIX_EPOCH + Duration::from_secs(1_800_000_000));
    agent.owner = Some("sam".into());
    agent.presence = ProgramPresence::Missing;
    agent.hits = vec![RuleHit {
        rule: borg_rules::RuleMatch {
            rule: "launch_agent_from_temp".into(),
            namespace: borg_rules::BUILT_IN.into(),
            level: borg_rules::Level::Warning,
            description: None,
            false_positives: None,
            evidence: None,
        },
        in_program: false,
    }];
    let quiet = StartupItem::new(&spot, "/home/sam/Library/LaunchAgents/quiet.plist".into(), StartupEntry::default());
    let report = StartupReport {
        items: vec![agent, quiet],
        unread: vec![("/usr/lib/cron/tabs".into(), "Permission denied".into())],
        problems: Vec::new(),
    };
    let reply = borg_core_locale(|| startup_items(&report, "", 1));
    assert!(reply.ok);
    let lines: Vec<&str> = reply.text.lines().collect();
    assert_eq!(lines[0], "Startup items: 2, showing 1 · 1 matched a rule · newest change first");
    assert_eq!(lines[1], "Could not read /usr/lib/cron/tabs: Permission denied");
    assert!(
        lines[3].starts_with("2027-01-15T08:00:00Z  user-launch-agent     sam       missing  /home/sam/"),
        "{}",
        lines[3]
    );
    assert_eq!(lines[5], "  runs: /tmp/.cache/updater --daemon");
    assert_eq!(lines[7], "  when: RunAtLoad");
    assert!(lines[8].starts_with("  rule launch_agent_from_temp (warning, in its file): "), "{}", lines[8]);
    assert!(!reply.text.contains("quiet.plist"));
    assert!(startup_items(&report, "quiet", 5).text.contains("quiet.plist"));
}

fn borg_core_locale<T>(run: impl FnOnce() -> T) -> T {
    borg_i18n::with_locale(borg_i18n::Locale::English, run)
}

#[test]
fn a_read_names_a_packet_from_1() {
    assert_eq!(packet_number(&json!({})), Ok(None));
    assert_eq!(packet_number(&json!({ "packet": 12 })), Ok(Some(12)));
    assert!(packet_number(&json!({ "packet": 0 })).is_err());
    assert!(packet_number(&json!({ "packet": "first" })).is_err());
}

#[test]
fn screen_calls_an_archive_read_only_only_when_borg_cannot_change_it() {
    use borg_archive::{Compression, Format};
    let place = |name: &str, format: Option<Format>, inside: &str| {
        let archive = Path::new("/srv").join(name);
        let mount = borg_core::ArchiveMount {
            archive: archive.clone(),
            host: "/srv".into(),
            password: None,
            verified: false,
            reading: None,
            format,
        };
        answers::archive_place(&mount, &archive.join(inside))
    };

    assert_eq!(place("site.zip", Some(Format::Zip), "docs"), "inside the archive /srv/site.zip, at /docs");
    assert_eq!(
        place("site.tar.xz", Some(Format::Tar(Some(Compression::Xz))), ""),
        "inside the archive /srv/site.tar.xz, at /"
    );
    assert_eq!(place("site.rar", Some(Format::Rar), "docs"), "inside the archive /srv/site.rar, at /docs, read only");
    assert_eq!(
        place("log.gz", Some(Format::Compressed(Compression::Gzip)), ""),
        "inside the archive /srv/log.gz, at /, read only"
    );
    assert_eq!(place("big.7z", None, ""), "inside the archive /srv/big.7z, at /", "unknown until it opens");
}

#[tokio::test]
async fn an_ask_is_withdrawn_once_its_asker_stops_waiting() {
    let mut waiting = Waiting::default();
    let (pairing, given_up) = oneshot::channel();
    let (reading, still_waiting) = oneshot::channel();
    waiting.0.insert(1, (pairing, Work::Pair { name: "me@laptop".into() }));
    waiting.0.insert(2, (reading, Work::Pair { name: "you@desk".into() }));
    drop(given_up);
    assert_eq!(waiting.withdrawn().await, 1);
    assert!(!waiting.0.contains_key(&1), "the withdrawn ask is forgotten");
    let pending = tokio::time::timeout(std::time::Duration::from_millis(50), waiting.withdrawn()).await;
    assert!(pending.is_err(), "an ask whose asker still waits stays");
    drop(still_waiting);
}

#[tokio::test]
async fn a_change_reads_only_a_regular_file_and_stops_past_the_edit_limit() {
    let device = load(&Spot::Host("/dev/zero".into())).await.unwrap_err();
    assert!(format!("{device:#}").contains("not a regular file"), "{device:#}");

    let temp = tempfile::tempdir().unwrap();
    let large = temp.path().join("large.txt");
    std::fs::write(&large, "a".repeat(usize::try_from(files::EDIT_CAP).unwrap() + 1)).unwrap();
    let too_large = load(&Spot::Host(large)).await.unwrap_err();
    assert!(format!("{too_large:#}").contains("larger than"), "{too_large:#}");

    let small = temp.path().join("small.txt");
    std::fs::write(&small, "fits\n").unwrap();
    assert_eq!(load(&Spot::Host(small)).await.unwrap().as_deref(), Some("fits\n"));
    assert_eq!(load(&Spot::Host(temp.path().join("new.txt"))).await.unwrap(), None);
}

#[test]
fn an_agents_download_lands_only_in_the_downloads_folder_and_replaces_nothing() {
    let temp = tempfile::tempdir().unwrap();
    let downloads = std::fs::canonicalize(temp.path()).unwrap().join("Downloads");
    let reports = downloads.join("reports");
    std::fs::create_dir_all(&reports).unwrap();
    let elsewhere = temp.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::os::unix::fs::symlink(&elsewhere, downloads.join("away")).unwrap();
    std::fs::write(reports.join("taken.csv"), "kept").unwrap();
    let place = |to: Option<&str>| server::download_destination(&downloads, to, "/srv/menu/prices.csv");

    assert_eq!(place(None), Ok(downloads.join("prices.csv")));
    assert_eq!(place(Some("reports")), Ok(reports.join("prices.csv")));
    assert_eq!(place(Some("reports/march.csv")), Ok(reports.join("march.csv")));
    assert_eq!(place(Some(&reports.join("april.csv").to_string_lossy())), Ok(reports.join("april.csv")));
    for refused in ["../prices.csv", "/etc/prices.csv", "reports/../../prices.csv", "away", "away/prices.csv"] {
        assert!(place(Some(refused)).is_err(), "{refused} leaves the Downloads folder");
    }
    assert!(place(Some("reports/taken.csv")).is_err(), "an existing file stays");
    assert_eq!(std::fs::read_to_string(reports.join("taken.csv")).unwrap(), "kept");
}
