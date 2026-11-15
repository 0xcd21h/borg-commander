//! What the hosts on a link say about themselves in the broadcast and multicast traffic Borg
//! hears: their addresses, names, models and services, and the switch port this machine is
//! plugged into.

use super::dns::{self, Data, Message, Record};
use crate::capture::{Datagram, layers};
use crate::network::Mac;
use serde::{Deserialize, Serialize};
use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    ops::RangeInclusive,
};

const LINK_ETHERNET: u32 = 1;
const MAC_BYTES: usize = 6;
const IPV4_BYTES: usize = 4;
const ETHERNET_HEADER: usize = 14;
const ETHER_TYPE_BYTES: usize = 2;
const VLAN_TAG: usize = 4;
const ETHER_VLANS: [u16; 3] = [0x8100, 0x88a8, 0x9100];
/// Type fields below this hold an 802.3 frame's length.
const ETHER_TYPES: u16 = 0x0600;
const ETHER_ARP: u16 = 0x0806;
const ETHER_LLDP: u16 = 0x88cc;

const ARP_ETHERNET: u16 = 1;
const ARP_IPV4: u16 = 0x0800;
const ARP_PROTOCOL_AT: usize = 2;
const ARP_SIZES: [u8; 2] = [6, 4];
const ARP_SIZES_AT: usize = 4;
const ARP_SENDER_MAC: usize = 8;
const ARP_SENDER_ADDRESS: usize = 14;

const NDP_ROUTER_SOLICITATION: u8 = 133;
const NDP_ROUTER_ADVERTISEMENT: u8 = 134;
const NDP_NEIGHBOR_SOLICITATION: u8 = 135;
const NDP_NEIGHBOR_ADVERTISEMENT: u8 = 136;
/// Where the options start in each message, after the ICMPv6 header.
const SOLICITATION_OPTIONS: usize = 4;
const ADVERTISEMENT_OPTIONS: usize = 12;
const TARGET: usize = 4;
const TARGET_OPTIONS: usize = 20;
const IPV6_BYTES: usize = 16;
const OPTION_SOURCE_LINK: u8 = 1;
const OPTION_TARGET_LINK: u8 = 2;
const OPTION_UNIT: usize = 8;
const OPTION_HEADER: usize = 2;
const ADVERTISED_BY_ROUTER: u8 = 0x80;

const PORT_MDNS: u16 = 5353;
const PORT_LLMNR: u16 = 5355;
const PORT_NETBIOS: u16 = 137;
const PORT_SSDP: u16 = 1900;
const PORT_DHCP_SERVER: u16 = 67;
const PORT_DHCP_CLIENT: u16 = 68;
const PORTS: [(u16, ListenProtocol); 6] = [
    (PORT_MDNS, ListenProtocol::Mdns),
    (PORT_LLMNR, ListenProtocol::Llmnr),
    (PORT_NETBIOS, ListenProtocol::Netbios),
    (PORT_SSDP, ListenProtocol::Ssdp),
    (PORT_DHCP_SERVER, ListenProtocol::Dhcp),
    (PORT_DHCP_CLIENT, ListenProtocol::Dhcp),
];

const LOCAL_DOMAIN: &str = ".local";
const REVERSE_IPV4: &str = ".in-addr.arpa";
const REVERSE_IPV6: &str = ".ip6.arpa";
const NIBBLES: usize = 32;
const HEX: u32 = 16;
/// TXT keys that name a device's model, the most telling first.
const MODEL_KEYS: [&str; 5] = ["ty", "md", "usb_MDL", "model", "am"];
const SERVICE_PROTOCOLS: [&str; 2] = ["_tcp", "_udp"];
const NOT_SERVICES: [&str; 2] = ["dns-sd", "device-info"];

const NETBIOS_QUERY: u8 = 0;
/// Registration, the two refresh codes and multihomed registration.
const NETBIOS_REGISTRATIONS: [u8; 4] = [5, 8, 9, 15];
const NETBIOS_ENCODED: usize = 32;
const NETBIOS_NAME: usize = 15;
const NETBIOS_GROUP: u16 = 0x8000;
/// The workstation and file server suffixes, which name the machine.
const NETBIOS_SUFFIXES: [u8; 2] = [0x00, 0x20];
const NETBIOS_ENCODING: RangeInclusive<u8> = b'A'..=b'P';
const NIBBLE_BITS: u8 = 4;
const TYPE_NB: u16 = 0x20;
const NB_ENTRY: usize = 6;
const NB_ADDRESS: usize = 2;

