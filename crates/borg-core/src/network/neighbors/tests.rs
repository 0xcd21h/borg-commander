use super::*;

const ROUTER: &str = "00:27:22:4c:01:01";
const OTHER_ROUTER: &str = "00:16:3e:44:55:66";
const NAS: &str = "00:11:32:aa:00:60";
const PRINTER: &str = "00:1b:a9:00:00:70";
const READ_AT: u64 = 1_800_000_000;
const EARLIER: u64 = READ_AT - 86_400;

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
        routes: Vec::new(),
        error: None,
        read_at: READ_AT,
    }
}

fn mac(text: &str) -> Mac {
    Mac::parse(text).unwrap()
}

fn v4(text: &str) -> Ipv4Addr {
    text.parse().unwrap()
}

fn probed(address: &str, ping: Option<u64>, open: &[u16], refused: bool) -> (IpAddr, Probed) {
    let probe = Probe {
        address: v4(address),
        ping: ping.map(Duration::from_millis),
        open: open.to_vec(),
        refused,
        trace: None,
    };
    (ip(address), Probed { interface: "en0".into(), probe })
}

fn home_place() -> LocalNetwork {
    LocalNetwork {
        interface: "en0".into(),
        network: v4("192.0.2.0"),
        prefix: 24,
        own: v4("192.0.2.20"),
        gateway: Some(mac(ROUTER)),
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
fn own_addresses_broadcasts_groups_and_unanswered_scans_are_left_out() {
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
    let scanned = HashSet::from([ip("192.0.2.78"), ip("192.0.2.79")]);
    let rows = rows(&neighborhood, &[], &home(), &GatewayWatch::default(), &scanned);
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
    let rows = rows(&neighborhood, &[], &home(), &GatewayWatch::default(), &HashSet::new());
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
fn a_scan_covers_the_hosts_of_a_small_subnet_and_the_slash_24_around_a_large_one() {
    let scan = SubnetScan::of(&home()[0]).unwrap();
    assert_eq!((scan.subnet(), scan.interface.as_str(), scan.addresses.len()), ("192.0.2.0/24".into(), "en0", 253));
    assert_eq!(scan.addresses.first(), Some(&Ipv4Addr::new(192, 0, 2, 1)));
    assert_eq!(scan.addresses.last(), Some(&Ipv4Addr::new(192, 0, 2, 254)));
    assert!(!scan.addresses.contains(&Ipv4Addr::new(192, 0, 2, 20)), "not this machine");
    let office = interface("eth0", InterfaceKind::Ethernet, &[("198.51.100.9", 16)]);
    assert_eq!(SubnetScan::of(&office).unwrap().subnet(), "198.51.100.0/24", "the /24 around this machine");
    let tiny = interface("eth1", InterfaceKind::Ethernet, &[("203.0.113.1", 30)]);
    assert_eq!(SubnetScan::of(&tiny).unwrap().addresses, [Ipv4Addr::new(203, 0, 113, 2)]);
}

#[test]
fn tunnels_point_to_point_links_and_interfaces_that_are_down_are_not_scanned() {
    let tunnel = interface("utun3", InterfaceKind::Tunnel, &[("198.51.100.2", 24)]);
    let link = interface("eth2", InterfaceKind::Ethernet, &[("198.51.100.2", 31)]);
    let ipv6 = interface("eth3", InterfaceKind::Ethernet, &[("2001:db8::2", 64)]);
    let down = Interface { up: false, ..home().remove(0) };
    for interface in [tunnel, link, ipv6, down] {
        assert_eq!(SubnetScan::of(&interface), None, "{}", interface.name);
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

#[test]
fn a_network_is_known_by_its_subnet_and_its_gateway_s_mac() {
    let table = neighborhood(vec![neighbor("192.0.2.1", Some(ROUTER))]);
    assert_eq!(places(&home(), &table), [home_place()]);
    let unanswered = neighborhood(vec![neighbor("192.0.2.1", None)]);
    assert_eq!(places(&home(), &unanswered), [], "a gateway without a MAC leaves the network unnamed");
    let no_gateway = Neighborhood { gateways: Vec::new(), ..neighborhood(Vec::new()) };
    assert_eq!(places(&home(), &no_gateway), [LocalNetwork { gateway: None, ..home_place() }]);
    let place = home_place();
    assert!(place.holds(v4("192.0.2.77")));
    for other in ["192.0.2.0", "192.0.2.20", "192.0.2.255", "198.51.100.7"] {
        assert!(!place.holds(v4(other)), "{other}");
    }
}

#[test]
fn scanned_hosts_that_answered_join_the_table_and_remembered_ones_show_offline() {
    let places = [home_place()];
    let mut memory = NeighborMemory::default();
    let earlier = neighborhood(vec![neighbor("192.0.2.60", Some(NAS)), neighbor("192.0.2.70", Some(PRINTER))]);
    memory.note_listed(&places, &earlier.neighbors, EARLIER);
    let table = neighborhood(vec![
        neighbor("192.0.2.1", Some(ROUTER)),
        neighbor("192.0.2.60", Some(NAS)),
        neighbor("192.0.2.70", None),
        neighbor("192.0.2.80", None),
    ]);
    let probes = HashMap::from([
        probed("192.0.2.80", Some(3), &[], false),
        probed("192.0.2.90", None, &[22], false),
        probed("192.0.2.91", None, &[], false),
    ]);
    let (merged, presence) = merge(&table, &probes, &memory, &places);
    let seen: Vec<(String, Option<Mac>, Presence)> = merged
        .neighbors
        .iter()
        .zip(&presence)
        .map(|(neighbor, presence)| (neighbor.address.to_string(), neighbor.mac, *presence))
        .collect();
    assert_eq!(
        seen,
        [
            ("192.0.2.1".into(), Some(mac(ROUTER)), Presence::Listed),
            ("192.0.2.60".into(), Some(mac(NAS)), Presence::Listed),
            ("192.0.2.70".into(), Some(mac(PRINTER)), Presence::Offline(EARLIER)),
            ("192.0.2.80".into(), None, Presence::Answered),
            ("192.0.2.90".into(), None, Presence::Answered),
        ]
    );
}

#[test]
fn offline_hosts_keep_their_remembered_ports_and_do_not_count_as_sharing_a_mac() {
    let places = [home_place()];
    let mut memory = NeighborMemory::default();
    let probe = Probe { address: v4("192.0.2.61"), ping: None, open: vec![80, 443], refused: false, trace: None };
    memory.note_probe(&places, &[], &probe, EARLIER);
    memory.note_listed(&places, &[neighbor("192.0.2.61", Some(NAS))], EARLIER);
    let table = neighborhood(vec![neighbor("192.0.2.1", Some(ROUTER)), neighbor("192.0.2.60", Some(NAS))]);
    let (merged, presence) = merge(&table, &HashMap::new(), &memory, &places);
    let rows = rows(&merged, &presence, &home(), &GatewayWatch::default(), &HashSet::new());
    let offline = rows.iter().find(|row| merged.neighbors[row.neighbor].address == ip("192.0.2.61")).unwrap();
    assert_eq!(offline.presence, Presence::Offline(EARLIER));
    assert!(rows.iter().all(|row| !row.shared), "a host that is gone shares nothing");
    assert_eq!(open_ports(&HashMap::new(), &memory, &places, ip("192.0.2.61")), Some([80, 443].as_slice()));
    let fresh = HashMap::from([probed("192.0.2.61", Some(1), &[22], false)]);
    assert_eq!(open_ports(&fresh, &memory, &places, ip("192.0.2.61")), Some([22].as_slice()), "this scan wins");
    assert_eq!(open_ports(&HashMap::new(), &memory, &places, ip("192.0.2.60")), None, "never scanned");
}
