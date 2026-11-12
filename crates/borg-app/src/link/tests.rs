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
        registered: false,
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
        keys::{ComputerKeys, computer_info, forget_cached, load_computer_keys, takes_old_key},
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
        let shared = Arc::new(Mutex::new(Shared {
            computers,
            pairing: None,
            connections: Default::default(),
            registered: false,
        }));
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
            members: None,
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

        let keys = load_computer_keys(computer.path()).unwrap();
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
        let new = hex(load_computer_keys(computer.path()).unwrap().current.public());

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
        assert_eq!(stage.accepted(), vec![hex(load_computer_keys(computer.path()).unwrap().current.public())]);
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
        assert_eq!(stage.accepted(), vec![hex(load_computer_keys(thief.path()).unwrap().current.public())]);
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
        let first = load_computer_keys(state.path()).unwrap().current.clone();
        forget_cached(state.path());
        assert_eq!(load_computer_keys(state.path()).unwrap().current, first);
        let second = super::super::keys::new_computer_key(state.path()).unwrap();
        assert_ne!(second, first);
        let keys = load_computer_keys(state.path()).unwrap();
        assert_eq!(keys.current, second);
        assert!(keys.held(&hex(first.public())).is_some(), "the old key stays while servers may take it");
        super::super::keys::retire_unused(state.path()).unwrap();
        assert!(load_computer_keys(state.path()).unwrap().held(&hex(first.public())).is_none(), "no server takes it");
        use std::os::unix::fs::PermissionsExt;
        let file = state.path().join(LINK_DIR).join(super::super::keys::COMPUTER_KEY_FILE);
        assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, PRIVATE_FILE);
        assert!(!state.path().join(LINK_DIR).join(KEY_FILE).exists(), "a computer makes no server key");
    }

    #[test]
    fn keys_found_in_a_plain_file_and_the_keychain_keep_the_current_key_and_every_other() {
        let [a, b, c] = [(); 3].map(|_| Identity::generate());
        let keychain = ComputerKeys { current: a.clone(), retired: vec![b.clone()] };
        let file = ComputerKeys { current: c.clone(), retired: vec![a.clone()] };
        let merged = keychain.merged(file, Some(&hex(c.public())));
        assert_eq!(merged.current, c);
        assert_eq!(merged.retired, vec![a.clone(), b.clone()]);
        let keychain = ComputerKeys { current: a.clone(), retired: Vec::new() };
        let merged = keychain.merged(ComputerKeys { current: b.clone(), retired: Vec::new() }, None);
        assert_eq!((merged.current, merged.retired), (a, vec![b]));
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "uses the login keychain and asks for Touch ID or the password"]
    fn the_keychain_gives_back_the_keys_it_was_given() {
        use super::super::keychain;
        let account = format!("borg test {}", std::process::id());
        keychain::write(&account, "first\n").unwrap();
        keychain::write(&account, "second\n").unwrap();
        assert_eq!(keychain::read(&account).unwrap().as_deref().map(String::as_str), Some("second\n"));
        keychain::delete(&account).unwrap();
        assert!(keychain::read(&account).unwrap().is_none());
    }
}

mod sealing {
    use super::super::{
        key_agent,
        keys::COMPUTER_KEY_FILE,
        sealed::{Cost, DataKey, KEY_LEN, SEALED_FILE, Sealed, agent_challenge},
        ssh_agent::{Agent, parse_identities, parse_signature, sign_request},
        words,
    };
    use std::{
        io::{Read, Write},
        os::unix::{fs::PermissionsExt, net::UnixListener},
        time::Duration,
    };
    use zeroize::Zeroizing;

    const QUICK: Cost = Cost { memory_kib: 8 * 1024, passes: 1, lanes: 1 };
    const KEYS: &str = "first key\nretired key\n";
    const GIB: u64 = 1024 * 1024 * 1024;

    fn string(bytes: &[u8]) -> Vec<u8> {
        [&u32::try_from(bytes.len()).unwrap().to_be_bytes()[..], bytes].concat()
    }