const SSDP_NOTIFY: &str = "NOTIFY";
const SSDP_RESPONSE: &str = "HTTP/";
const SSDP_SERVER: &str = "server";
const SSDP_TYPES: [&str; 2] = ["nt", "st"];
const UPNP_URN: &str = "urn";
const UPNP_DEVICE: &str = "device";
const UPNP_SERVICE: &str = "service";

const BOOTREQUEST: u8 = 1;
const BOOTREPLY: u8 = 2;
const HARDWARE: [u8; 2] = [1, 6];
const HARDWARE_AT: usize = 1;
const CLIENT_ADDRESS: usize = 12;
const YOUR_ADDRESS: usize = 16;
const CLIENT_HARDWARE: usize = 28;
const MAGIC_COOKIE: [u8; 4] = [0x63, 0x82, 0x53, 0x63];
const COOKIE_AT: usize = 236;
const OPTIONS_AT: usize = 240;
const OPTION_PAD: u8 = 0;
const OPTION_HEAD: usize = 2;
const OPTION_END: u8 = 255;
const OPTION_HOST_NAME: u8 = 12;
const OPTION_REQUESTED_ADDRESS: u8 = 50;
const OPTION_MESSAGE_TYPE: u8 = 53;
const OPTION_VENDOR_CLASS: u8 = 60;
const OPTION_CLIENT_FQDN: u8 = 81;
const DHCP_REQUEST: u8 = 3;
const DHCP_ACK: u8 = 5;
const FQDN_NAME: usize = 3;
const FQDN_WIRE_FORMAT: u8 = 0x04;

const LLDP_TYPE_SHIFT: u16 = 9;
const LLDP_LENGTH_MASK: u16 = 0x01ff;
const LLDP_UNIT_HEADER: usize = 2;
const LLDP_END: u16 = 0;
const LLDP_CHASSIS: u16 = 1;
const LLDP_PORT: u16 = 2;
const LLDP_PORT_DESCRIPTION: u16 = 4;
const LLDP_SYSTEM_NAME: u16 = 5;
const LLDP_SYSTEM_DESCRIPTION: u16 = 6;
const LLDP_MANAGEMENT: u16 = 8;
/// Chassis IDs given as an interface alias, an interface name or a local name.
const CHASSIS_TEXTS: [u8; 3] = [2, 6, 7];
const CHASSIS_MAC: u8 = 4;
/// Port IDs given as an interface alias, an interface name or a local name.
const PORT_TEXTS: [u8; 3] = [1, 5, 7];
const PORT_MAC: u8 = 3;
const MANAGEMENT_IPV4: u8 = 1;
const MANAGEMENT_IPV6: u8 = 2;

const CDP_ADDRESS: Mac = Mac([0x01, 0x00, 0x0c, 0xcc, 0xcc, 0xcc]);
const CDP_SNAP: [u8; 8] = [0xaa, 0xaa, 0x03, 0x00, 0x00, 0x0c, 0x20, 0x00];
const CDP_HEADER: usize = 4;
const CDP_UNIT_HEADER: usize = 4;
const CDP_LENGTH_AT: usize = 2;
/// An address entry's protocol type and protocol length bytes.
const CDP_PROTOCOL_HEAD: usize = 2;
const CDP_DEVICE: u16 = 0x0001;
const CDP_ADDRESSES: u16 = 0x0002;
const CDP_PORT: u16 = 0x0003;
const CDP_PLATFORM: u16 = 0x0006;
const CDP_COUNT: usize = 4;
/// An address entry's protocol as CDP writes IPv4: NLPID type, length 1, protocol 0xcc.
const CDP_IPV4: [u8; 3] = [0x01, 0x01, 0xcc];
const CDP_ADDRESS_LENGTH: usize = 2;

/// The longest name, model or service Borg keeps from an announcement.
pub const LONGEST_TEXT: usize = 80;
/// Characters that change how text around them shows without showing themselves.
const INVISIBLE: [RangeInclusive<char>; 4] =
    ['\u{200b}'..='\u{200f}', '\u{202a}'..='\u{202e}', '\u{2060}'..='\u{2069}', '\u{feff}'..='\u{feff}'];

/// The protocols Borg listens to. The order is how much a name each one announces is trusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ListenProtocol {
    Mdns,
    Dhcp,
    Netbios,
    Llmnr,
    Lldp,
    Cdp,
    Ssdp,
    Arp,
    Ndp,
}

