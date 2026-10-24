//! The relay joins a client to the server it asks for, and forwards what they exchange,
//! which only the two can read; it refuses a server that cannot prove its key.

use std::{
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
};

use borg_link::{Identity, Relay, hex, initiate, relay, respond};
use futures::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

struct Running {
    child: Child,
    relay: Relay,
    /// What the relay logged after it started listening.
    log: Arc<Mutex<Vec<String>>>,
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

/// Starts the relay without TLS on a free port, and waits until it listens.
fn start() -> Running {
    start_with(&["--plain"])
}

fn start_with(args: &[&str]) -> Running {
    let mut child = Command::new(env!("CARGO_BIN_EXE_borg-relay"))
        .args(args)
        .args(["--listen", "127.0.0.1:0"])
        .env_remove("BORG_RELAY_LOG")
        .stderr(Stdio::piped())
        .spawn()
        .expect("the relay starts");
    let mut stderr = BufReader::new(child.stderr.take().unwrap());
    let mut line = String::new();
    stderr.read_line(&mut line).unwrap();
    // Keeps reading the log, so the relay can go on writing it.
    let log = Arc::new(Mutex::new(Vec::new()));
    let kept = log.clone();
    std::thread::spawn(move || stderr.lines().map_while(Result::ok).for_each(|line| kept.lock().unwrap().push(line)));
    let address = line.trim().split(' ').find(|word| word.starts_with("127.0.0.1:")).unwrap().trim_end_matches(',');
    Running { child, relay: Relay::parse(&format!("ws://{address}")).unwrap(), log }
}

/// Opens a session between a new server and client, sends one message through it, and
/// closes it.
async fn session(relay: &Relay) {
    let (server, client) = (Identity::generate(), Identity::generate());
    let mut listener = relay::Listener::register(relay, &server).await.unwrap();
    let server_side = {
        let (relay, server) = (relay.clone(), server.clone());
        tokio::spawn(async move {
            let id = listener.next().await.unwrap().unwrap();
            let (mut channel, _) = respond(relay::take(&relay, &id).await.unwrap(), &server).await.unwrap();
            channel.recv().await.unwrap();
        })
    };
    let socket = relay::dial(relay, server.public()).await.unwrap();
    let mut channel = initiate(socket, &client, server.public(), b"hello").await.unwrap();
    channel.send(b"one").await.unwrap();
    server_side.await.unwrap();
}

#[tokio::test]
async fn a_client_reaches_the_server_it_asks_for_and_both_prove_their_keys() {
    let running = start();
    let relay = running.relay.clone();
    let (server, client) = (Identity::generate(), Identity::generate());
    let mut listener = relay::Listener::register(&relay, &server).await.unwrap();

    let server_side = {
        let (relay, server) = (relay.clone(), server.clone());
        tokio::spawn(async move {
            let id = listener.next().await.unwrap().unwrap();
            let socket = relay::take(&relay, &id).await.unwrap();
            let (mut channel, hello) = respond(socket, &server).await.unwrap();
            assert_eq!(hello, b"hello");
            let message = channel.recv().await.unwrap().unwrap();
            channel.send(&message).await.unwrap();
            (*channel.peer(), channel.pairing_code())
        })
    };
    let socket = relay::dial(&relay, server.public()).await.unwrap();
    let mut channel = initiate(socket, &client, server.public(), b"hello").await.unwrap();
    // Larger than one Noise message, so it travels in pieces.
    let big: Vec<u8> = (0..300_000u32).map(|n| (n % 251) as u8).collect();
    channel.send(&big).await.unwrap();
    assert_eq!(channel.recv().await.unwrap().unwrap(), big);
    let (peer, code) = server_side.await.unwrap();
    assert_eq!(&peer, client.public(), "the server learns the client's key");
    assert_eq!(code, channel.pairing_code(), "both ends show the same pairing code");
}

#[tokio::test]
async fn a_client_asking_for_an_absent_server_hears_it_is_not_open() {
    let running = start();
    let error = relay::dial(&running.relay, Identity::generate().public()).await.err().unwrap();
    assert!(error.to_string().contains("not open"), "{error}");
}

#[tokio::test]
async fn a_server_without_the_key_cannot_register_under_it() {
    let running = start();
    let (owner, impostor) = (Identity::generate(), Identity::generate());
    let url = running.relay.url(&format!("{}{}", relay::LISTEN, hex(owner.public())));
    let (mut socket, _) = tokio_tungstenite::connect_async(url).await.unwrap();
    let Some(Ok(Message::Binary(_challenge))) = socket.next().await else { panic!("no challenge") };
    // An answer made with another key, which is all an impostor can make.
    let mut handshake = snow::Builder::new("Noise_NK_25519_ChaChaPoly_BLAKE2s".parse().unwrap())
        .local_private_key(&[7; 32])
        .unwrap()
        .build_responder()
        .unwrap();
    let mut answer = vec![0; 1024];
    let size = handshake.write_message(&[], &mut answer).unwrap_or(0);
    socket.send(Message::Binary(answer[..size].to_vec().into())).await.unwrap();
    let refused = matches!(socket.next().await, Some(Ok(Message::Close(_))) | None | Some(Err(_)));
    assert!(refused, "the relay refuses the registration");
    let _ = impostor;
    let error = relay::dial(&running.relay, owner.public()).await.err().unwrap();
    assert!(error.to_string().contains("not open"), "{error}");
}

#[tokio::test]
async fn the_relay_answers_health_checks_over_plain_http() {
    let running = start();
    let address = running.relay.host.clone();
    assert!(tokio::task::spawn_blocking(move || borg_relay::health(&address)).await.unwrap());
    assert!(!borg_relay::health("127.0.0.1:1"), "nothing listens there");
}

#[tokio::test]
async fn behind_a_proxy_each_forwarded_client_has_its_own_share() {
    use tokio_tungstenite::tungstenite::{client::IntoClientRequest, http::HeaderValue};
    const SHARE: usize = 32;
    let running = start_with(&["--behind-proxy"]);
    let open = |client: &'static str| {
        let url = running.relay.url(&format!("{}{}", relay::LISTEN, hex(Identity::generate().public())));
        async move {
            let mut request = url.into_client_request().unwrap();
            request.headers_mut().insert("X-Forwarded-For", HeaderValue::from_static(client));
            tokio_tungstenite::connect_async(request).await
        }
    };
    // Registrations wait for their proof, so each holds its connection open.
    let mut held = Vec::new();
    for _ in 0..SHARE {
        held.push(open("203.0.113.1").await.expect("within the share"));
    }
    assert!(open("203.0.113.1").await.is_err(), "the same client past its share is refused");
    assert!(open("203.0.113.2").await.is_ok(), "another client behind the same proxy gets in");
}

#[tokio::test]
async fn the_relay_logs_servers_and_sessions_only_when_asked() {
    let quiet = start();
    session(&quiet.relay).await;
    let logged = start_with(&["--plain", "--log"]);
    session(&logged.relay).await;
    // The relay logs a session's end once both sides closed.
    for _ in 0..50 {
        if logged.log.lock().unwrap().iter().any(|line| line.starts_with("session")) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(*quiet.log.lock().unwrap(), Vec::<String>::new(), "nothing about servers or sessions by default");
    let lines = logged.log.lock().unwrap().join("\n");
    assert!(lines.contains(": registered") && lines.contains("session for server"), "{lines}");
}
