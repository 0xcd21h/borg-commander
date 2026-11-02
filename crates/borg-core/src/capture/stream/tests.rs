use super::*;
use crate::capture::{
    ACK, Capture, PSH,
    tests::{CLIENT, CLIENT_PORT, SERVER, over_tcp, over_udp, pcap, tcp, udp},
};

const SERVER_PORT: u16 = 80;
const CLIENT_SEQ: u32 = 1000;
const SERVER_SEQ: u32 = 5000;
const DATA: u16 = PSH | ACK;

fn sent(seq: u32, payload: &[u8]) -> Vec<u8> {
    over_tcp(CLIENT, SERVER, tcp(CLIENT_PORT, SERVER_PORT, seq, DATA, payload))
}

fn answered(seq: u32, payload: &[u8]) -> Vec<u8> {
    over_tcp(SERVER, CLIENT, tcp(SERVER_PORT, CLIENT_PORT, seq, DATA, payload))
}

fn opened() -> Vec<Vec<u8>> {
    vec![
        over_tcp(CLIENT, SERVER, tcp(CLIENT_PORT, SERVER_PORT, CLIENT_SEQ, SYN, b"")),
        over_tcp(SERVER, CLIENT, tcp(SERVER_PORT, CLIENT_PORT, SERVER_SEQ, SYN | ACK, b"")),
    ]
}

fn stream_of(frames: &[Vec<u8>]) -> Stream {
    let capture = Capture::read(&pcap(frames)).expect("a capture");
    Stream::of(&capture.flows[0], &capture.packets)
}

fn turns(stream: &Stream) -> Vec<(bool, &[u8], u64)> {
    stream.turns.iter().map(|turn| (turn.from_client, &stream.bytes[turn.range.clone()], turn.missing)).collect()
}

#[test]
fn each_side_speaks_in_turns_after_the_handshake() {
    let mut frames = opened();
    frames.push(sent(CLIENT_SEQ + 1, b"GET / HTTP/1.1\r\n"));
    frames.push(sent(CLIENT_SEQ + 17, b"\r\n"));
    frames.push(answered(SERVER_SEQ + 1, b"HTTP/1.1 200 OK\r\n"));
    frames.push(sent(CLIENT_SEQ + 19, b"bye"));
    let stream = stream_of(&frames);
    let expected: [(bool, &[u8], u64); 3] =
        [(true, b"GET / HTTP/1.1\r\n\r\n", 0), (false, b"HTTP/1.1 200 OK\r\n", 0), (true, b"bye", 0)];
    assert_eq!(turns(&stream), expected);
    assert_eq!(stream.turns[1].stamp, Duration::from_millis(100_004));
}

#[test]
fn a_repeated_segment_counts_once_and_an_overlap_only_for_what_is_new() {
    let mut frames = opened();
    frames.push(sent(CLIENT_SEQ + 1, b"hello"));
    frames.push(sent(CLIENT_SEQ + 1, b"hello"));
    frames.push(sent(CLIENT_SEQ + 4, b"lo world"));
    let expected: [(bool, &[u8], u64); 1] = [(true, b"hello world", 0)];
    assert_eq!(turns(&stream_of(&frames)), expected);
}

#[test]
fn segments_that_came_out_of_order_go_back_in_place_once_the_gap_fills() {
    let mut frames = opened();
    frames.push(sent(CLIENT_SEQ + 6, b"world"));
    frames.push(sent(CLIENT_SEQ + 1, b"hello"));
    let stream = stream_of(&frames);
    let expected: [(bool, &[u8], u64); 1] = [(true, b"helloworld", 0)];
    assert_eq!(turns(&stream), expected);
    assert_eq!(stream.turns[0].stamp, Duration::from_millis(100_003));
}

#[test]
fn bytes_the_capture_lacks_are_counted_and_begin_another_turn() {
    let mut frames = opened();
    frames.push(sent(CLIENT_SEQ + 1, b"abc"));
    frames.push(sent(CLIENT_SEQ + 10, b"xyz"));
    let expected: [(bool, &[u8], u64); 2] = [(true, b"abc", 0), (true, b"xyz", 6)];
    assert_eq!(turns(&stream_of(&frames)), expected);
}

#[test]
fn a_conversation_caught_after_its_start_begins_at_its_earliest_byte() {
    let frames = [sent(CLIENT_SEQ + 5, b"world"), sent(CLIENT_SEQ, b"hello"), answered(SERVER_SEQ, b"ok")];
    let expected: [(bool, &[u8], u64); 2] = [(true, b"helloworld", 0), (false, b"ok", 0)];
    assert_eq!(turns(&stream_of(&frames)), expected);
}

#[test]
fn datagrams_keep_the_order_they_came_in() {
    let frames = [
        over_udp(CLIENT, SERVER, udp(CLIENT_PORT, SERVER_PORT, b"ask")),
        over_udp(CLIENT, SERVER, udp(CLIENT_PORT, SERVER_PORT, b"again")),
        over_udp(SERVER, CLIENT, udp(SERVER_PORT, CLIENT_PORT, b"answer")),
    ];
    let expected: [(bool, &[u8], u64); 2] = [(true, b"askagain", 0), (false, b"answer", 0)];
    assert_eq!(turns(&stream_of(&frames)), expected);
}

#[test]
fn rows_start_at_any_row_and_turns_are_found_on_either_side() {
    let mut frames = opened();
    frames.push(sent(CLIENT_SEQ + 1, &[b'a'; 20]));
    frames.push(answered(SERVER_SEQ + 1, &[b'b'; 3]));
    frames.push(sent(CLIENT_SEQ + 21, &[b'c'; 9]));
    let stream = stream_of(&frames);
    let layout = HexLayout::full(20);
    assert_eq!(stream.row_count(layout), 7);
    let rows: Vec<StreamRow> = stream.rows_from(layout, 2).collect();
    assert_eq!(rows.len(), 5);
    assert_eq!(rows[0], StreamRow::Bytes { offset: 16, bytes: &[b'a'; 4] });
    assert_eq!(rows[1], StreamRow::Turn(&stream.turns[1]));
    assert_eq!(stream.rows_from(layout, 7).count(), 0);
    assert_eq!(stream.turn_row(layout, 0, true), Some(3));
    assert_eq!(stream.turn_row(layout, 3, true), Some(5));
    assert_eq!(stream.turn_row(layout, 5, true), None);
    assert_eq!(stream.turn_row(layout, 4, false), Some(3));
    assert_eq!(stream.turn_row(layout, 3, false), Some(0));
    assert_eq!(stream.turn_row(layout, 0, false), None);
}
