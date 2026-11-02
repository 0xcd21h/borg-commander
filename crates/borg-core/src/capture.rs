//! Captured traffic: the packets of a pcap or pcapng file, decoded down to the protocols Borg
//! reads, and put together into the conversations they belong to.

use borg_i18n::tr;
use std::{collections::HashMap, net::IpAddr, time::Duration};

mod apps;
mod decode;
mod files;
mod flows;
mod stream;

pub use apps::{App, Dns, DnsAnswer, DnsData, Http, dns_type};
pub use decode::{ACK, CWR, Datagram, ECE, FIN, Ip, Layers, Link, PSH, RST, SYN, Tcp, URG};
pub use files::PcapCounter;
pub use flows::{Flow, FlowKind};
pub use stream::{Stream, StreamRow, Turn};

/// The most packets Borg reads from one capture; the rest are counted and left out.
pub const PACKET_LIMIT: usize = 200_000;
/// Bytes on each line of a full hex dump.
const HEX_WIDTH: usize = 16;
/// What a DNS answer code says when the name does not exist.
const DNS_NO_NAME: u16 = 3;
const DNS_NO_NAME_LABEL: &str = "NXDOMAIN";
/// What separates the parts of a layer's description.
const SEPARATOR: &str = " · ";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureError {
    /// The bytes are neither pcap nor pcapng.
    Unknown,
    /// The file ends before its header does.
    Short,
}

