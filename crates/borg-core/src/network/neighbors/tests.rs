use super::*;

const ROUTER: &str = "00:27:22:4c:01:01";
const OTHER_ROUTER: &str = "00:16:3e:44:55:66";
const NAS: &str = "00:11:32:aa:00:60";

fn ip(text: &str) -> IpAddr {
    text.parse().unwrap()
}

fn neighbor(address: &str, mac: Option<&str>) -> Neighbor {
    Neighbor {
        address: ip(address),
        mac: mac.and_then(Mac::parse),
        interface: "en0".into(),
        state: Some(NeighborState::Reachable),
        router: false,
    }
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

fn home() -> Vec<Interface> {
    vec![interface("en0", InterfaceKind::WiFi, &[("192.0.2.20", 24), ("fe80::20", 64)])]
}

fn neighborhood(neighbors: Vec<Neighbor>) -> Neighborhood {
    Neighborhood {
        neighbors,
        gateways: vec![Gateway { address: ip("192.0.2.1"), interface: "en0".into() }],
        dns: vec![ip("192.0.2.1")],
        error: None,
    }
}

fn listed(neighborhood: &Neighborhood, rows: &[NeighborRow]) -> Vec<String> {
    rows.iter().map(|row| neighborhood.neighbors[row.neighbor].address.to_string()).collect()
}

#[test]
fn macs_read_with_or_without_leading_zeros_and_print_in_full() {
    let mac = Mac::parse("3c:22:fb:1:2:3").unwrap();
    assert_eq!(mac.to_string(), "3c:22:fb:01:02:03");
    assert_eq!(Mac::parse("3C-22-FB-01-02-03"), Some(mac));
    for nonsense in
        ["", "3c:22:fb:01:02", "3c:22:fb:01:02:03:04", "3c:22:fb:01:02:xyz", "3c::fb:01:02:03", "(incomplete)"]
    {
        assert_eq!(Mac::parse(nonsense), None, "{nonsense:?}");
    }
}

#[test]
fn the_first_octet_says_whether_a_mac_is_private_or_a_group() {
    assert!(Mac::parse("da:a1:19:12:34:56").unwrap().locally_administered());
    assert!(!Mac::parse(ROUTER).unwrap().locally_administered());
    assert!(Mac::parse("01:00:5e:00:00:fb").unwrap().group());
    assert!(Mac::parse("ff:ff:ff:ff:ff:ff").unwrap().group());
    assert!(Mac::parse("00:00:00:00:00:00").unwrap().unset());
}

#[test]
fn a_vendor_comes_from_the_registered_prefix() {
    assert_eq!(vendor(Mac::parse(ROUTER).unwrap()), Some("Ubiquiti"));
    assert_eq!(vendor(Mac::parse(NAS).unwrap()), Some("Synology"));
    assert_eq!(vendor(Mac::parse("da:a1:19:12:34:56").unwrap()), None, "a private address names no vendor");
    assert_eq!(vendor(Mac::parse("01:00:5e:00:00:fb").unwrap()), None, "a group address names no vendor");
}

#[test]
fn the_vendor_table_is_sorted_and_every_line_reads() {
    let lines = VENDORS.lines().count();
    let table = vendors();
    assert_eq!(table.len(), lines, "every line has a prefix and a name");
    assert!(table.windows(2).all(|pair| pair[0].0 < pair[1].0), "one line per prefix, in order");
    assert!(table.iter().all(|(_, name)| !name.is_empty() && name.trim() == *name));
}

#[test]
fn own_addresses_broadcasts_groups_and_unanswered_sweeps_are_left_out() {
    let neighborhood = neighborhood(vec![
        neighbor("192.0.2.1", Some(ROUTER)),
        neighbor("192.0.2.20", Some("02:00:5e:10:00:20")),
        neighbor("192.0.2.255", Some("ff:ff:ff:ff:ff:ff")),
        neighbor("224.0.0.251", Some("01:00:5e:00:00:fb")),
        neighbor("192.0.2.77", None),
        neighbor("192.0.2.78", None),
        neighbor("192.0.2.79", Some(NAS)),
        neighbor("fe80::20", Some("02:00:5e:10:00:20")),
        neighbor("ff02::fb", Some("33:33:00:00:00:fb")),
    ]);
    let swept = HashSet::from([ip("192.0.2.78"), ip("192.0.2.79")]);
    let rows = rows(&neighborhood, &home(), &GatewayWatch::default(), &swept);
    assert_eq!(listed(&neighborhood, &rows), ["192.0.2.1", "192.0.2.77", "192.0.2.79"]);
}

#[test]
fn the_gateway_and_ipv4_addresses_sharing_a_mac_are_flagged() {
    let mut router = neighbor("fe80::1", Some(ROUTER));
    router.router = true;
    let mut elsewhere = neighbor("192.0.2.62", Some(NAS));
    elsewhere.interface = "en7".into();
    let neighborhood = neighborhood(vec![
        neighbor("192.0.2.1", Some(ROUTER)),
        neighbor("192.0.2.60", Some(NAS)),
        neighbor("192.0.2.61", Some(NAS)),
        elsewhere,
        router,
        neighbor("2001:db8::60", Some(NAS)),
    ]);
    let rows = rows(&neighborhood, &home(), &GatewayWatch::default(), &HashSet::new());
    let flags: Vec<(String, bool, bool)> = rows
        .iter()
        .map(|row| (neighborhood.neighbors[row.neighbor].address.to_string(), row.gateway, row.shared))
        .collect();
    assert_eq!(
        flags,
        [
            ("192.0.2.1".into(), true, false),
            ("192.0.2.60".into(), false, true),
            ("192.0.2.61".into(), false, true),
            ("192.0.2.62".into(), false, false),
            ("fe80::1".into(), true, false),
            ("2001:db8::60".into(), false, false),
        ]
    );
}

#[test]
fn a_gateway_answering_with_another_mac_is_reported_once_and_stays_flagged() {
    let mut watch = GatewayWatch::default();
    let first = neighborhood(vec![neighbor("192.0.2.1", Some(ROUTER)), neighbor("192.0.2.60", Some(NAS))]);
    assert_eq!(watch.watch(&first), Vec::new());
    let unanswered = neighborhood(vec![neighbor("192.0.2.1", None)]);
    assert_eq!(watch.watch(&unanswered), Vec::new(), "an entry without a MAC says nothing");
    let spoofed = neighborhood(vec![neighbor("192.0.2.1", Some(OTHER_ROUTER)), neighbor("192.0.2.60", Some(ROUTER))]);
    let change = MacChange { from: Mac::parse(ROUTER).unwrap(), to: Mac::parse(OTHER_ROUTER).unwrap() };
    assert_eq!(watch.watch(&spoofed), [(ip("192.0.2.1"), change)]);
    assert_eq!(watch.watch(&spoofed), Vec::new(), "the change is told once");
    assert_eq!(watch.watch(&first), Vec::new());
    assert_eq!(watch.change(&first.neighbors[0]), Some(change), "the flag stays after the MAC comes back");
    assert_eq!(watch.change(&first.neighbors[1]), None, "only gateways are watched");
}

#[test]
fn a_sweep_covers_the_hosts_of_a_small_subnet_and_the_slash_24_around_a_large_one() {
    let sweep = Sweep::of(&home()[0]).unwrap();
    assert_eq!((sweep.subnet(), sweep.interface.as_str(), sweep.addresses.len()), ("192.0.2.0/24".into(), "en0", 253));
    assert_eq!(sweep.addresses.first(), Some(&Ipv4Addr::new(192, 0, 2, 1)));
    assert_eq!(sweep.addresses.last(), Some(&Ipv4Addr::new(192, 0, 2, 254)));
    assert!(!sweep.addresses.contains(&Ipv4Addr::new(192, 0, 2, 20)), "not this machine");
    let office = interface("eth0", InterfaceKind::Ethernet, &[("198.51.100.9", 16)]);
    assert_eq!(Sweep::of(&office).unwrap().subnet(), "198.51.100.0/24", "the /24 around this machine");
    let tiny = interface("eth1", InterfaceKind::Ethernet, &[("203.0.113.1", 30)]);
    assert_eq!(Sweep::of(&tiny).unwrap().addresses, [Ipv4Addr::new(203, 0, 113, 2)]);
}

#[test]
fn tunnels_point_to_point_links_and_interfaces_that_are_down_are_not_swept() {
    let tunnel = interface("utun3", InterfaceKind::Tunnel, &[("198.51.100.2", 24)]);
    let link = interface("eth2", InterfaceKind::Ethernet, &[("198.51.100.2", 31)]);
    let ipv6 = interface("eth3", InterfaceKind::Ethernet, &[("2001:db8::2", 64)]);
    let down = Interface { up: false, ..home().remove(0) };
    for interface in [tunnel, link, ipv6, down] {
        assert_eq!(Sweep::of(&interface), None, "{}", interface.name);
    }
}

#[test]
fn a_neighbor_s_subnet_is_the_interface_address_that_holds_it() {
    let found = subnet(&home(), &neighbor("192.0.2.1", Some(ROUTER))).unwrap();
    assert_eq!((found.network, found.prefix, found.own), (ip("192.0.2.0"), 24, ip("192.0.2.20")));
    let link = subnet(&home(), &neighbor("fe80::1", Some(ROUTER))).unwrap();
    assert_eq!((link.network, link.own), (ip("fe80::"), ip("fe80::20")));
    assert_eq!(subnet(&home(), &neighbor("198.51.100.1", Some(ROUTER))), None);
}
