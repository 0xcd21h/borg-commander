use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
};

use borg_core::{Gateway, Mac, Neighbor, NeighborState, Route};

#[cfg(target_os = "linux")]
pub(super) const PROC_ARP: &str = "/proc/net/arp";
#[cfg(target_os = "linux")]
pub(super) const PROC_ROUTE: &str = "/proc/net/route";
#[cfg(target_os = "linux")]
pub(super) const PROC_ROUTE_V6: &str = "/proc/net/ipv6_route";
#[cfg(target_os = "linux")]
const SYS_NET: &str = "/sys/class/net";
#[cfg(target_os = "linux")]
const IFINDEX: &str = "ifindex";
#[cfg(target_os = "linux")]
const DUMP_BUFFER: usize = 64 * 1024;
#[cfg(target_os = "linux")]
const DUMP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

const MESSAGE_HEADER: usize = 16;
const MESSAGE_TYPE: usize = 4;
const MESSAGE_ALIGN: usize = 4;
const MESSAGE_ERROR: u16 = 2;
const MESSAGE_DONE: u16 = 3;
const NEW_NEIGHBOR: u16 = 28;
const GET_NEIGHBOR: u16 = 30;
const REQUEST: u16 = 0x1;
const DUMP: u16 = 0x300;
const NEIGHBOR_HEADER: usize = 12;
const NEIGHBOR_INDEX: usize = 4;
const NEIGHBOR_STATE: usize = 8;
const NEIGHBOR_FLAGS: usize = 10;
const ATTRIBUTE_HEADER: usize = 4;
const ATTRIBUTE_TYPE: usize = 2;
const DESTINATION: u16 = 1;
const LINK_ADDRESS: u16 = 2;
const FAMILY_V4: u8 = 2;
const FAMILY_V6: u8 = 10;
const ROUTER: u8 = 0x80;
const NO_ARP: u16 = 0x40;
const KERNEL_STATES: [(u16, NeighborState); 7] = [
    (0x01, NeighborState::Incomplete),
    (0x02, NeighborState::Reachable),
    (0x04, NeighborState::Stale),
    (0x08, NeighborState::Delay),
    (0x10, NeighborState::Probe),
    (0x20, NeighborState::Failed),
    (0x80, NeighborState::Permanent),
];
const MAC_BYTES: usize = 6;
const IPV4_BYTES: usize = 4;
const IPV6_BYTES: usize = 16;
const HEX: u32 = 16;
const ROUTE_UP: u32 = 0x1;
const ROUTE_GATEWAY: u32 = 0x2;
const ROUTE_HOST: u32 = 0x4;
const ROUTE_INTERFACE: usize = 0;
const ROUTE_DESTINATION: usize = 1;
const ROUTE_NEXT_HOP: usize = 2;
const ROUTE_FLAGS: usize = 3;
const ROUTE_METRIC: usize = 6;
const ROUTE_MASK: usize = 7;
const ROUTE_V6_DESTINATION: usize = 0;
const ROUTE_V6_PREFIX: usize = 1;
const ROUTE_V6_NEXT_HOP: usize = 4;
const ROUTE_V6_METRIC: usize = 5;
const ROUTE_V6_FLAGS: usize = 8;
const ROUTE_V6_INTERFACE: usize = 9;
const ARP_ADDRESS: usize = 0;
const ARP_FLAGS: usize = 2;
const ARP_LINK: usize = 3;
const ARP_INTERFACE: usize = 5;
const ARP_COMPLETE: u32 = 0x2;
const ARP_PERMANENT: u32 = 0x4;
const HEX_PREFIX: &str = "0x";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Reply {
    More,
    Done,
    Failed(i32),
}

fn number(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_ne_bytes(bytes.get(at..at + size_of::<u32>())?.try_into().ok()?))
}

fn short(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_ne_bytes(bytes.get(at..at + size_of::<u16>())?.try_into().ok()?))
}

