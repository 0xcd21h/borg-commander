use super::*;
use std::net::{Ipv6Addr, SocketAddr};

const ETHERNET: u32 = 1;
const NULL: u32 = 0;
pub(crate) const CLIENT: [u8; 4] = [192, 168, 1, 20];
pub(crate) const SERVER: [u8; 4] = [93, 184, 216, 34];
pub(crate) const RESOLVER: [u8; 4] = [192, 168, 1, 1];
pub(crate) const CLIENT_PORT: u16 = 51000;

fn pcap_with(link: u32, big: bool, nanos: bool, records: &[(u32, u32, Vec<u8>)]) -> Vec<u8> {
    let word = |value: u32| if big { value.to_be_bytes() } else { value.to_le_bytes() };
    let half = |value: u16| if big { value.to_be_bytes() } else { value.to_le_bytes() };
    let mut bytes = Vec::new();
    bytes.extend(word(if nanos { 0xa1b2_3c4d } else { 0xa1b2_c3d4 }));
    bytes.extend(half(2));
    bytes.extend(half(4));
    bytes.extend(word(0));
    bytes.extend(word(0));
    bytes.extend(word(65535));
    bytes.extend(word(link));
    for (seconds, fraction, data) in records {
        bytes.extend(word(*seconds));
        bytes.extend(word(*fraction));
        bytes.extend(word(data.len() as u32));
        bytes.extend(word(data.len() as u32));
        bytes.extend(data);
    }
    bytes
}

pub(crate) fn pcap(frames: &[Vec<u8>]) -> Vec<u8> {
    let records: Vec<_> = frames.iter().enumerate().map(|(i, frame)| (100, i as u32 * 1000, frame.clone())).collect();
    pcap_with(ETHERNET, false, false, &records)
}

fn ethernet(ether_type: u16, payload: &[u8]) -> Vec<u8> {
    let mut frame = vec![0xaa, 0xbb, 0xcc, 0, 0, 1, 0xaa, 0xbb, 0xcc, 0, 0, 2];
    frame.extend(ether_type.to_be_bytes());
    frame.extend(payload);
    frame
}

