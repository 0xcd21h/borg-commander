//! A made-up capture on the network screen's made-up machine, a few seconds long: a name lookup,
//! the browser's two TLS connections and QUIC stream, the web app answering a health check, an
//! SSH session typing, and the backup agent uploading. Saved as tcpdump writes it, or as macOS's
//! `pktap` does, with the process behind each packet.

/// When the capture starts, in seconds since the Unix epoch.
pub const START: u32 = 1_767_268_800;
pub const MACHINE: [u8; 4] = [192, 0, 2, 20];
const ROUTER: [u8; 4] = [192, 0, 2, 1];
const CDN: [u8; 4] = [203, 0, 113, 80];
const API: [u8; 4] = [203, 0, 113, 81];
const MEDIA: [u8; 4] = [203, 0, 113, 82];
const BACKUP: [u8; 4] = [203, 0, 113, 200];
const VISITOR: [u8; 4] = [198, 51, 100, 24];
const SHOP: [u8; 4] = [203, 0, 113, 90];
const PCAP_MAGIC: u32 = 0xa1b2_c3d4;
const PCAP_VERSION: [u16; 2] = [2, 4];
const SNAPSHOT: u32 = 65_535;
const ETHERNET: u32 = 1;
const IPV4: u16 = 0x0800;
const TCP: u8 = 6;
const UDP: u8 = 17;
const TTL: u8 = 64;
const WINDOW: u16 = 2_048;
const SYN: u16 = 0x02;
const ACK: u16 = 0x10;
const PSH_ACK: u16 = 0x18;
const SYN_ACK: u16 = SYN | ACK;
const TLS_HANDSHAKE: u8 = 0x16;
const TLS_DATA: u8 = 0x17;
const TLS_VERSION: [u8; 2] = [3, 3];
const CLIENT_HELLO: u8 = 1;
const SERVER_HELLO: u8 = 2;
const SERVER_NAME: u16 = 0;
const AES_128_GCM: [u8; 2] = [0x13, 0x01];
const QUIC_LONG: u8 = 0xc3;
const QUIC_SHORT: u8 = 0x43;
const SEGMENT: usize = 1_380;
const DATAGRAM: usize = 1_200;
const MAC_MACHINE: [u8; 6] = [0x02, 0x00, 0x5e, 0x10, 0x00, 0x20];
const MAC_ROUTER: [u8; 6] = [0x02, 0x00, 0x5e, 0x10, 0x00, 0x01];

const PCAPNG_SECTION: u32 = 0x0a0d_0d0a;
const PCAPNG_BYTE_ORDER: u32 = 0x1a2b_3c4d;
const PCAPNG_VERSION: [u16; 2] = [1, 0];
const INTERFACE_BLOCK: u32 = 1;
const PACKET_BLOCK: u32 = 6;
const PROCESS_BLOCK: u32 = 0x8000_0001;
const NAME_OPTION: u16 = 2;
const PROCESS_OPTION: u16 = 0x8001;
const END_OF_OPTIONS: u16 = 0;
/// pcapng pads every block and option to whole 4-byte words.
const WORD: usize = 4;
/// A block's type and its length before the body, and the length again after it.
const BLOCK_FRAMING: usize = 12;
/// An option's code and length before its value.
const OPTION_HEADER: usize = 4;
/// A section header's length when it does not say how long the section is.
const UNKNOWN_LENGTH: i64 = -1;
const INTERFACE: &str = "en0";
const MICROS: u64 = 1_000_000;
/// Where a frame's source address starts, inside the IPv4 header, and its ports, after it.
const ADDRESSES_AT: usize = 26;
const PORTS_AT: usize = 34;
/// The updater that looked up a name and exited before the network screen saw it.
pub const UPDATER: u32 = 9120;
/// Each process on the made-up machine, by the local port its traffic uses.
const OWNERS: [(u16, u32, &str); 7] = [
    (22, 301, "sshd"),
    (3_000, crate::network::NODE, "node"),
    (50_412, 8239, "browser"),
    (50_413, 8239, "browser"),
    (62_556, 8239, "browser"),
    (50_600, 7001, "backup-agent"),
    (53_001, UPDATER, "updater"),
];

