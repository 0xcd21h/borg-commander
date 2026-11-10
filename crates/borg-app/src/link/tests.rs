use super::*;
use anyhow::Result;
use borg_link::hex;
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

#[test]
fn paired_client_status_counts_connections_per_identity_and_cleans_up_on_drop() {
    let computer = Computer { name: "laptop".into(), key: hex(&[7; 32]), changed: None };
    let shared = Arc::new(Mutex::new(Shared {
        computers: vec![computer.clone(), Computer { name: "offline".into(), key: hex(&[8; 32]), changed: None }],
        pairing: None,
        connections: Default::default(),
    }));
    let clients = Arc::new(AtomicUsize::new(0));
    let (messages, _) = tokio::sync::mpsc::unbounded_channel();
    let first = ClientConnection::new(shared.clone(), clients.clone(), messages.clone(), computer.clone());
    let second = ClientConnection::new(shared.clone(), clients.clone(), messages, computer);
    let status = client_status(&shared.lock().unwrap());
    assert_eq!(status.len(), 2);
    assert_eq!(status[0].connections, Some(2));
    assert_eq!(status[1].connections, Some(0));
    assert_ne!(status[0].fingerprint, status[1].fingerprint);
    drop(first);
    assert_eq!(client_status(&shared.lock().unwrap())[0].connections, Some(1));
    drop(second);
    assert_eq!(client_status(&shared.lock().unwrap())[0].connections, Some(0));
    assert_eq!(clients.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn paired_clients_remain_visible_when_the_status_owner_is_unavailable() -> Result<()> {
    let state = tempfile::tempdir()?;
    save(
        &state.path().join(LINK_DIR).join(COMPUTERS_FILE),
        &[Computer { name: "laptop".into(), key: hex(&[7; 32]), changed: None }],
    )?;
    let clients = paired_clients(state.path()).await;
    assert_eq!(clients.len(), 1);
    assert_eq!(clients[0].name, "laptop");
    assert_eq!(clients[0].connections, None);
    Ok(())
}

#[test]
fn one_borg_on_a_machine_has_borg_cloud_and_others_say_which() {
    let state = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(state.path().join(LINK_DIR)).unwrap();
    let first = claim(state.path()).unwrap();
    let error = claim(state.path()).unwrap_err().to_string();
    assert!(error.contains(&format!("PID {}", std::process::id())), "{error}");
    drop(first);
    // A process another test forks holds a copy of the file until it runs its program.
    let deadline = Instant::now() + Duration::from_secs(5);
    while claim(state.path()).is_err() {
        assert!(Instant::now() < deadline, "the claim ends with the borg that held it");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn secrets_compare_whole_and_names_stay_short_and_printable() {
    assert!(same(b"abcd", b"abcd") && !same(b"abcd", b"abce") && !same(b"abc", b"abcd"));
    assert_eq!(clean_name("  dev\u{7}box  "), "devbox");
    assert_eq!(clean_name(""), "computer");
    assert_eq!(clean_name(&"x".repeat(100)).len(), NAME_LIMIT);
}

mod rotation {
    use super::super::{
        keys::{computer_info, computer_keys, forget_cached, takes_old_key},
        remote::connect,
    };
    use super::*;
    use borg_link::{Identity, PublicKey, Relay, fingerprint};
    use serde_json::json;
    use std::path::Path;

    struct Stage {
        relay: Relay,
        server: Identity,
        server_state: tempfile::TempDir,
        shared: Arc<Mutex<Shared>>,
        notices: tokio::sync::mpsc::UnboundedReceiver<borg_core::Msg>,
        _served: tokio::task::JoinHandle<()>,
    }

    async fn relay() -> Relay {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(borg_relay::serve(listener, borg_relay::Options::default()));
        Relay::parse(&format!("ws://{address}")).unwrap()
    }

    fn computer_state(old: Option<&Identity>) -> tempfile::TempDir {
        let state = tempfile::tempdir().unwrap();
        let link = state.path().join(LINK_DIR);
        std::fs::create_dir_all(&link).unwrap();
        if let Some(old) = old {
            use std::os::unix::fs::OpenOptionsExt;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(PRIVATE_FILE)
                .open(link.join(KEY_FILE))
                .unwrap();
            std::io::Write::write_all(&mut file, old.to_text().as_bytes()).unwrap();
        }
        state
    }

    fn pair_with(computer: &Path, stage: &Stage) -> Server {
        let server = Server {
            name: "box".into(),
            relay: stage.relay.to_string(),
            key: hex(stage.server.public()),
            computer: None,
            offered: None,
        };
        save(&computer.join(LINK_DIR).join(SERVERS_FILE), std::slice::from_ref(&server)).unwrap();
        server
    }

    fn saved(computer: &Path) -> Server {
        load::<Server>(&computer.join(LINK_DIR).join(SERVERS_FILE)).remove(0)
    }

    async fn serve(relay: Relay, server: Identity, computers: Vec<Computer>) -> Stage {
        let server_state = tempfile::tempdir().unwrap();
        save(&server_state.path().join(LINK_DIR).join(COMPUTERS_FILE), &computers).unwrap();
        let shared = Arc::new(Mutex::new(Shared { computers, pairing: None, connections: Default::default() }));
        let (calls, mut asked) = tokio::sync::mpsc::unbounded_channel::<(
            crate::agent::Call,
            tokio::sync::oneshot::Sender<crate::agent::Reply>,
        )>();
        tokio::spawn(async move {
            while let Some((call, reply)) = asked.recv().await {
                let _ = reply.send(crate::agent::Reply::ok(call.tool));
            }
        });
        let (messages, notices) = tokio::sync::mpsc::unbounded_channel();
        let served = served::Served {
            background_service: false,
            relay: relay.clone(),
            identity: server.clone(),
            shared: shared.clone(),
            state: Some(server_state.path().to_path_buf()),
            runtime_dir: std::env::temp_dir(),
            session_id: None,
            calls,
            messages,
            clients: Arc::new(AtomicUsize::new(0)),
        };
        let served = tokio::spawn(async move { served.listen().await });
        let stage = Stage { relay, server, server_state, shared, notices, _served: served };
        stage.wait_registered().await;
        stage
    }

    impl Stage {
        async fn wait_registered(&self) {
            let deadline = Instant::now() + Duration::from_secs(10);
            while borg_link::relay::dial(&self.relay, self.server.public()).await.is_err() {
                assert!(Instant::now() < deadline, "the server registers at the relay");
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }

        fn accepted(&self) -> Vec<String> {
            self.shared.lock().unwrap().computers.iter().map(|computer| computer.key.clone()).collect()
        }

        async fn opens(&self, identity: &Identity, hello: &Hello) -> bool {
            let hello = serde_json::to_vec(hello).unwrap();
            connect(&self.relay, self.server.public(), identity, &hello, "box").await.unwrap().is_some()
        }

        fn changes_seen(&mut self) -> Vec<String> {
            let mut seen = Vec::new();
            while let Ok(message) = self.notices.try_recv() {
                if let borg_core::Msg::Notice(text) = message {
                    seen.push(text);
                }
            }
            seen
        }
    }

    fn laptop(key: &PublicKey) -> Computer {
        Computer { name: "laptop".into(), key: hex(key), changed: None }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_computer_moves_to_a_key_of_its_own_and_the_server_refuses_the_shared_one() {
        let old = Identity::generate();
        let mut stage = serve(relay().await, Identity::generate(), vec![laptop(old.public())]).await;
        let computer = computer_state(Some(&old));
        let server = pair_with(computer.path(), &stage);

        Remote::open(computer.path(), &server).await.unwrap();

        let keys = computer_keys(computer.path()).unwrap();
        assert_ne!(keys.current, old, "the computer key is a new one");
        let new = hex(keys.current.public());
        assert_eq!(stage.accepted(), vec![new.clone()]);
        let on_disk: Vec<Computer> = load(&stage.server_state.path().join(LINK_DIR).join(COMPUTERS_FILE));
        assert_eq!(on_disk[0].key, new, "the swap is saved");
        assert_eq!(on_disk[0].name, "laptop", "the entry keeps its name");
        assert_eq!(on_disk[0].changed.as_ref().map(|change| change.from.clone()), Some(hex(old.public())));
        assert_eq!(saved(computer.path()).computer, Some(new));
        assert_eq!(saved(computer.path()).offered, None);
        assert!(!takes_old_key(&saved(computer.path()), computer_info(computer.path()).as_ref()));

        let notices = stage.changes_seen();
        assert!(
            notices.iter().any(|notice| notice.contains(&fingerprint(old.public()))
                && notice.contains(&fingerprint(keys.current.public()))),
            "the server shows the change with both fingerprints: {notices:?}"
        );
        assert!(!stage.opens(&old, &Hello::Session).await, "the shared key opens nothing any more");
        assert!(stage.opens(&keys.current, &Hello::Session).await);
        assert!(identity(computer.path()).unwrap() == old, "the shared key stays as this machine's server key");
        Remote::open(computer.path(), &server).await.unwrap();
        assert_eq!(stage.accepted(), vec![hex(keys.current.public())], "one swap");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_swap_whose_answer_was_lost_is_settled_by_the_next_connection() {
        let old = Identity::generate();
        let stage = serve(relay().await, Identity::generate(), vec![laptop(old.public())]).await;
        let computer = computer_state(Some(&old));
        let server = pair_with(computer.path(), &stage);
        Remote::open(computer.path(), &server).await.unwrap();
        let new = hex(computer_keys(computer.path()).unwrap().current.public());

        for offered in [Some(new.clone()), None] {
            let lost = Server { computer: Some(hex(old.public())), offered, ..server.clone() };
            save(&computer.path().join(LINK_DIR).join(SERVERS_FILE), std::slice::from_ref(&lost)).unwrap();
            Remote::open(computer.path(), &lost).await.unwrap();
            assert_eq!(saved(computer.path()).computer, Some(new.clone()));
            assert_eq!(saved(computer.path()).offered, None);
            assert_eq!(stage.accepted(), vec![new.clone()]);
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn connections_that_rotate_at_once_swap_the_key_once_and_all_open() {
        let old = Identity::generate();
        let stage = serve(relay().await, Identity::generate(), vec![laptop(old.public())]).await;
        let computer = computer_state(Some(&old));
        let server = pair_with(computer.path(), &stage);
        let opened = futures::future::join_all((0..4).map(|_| Remote::open(computer.path(), &server))).await;
        for remote in opened {
            let mut remote = remote.unwrap();
            assert!(remote.call(&crate::agent::Call::new("host_info", json!({}))).await.unwrap().ok);
        }
        assert_eq!(stage.accepted(), vec![hex(computer_keys(computer.path()).unwrap().current.public())]);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_server_without_rotation_keeps_the_old_key_and_hears_no_rotate() {
        let relay = relay().await;
        let server = Identity::generate();
        let heard = Arc::new(Mutex::new(Vec::<String>::new()));
        let older = {
            let (relay, server, heard) = (relay.clone(), server.clone(), heard.clone());
            tokio::spawn(async move {
                let mut listener = borg_link::relay::Listener::register(&relay, &server).await.unwrap();
                while let Ok(Some(id)) = listener.next().await {
                    let (relay, server, heard) = (relay.clone(), server.clone(), heard.clone());
                    tokio::spawn(async move {
                        let socket = borg_link::relay::take(&relay, &id).await.unwrap();
                        let (mut channel, _) = borg_link::respond(socket, &server).await.unwrap();
                        channel.send(br#"{"ok":true,"name":"older"}"#).await.unwrap();
                        while let Ok(Some(message)) = channel.recv().await {
                            let call: crate::agent::Call = serde_json::from_slice(&message).unwrap();
                            heard.lock().unwrap().push(call.tool.clone());
                            let reply = crate::agent::Reply::error(format!("unknown tool {}", call.tool));
                            channel.send(&serde_json::to_vec(&reply).unwrap()).await.unwrap();
                        }
                    });
                }
            })
        };
        let old = Identity::generate();
        let computer = computer_state(Some(&old));
        let entry = Server {
            name: "box".into(),
            relay: relay.to_string(),
            key: hex(server.public()),
            computer: None,
            offered: None,
        };
        save(&computer.path().join(LINK_DIR).join(SERVERS_FILE), std::slice::from_ref(&entry)).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut remote = loop {
            match Remote::open(computer.path(), &entry).await {
                Ok(remote) => break remote,
                Err(error) => assert!(Instant::now() < deadline, "{error:#}"),
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        remote.call(&crate::agent::Call::new("host_info", json!({}))).await.unwrap();
        assert_eq!(*heard.lock().unwrap(), vec!["host_info".to_owned()], "no rotate reaches an older server");
        assert_eq!(saved(computer.path()).computer, Some(hex(old.public())));
        assert!(takes_old_key(&saved(computer.path()), computer_info(computer.path()).as_ref()));
        older.abort();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_thief_who_rotated_first_leaves_the_computer_a_message_to_pair_again() {
        let old = Identity::generate();
        let stage = serve(relay().await, Identity::generate(), vec![laptop(old.public())]).await;
        let thief = computer_state(Some(&old));
        let computer = computer_state(Some(&old));
        let server = pair_with(computer.path(), &stage);
        pair_with(thief.path(), &stage);

        Remote::open(thief.path(), &server).await.unwrap();
        let error = Remote::open(computer.path(), &server).await.err().unwrap();
        assert_eq!(error.to_string(), borg_i18n::tr!(cloud_key_refused, server = "box"));
        assert_eq!(stage.accepted(), vec![hex(computer_keys(thief.path()).unwrap().current.public())]);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_bad_proof_or_a_viewer_changes_no_key() {
        let old = Identity::generate();
        let stage = serve(relay().await, Identity::generate(), vec![laptop(old.public())]).await;
        let new = Identity::generate();
        let hello = serde_json::to_vec(&Hello::Session).unwrap();
        let (mut other, _) = connect(&stage.relay, stage.server.public(), &old, &hello, "box").await.unwrap().unwrap();
        let proof_for_another_channel = other.rotation_proof(&new).unwrap();
        for (proof, hello) in [
            (hex(&[7; borg_link::PROOF_LEN]), Hello::Session),
            (hex(&proof_for_another_channel), Hello::Session),
            (hex(&[7; 5]), Hello::Session),
            (hex(&proof_for_another_channel), Hello::Viewer),
        ] {
            let hello = serde_json::to_vec(&hello).unwrap();
            let (mut channel, _) =
                connect(&stage.relay, stage.server.public(), &old, &hello, "box").await.unwrap().unwrap();
            let call = crate::agent::Call::new(ROTATE_CALL, json!({ "key": hex(new.public()), "proof": proof }));
            channel.send(&serde_json::to_vec(&call).unwrap()).await.unwrap();
            let reply: crate::agent::Reply = serde_json::from_slice(&channel.recv().await.unwrap().unwrap()).unwrap();
            assert!(!reply.ok, "{proof}");
        }
        assert_eq!(stage.accepted(), vec![hex(old.public())]);
        assert!(super::super::remote::rotate(&mut other, &new).await.unwrap(), "the real proof works");
        assert_eq!(stage.accepted(), vec![hex(new.public())]);
    }

    #[test]
    fn computer_keys_are_made_once_and_kept_with_the_retired_ones() {
        let state = computer_state(None);
        let first = computer_keys(state.path()).unwrap().current.clone();
        forget_cached(state.path());
        assert_eq!(computer_keys(state.path()).unwrap().current, first);
        let second = super::super::keys::new_computer_key(state.path()).unwrap();
        assert_ne!(second, first);
        let keys = computer_keys(state.path()).unwrap();
        assert_eq!(keys.current, second);
        assert!(keys.held(&hex(first.public())).is_some(), "the old key stays while servers may take it");
        super::super::keys::retire_unused(state.path()).unwrap();
        assert!(computer_keys(state.path()).unwrap().held(&hex(first.public())).is_none(), "no server takes it");
        use std::os::unix::fs::PermissionsExt;
        let file = state.path().join(LINK_DIR).join(super::super::keys::COMPUTER_KEY_FILE);
        assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, PRIVATE_FILE);
        assert!(!state.path().join(LINK_DIR).join(KEY_FILE).exists(), "a computer makes no server key");
    }
}
