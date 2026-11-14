use super::*;
use crate::network::neighbors::Gateway;

const READ_AT: u64 = 1_800_000_000;

fn v4(text: &str) -> Ipv4Addr {
    text.parse().unwrap()
}

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

/// A laptop on a home network with a work VPN and a system tunnel that has only a link-local
/// address.
fn laptop() -> Vec<Interface> {
    vec![
        interface("lo0", InterfaceKind::Loopback, &[("127.0.0.1", 8)]),
        interface("en0", InterfaceKind::WiFi, &[("192.168.10.20", 24), ("fe80::20", 64)]),
        interface("utun4", InterfaceKind::Tunnel, &[("10.99.0.7", 32)]),
        interface("utun0", InterfaceKind::Tunnel, &[("fe80::1", 64)]),
    ]
}

fn route(cidr: &str, gateway: Option<&str>, interface: &str) -> Route {
    let (network, prefix) = cidr.split_once('/').unwrap();
    Route {
        network: v4(network),
        prefix: prefix.parse().unwrap(),
        gateway: gateway.map(v4),
        interface: interface.into(),
    }
}

fn neighborhood(routes: Vec<Route>) -> Neighborhood {
    Neighborhood {
        neighbors: Vec::new(),
        gateways: vec![Gateway { address: ip("192.168.10.1"), interface: "en0".into() }],
        routes,
        dns: Vec::new(),
        error: None,
        read_at: READ_AT,
    }
}

fn cidrs(routed: &[Routed]) -> Vec<String> {
    routed.iter().map(Routed::cidr).collect()
}

fn reached(cidr: &str, hops: Option<u8>, origin: Origin) -> Routed {
    let route = route(cidr, Some("192.168.10.1"), "en0");
    Routed { trace: Trace { hops, path: Vec::new() }, origin, ..Routed::from_table(&route) }
}

#[test]
fn the_table_s_routes_to_other_networks_count_and_the_rest_do_not() {
    let routes = [
        route("192.168.20.0/24", Some("192.168.10.1"), "en0"),
        route("10.20.0.0/16", None, "utun4"),
        route("10.30.0.0/16", None, "utun0"),
        route("0.0.0.0/1", None, "utun4"),
        route("192.168.10.0/24", None, "en0"),
        route("192.168.10.0/25", None, "en0"),
        route("192.168.30.7/32", Some("192.168.10.1"), "en0"),
        route("169.254.0.0/16", None, "en0"),
        route("224.0.0.0/4", None, "en0"),
        route("127.0.0.0/8", None, "lo0"),
        route("172.16.0.0/12", Some("192.168.10.1"), "en9"),
    ];
    let mut interfaces = laptop();
    interfaces[3].up = false;
    assert_eq!(cidrs(&table_networks(&routes, &interfaces)), ["192.168.20.0/24", "10.20.0.0/16"]);
}

#[test]
fn only_interfaces_with_an_address_beyond_their_link_show() {
    let interfaces = laptop();
    let shown: Vec<&str> =
        interfaces.iter().filter(|interface| shows(interface)).map(|interface| interface.name.as_str()).collect();
    assert_eq!(shown, ["en0", "utun4"]);
    let down = Interface { up: false, ..interfaces[1].clone() };
    assert!(!shows(&down));
}

#[test]
fn internet_traffic_takes_the_most_specific_wide_route_and_else_the_first_default() {
    let interfaces = laptop();
    let home = internet(&neighborhood(Vec::new()), &interfaces);
    assert_eq!(home.len(), 1);
    assert_eq!(
        (home[0].interface.as_str(), home[0].in_use, home[0].gateway()),
        ("en0", true, Some(v4("192.168.10.1")))
    );
    let vpn = neighborhood(vec![route("0.0.0.0/1", None, "utun4"), route("128.0.0.0/1", None, "utun4")]);
    let ways = internet(&vpn, &interfaces);
    let summary: Vec<(&str, bool, usize)> =
        ways.iter().map(|way| (way.interface.as_str(), way.in_use, way.wide.len())).collect();
    assert_eq!(summary, [("en0", false, 0), ("utun4", true, 2)]);
    let hidden = neighborhood(vec![route("0.0.0.0/1", None, "utun0")]);
    assert_eq!(internet(&hidden, &interfaces).len(), 1, "a tunnel the map hides has no way to show");
}

#[test]
fn discovered_and_remembered_networks_join_the_table_s_unless_a_route_covers_them() {
    let table = vec![reached("10.20.0.0/16", None, Origin::Table), reached("192.168.20.0/24", None, Origin::Table)];
    let found = [
        reached("192.168.20.0/24", Some(1), Origin::Discovery),
        reached("10.20.5.0/24", Some(3), Origin::Discovery),
        reached("192.168.40.0/24", Some(2), Origin::Discovery),
    ];
    let remembered = vec![Routed { remembered: Some(READ_AT), ..reached("172.16.0.0/24", None, Origin::Discovery) }];
    let routed = routed(table, &found, remembered);
    assert_eq!(cidrs(&routed), ["10.20.0.0/16", "172.16.0.0/24", "192.168.20.0/24", "192.168.40.0/24"]);
    assert!(routed[2].on_gateway(), "the table's route takes the hops discovery found");
    assert_eq!(routed[2].origin, Origin::Table);
    assert_eq!(holding(&routed, v4("10.20.5.9")), Some(0));
    assert_eq!(holding(&routed, v4("10.21.0.1")), None);
}

#[test]
fn a_wide_routed_network_scans_the_24_its_inner_router_is_on() {
    let mut network = reached("10.20.0.0/16", Some(3), Origin::Table);
    assert_eq!(network.scanned_part(), (v4("10.20.0.0"), 24));
    network.trace.path = vec![v4("192.168.10.1"), v4("10.20.7.1")];
    assert_eq!(network.scanned_part(), (v4("10.20.7.0"), 24));
    assert_eq!(network.path(), [v4("192.168.10.1"), v4("10.20.7.1")], "the gateway is listed once");
    let narrow = reached("192.168.20.128/26", None, Origin::Table);
    assert_eq!(narrow.scanned_part(), (v4("192.168.20.128"), 26));
}

#[test]
fn discovery_tries_the_likely_routers_around_this_network_the_way_internet_traffic_leaves() {
    let interfaces = laptop();
    let routes = [route("172.20.0.0/23", Some("192.168.10.1"), "en0"), route("10.0.0.0/8", None, "utun4")];
    let neighborhood = neighborhood(routes.to_vec());
    let ways = internet(&neighborhood, &interfaces);
    let discovery = discovery(&interfaces, &routes, &ways).unwrap();
    assert_eq!(discovery.way, Way { interface: "en0".into(), gateway: Some(v4("192.168.10.1")) });
    let around = 255;
    let routed = 2;
    let common = ["172.16.0.0"].len();
    assert_eq!(discovery.networks, around + routed + common);
    assert_eq!(discovery.addresses.len(), discovery.networks * ROUTER_HOSTS.len());
    assert_eq!(discovery.addresses[..2], [v4("192.168.0.1"), v4("192.168.0.254")]);
    for address in ["192.168.20.1", "192.168.255.254", "172.20.1.1", "172.16.0.254"] {
        assert!(discovery.addresses.contains(&v4(address)), "{address} is tried");
    }
    for address in ["192.168.10.1", "10.0.0.1", "10.10.10.1"] {
        assert!(!discovery.addresses.contains(&v4(address)), "{address} is attached or behind the VPN");
    }
    assert_eq!(super::discovery(&interfaces, &routes, &[]), None, "no way out, nothing to discover");
}