/// A TLS 1.3 conversation between two OpenSSL endpoints in one process, with its key log: the
/// browser asks the shop's API for an order. Each line is a line of the key log, or what one
/// side sent, in hex.
const TLS_SESSION: &str = include_str!("tls_session.txt");
const TLS_CLIENT: &str = "client ";
const TLS_SERVER: &str = "server ";
/// The browser's port for its connection to the shop.
const SHOP_PORT: u16 = 50_700;
/// Bytes of the record the shop sends last, which no key in the log opens.
const UNOPENED: usize = 32;

/// One packet: microseconds into the capture and the Ethernet frame.
type Frame = (u32, Vec<u8>);

/// The capture as a pcap file.
pub fn bytes() -> Vec<u8> {
    pcap(&frames())
}

/// The capture as macOS's `pktap` saves it: pcapng, each packet tagged with its process.
pub fn tagged() -> Vec<u8> {
    let mut bytes = block(PCAPNG_SECTION, &section());
    bytes.extend(block(INTERFACE_BLOCK, &interface()));
    let mut written: Vec<u32> = Vec::new();
    for (at, frame) in frames() {
        let owner = OWNERS.iter().find(|(port, ..)| *port == local_port(&frame));
        let mut options = Vec::new();
        if let Some(&(_, pid, name)) = owner {
            let index = written.iter().position(|known| *known == pid).unwrap_or_else(|| {
                let mut body = pid.to_le_bytes().to_vec();
                body.extend(option(NAME_OPTION, name.as_bytes()));
                body.extend(option(END_OF_OPTIONS, &[]));
                bytes.extend(block(PROCESS_BLOCK, &body));
                written.push(pid);
                written.len() - 1
            });
            options.extend(option(PROCESS_OPTION, &(index as u32).to_le_bytes()));
            options.extend(option(END_OF_OPTIONS, &[]));
        }
        bytes.extend(block(PACKET_BLOCK, &packet(at, &frame, &options)));
    }
    bytes
}

/// The browser's TLS connection to the shop's API as a pcap file, and the key log that decrypts
/// it. The shop's last record is one no key opens.
pub fn shop_session() -> (Vec<u8>, String) {
    let mut key_log = String::new();
    let mut turns: Vec<(bool, Vec<u8>)> = Vec::new();
    for line in TLS_SESSION.lines() {
        match (line.strip_prefix(TLS_CLIENT), line.strip_prefix(TLS_SERVER)) {
            (Some(bytes), _) => turns.push((true, hex(bytes))),
            (_, Some(bytes)) => turns.push((false, hex(bytes))),
            _ => key_log.push_str(&format!("{line}\n")),
        }
    }
    turns.push((false, record(TLS_DATA, &[0xa5; UNOPENED])));
    let frames = conversation(((MACHINE, SHOP_PORT), (SHOP, 443)), &turns);
    (pcap(&frames), key_log)
}

/// The TLS session saved as pcapng with its key log in it, as Borg saves a capture that kept
/// TLS keys.
pub fn decryptable() -> Vec<u8> {
    let (pcap, key_log) = shop_session();
    borg_core::with_secrets(&pcap, &key_log).expect("a pcap file")
}

fn hex(text: &str) -> Vec<u8> {
    (0..text.len()).step_by(2).map(|at| u8::from_str_radix(&text[at..at + 2], 16).expect("hex")).collect()
}