pub(super) fn request(sequence: u32) -> Vec<u8> {
    let length = (MESSAGE_HEADER + NEIGHBOR_HEADER) as u32;
    let mut message = Vec::with_capacity(length as usize);
    message.extend_from_slice(&length.to_ne_bytes());
    message.extend_from_slice(&GET_NEIGHBOR.to_ne_bytes());
    message.extend_from_slice(&(REQUEST | DUMP).to_ne_bytes());
    message.extend_from_slice(&sequence.to_ne_bytes());
    message.extend_from_slice(&0_u32.to_ne_bytes());
    message.resize(length as usize, 0);
    message
}

pub(super) fn parse(bytes: &[u8], names: &HashMap<u32, String>, neighbors: &mut Vec<Neighbor>) -> Reply {
    let mut at = 0;
    while let Some(length) = number(bytes, at).map(|length| length as usize) {
        if length < MESSAGE_HEADER || at.saturating_add(length) > bytes.len() {
            break;
        }
        let message = &bytes[at..at + length];
        match short(message, MESSAGE_TYPE) {
            Some(MESSAGE_DONE) => return Reply::Done,
            Some(MESSAGE_ERROR) => {
                let code = number(message, MESSAGE_HEADER).map_or(0, |code| code as i32);
                if code != 0 {
                    return Reply::Failed(-code);
                }
            }
            Some(NEW_NEIGHBOR) => neighbors.extend(neighbor(&message[MESSAGE_HEADER..], names)),
            _ => {}
        }
        at += length.next_multiple_of(MESSAGE_ALIGN);
    }
    Reply::More
}

fn neighbor(body: &[u8], names: &HashMap<u32, String>) -> Option<Neighbor> {
    let family = *body.first()?;
    let index = number(body, NEIGHBOR_INDEX)?;
    let state = short(body, NEIGHBOR_STATE)?;
    let flags = *body.get(NEIGHBOR_FLAGS)?;
    if state & NO_ARP != 0 {
        return None;
    }
    let mut destination = None;
    let mut mac = None;
    let mut at = NEIGHBOR_HEADER;
    while let Some(length) = short(body, at).map(usize::from) {
        if length < ATTRIBUTE_HEADER || at + length > body.len() {
            break;
        }
        let data = &body[at + ATTRIBUTE_HEADER..at + length];
        match short(body, at + ATTRIBUTE_TYPE) {
            Some(DESTINATION) => destination = address(family, data),
            Some(LINK_ADDRESS) => mac = <[u8; MAC_BYTES]>::try_from(data).ok().map(Mac),
            _ => {}
        }
        at += length.next_multiple_of(MESSAGE_ALIGN);
    }
    Some(Neighbor {
        address: destination?,
        mac: mac.filter(|mac| !mac.unset()),
        interface: names.get(&index).cloned().unwrap_or_else(|| index.to_string()),
        state: KERNEL_STATES.iter().find(|(bit, _)| state & bit != 0).map(|(_, state)| *state),
        router: flags & ROUTER != 0,
    })
}

fn address(family: u8, data: &[u8]) -> Option<IpAddr> {
    match family {
        FAMILY_V4 => <[u8; IPV4_BYTES]>::try_from(data).ok().map(|bytes| IpAddr::V4(Ipv4Addr::from(bytes))),
        FAMILY_V6 => <[u8; IPV6_BYTES]>::try_from(data).ok().map(|bytes| IpAddr::V6(Ipv6Addr::from(bytes))),
        _ => None,
    }
}

fn hex(word: &str) -> Option<u32> {
    u32::from_str_radix(word.trim_start_matches(HEX_PREFIX), HEX).ok()
}

pub(super) fn routes_v4(table: &str) -> Vec<Gateway> {
    let mut found: Vec<(u32, Gateway)> = table
        .lines()
        .filter_map(|line| {
            let words: Vec<&str> = line.split_whitespace().collect();
            let flags = hex(words.get(ROUTE_FLAGS)?)?;
            let default = hex(words.get(ROUTE_DESTINATION)?)? == 0 && hex(words.get(ROUTE_MASK)?)? == 0;
            let gateway = Ipv4Addr::from(hex(words.get(ROUTE_NEXT_HOP)?)?.to_ne_bytes());
            let routed = flags & (ROUTE_UP | ROUTE_GATEWAY) == ROUTE_UP | ROUTE_GATEWAY;
            (default && routed && !gateway.is_unspecified()).then_some(())?;
            let metric = words.get(ROUTE_METRIC)?.parse().ok()?;
            Some((metric, Gateway { address: IpAddr::V4(gateway), interface: words[ROUTE_INTERFACE].to_owned() }))
        })
        .collect();
    found.sort_by_key(|(metric, _)| *metric);
    found.into_iter().map(|(_, gateway)| gateway).collect()
}

