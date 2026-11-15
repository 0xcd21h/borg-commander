use std::net::Ipv4Addr;

use super::*;
use crate::network::neighbors::{NeighborState, Origin, Presence, Route, Trace};

fn ip(text: &str) -> IpAddr {
    text.parse().unwrap()
}

fn interface(name: &str, kind: InterfaceKind, addresses: &[(&str, u8)]) -> Interface {
    Interface {
        name: name.into(),
        kind,
        up: true,
        addresses: addresses.iter().map(|(address, prefix)| (ip(address), *prefix)).collect(),
        mac: None,
        mtu: None,
        received: 0,
        sent: 0,
        packets_received: 0,
        packets_sent: 0,
        errors: 0,
    }
}

fn neighbor(address: &str, interface: &str) -> Neighbor {
    Neighbor {
        address: ip(address),
        mac: None,
        interface: interface.into(),
        state: Some(NeighborState::Reachable),
        router: false,
    }
}

fn routed(cidr: &str, interface: &str) -> Routed {
    let (network, prefix) = cidr.split_once('/').unwrap();
    Routed {
        network: network.parse::<Ipv4Addr>().unwrap(),
        prefix: prefix.parse().unwrap(),
        interface: interface.into(),
        gateway: None,
        trace: Trace::default(),
        origin: Origin::Table,
        remembered: None,
    }
}

fn row(neighbor: usize) -> NeighborRow {
    NeighborRow { neighbor, presence: Presence::Listed, shared: false, gateway: false, changed: None, heard: false }
}

struct Laptop {
    interfaces: Vec<Interface>,
    routed: Vec<Routed>,
    internet: Vec<InternetRoute>,
    neighbors: Vec<Neighbor>,
}

/// Wi-Fi with a routed network behind its gateway, a work VPN that takes internet traffic, a
/// docker bridge, and a host reached through the gateway that no routed network holds.
fn laptop() -> Laptop {
    let wide = |interface: &str| Route {
        network: Ipv4Addr::UNSPECIFIED,
        prefix: 1,
        gateway: None,
        interface: interface.into(),
    };
    Laptop {
        interfaces: vec![
            interface("lo0", InterfaceKind::Loopback, &[("127.0.0.1", 8)]),
            interface("bridge0", InterfaceKind::Bridge, &[("172.17.0.1", 16)]),
            interface("en0", InterfaceKind::WiFi, &[("192.168.10.20", 24)]),
            interface("utun4", InterfaceKind::Tunnel, &[("10.99.0.7", 32)]),
        ],
        routed: vec![routed("10.20.0.0/16", "utun4"), routed("192.168.20.0/24", "en0")],
        internet: vec![
            InternetRoute {
                interface: "en0".into(),
                gateways: vec![ip("192.168.10.1")],
                wide: Vec::new(),
                in_use: false,
            },
            InternetRoute { interface: "utun4".into(), gateways: Vec::new(), wide: vec![wide("utun4")], in_use: true },
        ],
        neighbors: vec![
            neighbor("192.168.10.1", "en0"),
            neighbor("192.168.20.40", "en0"),
            neighbor("10.20.3.4", "utun4"),
            neighbor("172.17.0.2", "bridge0"),
            neighbor("203.0.113.9", "en0"),
        ],
    }
}

fn described(laptop: &Laptop, entries: &[NeighborEntry]) -> Vec<String> {
    entries
        .iter()
        .map(|entry| match *entry {
            NeighborEntry::Interface(index) => format!("[{}]", laptop.interfaces[index].name),
            NeighborEntry::Network(index) => format!("› {}", laptop.routed[index].cidr()),
            NeighborEntry::Internet(index) => format!("› internet {}", laptop.internet[index].interface),
            NeighborEntry::Host(row) => laptop.neighbors[row.neighbor].address.to_string(),
        })
        .collect()
}

fn parts(laptop: &Laptop) -> MapParts<'_> {
    MapParts {
        interfaces: &laptop.interfaces,
        routed: &laptop.routed,
        internet: &laptop.internet,
        neighbors: &laptop.neighbors,
    }
}

#[test]
fn the_map_starts_with_the_ways_to_the_internet_and_puts_each_host_where_it_is_reached() {
    let laptop = laptop();
    let hosts = (0..laptop.neighbors.len()).map(row).collect();
    let entries = entries(&parts(&laptop), hosts, false, |_| false);
    assert_eq!(
        described(&laptop, &entries),
        [
            "[utun4]",
            "› 10.20.0.0/16",
            "10.20.3.4",
            "› internet utun4",
            "[en0]",
            "192.168.10.1",
            "› 192.168.20.0/24",
            "192.168.20.40",
            "› internet en0",
            "203.0.113.9",
            "[bridge0]",
            "172.17.0.2",
        ]
    );
}

#[test]
fn a_filter_keeps_the_headings_that_match_or_hold_a_host_that_does() {
    let laptop = laptop();
    let entries = entries(&parts(&laptop), vec![row(1)], true, |entry| entry == NeighborEntry::Network(0));
    assert_eq!(
        described(&laptop, &entries),
        ["[utun4]", "› 10.20.0.0/16", "[en0]", "› 192.168.20.0/24", "192.168.20.40"]
    );
    let nothing = super::entries(&parts(&laptop), Vec::new(), true, |_| false);
    assert_eq!(nothing, Vec::new());
}
