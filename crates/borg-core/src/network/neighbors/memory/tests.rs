use std::time::Duration;

use super::*;
use crate::network::neighbors::NeighborState;

const ROUTER: &str = "00:27:22:4c:01:01";
const OTHER_ROUTER: &str = "00:16:3e:44:55:66";
const NAS: &str = "00:11:32:aa:00:60";
const EARLIER: u64 = 1_800_000_000;
const LATER: u64 = EARLIER + 3_600;

fn mac(text: &str) -> Mac {
    Mac::parse(text).unwrap()
}

fn v4(text: &str) -> Ipv4Addr {
    text.parse().unwrap()
}

fn place(gateway: &str) -> LocalNetwork {
    LocalNetwork {
        interface: "en0".into(),
        network: v4("192.0.2.0"),
        prefix: 24,
        own: v4("192.0.2.20"),
        gateway: Some(mac(gateway)),
    }
}

fn neighbor(address: &str, mac: Option<&str>) -> Neighbor {
    Neighbor {
        address: IpAddr::V4(v4(address)),
        mac: mac.map(self::mac),
        interface: "en0".into(),
        state: Some(NeighborState::Reachable),
        router: false,
    }
}

fn probe(address: &str, ping: Option<u64>, open: &[u16], refused: bool) -> Probe {
    Probe { address: v4(address), ping: ping.map(Duration::from_millis), open: open.to_vec(), refused, trace: None }
}

fn addresses(memory: &NeighborMemory, place: &LocalNetwork) -> Vec<String> {
    memory.hosts(place).iter().map(|host| host.address.to_string()).collect()
}

#[test]
fn listed_hosts_with_a_mac_are_learned_once_and_then_only_refreshed() {
    let places = [place(ROUTER)];
    let table = [
        neighbor("192.0.2.60", Some(NAS)),
        neighbor("192.0.2.77", None),
        neighbor("192.0.2.20", Some("02:00:5e:10:00:20")),
        neighbor("192.0.2.255", Some("ff:ff:ff:ff:ff:ff")),
        neighbor("198.51.100.4", Some(NAS)),
    ];
    let mut memory = NeighborMemory::default();
    assert_eq!(memory.note_listed(&places, &table, EARLIER), Noted { learned: true, refreshed: false });
    assert_eq!(addresses(&memory, &places[0]), ["192.0.2.60"]);
    assert_eq!(memory.note_listed(&places, &table, EARLIER), Noted::default(), "nothing new at the same time");
    assert_eq!(memory.note_listed(&places, &table, LATER), Noted { learned: false, refreshed: true });
    assert_eq!(memory.hosts(&places[0])[0].seen, LATER);
    let moved = [neighbor("192.0.2.60", Some(OTHER_ROUTER))];
    assert!(memory.note_listed(&places, &moved, LATER).learned, "a new MAC is news");
}

#[test]
fn a_probe_that_answered_records_its_ports_and_ping_and_one_that_did_not_is_ignored() {
    let places = [place(ROUTER)];
    let mut memory = NeighborMemory::default();
    assert_eq!(memory.note_probe(&places, &[], &probe("192.0.2.90", None, &[], false), EARLIER), Noted::default());
    assert_eq!(
        memory.note_probe(&places, &[], &probe("198.51.100.9", Some(2), &[22], false), EARLIER),
        Noted::default()
    );
    let noted = memory.note_probe(&places, &[], &probe("192.0.2.90", Some(4), &[22, 443], false), EARLIER);
    assert_eq!(noted, Noted { learned: true, refreshed: true });
    let host = &memory.hosts(&places[0])[0];
    assert_eq!(
        host,
        &KnownHost {
            address: v4("192.0.2.90"),
            mac: None,
            seen: EARLIER,
            scanned: Some(EARLIER),
            ports: vec![22, 443],
            ping: true,
            heard: None,
            announced: Announced::default(),
        }
    );
    let refused = memory.note_probe(&places, &[], &probe("192.0.2.90", None, &[], true), LATER);
    assert!(refused.learned, "the ports changed");
    assert_eq!(memory.hosts(&places[0])[0].ports, Vec::<u16>::new());
}