/// A TCP connection from `ends.0` to `ends.1` that carries `turns`, each the bytes one side
/// sent, the other side acknowledging each.
fn conversation(ends: (([u8; 4], u16), ([u8; 4], u16)), turns: &[(bool, Vec<u8>)]) -> Vec<Frame> {
    const RTT: u32 = 30_000;
    let (client, server) = ends;
    let (mut seq, mut ack) = (5_000_u32, 80_000_u32);
    let mut at = 0;
    let mut frames = vec![
        (at, tcp(Segment { from: client, to: server, seq, ack: 0, flags: SYN, payload: &[] })),
        (at + RTT, tcp(Segment { from: server, to: client, seq: ack, ack: seq + 1, flags: SYN_ACK, payload: &[] })),
    ];
    (seq, ack) = (seq + 1, ack + 1);
    at += RTT;
    for (from_client, bytes) in turns {
        at += RTT;
        let (from, to, sent, other) =
            if *from_client { (client, server, &mut seq, ack) } else { (server, client, &mut ack, seq) };
        for piece in bytes.chunks(SEGMENT) {
            frames.push((at, tcp(Segment { from, to, seq: *sent, ack: other, flags: PSH_ACK, payload: piece })));
            *sent += piece.len() as u32;
        }
        let (own, acked) = if *from_client { (ack, seq) } else { (seq, ack) };
        frames
            .push((at + RTT / 2, tcp(Segment { from: to, to: from, seq: own, ack: acked, flags: ACK, payload: &[] })));
    }
    frames
}

fn frames() -> Vec<Frame> {
    let mut frames = Vec::new();
    frames.extend(lookup());
    frames.extend(tls_session(TlsSession {
        at: 20_000,
        port: 50_412,
        server: CDN,
        name: "cdn.example.net",
        down: 9,
        up: 1,
    }));
    frames.extend(tls_session(TlsSession {
        at: 140_000,
        port: 50_413,
        server: API,
        name: "api.example.com",
        down: 2,
        up: 2,
    }));
    frames.extend(quic());
    frames.extend(health_check());
    frames.extend(ssh());
    frames.extend(backup());
    frames.sort_by_key(|(at, _)| *at);
    frames
}

/// The port a frame uses on the made-up machine.
fn local_port(frame: &[u8]) -> u16 {
    let outgoing = frame[ADDRESSES_AT..ADDRESSES_AT + 4] == MACHINE;
    let at = if outgoing { PORTS_AT } else { PORTS_AT + 2 };
    u16::from_be_bytes([frame[at], frame[at + 1]])
}

/// A pcapng block: its type, its length before and after, and its body padded to whole words.
fn block(kind: u32, body: &[u8]) -> Vec<u8> {
    let length = BLOCK_FRAMING + padded(body.len());
    let mut bytes = kind.to_le_bytes().to_vec();
    bytes.extend((length as u32).to_le_bytes());
    bytes.extend(body);
    bytes.resize(length - WORD, 0);
    bytes.extend((length as u32).to_le_bytes());
    bytes
}

fn padded(length: usize) -> usize {
    length.div_ceil(WORD) * WORD
}

fn option(code: u16, value: &[u8]) -> Vec<u8> {
    let mut bytes = code.to_le_bytes().to_vec();
    bytes.extend((value.len() as u16).to_le_bytes());
    bytes.extend(value);
    bytes.resize(OPTION_HEADER + padded(value.len()), 0);
    bytes
}

fn section() -> Vec<u8> {
    let mut body = PCAPNG_BYTE_ORDER.to_le_bytes().to_vec();
    PCAPNG_VERSION.iter().for_each(|part| body.extend(part.to_le_bytes()));
    body.extend(UNKNOWN_LENGTH.to_le_bytes());
    body
}

fn interface() -> Vec<u8> {
    let mut body = (ETHERNET as u16).to_le_bytes().to_vec();
    body.extend([0; 2]);
    body.extend(SNAPSHOT.to_le_bytes());
    body.extend(option(NAME_OPTION, INTERFACE.as_bytes()));
    body.extend(option(END_OF_OPTIONS, &[]));
    body
}