impl ListenProtocol {
    pub const ALL: [Self; 9] =
        [Self::Mdns, Self::Dhcp, Self::Netbios, Self::Llmnr, Self::Lldp, Self::Cdp, Self::Ssdp, Self::Arp, Self::Ndp];

    pub fn name(self) -> &'static str {
        match self {
            Self::Mdns => "mDNS",
            Self::Dhcp => "DHCP",
            Self::Netbios => "NetBIOS",
            Self::Llmnr => "LLMNR",
            Self::Lldp => "LLDP",
            Self::Cdp => "CDP",
            Self::Ssdp => "SSDP",
            Self::Arp => "ARP",
            Self::Ndp => "NDP",
        }
    }
}

/// What one packet says about one host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Announcement {
    pub protocol: ListenProtocol,
    pub address: Option<IpAddr>,
    pub mac: Option<Mac>,
    pub name: Option<String>,
    pub model: Option<String>,
    /// Device types, as UPnP names them.
    pub kinds: Vec<String>,
    pub services: Vec<String>,
    /// The vendor class a DHCP client sends, which names its operating system or firmware.
    pub client: Option<String>,
    pub router: bool,
}

impl Announcement {
    pub fn new(protocol: ListenProtocol) -> Self {
        Self {
            protocol,
            address: None,
            mac: None,
            name: None,
            model: None,
            kinds: Vec::new(),
            services: Vec::new(),
            client: None,
            router: false,
        }
    }

    fn of(protocol: ListenProtocol, address: IpAddr) -> Self {
        Self { address: Some(address), ..Self::new(protocol) }
    }
}

/// The switch, or whatever device sits at the other end of the cable, as LLDP or CDP names it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwitchPort {
    pub protocol: ListenProtocol,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<IpAddr>,
}

