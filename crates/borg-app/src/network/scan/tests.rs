use super::*;

const IPV4_HEADER: [u8; 20] =
    [0x45, 0x00, 0x00, 0x24, 0x00, 0x00, 0x00, 0x00, 0x40, 0x01, 0x00, 0x00, 192, 0, 2, 77, 192, 0, 2, 20];

fn reply(sequence: u16) -> Vec<u8> {
    let mut packet = echo_request(sequence).to_vec();
    packet[0] = ECHO_REPLY;
    packet
}

#[test]
fn an_echo_request_carries_its_sequence_and_a_checksum_that_adds_up() {
    let packet = echo_request(0x0102);
    assert_eq!(packet[0], ECHO_REQUEST);
    assert_eq!(&packet[SEQUENCE_AT..ECHO_HEADER], &[0x01, 0x02]);
    assert_eq!(checksum(&packet), 0, "a packet with its checksum in place sums to zero");
    assert_eq!(checksum(&[0x00, 0x01, 0xf2]), !0xf201, "an odd byte is padded");
}

#[test]
fn an_echo_reply_reads_with_or_without_the_ipv4_header() {
    assert_eq!(icmp_answer(&reply(7)), Some(Icmp::Reply(7)), "Linux hands over the ICMP message alone");
    let with_header: Vec<u8> = IPV4_HEADER.iter().copied().chain(reply(254)).collect();
    assert_eq!(icmp_answer(&with_header), Some(Icmp::Reply(254)), "macOS keeps the IPv4 header");
    assert_eq!(icmp_answer(&echo_request(7)), None, "a request is no reply");
    assert_eq!(icmp_answer(&reply(7)[..ECHO_HEADER - 1]), None, "too short");
    assert_eq!(icmp_answer(&IPV4_HEADER), None, "a header with nothing after it");
    assert_eq!(icmp_answer(&[]), None);
}

/// A router's time exceeded message for an echo to 198.51.100.1, as macOS hands it over: the
/// outer IPv4 header, the ICMP header, then the dropped packet's header and first eight bytes.
fn dropped(sequence: u16) -> Vec<u8> {
    let mut original = IPV4_HEADER;
    original[16..20].copy_from_slice(&[198, 51, 100, 1]);
    let icmp = [TIME_EXCEEDED, 0, 0, 0, 0, 0, 0, 0];
    IPV4_HEADER
        .iter()
        .chain(&icmp)
        .chain(&original)
        .copied()
        .chain(echo_request(sequence)[..ECHO_HEADER].iter().copied())
        .collect()
}

#[test]
fn a_router_s_time_exceeded_names_the_echo_it_dropped() {
    let destination = Ipv4Addr::new(198, 51, 100, 1);
    assert_eq!(icmp_answer(&dropped(35)), Some(Icmp::Dropped { sequence: 35, destination }));
    let cut = dropped(35);
    assert_eq!(icmp_answer(&cut[..cut.len() - 2]), None, "too short to hold the echo");
}

#[test]
fn the_first_time_to_live_that_reaches_the_address_counts_the_hops_and_the_routers_before_it_are_the_path() {
    let (first, second) = (Ipv4Addr::new(192, 0, 2, 1), Ipv4Addr::new(203, 0, 113, 1));
    let heard =
        [Some(Heard::Router(first)), None, Some(Heard::Router(second)), Some(Heard::Reached), Some(Heard::Reached)];
    assert_eq!(path(&heard), Trace { hops: Some(4), path: vec![first, second] });
    assert_eq!(path(&[Some(Heard::Reached)]), Trace { hops: Some(1), path: Vec::new() }, "on the gateway");
    assert_eq!(path(&[Some(Heard::Router(first)), None]), Trace::default(), "never reached");
}

#[tokio::test]
async fn a_port_that_takes_the_connection_is_open_and_a_closed_one_refuses() {
    let listener = std::net::TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    assert_eq!(connect(Ipv4Addr::LOCALHOST, port, None).await, Answer::Open);
    assert_eq!(connect(Ipv4Addr::LOCALHOST, port, Some(1)).await, Answer::Open, "one hop reaches this machine");
    drop(listener);
    assert_eq!(connect(Ipv4Addr::LOCALHOST, port, None).await, Answer::Refused);
}