fn ipv4(protocol: u8, source: [u8; 4], destination: [u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut packet = vec![0x45, 0];
    packet.extend((20 + payload.len() as u16).to_be_bytes());
    packet.extend([0, 1, 0x40, 0, 64, protocol, 0, 0]);
    packet.extend(source);
    packet.extend(destination);
    packet.extend(payload);
    packet
}

pub(crate) fn tcp(source: u16, destination: u16, seq: u32, flags: u16, payload: &[u8]) -> Vec<u8> {
    let mut segment = Vec::new();
    segment.extend(source.to_be_bytes());
    segment.extend(destination.to_be_bytes());
    segment.extend(seq.to_be_bytes());
    segment.extend(1u32.to_be_bytes());
    segment.extend((0x5000 | flags).to_be_bytes());
    segment.extend(65535u16.to_be_bytes());
    segment.extend([0, 0, 0, 0]);
    segment.extend(payload);
    segment
}

pub(crate) fn udp(source: u16, destination: u16, payload: &[u8]) -> Vec<u8> {
    let mut datagram = Vec::new();
    datagram.extend(source.to_be_bytes());
    datagram.extend(destination.to_be_bytes());
    datagram.extend((8 + payload.len() as u16).to_be_bytes());
    datagram.extend([0, 0]);
    datagram.extend(payload);
    datagram
}

pub(crate) fn over_tcp(source: [u8; 4], destination: [u8; 4], segment: Vec<u8>) -> Vec<u8> {
    ethernet(0x0800, &ipv4(6, source, destination, &segment))
}

pub(crate) fn over_udp(source: [u8; 4], destination: [u8; 4], datagram: Vec<u8>) -> Vec<u8> {
    ethernet(0x0800, &ipv4(17, source, destination, &datagram))
}

fn name(text: &str) -> Vec<u8> {
    let mut encoded = Vec::new();
    for label in text.split('.') {
        encoded.push(label.len() as u8);
        encoded.extend(label.as_bytes());
    }
    encoded.push(0);
    encoded
}

/// A DNS message asking for `asked`'s A record, answered with `answers` when given.
pub(crate) fn dns(asked: &str, answers: Option<&[[u8; 4]]>) -> Vec<u8> {
    let mut message = vec![0x12, 0x34];
    message.extend(if answers.is_some() { [0x81, 0x80] } else { [0x01, 0x00] });
    message.extend(1u16.to_be_bytes());
    message.extend((answers.map_or(0, <[_]>::len) as u16).to_be_bytes());
    message.extend([0, 0, 0, 0]);
    message.extend(name(asked));
    message.extend([0, 1, 0, 1]);
    for address in answers.unwrap_or_default() {
        message.extend([0xc0, 12, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4]);
        message.extend(address);
    }
    message
}

/// A TLS client hello asking for `server`, with padding to make it span segments.
pub(crate) fn client_hello(server: &str, padding: usize) -> Vec<u8> {
    let mut sni = Vec::new();
    sni.extend((server.len() as u16 + 3).to_be_bytes());
    sni.push(0);
    sni.extend((server.len() as u16).to_be_bytes());
    sni.extend(server.as_bytes());
    let mut extensions = Vec::new();
    extensions.extend(21u16.to_be_bytes());
    extensions.extend((padding as u16).to_be_bytes());
    extensions.extend(vec![0; padding]);
    extensions.extend(0u16.to_be_bytes());
    extensions.extend((sni.len() as u16).to_be_bytes());
    extensions.extend(sni);
    let mut hello = vec![3, 3];
    hello.extend([7; 32]);
    hello.push(0);
    hello.extend([0, 2, 0x13, 0x01]);
    hello.extend([1, 0]);
    hello.extend((extensions.len() as u16).to_be_bytes());
    hello.extend(extensions);
    let mut handshake = vec![1];
    handshake.extend(&(hello.len() as u32).to_be_bytes()[1..]);
    handshake.extend(hello);
    let mut record = vec![0x16, 3, 1];
    record.extend((handshake.len() as u16).to_be_bytes());
    record.extend(handshake);
    record
}

#[test]
fn reads_pcap_in_either_byte_order_and_unit() {
    let frame = over_udp(CLIENT, RESOLVER, udp(CLIENT_PORT, 53, &dns("example.com", None)));
    for (big, nanos, fraction, expected) in [
        (false, false, 250_000, Duration::from_millis(1250)),
        (true, false, 250_000, Duration::from_millis(1250)),
        (false, true, 250_000_000, Duration::from_millis(1250)),
    ] {
        let capture = Capture::read(&pcap_with(ETHERNET, big, nanos, &[(1, fraction, frame.clone())])).unwrap();
        assert_eq!(capture.packets.len(), 1);
        assert_eq!(capture.packets[0].stamp, expected);
        assert_eq!(capture.packets[0].layers.protocol(), "DNS");
        assert!(!capture.cut);
    }
}

#[test]
fn a_file_that_ends_inside_a_packet_keeps_the_whole_ones() {
    let frame = over_udp(CLIENT, RESOLVER, udp(CLIENT_PORT, 53, &dns("example.com", None)));
    let mut bytes = pcap(&[frame.clone(), frame]);
    bytes.truncate(bytes.len() - 5);
    let capture = Capture::read(&bytes).unwrap();
    assert_eq!(capture.packets.len(), 1);
    assert!(capture.cut);
}

#[test]
fn other_files_are_not_captures() {
    assert_eq!(Capture::read(b"hello world, this is text").unwrap_err(), CaptureError::Unknown);
    assert_eq!(Capture::read(b"ab").unwrap_err(), CaptureError::Short);
}

#[test]
fn packets_past_the_limit_are_counted_and_left_out() {
    let frame = over_udp(CLIENT, RESOLVER, udp(CLIENT_PORT, 53, &dns("example.com", None)));
    let found = files::frames(&pcap(&[frame.clone(), frame.clone(), frame]), 2).unwrap();
    assert_eq!((found.frames.len(), found.skipped), (2, 1));
}

fn block(kind: u32, body: &[u8]) -> Vec<u8> {
    let total = (12 + body.len().div_ceil(4) * 4) as u32;
    let mut block = Vec::new();
    block.extend(kind.to_le_bytes());
    block.extend(total.to_le_bytes());
    block.extend(body);
    block.resize(total as usize - 4, 0);
    block.extend(total.to_le_bytes());
    block
}

const SECTION: u32 = 0x0a0d_0d0a;
const INTERFACE: u32 = 1;
const ENHANCED: u32 = 6;
const PROCESS: u32 = 0x8000_0001;
const IF_NAME: u16 = 2;
const IF_RESOLUTION: u16 = 9;
const PROCESS_NAME: u16 = 2;
const PROCESS_INDEX: u16 = 0x8001;
const EFFECTIVE_PROCESS_INDEX: u16 = 0x8003;
const PACKET_FLAGS: u16 = 2;
const OUTBOUND: u32 = 2;

/// An option with its value, padded to whole words.
fn option(code: u16, value: &[u8]) -> Vec<u8> {
    let mut option = Vec::new();
    option.extend(code.to_le_bytes());
    option.extend((value.len() as u16).to_le_bytes());
    option.extend(value);
    option.resize(4 + value.len().div_ceil(4) * 4, 0);
    option
}

/// Options followed by the end of options.
fn options(options: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = options.concat();
    bytes.extend([0; 4]);
    bytes
}

fn section() -> Vec<u8> {
    let mut body = Vec::new();
    body.extend(0x1a2b_3c4du32.to_le_bytes());
    body.extend([1, 0, 0, 0]);
    body.extend((-1i64).to_le_bytes());
    block(SECTION, &body)
}

fn interface(extra: &[Vec<u8>]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend((ETHERNET as u16).to_le_bytes());
    body.extend([0, 0]);
    body.extend(0u32.to_le_bytes());
    let named = [option(IF_NAME, b"en0")].into_iter().chain(extra.iter().cloned()).collect::<Vec<_>>();
    body.extend(options(&named));
    block(INTERFACE, &body)
}

fn process(pid: u32, name: &str) -> Vec<u8> {
    let mut body = pid.to_le_bytes().to_vec();
    body.extend(options(&[option(PROCESS_NAME, name.as_bytes())]));
    block(PROCESS, &body)
}

fn enhanced(ticks: u64, frame: &[u8], extra: &[Vec<u8>]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend(0u32.to_le_bytes());
    body.extend(((ticks >> 32) as u32).to_le_bytes());
    body.extend((ticks as u32).to_le_bytes());
    body.extend((frame.len() as u32).to_le_bytes());
    body.extend((frame.len() as u32).to_le_bytes());
    body.extend(frame);
    body.resize(body.len().div_ceil(4) * 4, 0);
    if !extra.is_empty() {
        body.extend(options(extra));
    }
    block(ENHANCED, &body)
}

/// An enhanced packet that points at the section's process block `index`, as macOS's
/// tcpdump writes it on its `pktap` interface.
fn tagged(frame: &[u8], index: u32) -> Vec<u8> {
    let tags = [
        option(PROCESS_INDEX, &index.to_le_bytes()),
        option(EFFECTIVE_PROCESS_INDEX, &index.to_le_bytes()),
        option(PACKET_FLAGS, &OUTBOUND.to_le_bytes()),
    ];
    enhanced(0, frame, &tags)
}

/// A `pktap` capture of `frames`, each tagged with the PID beside it, after a process block for
/// each of `processes`.
pub(crate) fn pktap_of(frames: &[(Vec<u8>, Option<u32>)], processes: &[(u32, &str)]) -> Vec<u8> {
    let mut bytes = [section(), interface(&[])].concat();
    bytes.extend(processes.iter().flat_map(|(pid, name)| process(*pid, name)));
    for (frame, pid) in frames {
        let index = pid.and_then(|pid| processes.iter().position(|(known, _)| *known == pid));
        bytes.extend(match index {
            Some(index) => tagged(frame, index as u32),
            None => enhanced(0, frame, &[]),
        });
    }
    bytes
}

#[test]
fn reads_pcapng_with_the_interface_name_and_unit() {
    let frame = over_udp(CLIENT, RESOLVER, udp(CLIENT_PORT, 53, &dns("example.com", None)));
    let nanos = option(IF_RESOLUTION, &[9]);
    let bytes = [section(), interface(&[nanos]), enhanced(3_500_000_000, &frame, &[])].concat();
    let capture = Capture::read(&bytes).unwrap();
    assert_eq!(capture.packets.len(), 1);
    assert_eq!(capture.packets[0].interface.as_deref(), Some("en0"));
    assert_eq!(capture.packets[0].stamp, Duration::from_millis(3500));
    assert_eq!(capture.packets[0].data, frame);
    assert_eq!(capture.packets[0].pid, None, "a plain pcapng file tells no process");
}

const WEB: u32 = 4100;
const BROWSER: u32 = 8239;
const LATER: u32 = 77;

/// macOS's `pktap` capture: each process's block comes before its first packet, and a new
/// section numbers its process blocks again.
fn pktap() -> Vec<u8> {
    let question = over_udp(CLIENT, RESOLVER, udp(CLIENT_PORT, 53, &dns("example.com", None)));
    let hello = over_tcp(CLIENT, SERVER, tcp(CLIENT_PORT, 443, 1, PSH | ACK, &client_hello("example.com", 0)));
    [
        section(),
        interface(&[]),
        process(WEB, "web"),
        tagged(&question, 0),
        process(BROWSER, "browser"),
        tagged(&hello, 1),
        tagged(&question, 0),
        process(u32::MAX, ""),
        tagged(&question, 2),
        tagged(&question, 9),
        enhanced(0, &question, &[]),
        section(),
        interface(&[]),
        process(LATER, "later"),
        tagged(&question, 0),
    ]
    .concat()
}

#[test]
fn pktap_packets_carry_the_process_that_sent_or_received_them() {
    let capture = Capture::read(&pktap()).unwrap();
    let pids: Vec<Option<u32>> = capture.packets.iter().map(|packet| packet.pid).collect();
    assert_eq!(pids, [Some(WEB), Some(BROWSER), Some(WEB), None, None, None, Some(LATER)]);
    assert_eq!(
        capture.processes,
        [(WEB, "web".to_owned()), (BROWSER, "browser".to_owned()), (LATER, "later".to_owned())].into()
    );
    assert_eq!(capture.packets[1].layers.protocol(), "TLS", "the options after the data leave the packet whole");
}

#[test]
fn ethernet_tcp_gives_ports_flags_and_the_payload_without_padding() {
    let mut frame = over_tcp(CLIENT, SERVER, tcp(CLIENT_PORT, 80, 7, PSH | ACK, b"hi"));
    frame.extend([0; 6]);
    let capture = Capture::read(&pcap(&[frame])).unwrap();
    let packet = &capture.packets[0];
    let Some(Datagram::Tcp(segment)) = packet.layers.datagram else { panic!("not TCP") };
    assert_eq!((segment.source, segment.destination, segment.seq), (CLIENT_PORT, 80, 7));
    assert_eq!(segment.flag_names(), ["PSH", "ACK"]);
    assert_eq!(packet.payload(), b"hi");
    assert_eq!(packet.layers.ip.map(|ip| ip.source), Some(IpAddr::from(CLIENT)));
}

#[test]
fn vlan_tags_are_stepped_over() {
    let inner = ipv4(17, CLIENT, RESOLVER, &udp(CLIENT_PORT, 53, &dns("example.com", None)));
    let mut tagged = vec![0x00, 0x2a];
    tagged.extend(0x0800u16.to_be_bytes());
    tagged.extend(inner);
    let capture = Capture::read(&pcap(&[ethernet(0x8100, &tagged)])).unwrap();
    assert_eq!(capture.packets[0].layers.protocol(), "DNS");
}

#[test]
fn loopback_captures_name_the_family_in_either_order() {
    let inner = ipv4(17, [127, 0, 0, 1], [127, 0, 0, 1], &udp(CLIENT_PORT, 9999, b"x"));
    let mut frame = 2u32.to_le_bytes().to_vec();
    frame.extend(&inner);
    let mut v6 = 30u32.to_le_bytes().to_vec();
    let mut packet = vec![0x60, 0, 0, 0];
    let datagram = udp(CLIENT_PORT, 9999, b"x");
    packet.extend((datagram.len() as u16).to_be_bytes());
    packet.extend([17, 64]);
    packet.extend(Ipv6Addr::LOCALHOST.octets());
    packet.extend(Ipv6Addr::LOCALHOST.octets());
    packet.extend(datagram);
    v6.extend(packet);
    let capture = Capture::read(&pcap_with(NULL, false, false, &[(1, 0, frame), (1, 0, v6)])).unwrap();
    assert_eq!(capture.packets[0].layers.protocol(), "UDP");
    assert_eq!(capture.packets[1].layers.ip.map(|ip| ip.source), Some(IpAddr::V6(Ipv6Addr::LOCALHOST)));
}

#[test]
fn ipv6_extension_headers_lead_to_the_datagram() {
    let datagram = udp(CLIENT_PORT, 53, &dns("example.com", None));
    let mut packet = vec![0x60, 0, 0, 0];
    packet.extend((datagram.len() as u16 + 8).to_be_bytes());
    packet.extend([0, 64]);
    packet.extend(Ipv6Addr::LOCALHOST.octets());
    packet.extend(Ipv6Addr::LOCALHOST.octets());
    packet.extend([17, 0, 0, 0, 0, 0, 0, 0]);
    packet.extend(datagram);
    let capture = Capture::read(&pcap(&[ethernet(0x86dd, &packet)])).unwrap();
    assert_eq!(capture.packets[0].layers.protocol(), "DNS");
}

#[test]
fn a_later_fragment_has_no_datagram() {
    let mut packet = ipv4(17, CLIENT, SERVER, &[1, 2, 3, 4, 5, 6, 7, 8]);
    packet[6] = 0;
    packet[7] = 0x10;
    let capture = Capture::read(&pcap(&[ethernet(0x0800, &packet)])).unwrap();
    assert!(capture.packets[0].layers.ip.is_some_and(|ip| ip.fragment));
    assert_eq!(capture.packets[0].layers.datagram, None);
    assert_eq!(capture.packets[0].layers.protocol(), "IPv4");
}

#[test]
fn dns_answers_name_the_addresses_they_give() {
    let query = over_udp(CLIENT, RESOLVER, udp(CLIENT_PORT, 53, &dns("example.com", None)));
    let answer = over_udp(RESOLVER, CLIENT, udp(53, CLIENT_PORT, &dns("example.com", Some(&[SERVER]))));
    let capture = Capture::read(&pcap(&[query, answer])).unwrap();
    let Some(App::Dns(reply)) = &capture.packets[1].layers.app else { panic!("not DNS") };
    assert!(reply.response);
    assert_eq!(reply.questions, [("example.com".to_owned(), 1)]);
    assert_eq!(reply.answers[0].data, DnsData::Address(IpAddr::from(SERVER)));
    assert_eq!(reply.answers[0].name, "example.com");
    assert_eq!(capture.names.get(&IpAddr::from(SERVER)).map(String::as_str), Some("example.com"));
    assert_eq!(capture.flows.len(), 1);
    assert_eq!(capture.flows[0].server_name.as_deref(), Some("example.com"));
    assert_eq!(capture.flows[0].client, SocketAddr::new(IpAddr::from(CLIENT), CLIENT_PORT));
}

#[test]
fn a_client_hello_split_over_segments_still_names_the_server() {
    let hello = client_hello("borg.example", 1500);
    let (first, second) = hello.split_at(1200);
    let frames = [
        over_tcp(CLIENT, SERVER, tcp(CLIENT_PORT, 443, 100, SYN, b"")),
        over_tcp(SERVER, CLIENT, tcp(443, CLIENT_PORT, 900, SYN | ACK, b"")),
        over_tcp(CLIENT, SERVER, tcp(CLIENT_PORT, 443, 1301, PSH | ACK, second)),
        over_tcp(CLIENT, SERVER, tcp(CLIENT_PORT, 443, 101, ACK, first)),
    ];
    let capture = Capture::read(&pcap(&frames)).unwrap();
    assert_eq!(capture.packets[3].layers.app, Some(App::TlsHello { server: None }));
    let flow = &capture.flows[0];
    assert_eq!(flow.server_name.as_deref(), Some("borg.example"));
    assert_eq!(flow.app, Some("TLS"));
    assert_eq!(flow.server, SocketAddr::new(IpAddr::from(SERVER), 443));
    assert_eq!(flow.packets, [0, 1, 2, 3]);
}

#[test]
fn a_whole_client_hello_names_the_server_in_its_packet() {
    let frame = over_tcp(CLIENT, SERVER, tcp(CLIENT_PORT, 443, 1, PSH | ACK, &client_hello("one.example", 0)));
    let capture = Capture::read(&pcap(&[frame])).unwrap();
    assert_eq!(capture.packets[0].layers.app, Some(App::TlsHello { server: Some("one.example".into()) }));
}

#[test]
fn http_gives_the_request_line_the_host_and_the_status() {
    let request = b"GET /index.html HTTP/1.1\r\nHost: plain.example\r\nAccept: */*\r\n\r\n";
    let frames = [
        over_tcp(CLIENT, SERVER, tcp(CLIENT_PORT, 80, 1, PSH | ACK, request)),
        over_tcp(SERVER, CLIENT, tcp(80, CLIENT_PORT, 1, PSH | ACK, b"HTTP/1.1 404 Not Found\r\n\r\n")),
    ];
    let capture = Capture::read(&pcap(&frames)).unwrap();
    let request =
        Http::Request { method: "GET".into(), target: "/index.html".into(), host: Some("plain.example".into()) };
    assert_eq!(capture.packets[0].layers.app, Some(App::Http(request)));
    assert_eq!(
        capture.packets[1].layers.app,
        Some(App::Http(Http::Response { status: 404, reason: "Not Found".into() }))
    );
    assert_eq!(capture.flows[0].server_name.as_deref(), Some("plain.example"));
}

#[test]
fn a_conversation_caught_after_its_start_still_knows_its_client() {
    let frames = [
        over_tcp(SERVER, CLIENT, tcp(443, CLIENT_PORT, 1, SYN | ACK, b"")),
        over_tcp(CLIENT, SERVER, tcp(CLIENT_PORT, 443, 1, ACK, b"abcd")),
        over_udp(SERVER, CLIENT, udp(123, CLIENT_PORT, b"time")),
    ];
    let capture = Capture::read(&pcap(&frames)).unwrap();
    let client = SocketAddr::new(IpAddr::from(CLIENT), CLIENT_PORT);
    assert_eq!(capture.flows[0].client, client);
    assert_eq!(capture.flows[1].client, client);
    let flow = &capture.flows[0];
    assert_eq!((flow.to_client, flow.to_server), (54, 58));
    assert_eq!(flow.bytes(), capture.packets[..2].iter().map(|packet| u64::from(packet.length)).sum::<u64>());
}

#[test]
fn udp_on_port_443_is_quic() {
    let frame = over_udp(CLIENT, SERVER, udp(CLIENT_PORT, 443, &[0xc3, 0, 0, 0, 1]));
    let capture = Capture::read(&pcap(&[frame])).unwrap();
    assert_eq!(capture.packets[0].layers.protocol(), "QUIC");
}

#[test]
fn the_counter_counts_whole_packets_from_the_chunks_of_a_stream() {
    let frame = over_udp(CLIENT, RESOLVER, udp(CLIENT_PORT, 53, &dns("example.com", None)));
    let bytes = pcap(&[frame.clone(), frame]);
    let mut counter = PcapCounter::default();
    counter.feed(&bytes[..10]);
    counter.feed(&bytes[10..30]);
    assert_eq!(counter.packets, 0);
    counter.feed(&bytes[30..bytes.len() - 1]);
    assert_eq!(counter.packets, 1);
    counter.feed(&bytes[bytes.len() - 1..]);
    assert_eq!(counter.packets, 2);
    let mut whole = PcapCounter::default();
    whole.feed(&bytes);
    assert_eq!(whole.packets, 2);
}

#[test]
fn the_counter_counts_pcapng_packets_and_not_the_blocks_around_them() {
    let bytes = pktap();
    let mut whole = PcapCounter::default();
    whole.feed(&bytes);
    assert_eq!(whole.packets, 7);
    let mut trickle = PcapCounter::default();
    bytes.chunks(3).for_each(|chunk| trickle.feed(chunk));
    assert_eq!(trickle.packets, 7);
    let mut cut = PcapCounter::default();
    cut.feed(&bytes[..bytes.len() - 1]);
    assert_eq!(cut.packets, 6, "the last packet is counted once it is whole");
}

#[test]
fn the_counter_stops_at_bytes_that_are_no_capture() {
    let mut counter = PcapCounter::default();
    counter.feed(b"not a capture at all, nothing to count here");
    assert_eq!(counter.packets, 0);
}

/// `bytes` with each conversation kept whole for `budget` bytes, fed in chunks of `size`.
fn budgeted(bytes: &[u8], budget: u64, size: usize) -> Vec<u8> {
    let mut trimmer = ConnectionBudget::new(budget);
    let mut out: Vec<u8> = bytes.chunks(size).flat_map(|chunk| trimmer.feed(chunk)).collect();
    out.extend(trimmer.rest());
    out
}

/// What each packet kept of its length on the wire.
fn kept(capture: &Capture) -> Vec<(usize, u32)> {
    capture.packets.iter().map(|packet| (packet.data.len(), packet.length)).collect()
}

const BUDGET: u64 = 100;
const ODD_PAYLOAD: [u8; 61] = [7; 61];
/// Ethernet, IPv4 and TCP without options.
const TCP_HEADERS: usize = 54;

/// Three segments from the client and an answer, with a DNS question between them.
fn budget_frames() -> Vec<Vec<u8>> {
    let sent = |seq| over_tcp(CLIENT, SERVER, tcp(CLIENT_PORT, 443, seq, PSH | ACK, &ODD_PAYLOAD));
    vec![
        sent(1),
        sent(62),
        over_udp(CLIENT, RESOLVER, udp(CLIENT_PORT, 53, &dns("example.com", None))),
        sent(123),
        over_tcp(SERVER, CLIENT, tcp(443, CLIENT_PORT, 9, PSH | ACK, &ODD_PAYLOAD)),
    ]
}

#[test]
fn the_budget_trims_a_conversation_to_its_headers_once_it_carried_the_budget() {
    let frames = budget_frames();
    let bytes = pcap(&frames);
    let whole = budgeted(&bytes, BUDGET, bytes.len());
    for size in [1, 7, 64, 1000] {
        assert_eq!(budgeted(&bytes, BUDGET, size), whole, "the same in chunks of {size}");
    }
    let capture = Capture::read(&whole).unwrap();
    let (frame, question) = (frames[0].len(), frames[2].len());
    let wire = frame as u32;
    assert_eq!(
        kept(&capture),
        [(frame, wire), (frame, wire), (question, question as u32), (TCP_HEADERS, wire), (TCP_HEADERS, wire)],
        "122 bytes carried before the third segment, and the answer belongs to the same conversation"
    );
    let tcp = capture.flows.iter().find(|flow| flow.kind == FlowKind::Tcp).unwrap();
    let stream = Stream::of(tcp, &capture.packets);
    assert_eq!((stream.bytes.len(), stream.left_out), (2 * ODD_PAYLOAD.len(), 2 * ODD_PAYLOAD.len() as u64));
    assert_eq!(Stream::of(tcp, &Capture::read(&bytes).unwrap().packets).left_out, 0);
}

#[test]
fn the_budget_trims_pcapng_packets_and_keeps_their_options() {
    let frames = budget_frames();
    let tagged: Vec<(Vec<u8>, Option<u32>)> = frames.iter().map(|frame| (frame.clone(), Some(BROWSER))).collect();
    let bytes = pktap_of(&tagged, &[(BROWSER, "browser")]);
    let whole = budgeted(&bytes, BUDGET, bytes.len());
    assert_eq!(budgeted(&bytes, BUDGET, 3), whole);
    let capture = Capture::read(&whole).unwrap();
    let wire = frames[0].len() as u32;
    assert_eq!(kept(&capture)[3..], [(TCP_HEADERS, wire), (TCP_HEADERS, wire)]);
    assert!(capture.packets.iter().all(|packet| packet.pid == Some(BROWSER)), "each packet keeps its process");
    let mut counter = PcapCounter::default();
    counter.feed(&whole);
    assert_eq!(counter.packets, frames.len());
}

#[test]
fn the_budget_passes_what_is_no_capture_and_holds_an_unfinished_record_until_the_end() {
    let text = b"not a capture at all, nothing to trim here";
    assert_eq!(budgeted(text, BUDGET, 5), text);
    let bytes = pcap(&budget_frames());
    let cut = &bytes[..bytes.len() - 1];
    let mut trimmer = ConnectionBudget::new(BUDGET);
    let fed = trimmer.feed(cut);
    assert!(fed.len() < cut.len(), "the last record waits for the rest of it");
    let out = [fed, trimmer.rest()].concat();
    assert_eq!(out.len(), cut.len() - ODD_PAYLOAD.len(), "only the third segment loses its payload");
}

#[test]
fn a_packet_cut_short_still_tells_how_much_payload_it_carried() {
    let frame = over_tcp(CLIENT, SERVER, tcp(CLIENT_PORT, 443, 1, PSH | ACK, &ODD_PAYLOAD));
    let layers = decode::layers(ETHERNET, &frame[..TCP_HEADERS + 1]);
    assert_eq!((layers.payload.len(), layers.carried), (1, ODD_PAYLOAD.len()));
    assert_eq!(decode::layers(ETHERNET, &frame).carried, ODD_PAYLOAD.len());
}

#[test]
fn dns_names_follow_pointers_and_stop_at_loops() {
    let mut message = dns("a.example", None);
    message[5] = 1;
    message.truncate(12);
    message.extend([0xc0, 12, 0, 1, 0, 1]);
    assert_eq!(apps::dns(&message, false).map(|dns| dns.questions), None);
}

/// Key log lines for the connection `client_hello` begins, and for one the captures lack.
fn key_log() -> String {
    format!("# keys\nCLIENT_RANDOM {} {}\nCLIENT_RANDOM {} 02\n", "07".repeat(32), "01".repeat(48), "08".repeat(32))
}

#[test]
fn a_pcap_with_secrets_turns_pcapng_and_keeps_its_packets() {
    let hello = over_tcp(CLIENT, SERVER, tcp(CLIENT_PORT, 443, 1, PSH | ACK, &client_hello("example.com", 0)));
    let question = over_udp(CLIENT, RESOLVER, udp(CLIENT_PORT, 53, &dns("example.com", None)));
    let records = [(100, 250, hello.clone()), (101, 999_999_999, question.clone())];
    for big in [false, true] {
        let pcap = pcap_with(ETHERNET, big, true, &records);
        let before = Capture::read(&pcap).unwrap();
        let with = with_secrets(&pcap, &key_log()).unwrap();
        assert_eq!(&with[..4], SECTION.to_le_bytes(), "pcapng");
        let after = Capture::read(&with).unwrap();
        assert_eq!(after.packets, before.packets);
        assert_eq!(after.keys, KeyLog::parse(&key_log()));
    }
}

#[test]
fn secrets_go_right_after_a_pcapng_section_header() {
    let capture = pktap();
    let with = with_secrets(&capture, &key_log()).unwrap();
    let header = u32::from_le_bytes(capture[4..8].try_into().unwrap()) as usize;
    assert_eq!(with[..header], capture[..header]);
    assert_eq!(&with[header..header + 4], 0x0000_000a_u32.to_le_bytes(), "a decryption secrets block");
    let (before, after) = (Capture::read(&capture).unwrap(), Capture::read(&with).unwrap());
    assert_eq!((after.packets, after.processes), (before.packets, before.processes));
    assert_eq!(after.keys.len(), 2);
    assert_eq!(with_secrets(b"not a capture at all", &key_log()), None);
}

#[test]
fn a_capture_keeps_the_key_log_lines_of_its_connections() {
    let hello = over_tcp(CLIENT, SERVER, tcp(CLIENT_PORT, 443, 1, PSH | ACK, &client_hello("example.com", 0)));
    let capture = Capture::read(&pcap(&[hello])).unwrap();
    assert_eq!(capture.keys_for(&key_log()), format!("CLIENT_RANDOM {} {}\n", "07".repeat(32), "01".repeat(48)));
    let mut added = capture.clone();
    added.add_keys(&key_log());
    assert_eq!(added.keys.len(), 2);
}

#[test]
fn live_frames_come_whole_however_the_stream_is_cut() {
    let frames = vec![ethernet(0x0806, &[1; 28]), ethernet(0x0800, &[2; 40])];
    let expected: Vec<LiveFrame> = frames.iter().map(|data| LiveFrame { link: ETHERNET, data: data.clone() }).collect();
    let stream = pcap(&frames);
    for cut in 0..stream.len() {
        let mut live = LiveFrames::default();
        let mut found = live.feed(&stream[..cut]);
        found.extend(live.feed(&stream[cut..]));
        assert_eq!(found, expected, "cut at {cut}");
    }
}

#[test]
fn live_frames_read_pcapng_with_the_link_of_each_interface() {
    let frame = ethernet(0x0806, &[1; 28]);
    let stream = [section(), interface(&[]), enhanced(1, &frame, &[])].concat();
    let mut live = LiveFrames::default();
    assert_eq!(live.feed(&stream), vec![LiveFrame { link: ETHERNET, data: frame }]);
}

#[test]
fn live_frames_pass_bytes_that_are_no_capture() {
    let mut live = LiveFrames::default();
    assert!(live.feed(b"tcpdump: listening on en0, link-type EN10MB (Ethernet)").is_empty());
    assert!(live.feed(&pcap(&[ethernet(0x0806, &[1; 28])])).is_empty());
}
