//! Conversations: the packets between two endpoints over one protocol, the side that began, what
//! each side sent, and the server's name when the conversation gives it.

use super::{
    Packet,
    apps::{self, App, DnsData, Http},
    decode::{ACK, Datagram, SYN, Tcp},
};
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    time::Duration,
};

/// How much of a client's first bytes Borg puts together to read the server's name.
const STREAM_LIMIT: usize = 16 * 1024;
/// Ports below this belong to servers, for a conversation captured after it began.
const SERVER_PORTS: u16 = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FlowKind {
    Tcp,
    Udp,
    Icmp,
}

impl FlowKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Tcp => "TCP",
            Self::Udp => "UDP",
            Self::Icmp => "ICMP",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Flow {
    pub kind: FlowKind,
    /// The side that began: the one that sent the first SYN, or else the one that spoke first.
    pub client: SocketAddr,
    pub server: SocketAddr,
    /// The conversation's packets, by their place in the capture.
    pub packets: Vec<usize>,
    /// Bytes on the wire each way.
    pub to_server: u64,
    pub to_client: u64,
    pub first: Duration,
    pub last: Duration,
    /// The protocol the conversation carries, when Borg recognized one.
    pub app: Option<&'static str>,
    /// The server's name as the conversation gives it: the name a TLS client asks for, HTTP's
    /// Host, or the name a DNS query asks about.
    pub server_name: Option<String>,
}

impl Flow {
    fn begin(kind: FlowKind, source: SocketAddr, destination: SocketAddr, packet: &Packet) -> Self {
        let answered = matches!(packet.layers.datagram, Some(Datagram::Tcp(tcp)) if tcp.has(SYN) && tcp.has(ACK));
        let from_server = source.port() < SERVER_PORTS && destination.port() >= SERVER_PORTS;
        let (client, server) = if answered || from_server { (destination, source) } else { (source, destination) };
        Self {
            kind,
            client,
            server,
            packets: Vec::new(),
            to_server: 0,
            to_client: 0,
            first: packet.stamp,
            last: packet.stamp,
            app: None,
            server_name: None,
        }
    }

    fn add(&mut self, number: usize, packet: &Packet, source: SocketAddr) {
        self.packets.push(number);
        if source == self.client {
            self.to_server += u64::from(packet.length);
        } else {
            self.to_client += u64::from(packet.length);
        }
        self.last = self.last.max(packet.stamp);
        self.app = self.app.or(packet.layers.app.as_ref().map(App::name));
    }

    pub fn bytes(&self) -> u64 {
        self.to_server + self.to_client
    }

    /// Whether the client sent `packet`.
    pub fn from_client(&self, packet: &Packet) -> bool {
        endpoints(packet).is_some_and(|(_, source, _)| source == self.client)
    }

    pub fn duration(&self) -> Duration {
        self.last.saturating_sub(self.first)
    }
}

/// The protocol and both ends of a packet's conversation, sender first.
pub(super) fn endpoints(packet: &Packet) -> Option<(FlowKind, SocketAddr, SocketAddr)> {
    let ip = packet.layers.ip?;
    let (kind, (source, destination)) = match packet.layers.datagram? {
        Datagram::Tcp(tcp) => (FlowKind::Tcp, (tcp.source, tcp.destination)),
        Datagram::Udp { source, destination } => (FlowKind::Udp, (source, destination)),
        Datagram::Icmp { .. } => (FlowKind::Icmp, (0, 0)),
    };
    Some((kind, SocketAddr::new(ip.source, source), SocketAddr::new(ip.destination, destination)))
}

/// Puts each packet in its conversation, and gives each conversation its server's name.
pub(super) fn group(packets: &mut [Packet]) -> Vec<Flow> {
    let mut flows: Vec<Flow> = Vec::new();
    let mut known: HashMap<(FlowKind, SocketAddr, SocketAddr), usize> = HashMap::new();
    for (number, packet) in packets.iter_mut().enumerate() {
        let Some((kind, source, destination)) = endpoints(packet) else { continue };
        let key = (kind, source.min(destination), source.max(destination));
        let flow = *known.entry(key).or_insert_with(|| {
            flows.push(Flow::begin(kind, source, destination, packet));
            flows.len() - 1
        });
        packet.flow = Some(flow);
        flows[flow].add(number, packet, source);
    }
    for flow in &mut flows {
        flow.server_name = server_name(flow, packets);
    }
    flows
}

fn server_name(flow: &Flow, packets: &[Packet]) -> Option<String> {
    let apps = || flow.packets.iter().filter_map(|&number| packets[number].layers.app.as_ref());
    let asked = apps().find_map(|app| match app {
        App::Dns(dns) => dns.questions.first().map(|(name, _)| name.clone()),
        App::Http(Http::Request { host, .. }) => host.clone(),
        _ => None,
    });
    asked.or_else(|| {
        apps()
            .any(|app| matches!(app, App::TlsHello { .. }))
            .then(|| apps::server_name(&client_stream(flow, packets)))?
    })
}

/// The first bytes the client sent over TCP, put back in order.
fn client_stream(flow: &Flow, packets: &[Packet]) -> Vec<u8> {
    let sent: Vec<(Tcp, &[u8])> = flow
        .packets
        .iter()
        .map(|&number| &packets[number])
        .filter(|packet| flow.from_client(packet))
        .filter_map(|packet| match packet.layers.datagram {
            Some(Datagram::Tcp(tcp)) => Some((tcp, packet.payload())),
            _ => None,
        })
        .collect();
    let opened = sent.iter().find(|(tcp, _)| tcp.has(SYN)).map(|(tcp, _)| tcp.seq.wrapping_add(1));
    let mut segments: Vec<(u32, &[u8])> =
        sent.iter().filter(|(_, payload)| !payload.is_empty()).map(|(tcp, payload)| (tcp.seq, *payload)).collect();
    let Some(base) = opened.or_else(|| segments.iter().map(|(seq, _)| *seq).min()) else { return Vec::new() };
    segments.sort_by_key(|(seq, _)| seq.wrapping_sub(base));
    let mut stream = Vec::new();
    for (seq, payload) in segments {
        let offset = seq.wrapping_sub(base) as usize;
        if offset > stream.len() {
            break;
        }
        stream.extend(payload.iter().skip(stream.len() - offset));
        let wanted = apps::hello_length(&stream).unwrap_or(STREAM_LIMIT).min(STREAM_LIMIT);
        if stream.len() >= wanted {
            break;
        }
    }
    stream
}

/// The names DNS answers in the capture gave addresses: the name each question asked about.
pub(super) fn names(packets: &[Packet]) -> HashMap<IpAddr, String> {
    let mut names = HashMap::new();
    for packet in packets {
        let Some(App::Dns(dns)) = &packet.layers.app else { continue };
        let Some((asked, _)) = dns.questions.first().filter(|_| dns.response) else { continue };
        for answer in &dns.answers {
            if let DnsData::Address(address) = answer.data {
                names.entry(address).or_insert_with(|| asked.clone());
            }
        }
    }
    names
}
