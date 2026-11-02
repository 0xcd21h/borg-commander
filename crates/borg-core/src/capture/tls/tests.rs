use super::*;
use std::time::Duration;

const RFC_8448: &str = include_str!("vectors/rfc8448.txt");
/// Conversations between two OpenSSL endpoints in one process, one for each cipher and version.
const LOCAL: [&str; 5] = [
    include_str!("vectors/tls13-aes256.txt"),
    include_str!("vectors/tls13-chacha20.txt"),
    include_str!("vectors/tls12-aes128.txt"),
    include_str!("vectors/tls12-aes256.txt"),
    include_str!("vectors/tls12-chacha20.txt"),
];
const REQUEST: &[u8] = b"GET /orders HTTP/1.1\r\nHost: shop.test\r\n\r\n";
const RESPONSE: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 6\r\n\r\norders";
/// What RFC 8448's client and server send each other once the handshake is done.
const RFC_DATA_LENGTH: u8 = 50;
const RFC_CLIENT_TRAFFIC: &str = "CLIENT_TRAFFIC_SECRET_0";
/// The stream turns of RFC 8448's conversation: the client's application data, the server's,
/// then each side's alert.
const RFC_CLIENT_DATA: usize = 4;
const RFC_SERVER_DATA: usize = 5;

fn hex(text: &str) -> Vec<u8> {
    (0..text.len()).step_by(2).map(|at| u8::from_str_radix(&text[at..at + 2], 16).unwrap()).collect()
}

fn rfc_data() -> Vec<u8> {
    (0..RFC_DATA_LENGTH).collect()
}

/// A vector's key log text, and its turns as a stream.
fn vector(text: &str) -> (String, Stream) {
    let mut log = String::new();
    let mut stream = Stream::default();
    for line in text.lines() {
        match line.split_once(' ') {
            Some((side @ ("client" | "server"), bytes)) => push(&mut stream, side == "client", &hex(bytes)),
            _ => log.push_str(&format!("{line}\n")),
        }
    }
    (log, stream)
}

fn push(stream: &mut Stream, from_client: bool, bytes: &[u8]) {
    let start = stream.bytes.len();
    stream.bytes.extend_from_slice(bytes);
    let stamp = Duration::from_millis(stream.turns.len() as u64);
    stream.turns.push(Turn { from_client, stamp, range: start..stream.bytes.len(), missing: 0, sealed: false });
}

/// Each turn as who sent it, whether it stayed sealed, and its bytes.
fn said(stream: &Stream) -> Vec<(bool, bool, Vec<u8>)> {
    stream.turns.iter().map(|turn| (turn.from_client, turn.sealed, stream.bytes[turn.range.clone()].to_vec())).collect()
}

fn opened(text: &str) -> Stream {
    let (log, stream) = vector(text);
    stream.decrypted(&KeyLog::parse(&log)).expect("the key log opens the stream")
}

#[test]
fn rfc_8448_handshake_keys_come_out_of_its_secret() {
    let suite = cipher::suite(0x1301).unwrap();
    let secret = hex("b67b7d690cc16c4e75e54213cb2d37b4e9c912bcded9105d42befd59d391ad38");
    let key = cipher::traffic_key(suite, &secret).unwrap();
    assert_eq!(key.key, hex("3fce516009c21727d0f2e4e86ee403bc"));
    assert_eq!(key.iv, hex("5d313eb2671276ee13000b30"));
}

#[test]
fn opens_the_rfc_8448_conversation() {
    let stream = opened(RFC_8448);
    assert!(stream.decrypted);
    assert_eq!(said(&stream), [(true, false, rfc_data()), (false, false, rfc_data())]);
    assert_eq!(stream.turns[0].stamp, Duration::from_millis(RFC_CLIENT_DATA as u64), "the turn the record came in");
}

#[test]
fn opens_each_cipher_of_both_versions() {
    for vector in LOCAL {
        let label = vector.lines().next().unwrap();
        assert_eq!(
            said(&opened(vector)),
            [(true, false, REQUEST.to_vec()), (false, false, RESPONSE.to_vec())],
            "{label}"
        );
    }
}

