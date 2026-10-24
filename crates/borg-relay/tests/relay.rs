//! The relay joins a client to the server it asks for, and forwards what they exchange,
//! which only the two can read; it refuses a server that cannot prove its key.

use std::{
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
};

use borg_link::{Identity, Relay, hex, initiate, relay, respond};
use futures::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

struct Running {
    child: Child,
    relay: Relay,
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

/// Starts the relay without TLS on a free port, and waits until it listens.
fn start() -> Running {
    let mut child = Command::new(env!("CARGO_BIN_EXE_borg-relay"))
        .args(["--plain", "--listen", "127.0.0.1:0"])
        .stderr(Stdio::piped())
        .spawn()
        .expect("the relay starts");
    let mut log = BufReader::new(child.stderr.take().unwrap());
    let mut line = String::new();
    log.read_line(&mut line).unwrap();
    // Keeps reading the log, so the relay can go on writing it.
    std::thread::spawn(move || for _ in log.lines() {});
    let address = line.trim().rsplit(' ').next().unwrap().to_owned();
    Running { child, relay: Relay::parse(&format!("ws://{address}")).unwrap() }
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
            *channel.peer()
        })
    };
    let socket = relay::dial(&relay, server.public()).await.unwrap();
    let mut channel = initiate(socket, &client, server.public(), b"hello").await.unwrap();
    // Larger than one Noise message, so it travels in pieces.
    let big: Vec<u8> = (0..300_000u32).map(|n| (n % 251) as u8).collect();
    channel.send(&big).await.unwrap();
    assert_eq!(channel.recv().await.unwrap().unwrap(), big);
    assert_eq!(&server_side.await.unwrap(), client.public(), "the server learns the client's key");
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