    fn blob(kind: &str) -> Vec<u8> {
        [string(kind.as_bytes()), string(&[9; 32])].concat()
    }

    #[test]
    fn a_sealed_key_file_opens_with_its_password_and_refuses_a_wrong_one() {
        assert_ne!(SEALED_FILE, COMPUTER_KEY_FILE, "a sealed file never takes the plain file's place");
        let (sealed, data) = Sealed::seal("plume orbit cactus ledger", QUICK, KEYS).unwrap();
        let text = sealed.to_text().unwrap();
        assert!(!text.contains("first key"), "the file holds no key in the clear");
        let mut read = Sealed::from_text(&text).unwrap();
        assert!(text.contains("\"memory_kib\": 8192") && text.contains("\"passes\": 1"), "the file names its cost");
        assert!(read.open_with_password("plume orbit cactus").unwrap().is_none());
        let opened = read.open_with_password("plume orbit cactus ledger").unwrap().unwrap();
        assert_eq!(*opened, *data);
        assert_eq!(read.open(&opened).unwrap().as_str(), KEYS);
        read.reseal(&opened, "new key\n").unwrap();
        assert_eq!(read.open(&data).unwrap().as_str(), "new key\n");
        assert!(read.open(&Zeroizing::new([1; KEY_LEN])).is_err(), "another key opens nothing");
    }

    #[test]
    fn an_ssh_agent_signature_opens_the_slot_it_made_and_no_other() {
        let (mut sealed, data) = Sealed::seal("password", QUICK, KEYS).unwrap();
        let salt = Sealed::new_agent_salt().unwrap();
        let challenge = agent_challenge(&salt);
        assert!(
            challenge.ends_with(&salt) && challenge.len() > salt.len(),
            "the agent signs a borg label and the salt"
        );
        sealed.allow_agent(&data, "SHA256:abc", &salt, b"the agent's signature").unwrap();
        let sealed = Sealed::from_text(&sealed.to_text().unwrap()).unwrap();
        assert_eq!(sealed.agent_salt().unwrap(), salt);
        let opened: DataKey = sealed.open_with_signature(b"the agent's signature").unwrap();
        assert_eq!(*opened, *data);
        assert!(sealed.open_with_signature(b"another signature").is_none());
    }

    #[test]
    fn small_machines_get_a_lighter_password_stretch() {
        assert_eq!(Cost::for_memory(16 * GIB).memory_kib, 1024 * 1024);
        assert_eq!(Cost::for_memory(4 * GIB).memory_kib, 512 * 1024);
        assert_eq!(Cost::for_memory(GIB / 4).memory_kib, 64 * 1024);
        assert!((64 * 1024..=1024 * 1024).contains(&Cost::for_this_machine().memory_kib));
    }

    #[test]
    fn suggestions_are_four_words_from_a_list_of_2048() {
        let list = words::list();
        assert_eq!(list.len(), 2048);
        assert_eq!(list.iter().collect::<std::collections::HashSet<_>>().len(), 2048);
        let suggestion = words::suggestion().unwrap();
        let chosen: Vec<&str> = suggestion.split(' ').collect();
        assert_eq!(chosen.len(), words::SUGGESTED);
        assert!(chosen.iter().all(|word| list.contains(word)));
    }

