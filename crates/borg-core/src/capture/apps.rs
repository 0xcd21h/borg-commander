//! What the payload of a TCP or UDP packet says, for the protocols Borg reads: DNS questions and
//! answers, the server name a TLS client asks for, HTTP's first line and QUIC.

use super::decode::Datagram;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

const DNS_PORT: u16 = 53;
const MDNS_PORT: u16 = 5353;
const LLMNR_PORT: u16 = 5355;
const QUIC_PORT: u16 = 443;
const DNS_HEADER: usize = 12;
const DNS_RESPONSE: u16 = 0x8000;
const DNS_CODE: u16 = 0x000f;
const DNS_POINTER: u8 = 0xc0;
const DNS_LABEL_LIMIT: usize = 128;
const DNS_RECORD_LIMIT: usize = 64;
const DNS_NAME_LIMIT: usize = 255;
const DNS_A: u16 = 1;
const DNS_NS: u16 = 2;
const DNS_CNAME: u16 = 5;
const DNS_PTR: u16 = 12;
const DNS_AAAA: u16 = 28;
const DNS_TYPES: [(u16, &str); 14] = [
    (DNS_A, "A"),
    (DNS_NS, "NS"),
    (DNS_CNAME, "CNAME"),
    (6, "SOA"),
    (DNS_PTR, "PTR"),
    (15, "MX"),
    (16, "TXT"),
    (DNS_AAAA, "AAAA"),
    (33, "SRV"),
    (35, "NAPTR"),
    (41, "OPT"),
    (64, "SVCB"),
    (65, "HTTPS"),
    (255, "ANY"),
];

const TLS_HANDSHAKE: u8 = 0x16;
const TLS_RECORD_TYPES: std::ops::RangeInclusive<u8> = 0x14..=0x17;
const TLS_MAJOR: u8 = 3;
const TLS_RECORD_HEADER: usize = 5;
const TLS_CLIENT_HELLO: u8 = 1;
const TLS_SERVER_HELLO: u8 = 2;
const TLS_HANDSHAKE_HEADER: usize = 4;
const TLS_RANDOM: usize = 32;
const TLS_VERSION: usize = 2;
const TLS_SERVER_NAME: u16 = 0;
const TLS_HOST_NAME: u8 = 0;

const QUIC_FIXED_BIT: u8 = 0x40;

const HTTP_METHODS: [&str; 9] = ["GET", "POST", "PUT", "DELETE", "HEAD", "OPTIONS", "PATCH", "CONNECT", "TRACE"];
const HTTP_VERSION: &str = "HTTP/1.";
const HTTP_HOST: &str = "host:";
const HTTP_LINE_LIMIT: usize = 2048;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum App {
    Dns(Dns),
    /// A TLS client's hello, with the server name it asks for when it gives one.
    TlsHello {
        server: Option<String>,
    },
    TlsServerHello,
    Tls,
    Quic,
    Http(Http),
}