fn packet(at: u32, frame: &[u8], options: &[u8]) -> Vec<u8> {
    let ticks = u64::from(START) * MICROS + u64::from(at);
    let length = frame.len() as u32;
    let mut body = 0u32.to_le_bytes().to_vec();
    [(ticks >> 32) as u32, ticks as u32, length, length].iter().for_each(|word| body.extend(word.to_le_bytes()));
    body.extend(frame);
    body.resize(padded(body.len()), 0);
    body.extend(options);
    body
}

fn pcap(frames: &[Frame]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend(PCAP_MAGIC.to_le_bytes());
    PCAP_VERSION.iter().for_each(|part| bytes.extend(part.to_le_bytes()));
    bytes.extend([0; 8]);
    bytes.extend(SNAPSHOT.to_le_bytes());
    bytes.extend(ETHERNET.to_le_bytes());
    for (at, frame) in frames {
        let length = frame.len() as u32;
        [START + at / 1_000_000, at % 1_000_000, length, length]
            .iter()
            .for_each(|word| bytes.extend(word.to_le_bytes()));
        bytes.extend(frame);
    }
    bytes
}

fn frame(source: [u8; 4], destination: [u8; 4], protocol: u8, payload: &[u8]) -> Vec<u8> {
    let outgoing = source == MACHINE;
    let (from, to) = if outgoing { (MAC_MACHINE, MAC_ROUTER) } else { (MAC_ROUTER, MAC_MACHINE) };
    let mut bytes = Vec::new();
    bytes.extend(to);
    bytes.extend(from);
    bytes.extend(IPV4.to_be_bytes());
    bytes.extend([0x45, 0]);
    bytes.extend((20 + payload.len() as u16).to_be_bytes());
    bytes.extend([0, 1, 0x40, 0, TTL, protocol, 0, 0]);
    bytes.extend(source);
    bytes.extend(destination);
    bytes.extend(payload);
    bytes
}

/// A TCP segment between two ends, each an address and a port.
struct Segment<'a> {
    from: ([u8; 4], u16),
    to: ([u8; 4], u16),
    seq: u32,
    ack: u32,
    flags: u16,
    payload: &'a [u8],
}

fn tcp(segment: Segment<'_>) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend(segment.from.1.to_be_bytes());
    bytes.extend(segment.to.1.to_be_bytes());
    bytes.extend(segment.seq.to_be_bytes());
    bytes.extend(segment.ack.to_be_bytes());
    bytes.extend((0x5000 | segment.flags).to_be_bytes());
    bytes.extend(WINDOW.to_be_bytes());
    bytes.extend([0; 4]);
    bytes.extend(segment.payload);
    frame(segment.from.0, segment.to.0, TCP, &bytes)
}

fn udp(from: ([u8; 4], u16), to: ([u8; 4], u16), payload: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend(from.1.to_be_bytes());
    bytes.extend(to.1.to_be_bytes());
    bytes.extend((8 + payload.len() as u16).to_be_bytes());
    bytes.extend([0; 2]);
    bytes.extend(payload);
    frame(from.0, to.0, UDP, &bytes)
}

fn dns_name(name: &str) -> Vec<u8> {
    let mut bytes: Vec<u8> =
        name.split('.').flat_map(|label| [&[label.len() as u8], label.as_bytes()].concat()).collect();
    bytes.push(0);
    bytes
}

/// A short-lived updater asks the router for api.example.com and gets its address.
fn lookup() -> Vec<Frame> {
    const ID: [u8; 2] = [0x5e, 0x11];
    const QUESTION: [u8; 4] = [0, 1, 0, 1];
    const ANSWER: [u8; 12] = [0xc0, 12, 0, 1, 0, 1, 0, 0, 1, 0x2c, 0, 4];
    let (client, resolver) = ((MACHINE, 53_001), (ROUTER, 53));
    let mut query = [&ID[..], &[0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0]].concat();
    query.extend(dns_name("api.example.com"));
    query.extend(QUESTION);
    let mut answer = [&ID[..], &[0x81, 0x80, 0, 1, 0, 1, 0, 0, 0, 0]].concat();
    answer.extend(dns_name("api.example.com"));
    answer.extend(QUESTION);
    answer.extend(ANSWER);
    answer.extend(API);
    vec![(0, udp(client, resolver, &query)), (11_800, udp(resolver, client, &answer))]
}

