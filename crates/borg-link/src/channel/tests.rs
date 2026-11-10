use super::*;
use std::time::{Duration, Instant};

async fn sockets() -> (Socket, Socket) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}", listener.local_addr().unwrap());
    let server = async {
        let (tcp, _) = listener.accept().await.unwrap();
        tokio_tungstenite::accept_async(MaybeTlsStream::Plain(tcp)).await.unwrap()
    };
    let client = async { tokio_tungstenite::connect_async(url).await.unwrap().0 };
    let (client, server) = tokio::join!(client, server);
    (client, server)
}

#[tokio::test]
async fn a_server_lets_go_of_a_client_that_says_nothing() {
    let (_client, server) = sockets().await;
    let wait = Duration::from_millis(200);
    let start = Instant::now();
    let Err(error) = respond_within(server, &Identity::generate(), wait).await else { panic!("answered nobody") };
    assert!(error.to_string().contains("sent nothing in time"), "{error:#}");
    assert!(start.elapsed() >= wait);
}

#[tokio::test]
async fn a_client_that_says_hello_in_time_is_answered() {
    let (client, server) = sockets().await;
    let (identity, caller) = (Identity::generate(), Identity::generate());
    let (opened, answered) = tokio::join!(
        initiate(client, &caller, identity.public(), b"hello"),
        respond_within(server, &identity, Duration::from_secs(5)),
    );
    opened.unwrap();
    let (channel, hello) = answered.unwrap();
    assert_eq!(hello, b"hello");
    assert_eq!(channel.peer(), caller.public());
}

#[test]
fn only_the_holder_of_the_new_key_proves_it_and_only_for_its_own_channel() {
    let server = Identity::generate();
    let new = Identity::generate();
    let hash = [3; 32];
    let proof = rotation_proof(&new, server.public(), &hash).unwrap();
    assert!(proves_rotation(&server, new.public(), &hash, &proof));
    assert!(!proves_rotation(&server, new.public(), &[4; 32], &proof), "another channel");
    assert!(!proves_rotation(&server, Identity::generate().public(), &hash, &proof), "another key");
    assert!(!proves_rotation(&Identity::generate(), new.public(), &hash, &proof), "another server");
    assert!(!proves_rotation(&server, new.public(), &hash, &proof[..PROOF_LEN - 1]));
    assert!(!proves_rotation(&server, &[0; KEY_LEN], &hash, &[0; PROOF_LEN]), "a key with no secret");
}