impl App {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Dns(dns) if dns.multicast => "mDNS",
            Self::Dns(_) => "DNS",
            Self::TlsHello { .. } | Self::TlsServerHello | Self::Tls => "TLS",
            Self::Quic => "QUIC",
            Self::Http(_) => "HTTP",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dns {
    pub id: u16,
    pub response: bool,
    /// The response code: 0 for none, 3 for a name that does not exist.
    pub code: u16,
    pub multicast: bool,
    /// Each name asked about, with the record type asked for.
    pub questions: Vec<(String, u16)>,
    pub answers: Vec<DnsAnswer>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DnsAnswer {
    pub name: String,
    pub kind: u16,
    pub data: DnsData,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DnsData {
    Address(IpAddr),
    Name(String),
    Other,
}

/// A DNS record type's name, the same in every language.
pub fn dns_type(kind: u16) -> String {
    DNS_TYPES.iter().find(|(known, _)| *known == kind).map_or_else(|| format!("TYPE{kind}"), |(_, name)| (*name).into())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Http {
    Request { method: String, target: String, host: Option<String> },
    Response { status: u16, reason: String },
}

/// What protocol a payload carries, when Borg can tell.
pub(super) fn detect(datagram: &Datagram, payload: &[u8]) -> Option<App> {
    let (source, destination) = datagram.ports()?;
    let on = |port: u16| source == port || destination == port;
    if payload.is_empty() {
        return None;
    }
    match datagram {
        Datagram::Udp { .. } if on(DNS_PORT) || on(MDNS_PORT) || on(LLMNR_PORT) => {
            dns(payload, on(MDNS_PORT)).map(App::Dns)
        }
        Datagram::Udp { .. } if on(QUIC_PORT) => quic(payload).then_some(App::Quic),
        Datagram::Tcp(_) if on(DNS_PORT) => dns(payload.get(2..)?, false).map(App::Dns),
        Datagram::Tcp(_) => tls(payload).or_else(|| http(payload).map(App::Http)),
        _ => None,
    }
}

fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(data.get(at..at + 2)?.try_into().ok()?))
}

fn u24_at(data: &[u8], at: usize) -> Option<usize> {
    let bytes = data.get(at..at + 3)?;
    Some(usize::from(bytes[0]) << 16 | usize::from(bytes[1]) << 8 | usize::from(bytes[2]))
}

pub(super) fn dns(data: &[u8], multicast: bool) -> Option<Dns> {
    let flags = u16_at(data, 2)?;
    let questions = usize::from(u16_at(data, 4)?).min(DNS_RECORD_LIMIT);
    let answers = usize::from(u16_at(data, 6)?).min(DNS_RECORD_LIMIT);
    let mut at = DNS_HEADER;
    let mut asked = Vec::new();
    for _ in 0..questions {
        let (name, next) = dns_name(data, at)?;
        asked.push((name, u16_at(data, next)?));
        at = next + 4;
    }
    let mut found = Vec::new();
    for _ in 0..answers {
        let Some((answer, next)) = dns_answer(data, at) else { break };
        found.push(answer);
        at = next;
    }
    Some(Dns {
        id: u16_at(data, 0)?,
        response: flags & DNS_RESPONSE != 0,
        code: flags & DNS_CODE,
        multicast,
        questions: asked,
        answers: found,
    })
}

fn dns_answer(data: &[u8], at: usize) -> Option<(DnsAnswer, usize)> {
    const FIXED: usize = 10;
    let (name, next) = dns_name(data, at)?;
    let kind = u16_at(data, next)?;
    let length = usize::from(u16_at(data, next + 8)?);
    let start = next + FIXED;
    let raw = data.get(start..start + length)?;
    let value = match kind {
        DNS_A => <[u8; 4]>::try_from(raw).map_or(DnsData::Other, |raw| DnsData::Address(Ipv4Addr::from(raw).into())),
        DNS_AAAA => {
            <[u8; 16]>::try_from(raw).map_or(DnsData::Other, |raw| DnsData::Address(Ipv6Addr::from(raw).into()))
        }
        DNS_CNAME | DNS_PTR | DNS_NS => dns_name(data, start).map_or(DnsData::Other, |(name, _)| DnsData::Name(name)),
        _ => DnsData::Other,
    };
    Some((DnsAnswer { name, kind, data: value }, start + length))
}

/// The name at `at`, following compression pointers, and where the record goes on after it.
fn dns_name(data: &[u8], at: usize) -> Option<(String, usize)> {
    let mut labels: Vec<String> = Vec::new();
    let mut position = at;
    let mut after = None;
    for _ in 0..DNS_LABEL_LIMIT {
        let length = *data.get(position)?;
        if length & DNS_POINTER == DNS_POINTER {
            let target = usize::from(u16_at(data, position)? & !(u16::from(DNS_POINTER) << 8));
            after.get_or_insert(position + 2);
            position = target;
            continue;
        }
        if length == 0 {
            let name = labels.join(".");
            return (name.len() <= DNS_NAME_LIMIT).then(|| (name, after.unwrap_or(position + 1)));
        }
        let start = position + 1;
        let label = data.get(start..start + usize::from(length))?;
        labels.push(String::from_utf8_lossy(label).into_owned());
        position = start + usize::from(length);
    }
    None
}

fn tls(payload: &[u8]) -> Option<App> {
    let (&kind, rest) = payload.split_first()?;
    if !TLS_RECORD_TYPES.contains(&kind) || rest.first() != Some(&TLS_MAJOR) {
        return None;
    }
    if kind != TLS_HANDSHAKE {
        return Some(App::Tls);
    }
    Some(match payload.get(TLS_RECORD_HEADER) {
        Some(&TLS_CLIENT_HELLO) => App::TlsHello { server: server_name(payload) },
        Some(&TLS_SERVER_HELLO) => App::TlsServerHello,
        _ => App::Tls,
    })
}

/// The server name a TLS client hello asks for, from the start of the client's stream.
pub(super) fn server_name(stream: &[u8]) -> Option<String> {
    if stream.first() != Some(&TLS_HANDSHAKE) || stream.get(TLS_RECORD_HEADER) != Some(&TLS_CLIENT_HELLO) {
        return None;
    }
    let hello = TLS_RECORD_HEADER + TLS_HANDSHAKE_HEADER;
    let mut at = hello + TLS_VERSION + TLS_RANDOM;
    at += 1 + usize::from(*stream.get(at)?);
    at += 2 + usize::from(u16_at(stream, at)?);
    at += 1 + usize::from(*stream.get(at)?);
    let end = (at + 2 + usize::from(u16_at(stream, at)?)).min(stream.len());
    at += 2;
    while at + 4 <= end {
        let kind = u16_at(stream, at)?;
        let length = usize::from(u16_at(stream, at + 2)?);
        let body = at + 4;
        if kind == TLS_SERVER_NAME {
            return host_name(stream.get(body..body + length)?);
        }
        at = body + length;
    }
    None
}

fn host_name(extension: &[u8]) -> Option<String> {
    let mut at = 2;
    while at + 3 <= extension.len() {
        let kind = extension[at];
        let length = usize::from(u16_at(extension, at + 1)?);
        let name = extension.get(at + 3..at + 3 + length)?;
        if kind == TLS_HOST_NAME {
            return std::str::from_utf8(name).ok().map(str::to_owned);
        }
        at += 3 + length;
    }
    None
}

/// The length of the TLS handshake message the stream starts with, as its header gives it.
pub(super) fn hello_length(stream: &[u8]) -> Option<usize> {
    Some(TLS_RECORD_HEADER + TLS_HANDSHAKE_HEADER + u24_at(stream, TLS_RECORD_HEADER + 1)?)
}

fn quic(payload: &[u8]) -> bool {
    payload.first().is_some_and(|first| first & QUIC_FIXED_BIT != 0)
}

pub(super) fn http(payload: &[u8]) -> Option<Http> {
    let head = &payload[..payload.len().min(HTTP_LINE_LIMIT)];
    let text = String::from_utf8_lossy(head);
    let mut lines = text.split("\r\n");
    let first = lines.next()?;
    let mut words = first.splitn(3, ' ');
    let (one, two, three) = (words.next()?, words.next()?, words.next().unwrap_or_default());
    if one.starts_with(HTTP_VERSION) {
        return Some(Http::Response { status: two.parse().ok()?, reason: three.to_owned() });
    }
    if !HTTP_METHODS.contains(&one) || !three.starts_with(HTTP_VERSION) {
        return None;
    }
    let host = lines.take_while(|line| !line.is_empty()).find_map(|line| {
        let prefix = line.get(..HTTP_HOST.len())?;
        prefix.eq_ignore_ascii_case(HTTP_HOST).then(|| line[HTTP_HOST.len()..].trim().to_owned())
    });
    Some(Http::Request { method: one.to_owned(), target: two.to_owned(), host })
}