    #[test]
    fn ssh_agent_lists_only_ed25519_and_rsa_keys_and_asks_rsa_for_sha_256() {
        let answer = [
            vec![12],
            2u32.to_be_bytes().to_vec(),
            string(&blob("ecdsa-sha2-nistp256")),
            string(b"ecdsa"),
            string(&blob("ssh-rsa")),
            string(b"work"),
        ]
        .concat();
        let keys = parse_identities(&answer[..answer.len()]).unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].comment, "work");
        assert!(keys[0].fingerprint().starts_with("SHA256:"));
        let request = sign_request(&keys[0], b"challenge");
        assert_eq!(request[0], 13);
        assert_eq!(request[request.len() - 4..], 2u32.to_be_bytes());
        assert_eq!(parse_signature(&[vec![14], string(b"signed")].concat()).unwrap(), b"signed");
        assert!(parse_signature(&[5]).is_none());
    }

    #[test]
    fn ssh_agent_answers_over_its_socket() {
        let folder = tempfile::tempdir().unwrap();
        let socket = folder.path().join("agent.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let fake = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            for answer in [
                [vec![12], 1u32.to_be_bytes().to_vec(), string(&blob("ssh-ed25519")), string(b"laptop")].concat(),
                [vec![14], string(b"deterministic")].concat(),
            ] {
                let mut len = [0; 4];
                stream.read_exact(&mut len).unwrap();
                let mut request = vec![0; u32::from_be_bytes(len) as usize];
                stream.read_exact(&mut request).unwrap();
                stream.write_all(&string(&answer)).unwrap();
            }
        });
        let mut agent = Agent::at(&socket).unwrap();
        let keys = agent.keys().unwrap();
        assert_eq!(keys[0].kind, "ssh-ed25519");
        assert_eq!(agent.sign(&keys[0], b"challenge").unwrap(), b"deterministic");
        fake.join().unwrap();
    }

    #[tokio::test]
    async fn borgs_key_agent_hands_the_key_to_its_owner_and_ends_when_idle_or_told() {
        let folder = tempfile::tempdir().unwrap();
        let socket = folder.path().join(key_agent::SOCKET_FILE);
        let serving = tokio::spawn({
            let socket = socket.clone();
            async move { key_agent::serve(&socket, Zeroizing::new([3; KEY_LEN]), Duration::from_secs(5)).await }
        });
        while !socket.exists() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert_eq!(std::fs::metadata(&socket).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(*key_agent::read_key(&borg_link::hex(&[3; KEY_LEN])).unwrap(), [3; KEY_LEN]);
        assert!(key_agent::read_key("not a key").is_none());
        let asked = socket.clone();
        let held = tokio::task::spawn_blocking(move || {
            let first = key_agent::recall(&asked).unwrap();
            assert!(key_agent::put(&asked, &Zeroizing::new([4; KEY_LEN])));
            (first, key_agent::recall(&asked).unwrap(), key_agent::forget(&asked))
        })
        .await
        .unwrap();
        assert_eq!((*held.0, *held.1, held.2), ([3; KEY_LEN], [4; KEY_LEN], true));
        serving.await.unwrap().unwrap();
        assert!(!socket.exists(), "a forgotten key leaves no socket");

        let idle = tokio::spawn({
            let socket = socket.clone();
            async move { key_agent::serve(&socket, Zeroizing::new([5; KEY_LEN]), Duration::from_millis(200)).await }
        });
        tokio::time::timeout(Duration::from_secs(5), idle).await.unwrap().unwrap().unwrap();
        assert!(!socket.exists(), "an idle agent ends and removes its socket");
        assert!(key_agent::recall(&socket).is_none());
    }

    #[tokio::test]
    async fn a_client_that_says_nothing_neither_holds_borgs_key_agent_nor_keeps_it_unlocked() {
        let folder = tempfile::tempdir().unwrap();
        let socket = folder.path().join(key_agent::SOCKET_FILE);
        let serving = tokio::spawn({
            let socket = socket.clone();
            async move { key_agent::serve(&socket, Zeroizing::new([6; KEY_LEN]), Duration::from_secs(30)).await }
        });
        while !socket.exists() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        let _silent = tokio::net::UnixStream::connect(&socket).await.unwrap();
        let asked = socket.clone();
        let recalled = tokio::task::spawn_blocking(move || key_agent::recall(&asked).map(|key| *key)).await.unwrap();
        assert_eq!(recalled, Some([6; KEY_LEN]), "a silent client does not keep others waiting");
        let asked = socket.clone();
        assert!(tokio::task::spawn_blocking(move || key_agent::forget(&asked)).await.unwrap());
        serving.await.unwrap().unwrap();

        let idle = tokio::spawn({
            let socket = socket.clone();
            async move { key_agent::serve(&socket, Zeroizing::new([7; KEY_LEN]), Duration::from_millis(300)).await }
        });
        while !socket.exists() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        let _silent = tokio::net::UnixStream::connect(&socket).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), idle)
            .await
            .expect("the agent still ends when idle")
            .unwrap()
            .unwrap();
        assert!(!socket.exists());
    }
}