fn client_hello(name: &str) -> Vec<u8> {
    let mut server = vec![0];
    server.extend((name.len() as u16).to_be_bytes());
    server.extend(name.as_bytes());
    let mut list = (server.len() as u16).to_be_bytes().to_vec();
    list.extend(server);
    let mut extensions = SERVER_NAME.to_be_bytes().to_vec();
    extensions.extend((list.len() as u16).to_be_bytes());
    extensions.extend(list);
    let mut hello = TLS_VERSION.to_vec();
    hello.extend([0x5a; 32]);
    hello.push(0);
    hello.extend([0, 2]);
    hello.extend(AES_128_GCM);
    hello.extend([1, 0]);
    hello.extend((extensions.len() as u16).to_be_bytes());
    hello.extend(extensions);
    handshake(CLIENT_HELLO, &hello)
}

fn server_hello() -> Vec<u8> {
    let mut hello = TLS_VERSION.to_vec();
    hello.extend([0xa5; 32]);
    hello.push(0);
    hello.extend(AES_128_GCM);
    hello.extend([0, 0, 0]);
    handshake(SERVER_HELLO, &hello)
}

fn handshake(kind: u8, body: &[u8]) -> Vec<u8> {
    let mut message = vec![kind];
    message.extend(&(body.len() as u32).to_be_bytes()[1..]);
    message.extend(body);
    record(TLS_HANDSHAKE, &message)
}

fn record(kind: u8, body: &[u8]) -> Vec<u8> {
    let mut bytes = vec![kind];
    bytes.extend(TLS_VERSION);
    bytes.extend((body.len() as u16).to_be_bytes());
    bytes.extend(body);
    bytes
}

/// Application data as one TLS record filling a segment.
fn sealed(fill: u8) -> Vec<u8> {
    record(TLS_DATA, &[fill; SEGMENT - 5])
}

/// A TLS connection from the browser: the handshake, then `down` segments of data from the
/// server, each answered, and `up` from the browser.
struct TlsSession {
    at: u32,
    port: u16,
    server: [u8; 4],
    name: &'static str,
    down: u32,
    up: u32,
}

fn tls_session(session: TlsSession) -> Vec<Frame> {
    const RTT: u32 = 24_000;
    const STEP: u32 = 18_000;
    let (client, server) = ((MACHINE, session.port), (session.server, 443));
    let (mut seq, mut ack) = (1_000_u32, 70_000_u32);
    let mut at = session.at;
    let mut frames = Vec::new();
    let mut send = |at: u32, from_client: bool, seq: u32, ack: u32, flags: u16, payload: &[u8]| {
        let (from, to) = if from_client { (client, server) } else { (server, client) };
        frames.push((at, tcp(Segment { from, to, seq, ack, flags, payload })));
    };
    send(at, true, seq, 0, SYN, &[]);
    send(at + RTT, false, ack, seq + 1, SYN_ACK, &[]);
    (seq, ack) = (seq + 1, ack + 1);
    at += RTT + 300;
    send(at, true, seq, ack, ACK, &[]);
    let hello = client_hello(session.name);
    send(at + 200, true, seq, ack, PSH_ACK, &hello);
    seq += hello.len() as u32;
    at += RTT + 400;
    let reply = server_hello();
    send(at, false, ack, seq, PSH_ACK, &reply);
    ack += reply.len() as u32;
    for index in 0..session.down {
        at += STEP;
        let data = sealed(0x30 + index as u8);
        send(at, false, ack, seq, PSH_ACK, &data);
        ack += data.len() as u32;
        send(at + 900, true, seq, ack, ACK, &[]);
    }
    for index in 0..session.up {
        at += STEP;
        let data = sealed(0x60 + index as u8);
        send(at, true, seq, ack, PSH_ACK, &data);
        seq += data.len() as u32;
        send(at + RTT, false, ack, seq, ACK, &[]);
    }
    frames
}