#[test]
fn the_same_addresses_behind_another_gateway_are_another_network() {
    let home = place(ROUTER);
    let cafe = place(OTHER_ROUTER);
    let mut memory = NeighborMemory::default();
    memory.note_listed(std::slice::from_ref(&home), &[neighbor("192.0.2.60", Some(NAS))], EARLIER);
    assert_eq!(addresses(&memory, &cafe), Vec::<String>::new());
    let places = [home.clone(), cafe.clone()];
    assert!(memory.host(&places, IpAddr::V4(v4("192.0.2.60"))).is_some());
    memory.note_listed(std::slice::from_ref(&cafe), &[neighbor("192.0.2.61", Some(NAS))], LATER);
    assert_eq!(addresses(&memory, &home), ["192.0.2.60"]);
    assert_eq!(addresses(&memory, &cafe), ["192.0.2.61"]);
}

#[test]
fn forgetting_the_last_host_of_a_network_forgets_the_network() {
    let home = place(ROUTER);
    let mut memory = NeighborMemory::default();
    let table = [neighbor("192.0.2.60", Some(NAS)), neighbor("192.0.2.61", Some(NAS))];
    memory.note_listed(std::slice::from_ref(&home), &table, EARLIER);
    assert!(memory.forget(&home, v4("192.0.2.60")));
    assert!(!memory.forget(&home, v4("192.0.2.60")), "already gone");
    assert!(!memory.forget(&place(OTHER_ROUTER), v4("192.0.2.61")), "another network");
    assert!(memory.forget(&home, v4("192.0.2.61")));
    assert_eq!(memory.saved().networks, Vec::new());
}

#[test]
fn a_full_network_keeps_the_hosts_seen_last() {
    let home = LocalNetwork { network: v4("10.0.0.0"), prefix: 16, own: v4("10.0.0.1"), ..place(ROUTER) };
    let mut memory = NeighborMemory::default();
    let extra: u32 = 4;
    for (index, address) in (u32::from(v4("10.0.1.0"))..).take(MAX_HOSTS + extra as usize).enumerate() {
        let probe = Probe {
            address: Ipv4Addr::from(address),
            ping: Some(Duration::ZERO),
            open: Vec::new(),
            refused: false,
            trace: None,
        };
        memory.note_probe(std::slice::from_ref(&home), &[], &probe, EARLIER + index as u64);
    }
    let hosts = memory.hosts(&home);
    assert_eq!(hosts.len(), MAX_HOSTS);
    assert_eq!(hosts[0].address, Ipv4Addr::from(u32::from(v4("10.0.1.0")) + extra), "the oldest went first");
    assert!(hosts.windows(2).all(|pair| pair[0].address < pair[1].address), "kept in address order");
}

#[test]
fn the_stored_format_reads_back_and_names_every_field() {
    let home = place(ROUTER);
    let mut memory = NeighborMemory::default();
    memory.note_listed(std::slice::from_ref(&home), &[neighbor("192.0.2.60", Some(NAS))], EARLIER);
    memory.note_probe(std::slice::from_ref(&home), &[], &probe("192.0.2.90", Some(4), &[22], false), LATER);
    let json = serde_json::to_value(memory.saved()).unwrap();
    assert_eq!(
        json,
        serde_json::json!({
            "version": NEIGHBORS_FORMAT,
            "networks": [{
                "network": "192.0.2.0",
                "prefix": 24,
                "gateway": ROUTER,
                "hosts": [
                    {"address": "192.0.2.60", "mac": NAS, "seen": EARLIER, "scanned": null, "ports": [], "ping": false},
                    {"address": "192.0.2.90", "mac": null, "seen": LATER, "scanned": LATER, "ports": [22], "ping": true},
                ],
                "routes": [],
            }],
        })
    );
    let read: SavedNeighbors = serde_json::from_value(json).unwrap();
    assert_eq!(NeighborMemory::restore(read), Ok(memory));
    let older = serde_json::json!({
        "version": NEIGHBORS_FORMAT,
        "networks": [{"network": "192.0.2.0", "prefix": 24, "gateway": null,
            "hosts": [{"address": "192.0.2.60", "mac": NAS, "seen": EARLIER}]}],
    });
    let read: SavedNeighbors = serde_json::from_value(older).unwrap();
    assert_eq!(read.networks[0].hosts[0].scanned, None, "fields added later default");
}