/// The IPv4 routes to networks other than the default ones.
pub(super) fn network_routes(table: &str) -> Vec<Route> {
    table
        .lines()
        .filter_map(|line| {
            let words: Vec<&str> = line.split_whitespace().collect();
            let flags = hex(words.get(ROUTE_FLAGS)?)?;
            let network = Ipv4Addr::from(hex(words.get(ROUTE_DESTINATION)?)?.to_ne_bytes());
            let mask = u32::from(Ipv4Addr::from(hex(words.get(ROUTE_MASK)?)?.to_ne_bytes()));
            let prefix = u8::try_from(mask.leading_ones()).ok()?;
            let contiguous = mask.count_ones() == mask.leading_ones();
            (flags & ROUTE_UP != 0 && flags & ROUTE_HOST == 0 && prefix > 0 && contiguous).then_some(())?;
            let next_hop = Ipv4Addr::from(hex(words.get(ROUTE_NEXT_HOP)?)?.to_ne_bytes());
            let gateway = (flags & ROUTE_GATEWAY != 0 && !next_hop.is_unspecified()).then_some(next_hop);
            Some(Route { network, prefix, gateway, interface: (*words.get(ROUTE_INTERFACE)?).to_owned() })
        })
        .collect()
}

pub(super) fn routes_v6(table: &str) -> Vec<Gateway> {
    let mut found: Vec<(u32, Gateway)> = table
        .lines()
        .filter_map(|line| {
            let words: Vec<&str> = line.split_whitespace().collect();
            let destination = u128::from_str_radix(words.get(ROUTE_V6_DESTINATION)?, HEX).ok()?;
            let prefix = hex(words.get(ROUTE_V6_PREFIX)?)?;
            let next_hop = Ipv6Addr::from(u128::from_str_radix(words.get(ROUTE_V6_NEXT_HOP)?, HEX).ok()?);
            let flags = hex(words.get(ROUTE_V6_FLAGS)?)?;
            let routed = flags & (ROUTE_UP | ROUTE_GATEWAY) == ROUTE_UP | ROUTE_GATEWAY;
            (destination == 0 && prefix == 0 && routed && !next_hop.is_unspecified()).then_some(())?;
            let metric = hex(words.get(ROUTE_V6_METRIC)?)?;
            let interface = (*words.get(ROUTE_V6_INTERFACE)?).to_owned();
            Some((metric, Gateway { address: IpAddr::V6(next_hop), interface }))
        })
        .collect();
    found.sort_by_key(|(metric, _)| *metric);
    found.into_iter().map(|(_, gateway)| gateway).collect()
}

pub(super) fn proc_arp(table: &str) -> Vec<Neighbor> {
    table
        .lines()
        .filter_map(|line| {
            let words: Vec<&str> = line.split_whitespace().collect();
            let address: Ipv4Addr = words.get(ARP_ADDRESS)?.parse().ok()?;
            let flags = hex(words.get(ARP_FLAGS)?)?;
            let state = if flags & ARP_PERMANENT != 0 {
                Some(NeighborState::Permanent)
            } else if flags & ARP_COMPLETE == 0 {
                Some(NeighborState::Incomplete)
            } else {
                None
            };
            Some(Neighbor {
                address: IpAddr::V4(address),
                mac: Mac::parse(words.get(ARP_LINK)?).filter(|mac| !mac.unset()),
                interface: (*words.get(ARP_INTERFACE)?).to_owned(),
                state,
                router: false,
            })
        })
        .collect()
}

