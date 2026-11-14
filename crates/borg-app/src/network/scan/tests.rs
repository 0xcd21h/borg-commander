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
    assert_eq!(echo_reply(&reply(7)), Some(7), "Linux hands over the ICMP message alone");
    let with_header: Vec<u8> = IPV4_HEADER.iter().copied().chain(reply(254)).collect();
    assert_eq!(echo_reply(&with_header), Some(254), "macOS keeps the IPv4 header");
    assert_eq!(echo_reply(&echo_request(7)), None, "a request is no reply");
    assert_eq!(echo_reply(&reply(7)[..ECHO_HEADER - 1]), None, "too short");
    assert_eq!(echo_reply(&IPV4_HEADER), None, "a header with nothing after it");
    assert_eq!(echo_reply(&[]), None);
}

#[tokio::test]
async fn a_port_that_takes_the_connection_is_open_and_a_closed_one_refuses() {
    let listener = std::net::TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    assert_eq!(connect(Ipv4Addr::LOCALHOST, port).await, Answer::Open);
    drop(listener);
    assert_eq!(connect(Ipv4Addr::LOCALHOST, port).await, Answer::Refused);
}