impl SwitchPort {
    fn new(protocol: ListenProtocol) -> Self {
        Self { protocol, name: None, port: None, model: None, address: None }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Heard {
    Host(Announcement),
    Switch(SwitchPort),
}

/// Everything a frame captured on a link of type `link` announces.
pub fn hear(link: u32, data: &[u8]) -> Vec<Heard> {
    if link == LINK_ETHERNET
        && let Some(frame) = ethernet(data)
    {
        let body = data.get(frame.body..).unwrap_or_default();
        match frame.kind {
            ETHER_ARP => return arp(body).map(Heard::Host).into_iter().collect(),
            ETHER_LLDP => return lldp(body),
            length if length < ETHER_TYPES => return cdp(frame.destination, body),
            _ => {}
        }
    }
    carried(link, data)
}

/// Cleans announced text for showing: its first line without control or invisible
/// characters, cut to [`LONGEST_TEXT`].
pub fn clean(bytes: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(bytes);
    let line = text.split(['\r', '\n']).map(str::trim).find(|line| !line.is_empty())?;
    let kept: String = line
        .chars()
        .filter(|c| !c.is_control() && !INVISIBLE.iter().any(|range| range.contains(c)))
        .take(LONGEST_TEXT)
        .collect();
    let kept = kept.trim();
    (!kept.is_empty()).then(|| kept.to_owned())
}

struct Frame {
    destination: Mac,
    kind: u16,
    body: usize,
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn mac_at(bytes: &[u8], at: usize) -> Option<Mac> {
    Some(Mac(bytes.get(at..at + MAC_BYTES)?.try_into().ok()?))
}

fn ipv4_at(bytes: &[u8], at: usize) -> Option<Ipv4Addr> {
    Some(Ipv4Addr::from(<[u8; IPV4_BYTES]>::try_from(bytes.get(at..at + IPV4_BYTES)?).ok()?))
}

fn ipv6_at(bytes: &[u8], at: usize) -> Option<Ipv6Addr> {
    Some(Ipv6Addr::from(<[u8; IPV6_BYTES]>::try_from(bytes.get(at..at + IPV6_BYTES)?).ok()?))
}

fn ethernet(data: &[u8]) -> Option<Frame> {
    let destination = mac_at(data, 0)?;
    let mut at = ETHERNET_HEADER;
    let mut kind = u16_at(data, at - ETHER_TYPE_BYTES)?;
    while ETHER_VLANS.contains(&kind) {
        at += VLAN_TAG;
        kind = u16_at(data, at - ETHER_TYPE_BYTES)?;
    }
    Some(Frame { destination, kind, body: at })
}

fn arp(body: &[u8]) -> Option<Announcement> {
    let sizes = body.get(ARP_SIZES_AT..ARP_SIZES_AT + ARP_SIZES.len())?;
    if u16_at(body, 0)? != ARP_ETHERNET || u16_at(body, ARP_PROTOCOL_AT)? != ARP_IPV4 || sizes != ARP_SIZES {
        return None;
    }
    let address = ipv4_at(body, ARP_SENDER_ADDRESS)?;
    if address.is_unspecified() {
        return None;
    }
    Some(Announcement {
        mac: Some(mac_at(body, ARP_SENDER_MAC)?),
        ..Announcement::of(ListenProtocol::Arp, address.into())
    })
}

/// What an IP packet announces, with its sender among the hosts.
fn carried(link: u32, data: &[u8]) -> Vec<Heard> {
    let layers = layers(link, data);
    let (Some(ip), Some(datagram)) = (layers.ip, layers.datagram) else { return Vec::new() };
    let body = data.get(layers.payload.clone()).unwrap_or_default();
    let mac = match (layers.link.source, layers.link.destination) {
        (Some(source), Some(destination)) if Mac(destination).group() => Some(Mac(source)),
        _ => None,
    };
    let (protocol, mut heard) = match datagram {
        Datagram::Udp { source, destination } => {
            let Some(protocol) = by_port(source, destination) else { return Vec::new() };
            (protocol, spoken(protocol, body, ip.source))
        }
        Datagram::Icmp { kind, .. } if ip.source.is_ipv6() => {
            let Some(announcement) = ndp(kind, body, ip.source, mac) else { return Vec::new() };
            (ListenProtocol::Ndp, vec![announcement])
        }
        _ => return Vec::new(),
    };
    pair(&mut heard, protocol, ip.source, mac);
    heard.into_iter().map(Heard::Host).collect()
}

fn by_port(source: u16, destination: u16) -> Option<ListenProtocol> {
    PORTS.iter().find(|(port, _)| *port == source || *port == destination).map(|(_, protocol)| *protocol)
}

/// Adds the sender to what its packet announced, with the frame's hardware address.
fn pair(heard: &mut Vec<Announcement>, protocol: ListenProtocol, source: IpAddr, mac: Option<Mac>) {
    let broadcast = matches!(source, IpAddr::V4(address) if address.is_broadcast());
    if source.is_unspecified() || source.is_multicast() || broadcast {
        return;
    }
    match heard.iter_mut().find(|announcement| announcement.address == Some(source)) {
        Some(sender) => sender.mac = sender.mac.or(mac),
        None => heard.push(Announcement { mac, ..Announcement::of(protocol, source) }),
    }
}

fn spoken(protocol: ListenProtocol, body: &[u8], source: IpAddr) -> Vec<Announcement> {
    match protocol {
        ListenProtocol::Mdns => dns::read(body).map(|message| mdns(&message, source)).unwrap_or_default(),
        ListenProtocol::Llmnr => dns::read(body).map(|message| llmnr(&message)).unwrap_or_default(),
        ListenProtocol::Netbios => dns::read(body).map(|message| netbios(&message)).unwrap_or_default(),
        ListenProtocol::Ssdp => ssdp(body, source).into_iter().collect(),
        ListenProtocol::Dhcp => dhcp(body).into_iter().collect(),
        _ => Vec::new(),
    }
}

fn link_option(options: &[u8], wanted: u8) -> Option<Mac> {
    let mut at = 0;
    while let (Some(&kind), Some(&units)) = (options.get(at), options.get(at + 1)) {
        if units == 0 {
            return None;
        }
        if kind == wanted {
            return mac_at(options, at + OPTION_HEADER);
        }
        at += usize::from(units) * OPTION_UNIT;
    }
    None
}

/// What a neighbor discovery message says, where `body` follows the ICMPv6 header.
fn ndp(kind: u8, body: &[u8], source: IpAddr, frame: Option<Mac>) -> Option<Announcement> {
    let options = |at: usize| body.get(at..).unwrap_or_default();
    let target = || ipv6_at(body, TARGET).map(IpAddr::V6);
    let (address, mac, router) = match kind {
        NDP_ROUTER_SOLICITATION => (source, link_option(options(SOLICITATION_OPTIONS), OPTION_SOURCE_LINK), false),
        NDP_ROUTER_ADVERTISEMENT => (source, link_option(options(ADVERTISEMENT_OPTIONS), OPTION_SOURCE_LINK), true),
        NDP_NEIGHBOR_SOLICITATION if source.is_unspecified() => (target()?, None, false),
        NDP_NEIGHBOR_SOLICITATION => (source, link_option(options(TARGET_OPTIONS), OPTION_SOURCE_LINK), false),
        NDP_NEIGHBOR_ADVERTISEMENT => (
            target()?,
            link_option(options(TARGET_OPTIONS), OPTION_TARGET_LINK),
            body.first()? & ADVERTISED_BY_ROUTER != 0,
        ),
        _ => return None,
    };
    if address.is_unspecified() || address.is_multicast() {
        return None;
    }
    Some(Announcement { mac: mac.or(frame), router, ..Announcement::of(ListenProtocol::Ndp, address) })
}

fn strip_suffix<'a>(name: &'a str, suffix: &str) -> Option<&'a str> {
    let at = name.len().checked_sub(suffix.len())?;
    name.get(at..)?.eq_ignore_ascii_case(suffix).then(|| &name[..at])
}

fn host_name(name: &str) -> Option<String> {
    clean(strip_suffix(name, LOCAL_DOMAIN).unwrap_or(name).as_bytes())
}

/// The address a reverse lookup name such as `44.2.0.192.in-addr.arpa` stands for.
fn reversed(name: &str) -> Option<IpAddr> {
    if let Some(digits) = strip_suffix(name, REVERSE_IPV4) {
        let mut octets = digits.split('.').map(|digit| digit.parse::<u8>().ok()).collect::<Option<Vec<_>>>()?;
        octets.reverse();
        return Some(IpAddr::V4(Ipv4Addr::from(<[u8; IPV4_BYTES]>::try_from(octets).ok()?)));
    }
    let nibbles: Vec<&str> = strip_suffix(name, REVERSE_IPV6)?.split('.').collect();
    let hex =
        nibbles.len() == NIBBLES && nibbles.iter().all(|n| n.len() == 1 && n.chars().all(|c| c.is_ascii_hexdigit()));
    if !hex {
        return None;
    }
    let digits: String = nibbles.iter().rev().copied().collect();
    Some(IpAddr::V6(Ipv6Addr::from(u128::from_str_radix(&digits, HEX).ok()?)))
}

/// The hosts the address records and reverse pointers among `records` name.
fn named(records: &[&Record], protocol: ListenProtocol) -> Vec<Announcement> {
    let mut heard: Vec<Announcement> = Vec::new();
    for record in records {
        let (address, name) = match &record.data {
            Data::Address(address) => (*address, record.name.as_str()),
            Data::Pointer(target) => match reversed(&record.name) {
                Some(address) => (address, target.as_str()),
                None => continue,
            },
            _ => continue,
        };
        if !heard.iter().any(|announcement| announcement.address == Some(address)) {
            heard.push(Announcement { name: host_name(name), ..Announcement::of(protocol, address) });
        }
    }
    heard
}

/// The service a DNS-SD name such as `Printer._ipp._tcp.local` belongs to.
fn service_type(name: &str) -> Option<String> {
    let labels: Vec<&str> = name.split('.').collect();
    let at = labels.iter().position(|label| SERVICE_PROTOCOLS.iter().any(|kind| label.eq_ignore_ascii_case(kind)))?;
    let service = labels.get(at.checked_sub(1)?)?.strip_prefix('_')?;
    if NOT_SERVICES.iter().any(|skipped| service.eq_ignore_ascii_case(skipped)) {
        return None;
    }
    clean(service.as_bytes())
}

fn services(records: &[&Record]) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for record in records {
        let target = match &record.data {
            Data::Pointer(target) => Some(target.as_str()),
            _ => None,
        };
        for name in std::iter::once(record.name.as_str()).chain(target) {
            if let Some(service) = service_type(name)
                && !found.contains(&service)
            {
                found.push(service);
            }
        }
    }
    found
}

