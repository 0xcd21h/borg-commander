//! A frame's layers: the link header, IPv4 or IPv6, then TCP, UDP or ICMP, and where the
//! payload lies.

use super::apps::{self, App};
use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    ops::Range,
};

const LINK_NULL: u32 = 0;
const LINK_ETHERNET: u32 = 1;
const LINK_RAW: u32 = 101;
const LINK_RAW_BSD: u32 = 12;
const LINK_RAW_OPENBSD: u32 = 14;
const LINK_LOOP: u32 = 108;
const LINK_SLL: u32 = 113;
const LINK_IPV4: u32 = 228;
const LINK_IPV6: u32 = 229;
const LINK_SLL2: u32 = 276;

pub const ETHER_IPV4: u16 = 0x0800;
pub const ETHER_IPV6: u16 = 0x86dd;
pub const ETHER_ARP: u16 = 0x0806;
const ETHER_VLANS: [u16; 3] = [0x8100, 0x88a8, 0x9100];
const ETHERNET_HEADER: usize = 14;
const VLAN_TAG: usize = 4;
const MAC: usize = 6;
const SLL_HEADER: usize = 16;
const SLL2_HEADER: usize = 20;
const FAMILY_HEADER: usize = 4;
const FAMILY_INET: u32 = 2;
/// AF_INET6 as Linux, the BSDs and macOS number it.
const FAMILIES_INET6: [u32; 4] = [10, 24, 28, 30];

const IPV4_HEADER: usize = 20;
const IPV6_HEADER: usize = 40;
const FRAGMENT_OFFSET: u16 = 0x1fff;
const FRAGMENT_SHIFT: u16 = 3;

pub const PROTOCOL_ICMP: u8 = 1;
pub const PROTOCOL_TCP: u8 = 6;
pub const PROTOCOL_UDP: u8 = 17;
pub const PROTOCOL_ICMPV6: u8 = 58;
const HOP_BY_HOP: u8 = 0;
const ROUTING: u8 = 43;
const FRAGMENT: u8 = 44;
const AUTHENTICATION: u8 = 51;
const DESTINATION_OPTIONS: u8 = 60;
const EXTENSION_LIMIT: usize = 8;
const FRAGMENT_HEADER: usize = 8;
const EXTENSION_UNIT: usize = 8;
const AUTHENTICATION_UNIT: usize = 4;

const TCP_HEADER: usize = 20;
const UDP_HEADER: usize = 8;
const ICMP_HEADER: usize = 4;

/// TCP's flags, as the header's low nine bits hold them.
pub const FIN: u16 = 0x001;
pub const SYN: u16 = 0x002;
pub const RST: u16 = 0x004;
pub const PSH: u16 = 0x008;
pub const ACK: u16 = 0x010;
pub const URG: u16 = 0x020;
pub const ECE: u16 = 0x040;
pub const CWR: u16 = 0x080;
const FLAG_NAMES: [(u16, &str); 8] =
    [(SYN, "SYN"), (FIN, "FIN"), (RST, "RST"), (PSH, "PSH"), (ACK, "ACK"), (URG, "URG"), (ECE, "ECE"), (CWR, "CWR")];

