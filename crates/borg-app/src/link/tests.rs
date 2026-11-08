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
    let computer = Computer { name: "laptop".into(), key: hex(&[7; 32]) };
    let shared = Arc::new(Mutex::new(Shared {
        computers: vec![computer.clone(), Computer { name: "offline".into(), key: hex(&[8; 32]) }],
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
    save(&state.path().join(LINK_DIR).join(COMPUTERS_FILE), &[Computer { name: "laptop".into(), key: hex(&[7; 32]) }])?;
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
        background_service: false,
        relay,
        identity,
        shared: Arc::new(Mutex::new(Shared { computers: Vec::new(), pairing: None, connections: Default::default() })),
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