/// The browser's QUIC stream to the media host: a long-header packet each way, then data.
fn quic() -> Vec<Frame> {
    let (client, server) = ((MACHINE, 62_556), (MEDIA, 443));
    let packet = |first: u8| [&[first][..], &[0x4b; DATAGRAM - 1]].concat();
    let mut frames = vec![(300_000, udp(client, server, &packet(QUIC_LONG)))];
    frames.push((322_000, udp(server, client, &packet(QUIC_LONG))));
    frames.extend((0..6).map(|index| (340_000 + index * 15_000, udp(server, client, &packet(QUIC_SHORT)))));
    frames
}

/// A visitor asks the web app on port 3000 for its health and gets a 200.
fn health_check() -> Vec<Frame> {
    let (visitor, app) = ((VISITOR, 50_220), (MACHINE, 3_000));
    let request = b"GET /health HTTP/1.1\r\nHost: shop.example.com\r\nUser-Agent: probe/1.0\r\n\r\n";
    let response = b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 2\r\n\r\nok";
    let (seq, ack) = (40_000_u32, 90_000_u32);
    let segment =
        |from, to, seq, ack, flags, payload: &'static [u8]| tcp(Segment { from, to, seq, ack, flags, payload });
    vec![
        (900_000, segment(visitor, app, seq, 0, SYN, b"")),
        (900_150, segment(app, visitor, ack, seq + 1, SYN_ACK, b"")),
        (931_000, segment(visitor, app, seq + 1, ack + 1, ACK, b"")),
        (931_200, segment(visitor, app, seq + 1, ack + 1, PSH_ACK, request)),
        (933_400, segment(app, visitor, ack + 1, seq + 1 + request.len() as u32, PSH_ACK, response)),
    ]
}

/// Keystrokes in an SSH session already open, each echoed.
fn ssh() -> Vec<Frame> {
    let (server, visitor) = ((MACHINE, 22), (VISITOR, 51_522));
    let (mut seq, mut ack) = (500_000_u32, 800_000_u32);
    let mut frames = Vec::new();
    for index in 0..4u32 {
        let at = 1_200_000 + index * 210_000;
        let keystroke = [0x21; 36];
        frames.push((
            at,
            tcp(Segment { from: visitor, to: server, seq: ack, ack: seq, flags: PSH_ACK, payload: &keystroke }),
        ));
        ack += keystroke.len() as u32;
        let echo = [0x22; 36];
        frames.push((at + 1_100, tcp(Segment { from: server, to: visitor, seq, ack, flags: PSH_ACK, payload: &echo })));
        seq += echo.len() as u32;
    }
    frames
}

/// The backup agent's upload, mid-stream.
fn backup() -> Vec<Frame> {
    let (agent, store) = ((MACHINE, 50_600), (BACKUP, 443));
    let (mut seq, ack) = (2_000_000_u32, 3_000_000_u32);
    let mut frames = Vec::new();
    for index in 0..6u32 {
        let at = 2_000_000 + index * 120_000;
        let data = sealed(0x70 + index as u8);
        frames.push((at, tcp(Segment { from: agent, to: store, seq, ack, flags: PSH_ACK, payload: &data })));
        seq += data.len() as u32;
        frames
            .push((at + 30_000, tcp(Segment { from: store, to: agent, seq: ack, ack: seq, flags: ACK, payload: &[] })));
    }
    frames
}