/// The link header: the hardware addresses, when it has them, and the protocol it carries.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Link {
    pub source: Option<[u8; MAC]>,
    pub destination: Option<[u8; MAC]>,
    pub ether_type: Option<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ip {
    pub source: IpAddr,
    pub destination: IpAddr,
    pub ttl: u8,
    pub protocol: u8,
    /// A piece of a larger packet after the first, which carries no TCP or UDP header.
    pub fragment: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tcp {
    pub source: u16,
    pub destination: u16,
    pub seq: u32,
    pub ack: u32,
    pub flags: u16,
    pub window: u16,
}

impl Tcp {
    pub fn has(&self, flag: u16) -> bool {
        self.flags & flag != 0
    }

    /// The flags that are set, by name.
    pub fn flag_names(&self) -> Vec<&'static str> {
        FLAG_NAMES.iter().filter(|(flag, _)| self.has(*flag)).map(|(_, name)| *name).collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Datagram {
    Tcp(Tcp),
    Udp { source: u16, destination: u16 },
    Icmp { kind: u8, code: u8 },
}

impl Datagram {
    pub fn ports(&self) -> Option<(u16, u16)> {
        match self {
            Self::Tcp(tcp) => Some((tcp.source, tcp.destination)),
            Self::Udp { source, destination } => Some((*source, *destination)),
            Self::Icmp { .. } => None,
        }
    }
}

/// What Borg reads of a packet.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Layers {
    pub link: Link,
    pub ip: Option<Ip>,
    pub datagram: Option<Datagram>,
    /// Where the TCP or UDP payload lies in the captured bytes.
    pub payload: Range<usize>,
    /// How many payload bytes the packet carried, as its IP header tells, where the capture
    /// kept fewer.
    pub carried: usize,
    pub app: Option<App>,
}

impl Layers {
    /// The topmost protocol Borg recognized, by its usual name.
    pub fn protocol(&self) -> &'static str {
        if let Some(app) = &self.app {
            return app.name();
        }
        match (self.datagram, self.ip, self.link.ether_type) {
            (Some(Datagram::Tcp(_)), _, _) => "TCP",
            (Some(Datagram::Udp { .. }), _, _) => "UDP",
            (Some(Datagram::Icmp { .. }), Some(ip), _) if ip.protocol == PROTOCOL_ICMPV6 => "ICMPv6",
            (Some(Datagram::Icmp { .. }), _, _) => "ICMP",
            (None, Some(ip), _) if ip.source.is_ipv6() => "IPv6",
            (None, Some(_), _) => "IPv4",
            (None, None, Some(ETHER_ARP)) => "ARP",
            _ => "Ethernet",
        }
    }
}

/// Reads the layers of a packet captured on a link of type `link`.
pub(crate) fn layers(link: u32, data: &[u8]) -> Layers {
    let mut layers = Layers::default();
    let Some((ether_type, at)) = link_header(link, data, &mut layers.link) else { return layers };
    layers.link.ether_type = Some(ether_type);
    let Some((ip, at, announced)) = network(ether_type, data, at) else { return layers };
    layers.ip = Some(ip);
    if ip.fragment {
        return layers;
    }
    let end = announced.min(data.len());
    let Some((datagram, start)) = transport(ip.protocol, data, at, end) else { return layers };
    layers.datagram = Some(datagram);
    layers.payload = start..end;
    layers.carried = announced.max(end) - start;
    layers.app = apps::detect(&datagram, &data[start..end]);
    layers
}

fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(data.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

fn mac(data: &[u8], at: usize) -> Option<[u8; MAC]> {
    data.get(at..at + MAC)?.try_into().ok()
}

/// The protocol the link header announces and where it ends.
fn link_header(link: u32, data: &[u8], found: &mut Link) -> Option<(u16, usize)> {
    match link {
        LINK_ETHERNET => {
            found.destination = mac(data, 0);
            found.source = mac(data, MAC);
            let mut at = ETHERNET_HEADER;
            let mut ether_type = u16_at(data, at - 2)?;
            while ETHER_VLANS.contains(&ether_type) {
                at += VLAN_TAG;
                ether_type = u16_at(data, at - 2)?;
            }
            Some((ether_type, at))
        }
        LINK_NULL | LINK_LOOP => {
            let raw: [u8; 4] = data.get(..FAMILY_HEADER)?.try_into().ok()?;
            let family =
                if link == LINK_LOOP || raw[0] == 0 { u32::from_be_bytes(raw) } else { u32::from_le_bytes(raw) };
            let ether_type = if family == FAMILY_INET {
                ETHER_IPV4
            } else if FAMILIES_INET6.contains(&family) {
                ETHER_IPV6
            } else {
                return None;
            };
            Some((ether_type, FAMILY_HEADER))
        }
        LINK_RAW | LINK_RAW_BSD | LINK_RAW_OPENBSD => version(data, 0).map(|ether_type| (ether_type, 0)),
        LINK_IPV4 => Some((ETHER_IPV4, 0)),
        LINK_IPV6 => Some((ETHER_IPV6, 0)),
        LINK_SLL => Some((u16_at(data, SLL_HEADER - 2)?, SLL_HEADER)),
        LINK_SLL2 => Some((u16_at(data, 0)?, SLL2_HEADER)),
        _ => None,
    }
}

/// The protocol a raw IP packet's version says it is.
fn version(data: &[u8], at: usize) -> Option<u16> {
    match data.get(at)? >> 4 {
        4 => Some(ETHER_IPV4),
        6 => Some(ETHER_IPV6),
        _ => None,
    }
}

/// The IP header at `at`, where what it carries starts, and where the packet ends as the
/// header tells, which may be past the bytes the capture kept.
fn network(ether_type: u16, data: &[u8], at: usize) -> Option<(Ip, usize, usize)> {
    match ether_type {
        ETHER_IPV4 => ipv4(data, at),
        ETHER_IPV6 => ipv6(data, at),
        _ => None,
    }
}

fn ipv4(data: &[u8], at: usize) -> Option<(Ip, usize, usize)> {
    let first = *data.get(at)?;
    let header = usize::from(first & 0x0f) * 4;
    if first >> 4 != 4 || header < IPV4_HEADER {
        return None;
    }
    let total = usize::from(u16_at(data, at + 2)?);
    let fragment = u16_at(data, at + 6)?;
    let address = |offset: usize| -> Option<IpAddr> {
        let raw: [u8; 4] = data.get(at + offset..at + offset + 4)?.try_into().ok()?;
        Some(IpAddr::V4(Ipv4Addr::from(raw)))
    };
    let ip = Ip {
        source: address(12)?,
        destination: address(16)?,
        ttl: *data.get(at + 8)?,
        protocol: *data.get(at + 9)?,
        fragment: fragment & FRAGMENT_OFFSET != 0,
    };
    Some((ip, at + header, at + total.max(header)))
}

fn ipv6(data: &[u8], at: usize) -> Option<(Ip, usize, usize)> {
    if data.get(at)? >> 4 != 6 {
        return None;
    }
    let length = usize::from(u16_at(data, at + 4)?);
    let address = |offset: usize| -> Option<IpAddr> {
        let raw: [u8; 16] = data.get(at + offset..at + offset + 16)?.try_into().ok()?;
        Some(IpAddr::V6(Ipv6Addr::from(raw)))
    };
    let mut ip = Ip {
        source: address(8)?,
        destination: address(24)?,
        ttl: *data.get(at + 7)?,
        protocol: *data.get(at + 6)?,
        fragment: false,
    };
    let end = at + IPV6_HEADER + length;
    let mut next = at + IPV6_HEADER;
    for _ in 0..EXTENSION_LIMIT {
        let length = match ip.protocol {
            HOP_BY_HOP | ROUTING | DESTINATION_OPTIONS => (usize::from(*data.get(next + 1)?) + 1) * EXTENSION_UNIT,
            AUTHENTICATION => (usize::from(*data.get(next + 1)?) + 2) * AUTHENTICATION_UNIT,
            FRAGMENT => {
                ip.fragment = u16_at(data, next + 2)? >> FRAGMENT_SHIFT != 0;
                FRAGMENT_HEADER
            }
            _ => break,
        };
        ip.protocol = *data.get(next)?;
        next += length;
    }
    Some((ip, next, end))
}

/// The TCP, UDP or ICMP header at `at`, and where its payload starts.
fn transport(protocol: u8, data: &[u8], at: usize, end: usize) -> Option<(Datagram, usize)> {
    match protocol {
        PROTOCOL_TCP => {
            let offset = usize::from(data.get(at + 12)? >> 4) * 4;
            if offset < TCP_HEADER {
                return None;
            }
            let tcp = Tcp {
                source: u16_at(data, at)?,
                destination: u16_at(data, at + 2)?,
                seq: u32_at(data, at + 4)?,
                ack: u32_at(data, at + 8)?,
                flags: u16_at(data, at + 12)? & 0x01ff,
                window: u16_at(data, at + 14)?,
            };
            Some((Datagram::Tcp(tcp), (at + offset).min(end)))
        }
        PROTOCOL_UDP => {
            let datagram = Datagram::Udp { source: u16_at(data, at)?, destination: u16_at(data, at + 2)? };
            Some((datagram, (at + UDP_HEADER).min(end)))
        }
        PROTOCOL_ICMP | PROTOCOL_ICMPV6 => {
            let datagram = Datagram::Icmp { kind: *data.get(at)?, code: *data.get(at + 1)? };
            Some((datagram, (at + ICMP_HEADER).min(end)))
        }
        _ => None,
    }
}