fn pairs(record: &Record) -> &[(String, String)] {
    match &record.data {
        Data::Text(pairs) => pairs,
        _ => &[],
    }
}

fn model(records: &[&Record]) -> Option<String> {
    MODEL_KEYS.iter().find_map(|key| {
        let mut values = records.iter().flat_map(|record| pairs(record));
        values
            .find(|(found, value)| found.eq_ignore_ascii_case(key) && !value.is_empty())
            .and_then(|(_, value)| clean(value.as_bytes()))
    })
}

/// What an mDNS message announces. A response speaks in its answers and additional records, a
/// probe in its authority records; the records a query lists as known are about other hosts.
fn mdns(message: &Message, source: IpAddr) -> Vec<Announcement> {
    let records: Vec<&Record> = if message.response {
        message.answers.iter().chain(&message.additional).collect()
    } else {
        message.authority.iter().collect()
    };
    let mut heard = named(&records, ListenProtocol::Mdns);
    let services = services(&records);
    let model = model(&records);
    if services.is_empty() && model.is_none() {
        return heard;
    }
    let targets: Vec<&str> = records
        .iter()
        .filter_map(|record| match &record.data {
            Data::Service { target } => Some(target.as_str()),
            _ => None,
        })
        .collect();
    let mut subjects: Vec<IpAddr> = records
        .iter()
        .filter(|record| targets.iter().any(|target| target.eq_ignore_ascii_case(&record.name)))
        .filter_map(|record| match record.data {
            Data::Address(address) => Some(address),
            _ => None,
        })
        .collect();
    if subjects.is_empty() {
        subjects.push(source);
    }
    for subject in subjects {
        let at = match heard.iter().position(|announcement| announcement.address == Some(subject)) {
            Some(at) => at,
            None => {
                heard.push(Announcement::of(ListenProtocol::Mdns, subject));
                heard.len() - 1
            }
        };
        heard[at].services.clone_from(&services);
        heard[at].model.clone_from(&model);
    }
    heard
}