#[cfg(target_os = "linux")]
mod unlocked_cache {
    use super::super::{
        LINK_DIR, key_agent,
        sealed::{DataKey, KEY_LEN},
        unlocked::Cache,
    };
    use linux_keyutils::{KeyRing, KeyRingIdentifier};
    use std::time::Duration;
    use zeroize::Zeroizing;

    fn key(byte: u8) -> DataKey {
        Zeroizing::new([byte; KEY_LEN])
    }

    struct Possessed(Option<(KeyRing, KeyRing)>);

    impl Possessed {
        fn user_keyring() -> Self {
            let session = KeyRing::from_special_id(KeyRingIdentifier::Session, false);
            let user = KeyRing::from_special_id(KeyRingIdentifier::User, true);
            let linked = session.and_then(|session| user.map(|user| (session, user)));
            Self(linked.ok().filter(|(session, user)| session.link_keyring(*user).is_ok()))
        }
    }

    impl Drop for Possessed {
        fn drop(&mut self) {
            if let Some((session, user)) = self.0.take() {
                session.unlink_keyring(user).ok();
            }
        }
    }

    #[test]
    fn the_keyring_forgets_an_unused_key_and_each_use_renews_it() {
        let state = tempfile::tempdir().unwrap();
        let _possessed = Cache::of(state.path()).keyring.is_none().then(Possessed::user_keyring);
        let cache = Cache::of(state.path());
        let Some(description) = &cache.keyring else {
            eprintln!("the kernel keyring is blocked here, so this test has nothing to check");
            return;
        };
        assert!(description.starts_with("borg-commander:"));
        assert_eq!(cache, Cache::choose(state.path(), true), "the same folder always finds the same key");
        assert_ne!(cache.keyring, Cache::choose(tempfile::tempdir().unwrap().path(), true).keyring);
        let idle = Duration::from_secs(3);
        let pause = Duration::from_millis(1600);
        cache.remember(&key(7), idle).unwrap();
        std::thread::sleep(pause);
        assert_eq!(*cache.recall(idle).unwrap(), [7; KEY_LEN]);
        std::thread::sleep(pause);
        assert_eq!(*cache.recall(idle).unwrap(), [7; KEY_LEN], "the recall a moment ago renewed it");
        std::thread::sleep(idle + Duration::from_millis(1100));
        assert!(cache.recall(idle).is_none(), "an unused key expires");

        cache.remember(&key(8), idle).unwrap();
        assert!(cache.forget());
        assert!(cache.recall(idle).is_none(), "borg lock ends it at once");
        assert!(!cache.forget());
    }

    #[tokio::test]
    async fn without_the_keyring_borgs_own_agent_holds_the_key() {
        let state = tempfile::tempdir().unwrap();
        std::fs::create_dir(state.path().join(LINK_DIR)).unwrap();
        let cache = Cache::choose(state.path(), false);
        let socket = state.path().join(LINK_DIR).join(key_agent::SOCKET_FILE);
        assert_eq!((cache.keyring.as_deref(), cache.socket.as_path()), (None, socket.as_path()));
        assert!(cache.recall(Duration::from_secs(1)).is_none(), "nothing is held before an unlock");
        let serving = tokio::spawn({
            let socket = socket.clone();
            async move { key_agent::serve(&socket, key(1), Duration::from_secs(5)).await }
        });
        while !socket.exists() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        let keyring_too = Cache::choose(state.path(), true);
        let held = tokio::task::spawn_blocking(move || {
            cache.remember(&key(2), Duration::from_secs(1)).unwrap();
            let recalled = cache.recall(Duration::from_secs(1)).map(|key| *key);
            let found_by_both = keyring_too.recall(Duration::from_secs(1)).map(|key| *key);
            (recalled, found_by_both, cache.forget(), cache.recall(Duration::from_secs(1)).is_none())
        })
        .await
        .unwrap();
        assert_eq!(held, (Some([2; KEY_LEN]), Some([2; KEY_LEN]), true, true));
        serving.await.unwrap().unwrap();
    }
}

