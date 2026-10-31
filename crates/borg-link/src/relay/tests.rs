use super::*;
use crate::key::Identity;
use futures::{SinkExt, StreamExt};
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;

async fn registration_sockets() -> (
    tokio_tungstenite::WebSocketStream<tokio::io::DuplexStream>,
    tokio_tungstenite::WebSocketStream<tokio::io::DuplexStream>,
) {
    use tokio_tungstenite::{WebSocketStream, tungstenite::protocol::Role};
    let (client, server) = tokio::io::duplex(8192);
    (
        WebSocketStream::from_raw_socket(client, Role::Client, None).await,
        WebSocketStream::from_raw_socket(server, Role::Server, None).await,
    )
}

#[tokio::test(start_paused = true)]
async fn registration_stalls_expire_before_and_after_the_proof() {
    for challenged in [false, true] {
        let identity = Identity::generate();
        let (mut client, mut server) = registration_sockets().await;
        let (proof, challenge) = challenge(identity.public()).unwrap();
        let relay = tokio::spawn(async move {
            if challenged {
                server.send(Message::Binary(challenge.into())).await.unwrap();
                let Some(Ok(Message::Binary(answer))) = server.next().await else { panic!("missing proof") };
                assert!(verify(proof, &answer));
            }
            let _socket = server;
            std::future::pending::<()>().await;
        });
        let start = tokio::time::Instant::now();
        let error = register_socket(&mut client, &identity).await.unwrap_err();
        assert!(error.to_string().contains("did not complete registration"));
        assert_eq!(start.elapsed(), CONNECT_TIMEOUT);
        relay.abort();
    }
}

#[tokio::test(start_paused = true)]
async fn registration_accepts_the_proof_with_interleaved_heartbeats() {
    let identity = Identity::generate();
    let (mut client, mut server) = registration_sockets().await;
    let (proof, challenge) = challenge(identity.public()).unwrap();
    let relay = tokio::spawn(async move {
        server.send(Message::Ping(Vec::new().into())).await.unwrap();
        server.send(Message::Binary(challenge.into())).await.unwrap();
        loop {
            match server.next().await {
                Some(Ok(Message::Binary(answer))) => {
                    assert!(verify(proof, &answer));
                    break;
                }
                Some(Ok(Message::Pong(_))) => {}
                message => panic!("unexpected proof: {message:?}"),
            }
        }
        server.send(Message::Ping(Vec::new().into())).await.unwrap();
        server.send(Message::Text(READY.into())).await.unwrap();
        server
    });
    register_socket(&mut client, &identity).await.unwrap();
    drop(relay.await.unwrap());
}

#[tokio::test(start_paused = true)]
async fn registration_silence_expires_even_when_waiting_is_cancelled() {
    let (mut client, _server) = registration_sockets().await;
    let start = tokio::time::Instant::now();
    let mut heard = start;
    let mut ping_due = start + PING_EVERY;
    assert!(
        tokio::time::timeout(REGISTRATION_SILENCE / 2, next_session(&mut client, &mut heard, &mut ping_due))
            .await
            .is_err()
    );
    let error = next_session(&mut client, &mut heard, &mut ping_due).await.unwrap_err();
    assert!(error.to_string().contains("stopped sending registration heartbeats"));
    assert_eq!(start.elapsed(), REGISTRATION_SILENCE);
}

#[tokio::test(start_paused = true)]
async fn registration_heartbeats_keep_a_quiet_host_available() {
    let (mut client, mut server) = registration_sockets().await;
    let relay = tokio::spawn(async move {
        for _ in 0..4 {
            tokio::time::sleep(Duration::from_secs(30)).await;
            server.send(Message::Ping(Vec::new().into())).await.unwrap();
            loop {
                match server.next().await {
                    Some(Ok(Message::Pong(_))) => break,
                    Some(Ok(Message::Ping(_))) => {}
                    message => panic!("unexpected heartbeat: {message:?}"),
                }
            }
        }
        server.send(Message::Text(format!("{SESSION}next-client").into())).await.unwrap();
        server
    });
    let start = tokio::time::Instant::now();
    let mut heard = start;
    let mut ping_due = start + PING_EVERY;
    assert_eq!(next_session(&mut client, &mut heard, &mut ping_due).await.unwrap(), Some("next-client".into()));
    assert_eq!(start.elapsed(), Duration::from_secs(120));
    drop(relay.await.unwrap());
}

#[tokio::test(start_paused = true)]
async fn registration_probes_keep_a_passive_relay_connected() {
    let (mut client, mut server) = registration_sockets().await;
    let relay = tokio::spawn(async move {
        for _ in 0..4 {
            assert!(matches!(server.next().await, Some(Ok(Message::Ping(_)))));
            server.flush().await.unwrap();
        }
        server.send(Message::Text(format!("{SESSION}passive-relay").into())).await.unwrap();
        server
    });
    let start = tokio::time::Instant::now();
    let mut heard = start;
    let mut ping_due = start + PING_EVERY;
    assert_eq!(next_session(&mut client, &mut heard, &mut ping_due).await.unwrap(), Some("passive-relay".into()));
    assert_eq!(start.elapsed(), Duration::from_secs(120));
    drop(relay.await.unwrap());
}

#[tokio::test]
async fn relay_connections_disable_nagle_before_the_websocket_handshake() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        tokio_tungstenite::accept_async(tcp).await.unwrap()
    });
    let socket = open(&url).await.unwrap();
    let tokio_tungstenite::MaybeTlsStream::Plain(tcp) = socket.get_ref() else {
        panic!("the test relay is plain TCP");
    };
    assert!(tcp.nodelay().unwrap());
    drop(server.await.unwrap());
}

#[test]
fn only_the_holder_of_a_key_answers_the_relays_challenge() {
    let server = Identity::generate();
    let impostor = Identity::generate();
    let (handshake, message) = challenge(server.public()).unwrap();
    assert!(verify(handshake, &answer(&server, &message).unwrap()));
    let (handshake, message) = challenge(server.public()).unwrap();
    let forged = answer(&impostor, &message);
    assert!(forged.is_err() || !verify(handshake, &forged.unwrap()));
}