#[cfg(target_os = "linux")]
pub(super) fn interface_names() -> std::io::Result<HashMap<u32, String>> {
    let mut names = HashMap::new();
    for entry in std::fs::read_dir(SYS_NET)? {
        let entry = entry?;
        let index = std::fs::read_to_string(entry.path().join(IFINDEX)).ok().and_then(|text| text.trim().parse().ok());
        if let Some(index) = index {
            names.insert(index, entry.file_name().to_string_lossy().into_owned());
        }
    }
    Ok(names)
}

#[cfg(target_os = "linux")]
pub(super) fn dump(names: &HashMap<u32, String>) -> std::io::Result<Vec<Neighbor>> {
    use rustix::net::{
        AddressFamily, RecvFlags, SendFlags, SocketFlags, SocketType, recv, send, socket_with,
        sockopt::{Timeout, set_socket_timeout},
    };
    let socket = socket_with(AddressFamily::NETLINK, SocketType::DGRAM, SocketFlags::CLOEXEC, None)?;
    set_socket_timeout(&socket, Timeout::Recv, Some(DUMP_TIMEOUT))?;
    send(&socket, &request(1), SendFlags::empty())?;
    let mut buffer = vec![0_u8; DUMP_BUFFER];
    let mut neighbors = Vec::new();
    loop {
        let (filled, _) = recv(&socket, &mut buffer[..], RecvFlags::empty())?;
        match parse(&buffer[..filled], names, &mut neighbors) {
            Reply::Done => return Ok(neighbors),
            Reply::Failed(code) => return Err(std::io::Error::from_raw_os_error(code)),
            Reply::More if filled == 0 => return Ok(neighbors),
            Reply::More => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attribute(kind: u16, data: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&((ATTRIBUTE_HEADER + data.len()) as u16).to_ne_bytes());
        bytes.extend_from_slice(&kind.to_ne_bytes());
        bytes.extend_from_slice(data);
        bytes.resize(bytes.len().next_multiple_of(MESSAGE_ALIGN), 0);
        bytes
    }

    fn message(kind: u16, body: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&((MESSAGE_HEADER + body.len()) as u32).to_ne_bytes());
        bytes.extend_from_slice(&kind.to_ne_bytes());
        bytes.extend_from_slice(&0_u16.to_ne_bytes());
        bytes.extend_from_slice(&1_u32.to_ne_bytes());
        bytes.extend_from_slice(&0_u32.to_ne_bytes());
        bytes.extend_from_slice(body);
        bytes.resize(bytes.len().next_multiple_of(MESSAGE_ALIGN), 0);
        bytes
    }

    fn entry(family: u8, index: u32, state: u16, flags: u8, destination: &[u8], mac: Option<[u8; 6]>) -> Vec<u8> {
        let mut body = vec![family, 0, 0, 0];
        body.extend_from_slice(&index.to_ne_bytes());
        body.extend_from_slice(&state.to_ne_bytes());
        body.extend_from_slice(&[flags, 1]);
        body.extend(attribute(DESTINATION, destination));
        if let Some(mac) = mac {
            body.extend(attribute(LINK_ADDRESS, &mac));
        }
        message(NEW_NEIGHBOR, &body)
    }

    fn names() -> HashMap<u32, String> {
        HashMap::from([(2, "eth0".to_owned()), (3, "wlan0".to_owned())])
    }

    const ROUTER_MAC: [u8; 6] = [0x3c, 0x22, 0xfb, 0x01, 0x02, 0x03];
    const LAPTOP_MAC: [u8; 6] = [0xa4, 0x83, 0xe7, 0xaa, 0xbb, 0xcc];

    #[test]
    fn a_neighbor_dump_reads_every_entry_until_done() {
        let mut datagram = entry(FAMILY_V4, 2, 0x02, 0, &[192, 0, 2, 1], Some(ROUTER_MAC));
        datagram.extend(entry(FAMILY_V4, 3, 0x04, 0, &[192, 0, 2, 20], Some(LAPTOP_MAC)));
        datagram.extend(entry(FAMILY_V4, 2, 0x01, 0, &[192, 0, 2, 77], None));
        let router = Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1).octets();
        datagram.extend(entry(FAMILY_V6, 2, 0x04, ROUTER, &router, Some(ROUTER_MAC)));
        let mut neighbors = Vec::new();
        assert_eq!(parse(&datagram, &names(), &mut neighbors), Reply::More);
        assert_eq!(parse(&message(MESSAGE_DONE, &0_u32.to_ne_bytes()), &names(), &mut neighbors), Reply::Done);
        assert_eq!(
            neighbors,
            [
                seen("192.0.2.1", Some("3c:22:fb:01:02:03"), "eth0", NeighborState::Reachable, false),
                seen("192.0.2.20", Some("a4:83:e7:aa:bb:cc"), "wlan0", NeighborState::Stale, false),
                seen("192.0.2.77", None, "eth0", NeighborState::Incomplete, false),
                seen("fe80::1", Some("3c:22:fb:01:02:03"), "eth0", NeighborState::Stale, true),
            ]
        );
    }

    fn seen(address: &str, mac: Option<&str>, interface: &str, state: NeighborState, router: bool) -> Neighbor {
        Neighbor {
            address: address.parse().unwrap(),
            mac: mac.and_then(Mac::parse),
            interface: interface.to_owned(),
            state: Some(state),
            router,
        }
    }

    #[test]
    fn entries_without_arp_or_an_address_and_bridge_entries_are_left_out() {
        let mut datagram = entry(FAMILY_V4, 2, NO_ARP, 0, &[224, 0, 0, 251], Some([1, 0, 0x5e, 0, 0, 0xfb]));
        datagram.extend(entry(7, 2, 0x80, 0, &[], Some(ROUTER_MAC)));
        datagram.extend(entry(FAMILY_V4, 9, 0x02, 0, &[192, 0, 2], Some(ROUTER_MAC)));
        let mut neighbors = Vec::new();
        parse(&datagram, &names(), &mut neighbors);
        assert_eq!(neighbors, Vec::new());
        let unnamed = entry(FAMILY_V4, 9, 0x02, 0, &[192, 0, 2, 9], Some(ROUTER_MAC));
        parse(&unnamed, &names(), &mut neighbors);
        assert_eq!(neighbors[0].interface, "9", "an interface Borg cannot name keeps its number");
    }

    #[test]
    fn an_error_reply_says_why_and_a_cut_datagram_reads_what_it_holds() {
        let refused = message(MESSAGE_ERROR, &(-13_i32).to_ne_bytes());
        assert_eq!(parse(&refused, &names(), &mut Vec::new()), Reply::Failed(13));
        let datagram = entry(FAMILY_V4, 2, 0x02, 0, &[192, 0, 2, 1], Some(ROUTER_MAC));
        for end in 0..datagram.len() {
            let mut neighbors = Vec::new();
            assert_eq!(parse(&datagram[..end], &names(), &mut neighbors), Reply::More, "{end}");
            assert!(neighbors.len() <= 1);
        }
    }

    #[test]
    fn the_request_asks_for_a_dump_of_every_neighbor() {
        let request = request(7);
        assert_eq!(number(&request, 0), Some(request.len() as u32));
        assert_eq!(short(&request, MESSAGE_TYPE), Some(GET_NEIGHBOR));
        assert_eq!(short(&request, MESSAGE_TYPE + 2), Some(REQUEST | DUMP));
        assert_eq!(request[MESSAGE_HEADER], 0, "every address family");
    }

    fn hex_address(octets: [u8; 4]) -> String {
        format!("{:08X}", u32::from_ne_bytes(octets))
    }

    #[test]
    fn default_routes_give_the_gateways_by_metric() {
        let table = format!(
            "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT\n\
             wlan0\t00000000\t{wifi}\t0003\t0\t0\t600\t00000000\t0\t0\t0\n\
             eth0\t00000000\t{wired}\t0003\t0\t0\t100\t00000000\t0\t0\t0\n\
             eth0\t{subnet}\t00000000\t0001\t0\t0\t100\t{mask}\t0\t0\t0\n\
             tun0\t00000000\t00000000\t0001\t0\t0\t50\t00000000\t0\t0\t0\n",
            wifi = hex_address([198, 51, 100, 1]),
            wired = hex_address([192, 0, 2, 1]),
            subnet = hex_address([192, 0, 2, 0]),
            mask = hex_address([255, 255, 255, 0]),
        );
        let found: Vec<(String, String)> =
            routes_v4(&table).into_iter().map(|g| (g.address.to_string(), g.interface)).collect();
        assert_eq!(found, [("192.0.2.1".into(), "eth0".into()), ("198.51.100.1".into(), "wlan0".into())]);
    }

    #[test]
    fn network_routes_keep_their_prefix_and_router_and_leave_out_defaults_and_hosts() {
        let row = |interface: &str, destination: [u8; 4], gateway: [u8; 4], flags: &str, mask: [u8; 4]| {
            format!(
                "{interface}\t{}\t{}\t{flags}\t0\t0\t0\t{}\t0\t0\t0\n",
                hex_address(destination),
                hex_address(gateway),
                hex_address(mask)
            )
        };
        let table = [
            "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT\n".to_owned(),
            row("eth0", [0, 0, 0, 0], [192, 0, 2, 1], "0003", [0, 0, 0, 0]),
            row("eth0", [192, 0, 2, 0], [0, 0, 0, 0], "0001", [255, 255, 255, 0]),
            row("eth0", [203, 0, 113, 0], [192, 0, 2, 1], "0003", [255, 255, 255, 0]),
            row("wg0", [10, 20, 0, 0], [0, 0, 0, 0], "0001", [255, 255, 0, 0]),
            row("wg0", [0, 0, 0, 0], [0, 0, 0, 0], "0001", [128, 0, 0, 0]),
            row("eth0", [198, 51, 100, 7], [192, 0, 2, 1], "0007", [255, 255, 255, 255]),
            row("eth0", [198, 51, 100, 0], [192, 0, 2, 1], "0002", [255, 255, 255, 0]),
            row("eth0", [198, 51, 0, 0], [192, 0, 2, 1], "0003", [255, 0, 255, 0]),
        ]
        .concat();
        let found: Vec<(String, Option<Ipv4Addr>, String)> = network_routes(&table)
            .into_iter()
            .map(|route| (format!("{}/{}", route.network, route.prefix), route.gateway, route.interface))
            .collect();
        assert_eq!(
            found,
            [
                ("192.0.2.0/24".into(), None, "eth0".into()),
                ("203.0.113.0/24".into(), Some(Ipv4Addr::new(192, 0, 2, 1)), "eth0".into()),
                ("10.20.0.0/16".into(), None, "wg0".into()),
                ("0.0.0.0/1".into(), None, "wg0".into()),
            ]
        );
    }

    #[test]
    fn the_ipv6_default_route_gives_its_link_local_gateway() {
        let zero = "0".repeat(32);
        let table = format!(
            "{zero} 00 {zero} 00 fe800000000000000000000000000001 00000400 00000001 00000000 00000003 eth0\n\
             20010db8000000000000000000000000 40 {zero} 00 {zero} 00000100 00000001 00000000 00000001 eth0\n\
             {zero} 00 {zero} 00 {zero} ffffffff 00000001 00000000 00200200 lo\n"
        );
        let found: Vec<(String, String)> =
            routes_v6(&table).into_iter().map(|g| (g.address.to_string(), g.interface)).collect();
        assert_eq!(found, [("fe80::1".into(), "eth0".into())]);
    }

    #[test]
    fn the_proc_arp_table_reads_complete_incomplete_and_permanent_entries() {
        let table = "IP address       HW type     Flags       HW address            Mask     Device\n\
                     192.0.2.1        0x1         0x2         3c:22:fb:01:02:03     *        eth0\n\
                     192.0.2.77       0x1         0x0         00:00:00:00:00:00     *        eth0\n\
                     192.0.2.5        0x1         0x6         02:00:5e:10:00:05     *        eth0\n";
        let found: Vec<(String, bool, Option<NeighborState>)> =
            proc_arp(table).into_iter().map(|n| (n.address.to_string(), n.mac.is_some(), n.state)).collect();
        assert_eq!(
            found,
            [
                ("192.0.2.1".into(), true, None),
                ("192.0.2.77".into(), false, Some(NeighborState::Incomplete)),
                ("192.0.2.5".into(), true, Some(NeighborState::Permanent)),
            ]
        );
    }
}