#[test]
fn a_stored_file_that_breaks_the_rules_is_refused() {
    let host = KnownHost {
        address: v4("192.0.2.60"),
        mac: None,
        seen: EARLIER,
        scanned: None,
        ports: Vec::new(),
        ping: false,
        heard: None,
        announced: Announced::default(),
    };
    let network = KnownNetwork {
        network: v4("192.0.2.0"),
        prefix: 24,
        gateway: None,
        hosts: vec![host.clone()],
        routes: Vec::new(),
    };
    let saved = |networks: Vec<KnownNetwork>| SavedNeighbors { version: NEIGHBORS_FORMAT, networks };
    assert!(NeighborMemory::restore(saved(vec![network.clone()])).is_ok());
    let newer = SavedNeighbors { version: NEIGHBORS_FORMAT + 1, networks: Vec::new() };
    assert_eq!(NeighborMemory::restore(newer), Err(UNKNOWN_FORMAT));
    let outside = KnownHost { address: v4("198.51.100.4"), ..host.clone() };
    assert_eq!(
        NeighborMemory::restore(saved(vec![KnownNetwork { hosts: vec![outside], ..network.clone() }])),
        Err(OUTSIDE)
    );
    let prefix = KnownNetwork { prefix: MAX_PREFIX + 1, ..network.clone() };
    assert_eq!(NeighborMemory::restore(saved(vec![prefix])), Err(BAD_PREFIX));
    let shouting = Announced { client: Some("x".repeat(crate::LONGEST_TEXT + 1)), ..Announced::default() };
    let loud = KnownHost { announced: shouting, ..host.clone() };
    assert_eq!(
        NeighborMemory::restore(saved(vec![KnownNetwork { hosts: vec![loud], ..network.clone() }])),
        Err(BAD_ANNOUNCEMENT)
    );
    let crowded = KnownNetwork { hosts: vec![host; MAX_HOSTS + 1], ..network.clone() };
    assert_eq!(NeighborMemory::restore(saved(vec![crowded])), Err(TOO_MANY_HOSTS));
    assert_eq!(NeighborMemory::restore(saved(vec![network; MAX_NETWORKS + 1])), Err(TOO_MANY_NETWORKS));
    assert!(
        serde_json::from_str::<SavedNeighbors>(
            r#"{"version":1,"networks":[{"network":"192.0.2.0","prefix":24,"gateway":"nonsense","hosts":[]}]}"#
        )
        .is_err()
    );
}

fn heard(address: &str, interface: &str, name: &str, at: u64) -> HeardHost {
    let said = crate::Said { protocol: crate::ListenProtocol::Mdns, text: name.into() };
    HeardHost {
        address: IpAddr::V4(v4(address)),
        mac: Some(mac(NAS)),
        interface: interface.into(),
        router: false,
        announced: Announced {
            names: vec![said],
            protocols: vec![crate::ListenProtocol::Mdns],
            ..Announced::default()
        },
        first: at,
        last: at,
    }
}

#[test]
fn hosts_listening_heard_on_a_network_are_remembered_with_what_they_announced() {
    let home = place(ROUTER);
    let mut memory = NeighborMemory::default();
    let hosts = [
        heard("192.0.2.60", "en0", "nas.local", EARLIER),
        heard("192.0.2.61", "en1", "elsewhere.local", EARLIER),
        heard("198.51.100.4", "en0", "outside.local", EARLIER),
    ];
    let noted = memory.note_heard(std::slice::from_ref(&home), &hosts);
    assert!(noted.learned && noted.refreshed);
    assert_eq!(addresses(&memory, &home), ["192.0.2.60"], "only hosts on the interface's own network");
    let known = &memory.hosts(&home)[0];
    assert_eq!((known.heard, known.seen, known.mac), (Some(EARLIER), EARLIER, Some(mac(NAS))));
    assert_eq!(known.announced.name().map(|name| name.text.as_str()), Some("nas.local"));

    let noted = memory.note_heard(std::slice::from_ref(&home), &hosts[..1]);
    assert!(!noted.learned && !noted.refreshed, "the same again tells nothing new");
    let noted = memory.note_heard(std::slice::from_ref(&home), &[heard("192.0.2.60", "en0", "nas.local", LATER)]);
    assert!(!noted.learned && noted.refreshed);
    let json = serde_json::to_value(memory.saved()).unwrap();
    let stored = &json["networks"][0]["hosts"][0];
    assert_eq!(stored["heard"], LATER);
    assert_eq!(
        stored["announced"],
        serde_json::json!({"names": [{"protocol": "mdns", "text": "nas.local"}], "protocols": ["mdns"]})
    );
    let read: SavedNeighbors = serde_json::from_value(json).unwrap();
    assert_eq!(NeighborMemory::restore(read), Ok(memory));
}