fn llmnr(message: &Message) -> Vec<Announcement> {
    if !message.response {
        return Vec::new();
    }
    named(&message.answers.iter().collect::<Vec<_>>(), ListenProtocol::Llmnr)
}

/// The machine name a NetBIOS name such as `FAEPFCEE...` encodes, when its suffix names one.
fn netbios_name(encoded: &str) -> Option<String> {
    let label = encoded.split('.').next()?.as_bytes();
    if label.len() != NETBIOS_ENCODED || !label.iter().all(|byte| NETBIOS_ENCODING.contains(byte)) {
        return None;
    }
    let base = *NETBIOS_ENCODING.start();
    let decoded: Vec<u8> =
        label.as_chunks::<2>().0.iter().map(|[high, low]| ((high - base) << NIBBLE_BITS) | (low - base)).collect();
    let (suffix, name) = decoded.split_last()?;
    if !NETBIOS_SUFFIXES.contains(suffix) || name.len() != NETBIOS_NAME {
        return None;
    }
    clean(name)
}

/// The names a registration or a positive query response gives, unique names only.
fn netbios(message: &Message) -> Vec<Announcement> {
    let records = match (message.response, message.opcode) {
        (true, NETBIOS_QUERY) => &message.answers,
        (false, opcode) if NETBIOS_REGISTRATIONS.contains(&opcode) => &message.additional,
        _ => return Vec::new(),
    };
    let mut heard = Vec::new();
    for record in records.iter().filter(|record| record.kind == TYPE_NB) {
        let (Data::Other(entries), Some(name)) = (&record.data, netbios_name(&record.name)) else { continue };
        for entry in entries.as_chunks::<NB_ENTRY>().0 {
            let Some(address) = ipv4_at(entry, NB_ADDRESS) else { continue };
            let group = u16_at(entry, 0).is_some_and(|flags| flags & NETBIOS_GROUP != 0);
            if !group && !address.is_unspecified() {
                heard.push(Announcement {
                    name: Some(name.clone()),
                    ..Announcement::of(ListenProtocol::Netbios, address.into())
                });
            }
        }
    }
    heard
}

enum Upnp {
    Device(String),
    Service(String),
}

/// The device or service type a URN such as `urn:schemas-upnp-org:device:MediaRenderer:1` names.
fn upnp(value: &str) -> Option<Upnp> {
    let parts: Vec<&str> = value.split(':').collect();
    let [urn, _, kind, name, ..] = parts.as_slice() else { return None };
    if !urn.eq_ignore_ascii_case(UPNP_URN) {
        return None;
    }
    let name = clean(name.as_bytes())?;
    if kind.eq_ignore_ascii_case(UPNP_DEVICE) {
        Some(Upnp::Device(name))
    } else if kind.eq_ignore_ascii_case(UPNP_SERVICE) {
        Some(Upnp::Service(name))
    } else {
        None
    }
}