#[tokio::test]
#[expect(clippy::result_large_err, reason = "the upgrade callback's error type is tungstenite's")]
async fn the_server_holds_only_so_many_sessions_whose_clients_have_not_said_hello() {
    use borg_link::relay::{ACCEPT, LISTEN, READY, SESSION, challenge, verify};
    use futures::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::{
        Message,
        handshake::server::{Request, Response},
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let relay = Relay::parse(&format!("ws://{}", listener.local_addr().unwrap())).unwrap();
    let identity = Identity::generate();
    let key = *identity.public();
    let (announce, announced) = tokio::sync::mpsc::unbounded_channel::<String>();
    let (taken, mut takes) = tokio::sync::mpsc::unbounded_channel();
    let stand_in = tokio::spawn(async move {
        let mut announced = Some(announced);
        loop {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut path = String::new();
            let mut socket = tokio_tungstenite::accept_hdr_async(tcp, |request: &Request, response: Response| {
                path = request.uri().path().to_owned();
                Ok(response)
            })
            .await
            .unwrap();
            if let Some(id) = path.strip_prefix(ACCEPT) {
                taken.send((id.to_owned(), socket)).unwrap();
                continue;
            }
            assert!(path.starts_with(LISTEN), "{path}");
            let mut announced = announced.take().expect("one registration");
            tokio::spawn(async move {
                let (proof, question) = challenge(&key).unwrap();
                socket.send(Message::Binary(question.into())).await.unwrap();
                let Some(Ok(Message::Binary(answer))) = socket.next().await else { panic!("no proof") };
                assert!(verify(proof, &answer));
                socket.send(Message::Text(READY.into())).await.unwrap();
                while let Some(id) = announced.recv().await {
                    socket.send(Message::Text(format!("{SESSION}{id}").into())).await.unwrap();
                }
                std::future::pending::<()>().await;
            });
        }
    });
    let (calls, _calls) = tokio::sync::mpsc::unbounded_channel();
    let (messages, _messages) = tokio::sync::mpsc::unbounded_channel();
    let served = Served {
        members: None,
        relay,
        identity,
        shared: Arc::new(Mutex::new(Shared {
            computers: Vec::new(),
            pairing: None,
            connections: Default::default(),
            registered: false,
        })),
        state: None,
        runtime_dir: std::env::temp_dir(),
        session_id: None,
        calls,
        messages,
        clients: Arc::new(AtomicUsize::new(0)),
    };
    let server = tokio::spawn(async move { served.listen().await });

    for n in 0..=served::OPENING_LIMIT {
        announce.send(format!("early-{n}")).unwrap();
    }
    let mut held = Vec::new();
    for _ in 0..served::OPENING_LIMIT {
        held.push(takes.recv().await.unwrap());
    }
    let mut ids: Vec<String> = held.iter().map(|(id, _)| id.clone()).collect();
    ids.sort();
    let mut expected: Vec<String> = (0..served::OPENING_LIMIT).map(|n| format!("early-{n}")).collect();
    expected.sort();
    assert_eq!(ids, expected, "the session announced past the limit is not taken");

    drop(held.pop());
    let mut n = 0;
    let next = loop {
        announce.send(format!("late-{n}")).unwrap();
        n += 1;
        if let Ok(Some((id, _socket))) = tokio::time::timeout(Duration::from_millis(50), takes.recv()).await {
            break id;
        }
    };
    assert!(next.starts_with("late-"), "a closed session frees its place, and the refused one stays refused: {next}");
    server.abort();
    stand_in.abort();
}
