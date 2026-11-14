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
    Probe { address: v4(address), ping: ping.map(Duration::from_millis), open: open.to_vec(), refused }
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
    assert_eq!(memory.note_probe(&places, &probe("192.0.2.90", None, &[], false), EARLIER), Noted::default());
    assert_eq!(memory.note_probe(&places, &probe("198.51.100.9", Some(2), &[22], false), EARLIER), Noted::default());
    let noted = memory.note_probe(&places, &probe("192.0.2.90", Some(4), &[22, 443], false), EARLIER);
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
            ping: true
        }
    );
    let refused = memory.note_probe(&places, &probe("192.0.2.90", None, &[], true), LATER);
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
        let probe =
            Probe { address: Ipv4Addr::from(address), ping: Some(Duration::ZERO), open: Vec::new(), refused: false };
        memory.note_probe(std::slice::from_ref(&home), &probe, EARLIER + index as u64);
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
    memory.note_probe(std::slice::from_ref(&home), &probe("192.0.2.90", Some(4), &[22], false), LATER);
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
    };
    let network = KnownNetwork { network: v4("192.0.2.0"), prefix: 24, gateway: None, hosts: vec![host.clone()] };
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