fn behind(cidr: &str, hops: Option<u8>) -> Routed {
    let (network, prefix) = cidr.split_once('/').unwrap();
    Routed {
        network: v4(network),
        prefix: prefix.parse().unwrap(),
        interface: "en0".into(),
        gateway: Some(v4("192.0.2.1")),
        trace: Trace { hops, path: Vec::new() },
        origin: Origin::Discovery,
        remembered: None,
    }
}

#[test]
fn hosts_behind_a_routed_network_are_remembered_with_it_and_forgotten_with_it() {
    let home = place(ROUTER);
    let places = std::slice::from_ref(&home);
    let routed = [behind("198.51.100.0/24", None)];
    let mut memory = NeighborMemory::default();
    let noted = memory.note_probe(places, &routed, &probe("198.51.100.9", Some(2), &[22], false), EARLIER);
    assert_eq!(noted, Noted { learned: true, refreshed: true });
    assert_eq!(addresses(&memory, &home), ["198.51.100.9"]);
    let traced = behind("198.51.100.0/24", Some(2));
    assert!(memory.note_reached(&home, &traced, &probe("198.51.100.1", Some(1), &[], false), LATER).learned);
    let remembered = memory.remembered(places);
    assert_eq!(remembered.len(), 1);
    assert_eq!((remembered[0].trace.hops, remembered[0].remembered), (Some(2), Some(LATER)));
    assert!(
        memory.note_probe(places, &[], &probe("203.0.113.4", Some(1), &[], false), LATER) == Noted::default(),
        "a host behind no known network is not remembered"
    );
    assert!(memory.forget_route(&home, v4("198.51.100.0"), 24));
    assert_eq!(memory.saved().networks, Vec::new(), "its hosts went with it");
}

#[test]
fn a_file_from_before_routes_reads_and_a_bad_route_is_refused() {
    let older = serde_json::json!({
        "version": 1,
        "networks": [{"network": "192.0.2.0", "prefix": 24, "gateway": ROUTER,
            "hosts": [{"address": "192.0.2.60", "mac": NAS, "seen": EARLIER}]}],
    });
    let read: SavedNeighbors = serde_json::from_value(older).unwrap();
    let memory = NeighborMemory::restore(read).unwrap();
    assert_eq!(addresses(&memory, &place(ROUTER)), ["192.0.2.60"]);
    let route = KnownRoute {
        network: v4("198.51.100.0"),
        prefix: 24,
        gateway: None,
        trace: Trace::default(),
        origin: Origin::Discovery,
        seen: EARLIER,
    };
    let network = |routes: Vec<KnownRoute>| SavedNeighbors {
        version: NEIGHBORS_FORMAT,
        networks: vec![KnownNetwork { network: v4("192.0.2.0"), prefix: 24, gateway: None, hosts: Vec::new(), routes }],
    };
    assert!(NeighborMemory::restore(network(vec![route.clone()])).is_ok());
    let unaligned = KnownRoute { network: v4("198.51.100.7"), ..route.clone() };
    assert_eq!(NeighborMemory::restore(network(vec![unaligned])), Err(BAD_ROUTE));
    let far = KnownRoute { trace: Trace { hops: Some(MAX_HOPS + 1), path: Vec::new() }, ..route.clone() };
    assert_eq!(NeighborMemory::restore(network(vec![far])), Err(BAD_ROUTE));
    assert_eq!(NeighborMemory::restore(network(vec![route; MAX_ROUTES + 1])), Err(TOO_MANY_ROUTES));
}