/// What an SSDP notice or search response says about its sender. A search only says the sender
/// is there.
fn ssdp(body: &[u8], source: IpAddr) -> Option<Announcement> {
    let text = String::from_utf8_lossy(body);
    let mut lines = text.lines();
    let first = lines.next()?;
    if !first.starts_with(SSDP_NOTIFY) && !first.starts_with(SSDP_RESPONSE) {
        return None;
    }
    let mut announcement = Announcement::of(ListenProtocol::Ssdp, source);
    for (key, value) in lines.filter_map(|line| line.split_once(':')) {
        let (key, value) = (key.trim(), value.trim());
        if key.eq_ignore_ascii_case(SSDP_SERVER) {
            announcement.model = clean(value.as_bytes());
        } else if SSDP_TYPES.iter().any(|kind| key.eq_ignore_ascii_case(kind)) {
            match upnp(value) {
                Some(Upnp::Device(name)) if !announcement.kinds.contains(&name) => announcement.kinds.push(name),
                Some(Upnp::Service(name)) if !announcement.services.contains(&name) => announcement.services.push(name),
                _ => {}
            }
        }
    }
    Some(announcement)
}

fn dhcp_options(bytes: &[u8]) -> Vec<(u8, &[u8])> {
    let mut found = Vec::new();
    let mut at = 0;
    while let Some(&code) = bytes.get(at) {
        match code {
            OPTION_PAD => at += 1,
            OPTION_END => break,
            _ => {
                let Some(&length) = bytes.get(at + 1) else { break };
                let Some(data) = bytes.get(at + OPTION_HEAD..at + OPTION_HEAD + usize::from(length)) else { break };
                found.push((code, data));
                at += OPTION_HEAD + usize::from(length);
            }
        }
    }
    found
}

/// The host part of the name a client FQDN option carries.
fn fqdn_name(data: &[u8]) -> Option<String> {
    let flags = *data.first()?;
    let name = data.get(FQDN_NAME..)?;
    if flags & FQDN_WIRE_FORMAT == 0 {
        return clean(name.split(|byte| *byte == b'.').next()?);
    }
    let (&length, rest) = name.split_first()?;
    clean(rest.get(..usize::from(length))?)
}

/// What a DHCP request says about its client, or an acknowledgment about the address it hands
/// out.
fn dhcp(body: &[u8]) -> Option<Announcement> {
    if body.get(COOKIE_AT..OPTIONS_AT)? != MAGIC_COOKIE {
        return None;
    }
    let options = dhcp_options(body.get(OPTIONS_AT..)?);
    let option = |code: u8| options.iter().find(|(found, _)| *found == code).map(|(_, data)| *data);
    let kind = option(OPTION_MESSAGE_TYPE).and_then(|data| data.first().copied());
    let ethernet = body.get(HARDWARE_AT..HARDWARE_AT + HARDWARE.len())? == HARDWARE;
    let mut announcement = Announcement {
        mac: ethernet.then(|| mac_at(body, CLIENT_HARDWARE)).flatten(),
        ..Announcement::new(ListenProtocol::Dhcp)
    };
    let given = |at: usize| ipv4_at(body, at).filter(|address| !address.is_unspecified());
    match *body.first()? {
        BOOTREQUEST => {
            let requested = option(OPTION_REQUESTED_ADDRESS).filter(|_| kind == Some(DHCP_REQUEST));
            announcement.address =
                given(CLIENT_ADDRESS).or_else(|| requested.and_then(|data| ipv4_at(data, 0))).map(IpAddr::V4);
            announcement.name =
                option(OPTION_HOST_NAME).and_then(clean).or_else(|| option(OPTION_CLIENT_FQDN).and_then(fqdn_name));
            announcement.client = option(OPTION_VENDOR_CLASS).and_then(clean);
        }
        BOOTREPLY if kind == Some(DHCP_ACK) => announcement.address = given(YOUR_ADDRESS).map(IpAddr::V4),
        _ => return None,
    }
    (announcement.address.is_some() || announcement.mac.is_some()).then_some(announcement)
}

fn lldp_units(body: &[u8]) -> Vec<(u16, &[u8])> {
    let mut units = Vec::new();
    let mut at = 0;
    while let Some(head) = u16_at(body, at) {
        let (kind, length) = (head >> LLDP_TYPE_SHIFT, usize::from(head & LLDP_LENGTH_MASK));
        if kind == LLDP_END {
            break;
        }
        let Some(value) = body.get(at + LLDP_UNIT_HEADER..at + LLDP_UNIT_HEADER + length) else { break };
        units.push((kind, value));
        at += LLDP_UNIT_HEADER + length;
    }
    units
}