impl CaptureError {
    pub fn describe(self) -> &'static str {
        match self {
            Self::Unknown => tr!(capture_not_pcap),
            Self::Short => tr!(capture_too_short),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet {
    /// When it was captured, since the Unix epoch.
    pub stamp: Duration,
    /// Its length on the wire; `data` holds what was captured of it.
    pub length: u32,
    pub data: Vec<u8>,
    /// The interface it was captured on, when the file names it.
    pub interface: Option<String>,
    /// The PID of the process that sent or received it, when the capture records one.
    pub pid: Option<u32>,
    pub layers: Layers,
    /// Its conversation's place in `Capture::flows`.
    pub flow: Option<usize>,
}

impl Packet {
    pub fn payload(&self) -> &[u8] {
        self.data.get(self.layers.payload.clone()).unwrap_or_default()
    }

    /// One line on what the packet says, in the protocols' own terms, the same in every
    /// language.
    pub fn summary(&self) -> String {
        let length = self.payload().len();
        let ports = self.layers.datagram.and_then(|datagram| datagram.ports());
        let route = ports.map_or_else(String::new, |(source, destination)| format!("{source} → {destination}"));
        match (&self.layers.app, self.layers.datagram) {
            (Some(App::Dns(dns)), _) => dns_summary(dns),
            (Some(App::TlsHello { server }), _) => {
                format!("ClientHello {}", server.as_deref().unwrap_or_default()).trim_end().to_owned()
            }
            (Some(App::TlsServerHello), _) => "ServerHello".into(),
            (Some(App::Http(Http::Request { method, target, .. })), _) => format!("{method} {target}"),
            (Some(App::Http(Http::Response { status, reason })), _) => format!("{status} {reason}"),
            (_, Some(Datagram::Tcp(tcp))) => format!(
                "{route} [{}] seq={} ack={} win={} len={length}",
                tcp.flag_names().join(", "),
                tcp.seq,
                tcp.ack,
                tcp.window
            ),
            (_, Some(Datagram::Udp { .. })) => format!("{route} len={length}"),
            (_, Some(Datagram::Icmp { kind, code })) => format!("type={kind} code={code}"),
            (_, None) => String::new(),
        }
    }

    /// Each layer Borg read, by its protocol's name, with what it says: the link, IP, TCP, UDP
    /// or ICMP, and DNS, TLS or HTTP. A layer on several rows names itself on the first.
    pub fn layer_rows(&self) -> Vec<(&'static str, String)> {
        let layers = &self.layers;
        let mut rows = Vec::new();
        if let (Some(source), Some(destination)) = (layers.link.source, layers.link.destination) {
            let kind = layers.link.ether_type.map_or_else(String::new, |kind| format!("{SEPARATOR}0x{kind:04x}"));
            rows.push(("Ethernet", format!("{} → {}{kind}", mac(source), mac(destination))));
        }
        if let Some(ip) = layers.ip {
            let version = if ip.source.is_ipv6() { "IPv6" } else { "IPv4" };
            let fragment = if ip.fragment { format!("{SEPARATOR}{}", tr!(traffic_fragment)) } else { String::new() };
            rows.push((version, format!("{} → {}{SEPARATOR}TTL {}{fragment}", ip.source, ip.destination, ip.ttl)));
        }
        match layers.datagram {
            Some(Datagram::Tcp(tcp)) => rows.push((
                "TCP",
                format!(
                    "{} → {}{SEPARATOR}[{}]{SEPARATOR}seq={} ack={} win={}",
                    tcp.source,
                    tcp.destination,
                    tcp.flag_names().join(", "),
                    tcp.seq,
                    tcp.ack,
                    tcp.window
                ),
            )),
            Some(Datagram::Udp { source, destination }) => rows.push(("UDP", format!("{source} → {destination}"))),
            Some(Datagram::Icmp { kind, code }) => rows.push((layers.protocol(), format!("type={kind} code={code}"))),
            None => {}
        }
        rows.extend(layers.app.as_ref().map(app_rows).unwrap_or_default());
        rows
    }
}

fn mac(address: [u8; 6]) -> String {
    address.iter().map(|byte| format!("{byte:02x}")).collect::<Vec<_>>().join(":")
}

fn app_rows(app: &App) -> Vec<(&'static str, String)> {
    let name = app.name();
    match app {
        App::Dns(dns) => {
            let mut rows: Vec<(&'static str, String)> =
                dns.questions.iter().map(|(asked, kind)| (name, format!("{} {asked}", dns_type(*kind)))).collect();
            rows.extend(dns.answers.iter().map(|answer| {
                let data = match &answer.data {
                    DnsData::Address(address) => address.to_string(),
                    DnsData::Name(name) => name.clone(),
                    DnsData::Other => String::new(),
                };
                ("", format!("→ {} {} {data}", answer.name, dns_type(answer.kind)))
            }));
            rows
        }
        App::TlsHello { server } => {
            vec![(name, format!("ClientHello{SEPARATOR}{}", server.clone().unwrap_or_default()))]
        }
        App::TlsServerHello => vec![(name, "ServerHello".to_owned())],
        App::Tls | App::Quic => Vec::new(),
        App::Http(Http::Request { method, target, host }) => {
            let mut rows = vec![(name, format!("{method} {target}"))];
            rows.extend(host.iter().map(|host| ("Host", host.clone())));
            rows
        }
        App::Http(Http::Response { status, reason }) => vec![(name, format!("{status} {reason}"))],
    }
}

fn dns_summary(dns: &Dns) -> String {
    let Some((name, kind)) = dns.questions.first() else { return String::new() };
    let asked = format!("{} {name}", dns_type(*kind));
    if !dns.response {
        return asked;
    }
    if dns.code == DNS_NO_NAME {
        return format!("{asked} {DNS_NO_NAME_LABEL}");
    }
    let answers: Vec<String> = dns
        .answers
        .iter()
        .filter_map(|answer| match &answer.data {
            DnsData::Address(address) => Some(address.to_string()),
            DnsData::Name(name) => Some(name.clone()),
            DnsData::Other => None,
        })
        .collect();
    if answers.is_empty() { asked } else { format!("{asked} → {}", answers.join(", ")) }
}

const OFFSET_DIGITS: usize = 4;
const HEX_GAP: usize = 2;
/// Cells a byte takes in hex, with the space before the next.
const HEX_CELLS: usize = 3;
const HEX_DIGIT_BITS: u32 = 4;
const FEWEST_BYTES: usize = 1;
/// Bytes on a line of a dump, from the most to the fewest, for its lines to fit their width.
const LINE_BYTES: [usize; 5] = [HEX_WIDTH, 8, 4, 2, FEWEST_BYTES];

/// How a hex dump lays out its lines: the bytes on each, and the digits of their offsets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HexLayout {
    pub columns: usize,
    pub digits: usize,
}

impl HexLayout {
    /// `HEX_WIDTH` bytes a line, for a dump of `len` bytes.
    pub fn full(len: usize) -> Self {
        Self { columns: HEX_WIDTH, digits: offset_digits(len) }
    }

    /// The most bytes a line, up to `HEX_WIDTH`, whose lines fit `width` cells, for a dump of
    /// `len` bytes.
    pub fn fitting(width: usize, len: usize) -> Self {
        let digits = offset_digits(len);
        let fits = |columns: usize| Self { columns, digits }.width() <= width;
        Self { columns: LINE_BYTES.into_iter().find(|&columns| fits(columns)).unwrap_or(FEWEST_BYTES), digits }
    }

    /// Cells a line takes: its offset, its bytes in hex and the bytes that print as text.
    pub fn width(self) -> usize {
        self.digits + HEX_GAP + self.hex_cells() + HEX_GAP + self.columns
    }

    pub fn rows(self, len: usize) -> usize {
        len.div_ceil(self.columns)
    }

    /// The line of `bytes`, found `offset` bytes into the dump.
    pub fn line(self, offset: usize, bytes: &[u8]) -> String {
        let hex: Vec<String> = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        let printed: String = bytes
            .iter()
            .map(|&byte| if byte.is_ascii_graphic() || byte == b' ' { char::from(byte) } else { '.' })
            .collect();
        let (digits, cells) = (self.digits, self.hex_cells());
        format!("{offset:0digits$x}{:HEX_GAP$}{:<cells$}{:HEX_GAP$}{printed}", "", hex.join(" "), "")
    }

    fn hex_cells(self) -> usize {
        self.columns * HEX_CELLS - 1
    }
}

/// Hex digits for the offsets of a dump of `len` bytes, at least `OFFSET_DIGITS`.
fn offset_digits(len: usize) -> usize {
    let bits = usize::BITS - len.saturating_sub(1).leading_zeros();
    (bits.div_ceil(HEX_DIGIT_BITS) as usize).max(OFFSET_DIGITS)
}

/// Bytes as a hex dump laid out as `layout` says.
pub fn hex_lines(data: &[u8], layout: HexLayout) -> Vec<String> {
    data.chunks(layout.columns).enumerate().map(|(row, chunk)| layout.line(row * layout.columns, chunk)).collect()
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Capture {
    pub packets: Vec<Packet>,
    pub flows: Vec<Flow>,
    /// The names the capture's DNS answers gave addresses.
    pub names: HashMap<IpAddr, String>,
    /// The names of the processes the capture records, by PID.
    pub processes: HashMap<u32, String>,
    /// Packets past `PACKET_LIMIT`, left out.
    pub skipped: usize,
    /// The file ends inside a packet.
    pub cut: bool,
}

impl Capture {
    /// Reads a pcap or pcapng file's packets and conversations.
    pub fn read(bytes: &[u8]) -> Result<Self, CaptureError> {
        let found = files::frames(bytes, PACKET_LIMIT)?;
        let mut packets: Vec<Packet> = found
            .frames
            .into_iter()
            .map(|frame| Packet {
                layers: decode::layers(frame.link, &frame.data),
                stamp: frame.stamp,
                length: frame.length,
                data: frame.data,
                interface: frame.interface,
                pid: frame.pid,
                flow: None,
            })
            .collect();
        let flows = flows::group(&mut packets);
        let names = flows::names(&packets);
        Ok(Self { packets, flows, names, processes: found.processes, skipped: found.skipped, cut: found.cut })
    }

    /// When the first packet was captured.
    pub fn start(&self) -> Duration {
        self.packets.iter().map(|packet| packet.stamp).min().unwrap_or_default()
    }

    /// How long from the first packet to the last.
    pub fn span(&self) -> Duration {
        self.packets.iter().map(|packet| packet.stamp).max().unwrap_or_default().saturating_sub(self.start())
    }

    pub fn bytes(&self) -> u64 {
        self.packets.iter().map(|packet| u64::from(packet.length)).sum()
    }
}

#[cfg(test)]
pub(crate) mod tests;