#[test]
fn a_stream_without_its_secrets_stays_as_it_is() {
    let (_, stream) = vector(RFC_8448);
    assert_eq!(stream.decrypted(&KeyLog::default()), None);
    let mut plain = Stream::default();
    push(&mut plain, true, REQUEST);
    push(&mut plain, false, RESPONSE);
    let (log, _) = vector(RFC_8448);
    assert_eq!(plain.decrypted(&KeyLog::parse(&log)), None, "not TLS");
}

#[test]
fn records_no_key_opens_stay_sealed() {
    let (log, stream) = vector(RFC_8448);
    let wrong: String = log
        .lines()
        .map(|line| match line.strip_prefix(RFC_CLIENT_TRAFFIC) {
            Some(rest) => format!("{RFC_CLIENT_TRAFFIC}{}{}\n", &rest[..rest.len() - 2], "00"),
            None => format!("{line}\n"),
        })
        .collect();
    let opened = stream.decrypted(&KeyLog::parse(&wrong)).unwrap();
    let record = |turn: usize| stream.bytes[stream.turns[turn].range.clone()].to_vec();
    assert_eq!(
        said(&opened),
        [(true, true, record(RFC_CLIENT_DATA)), (false, false, rfc_data()), (true, true, record(RFC_CLIENT_DATA + 2))]
    );
}

#[test]
fn bytes_after_a_hole_stay_sealed() {
    let (log, mut stream) = vector(RFC_8448);
    stream.turns[RFC_SERVER_DATA].missing = 10;
    let opened = stream.decrypted(&KeyLog::parse(&log)).unwrap();
    let record = |turn: usize| &stream.bytes[stream.turns[turn].range.clone()];
    let server_rest = [record(RFC_SERVER_DATA), record(RFC_SERVER_DATA + 2)].concat();
    assert_eq!(said(&opened), [(true, false, rfc_data()), (false, true, server_rest)]);
    assert_eq!(opened.turns[1].missing, 10);
}

#[test]
fn a_key_update_moves_on_to_the_next_secret() {
    let (log, mut stream) = vector(RFC_8448);
    let suite = cipher::suite(0x1301).unwrap();
    let secret = log.lines().find_map(|line| line.strip_prefix(RFC_CLIENT_TRAFFIC)).unwrap().split(' ').nth(2).unwrap();
    let next = cipher::next_secret(suite.hash, &hex(secret)).unwrap();
    let key = cipher::traffic_key(suite, &next).unwrap();
    let plain = [&b"after the update"[..], &[APPLICATION_DATA]].concat();
    let length = (plain.len() + TAG) as u16;
    let header = [&[APPLICATION_DATA, MAJOR_VERSION, 3][..], &length.to_be_bytes()].concat();
    let sealed = seal(&key, &key.nonce(0), &header, &plain);
    push(&mut stream, true, &[header, sealed].concat());
    let opened = stream.decrypted(&KeyLog::parse(&log)).unwrap();
    assert_eq!(said(&opened).last(), Some(&(true, false, b"after the update".to_vec())));
}

#[test]
fn reads_the_client_random_from_the_start_of_a_hello() {
    let (_, stream) = vector(RFC_8448);
    let start = &stream.bytes[..HEADER + MESSAGE_HEADER + VERSION + RANDOM];
    assert_eq!(
        client_random(start).map(Vec::from),
        Some(hex("cb34ecb1e78163ba1c38c6dacb196a6dffa21a8d9912ec18a2ef6283024dece7"))
    );
    assert_eq!(client_random(&start[..start.len() - 1]), None);
    assert_eq!(client_random(REQUEST), None);
}

fn seal(key: &Key, nonce: &[u8; cipher::NONCE], aad: &[u8], plain: &[u8]) -> Vec<u8> {
    use aes_gcm::{
        Aes128Gcm,
        aead::{Aead, KeyInit, Payload},
    };
    Aes128Gcm::new_from_slice(&key.key).unwrap().encrypt(nonce.into(), Payload { msg: plain, aad }).unwrap()
}