/// An LLDP ID given as text, when its subtype is one of `texts`.
fn text_id(value: &[u8], texts: &[u8]) -> Option<String> {
    let (subtype, id) = value.split_first()?;
    texts.contains(subtype).then(|| clean(id)).flatten()
}

/// An LLDP ID given as a hardware address, when its subtype is `wanted`.
fn mac_id(value: &[u8], wanted: u8) -> Option<String> {
    let (subtype, id) = value.split_first()?;
    (*subtype == wanted).then(|| mac_at(id, 0)).flatten().map(|mac| mac.to_string())
}

fn management(value: &[u8]) -> Option<IpAddr> {
    let (&length, rest) = value.split_first()?;
    let (&family, address) = rest.get(..usize::from(length))?.split_first()?;
    match family {
        MANAGEMENT_IPV4 => ipv4_at(address, 0).map(IpAddr::V4),
        MANAGEMENT_IPV6 => ipv6_at(address, 0).map(IpAddr::V6),
        _ => None,
    }
}

fn lldp(body: &[u8]) -> Vec<Heard> {
    let mut switch = SwitchPort::new(ListenProtocol::Lldp);
    let (mut chassis, mut port, mut description) = (None, None, None);
    for (kind, value) in lldp_units(body) {
        match kind {
            LLDP_CHASSIS => chassis = text_id(value, &CHASSIS_TEXTS).or_else(|| mac_id(value, CHASSIS_MAC)),
            LLDP_PORT => port = Some((text_id(value, &PORT_TEXTS), mac_id(value, PORT_MAC))),
            LLDP_PORT_DESCRIPTION => description = clean(value),
            LLDP_SYSTEM_NAME => switch.name = clean(value),
            LLDP_SYSTEM_DESCRIPTION => switch.model = clean(value),
            LLDP_MANAGEMENT => switch.address = switch.address.or(management(value)),
            _ => {}
        }
    }
    let (text, mac) = port.unwrap_or_default();
    switch.port = text.or(description).or(mac);
    switch.name = switch.name.or(chassis);
    switched(switch)
}

fn cdp_units(body: &[u8]) -> Vec<(u16, &[u8])> {
    let mut units = Vec::new();
    let mut at = 0;
    while let (Some(kind), Some(length)) = (u16_at(body, at), u16_at(body, at + CDP_LENGTH_AT)) {
        let length = usize::from(length);
        if length < CDP_UNIT_HEADER {
            break;
        }
        let Some(value) = body.get(at + CDP_UNIT_HEADER..at + length) else { break };
        units.push((kind, value));
        at += length;
    }
    units
}

/// The first IPv4 address a CDP addresses unit lists.
fn cdp_address(value: &[u8]) -> Option<IpAddr> {
    let mut at = CDP_COUNT;
    while let Some(protocol) = value.get(at..at + CDP_IPV4.len()) {
        let length_at = at + CDP_PROTOCOL_HEAD + usize::from(protocol[1]);
        let length = usize::from(u16_at(value, length_at)?);
        let start = length_at + CDP_ADDRESS_LENGTH;
        if protocol == CDP_IPV4 && length == IPV4_BYTES {
            return ipv4_at(value, start).map(IpAddr::V4);
        }
        at = start + length;
    }
    None
}

fn cdp(destination: Mac, body: &[u8]) -> Vec<Heard> {
    if destination != CDP_ADDRESS || body.get(..CDP_SNAP.len()) != Some(&CDP_SNAP[..]) {
        return Vec::new();
    }
    let mut switch = SwitchPort::new(ListenProtocol::Cdp);
    for (kind, value) in cdp_units(body.get(CDP_SNAP.len() + CDP_HEADER..).unwrap_or_default()) {
        match kind {
            CDP_DEVICE => switch.name = clean(value),
            CDP_ADDRESSES => switch.address = switch.address.or(cdp_address(value)),
            CDP_PORT => switch.port = clean(value),
            CDP_PLATFORM => switch.model = clean(value),
            _ => {}
        }
    }
    switched(switch)
}

/// The switch a discovery frame describes, and the switch as a host when it gives an address.
fn switched(switch: SwitchPort) -> Vec<Heard> {
    if switch.name.is_none() && switch.port.is_none() {
        return Vec::new();
    }
    let host = switch.address.map(|address| Announcement {
        name: switch.name.clone(),
        model: switch.model.clone(),
        ..Announcement::of(switch.protocol, address)
    });
    std::iter::once(Heard::Switch(switch)).chain(host.map(Heard::Host)).collect()
}

#[cfg(test)]
mod tests;
