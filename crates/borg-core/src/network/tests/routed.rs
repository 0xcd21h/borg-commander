use super::*;

fn tap(state: &mut AppState, code: KeyCode) -> Vec<Effect> {
    press(state, code, KeyModifiers::NONE)
}

fn typed(state: &mut AppState, text: &str) {
    for c in text.chars() {
        tap(state, KeyCode::Char(c));
    }
}

fn v4(text: &str) -> Ipv4Addr {
    text.parse().unwrap()
}

fn traced(address: &str, hops: u8, path: &[&str]) -> Probe {
    Probe {
        trace: Some(Trace { hops: Some(hops), path: path.iter().map(|router| v4(router)).collect() }),
        ..probe(address, Some(3), &[], false)
    }
}

fn scan_form(state: &AppState) -> &crate::ScanForm {
    match &state.overlay {
        Some(Overlay::ScanNetwork(form)) => form,
        other => panic!("the scan form is open: {other:?}"),
    }
}

/// The Neighbors page's rows in words: headings by what they name, hosts by address.
fn map(network: &Network) -> Vec<String> {
    network
        .neighbors
        .iter()
        .map(|entry| match *entry {
            NeighborEntry::Interface(index) => format!("[{}]", network.sample.interfaces[index].name),
            NeighborEntry::Network(index) => format!("› {}", network.routed[index].cidr()),
            NeighborEntry::Internet(index) => format!("› internet {}", network.internet[index].interface),
            NeighborEntry::Host(row) => network.neighborhood.neighbors[row.neighbor].address.to_string(),
        })
        .collect()
}

fn select(state: &mut AppState, row: &str) {
    let index = map(&state.network).iter().position(|shown| shown == row).unwrap();
    state.network.select(index);
}

/// Runs a discovery on the home network in which 10.0.0.0/24 answers on the gateway itself and
/// 172.16.0.0/24 three hops away.
fn discovered() -> (AppState, Instant) {
    let (mut state, now) = home_opened();
    tap(&mut state, KeyCode::Char('d'));
    let Some(Overlay::Confirm(confirm)) = &state.overlay else { panic!("D asks first: {:?}", state.overlay) };
    assert_eq!(
        confirm.message,
        "Look for networks through 192.0.2.1 on en0? Borg pings the likely routers of 7 networks, 14 addresses in \
         all, and traces the way to each one that answers, up to 8 hops."
    );
    assert!(confirm.accept, "Yes is the default");
    let effects = tap(&mut state, KeyCode::Enter);
    let Some(Effect::Discover { id, addresses }) = effects.first() else { panic!("Enter discovers: {effects:?}") };
    assert_eq!(addresses.len(), 14);
    assert!(addresses.contains(&v4("10.0.0.1")) && addresses.contains(&v4("192.168.1.254")));
    let id = *id;
    let found = [
        traced("10.0.0.1", 1, &[]),
        traced("10.0.0.254", 1, &[]),
        traced("172.16.0.1", 3, &["192.0.2.1", "198.51.100.1"]),
        probe("192.168.0.1", None, &[], false),
    ];
    let saved: Vec<bool> = found
        .into_iter()
        .map(|probe| saves(&update(&mut state, Msg::Probed { id, probe, at: HOME_READ_AT }, now)))
        .collect();
    assert_eq!(saved, [true, true, true, false], "each new host is saved at once");
    assert_eq!(state.network.scan.as_ref().map(|scan| (scan.done, scan.answered)), Some((4, 2)));
    update(&mut state, Msg::ScanFinished { id, result: Ok(()) }, now);
    assert_eq!(state.notice.as_deref(), Some("Looked through 192.0.2.1 on en0: 2 networks found."));
    (state, now)
}

#[test]
fn d_finds_the_networks_behind_the_gateway_and_the_map_shows_them_under_its_interface() {
    let (state, _) = discovered();
    assert_eq!(
        map(&state.network),
        [
            "[en0]",
            "192.0.2.1",
            "192.0.2.44",
            "192.0.2.60",
            "192.0.2.61",
            "192.0.2.77",
            "› 10.0.0.0/24",
            "10.0.0.1",
            "10.0.0.254",
            "› 172.16.0.0/24",
            "172.16.0.1",
            "› internet en0",
        ]
    );
    let routed = &state.network.routed;
    assert!(routed[0].on_gateway());
    assert_eq!(routed[1].path(), [v4("192.0.2.1"), v4("198.51.100.1")]);
    assert_eq!(routed[1].trace.hops, Some(3));
}

#[test]
fn a_discovered_network_is_remembered_for_the_next_visit_until_f8_forgets_it() {
    let (mut state, now) = discovered();
    let saved = state.network.memory.saved();
    state.network.memory = NeighborMemory::restore(saved).unwrap();
    state.network.reached.clear();
    state.network.probes.clear();
    let id = asked(&state.network.request()).unwrap().0;
    update(&mut state, Msg::Network { id, result: Ok(home_sample(2, ROUTER_MAC)) }, now);
    let remembered: Vec<(String, Option<u64>)> =
        state.network.routed.iter().map(|routed| (routed.cidr(), routed.remembered)).collect();
    assert_eq!(remembered, [("10.0.0.0/24".into(), Some(HOME_READ_AT)), ("172.16.0.0/24".into(), Some(HOME_READ_AT))]);
    assert_eq!(presence_of(&state.network, "10.0.0.254"), Some(Presence::Offline(HOME_READ_AT)));
    select(&mut state, "› 10.0.0.0/24");
    tap(&mut state, KeyCode::F(8));
    let Some(Overlay::Confirm(confirm)) = &state.overlay else { panic!("F8 asks first: {:?}", state.overlay) };
    assert_eq!(
        confirm.message,
        "Forget 10.0.0.0/24? Borg drops it and the hosts behind it from what it remembers for this network."
    );
    assert!(!confirm.accept, "No is the default");
    let effects = tap(&mut state, KeyCode::Char('y'));
    assert!(saves(&effects));
    assert_eq!(state.notice.as_deref(), Some("Forgot 10.0.0.0/24."));
    assert!(!map(&state.network).iter().any(|row| row.starts_with("10.0.0.")), "its hosts went with it");
}

#[test]
fn f8_leaves_a_network_the_route_table_lists() {
    let (mut state, now) = home_opened();
    let id = asked(&state.network.request()).unwrap().0;
    let mut sample = home_sample(2, ROUTER_MAC);
    let route = Route { network: v4("10.20.0.0"), prefix: 16, gateway: Some(v4("192.0.2.1")), interface: "en0".into() };
    sample.neighborhood.as_mut().unwrap().routes = vec![route];
    update(&mut state, Msg::Network { id, result: Ok(sample) }, now);
    select(&mut state, "› 10.20.0.0/16");
    tap(&mut state, KeyCode::F(8));
    assert!(state.overlay.is_none());
    assert_eq!(
        state.notice.as_deref(),
        Some("Borg forgets only networks it remembers that the route table does not list.")
    );
}

#[test]
fn s_on_a_routed_network_scans_it_through_the_router_and_answers_land_under_it() {
    let (mut state, now) = discovered();
    select(&mut state, "› 172.16.0.0/24");
    tap(&mut state, KeyCode::Char('s'));
    assert_eq!(scan_form(&state).network.text(), "172.16.0.0/24");
    assert_eq!(scan_form(&state).choices, ["192.0.2.0/24", "10.0.0.0/24", "172.16.0.0/24"]);
    tap(&mut state, KeyCode::Down);
    assert_eq!(scan_form(&state).network.text(), "192.0.2.0/24", "the choices wrap round");
    tap(&mut state, KeyCode::Up);
    let effects = tap(&mut state, KeyCode::Enter);
    let Some(Effect::ScanSubnet { id, addresses, nudge }) = effects.first() else { panic!("scans: {effects:?}") };
    assert_eq!(addresses.len(), 254);
    assert!(!nudge, "only the local link answers ARP");
    let id = *id;
    update(&mut state, Msg::Probed { id, probe: probe("172.16.0.30", Some(9), &[443], false), at: HOME_READ_AT }, now);
    let rows = map(&state.network);
    let heading = rows.iter().position(|row| row == "› 172.16.0.0/24").unwrap();
    assert_eq!(rows[heading + 1..heading + 3], ["172.16.0.30", "172.16.0.1"], "this scan's answers first");
    assert_eq!(state.network.open_ports("172.16.0.30".parse().unwrap()), Some([443].as_slice()));
}

#[test]
fn a_typed_network_must_read_and_one_beyond_every_known_network_asks_with_no_as_the_default() {
    let (mut state, now) = home_opened();
    tap(&mut state, KeyCode::Char('s'));
    for _ in 0.."192.0.2.0/24".len() {
        tap(&mut state, KeyCode::Backspace);
    }
    typed(&mut state, "10.0.0.0/16");
    tap(&mut state, KeyCode::Enter);
    assert_eq!(scan_form(&state).problem, Some(ScanProblem::TooWide));
    typed(&mut state, "x");
    assert_eq!(scan_form(&state).problem, None, "typing clears the problem");
    for _ in 0.."10.0.0.0/16x".len() {
        tap(&mut state, KeyCode::Backspace);
    }
    typed(&mut state, "203.0.113.0/28");
    tap(&mut state, KeyCode::Enter);
    let Some(Overlay::Confirm(confirm)) = &state.overlay else { panic!("asks first: {:?}", state.overlay) };
    assert_eq!(
        confirm.message,
        "Scan 203.0.113.0/28 on en0? Borg pings each of its 14 addresses and tries TCP ports 22, 80, 443.\n\n\
         203.0.113.0/28 lies outside the private ranges and the networks this machine reaches. Scan only networks \
         you are allowed to."
    );
    assert!(!confirm.accept);
    let effects = key(&mut state, KeyCode::Char('y'), now);
    let Some(Effect::ScanSubnet { id, nudge, .. }) = effects.first() else { panic!("Yes scans: {effects:?}") };
    assert!(!nudge);
    let id = *id;
    update(&mut state, Msg::Probed { id, probe: probe("203.0.113.5", Some(20), &[80], false), at: HOME_READ_AT }, now);
    assert!(
        map(&state.network).windows(2).any(|pair| pair == ["› 203.0.113.0/24", "203.0.113.5"]),
        "a host found outside the known networks brings its network onto the map"
    );
}

#[test]
fn a_port_scan_of_a_host_behind_a_routed_network_counts_it_as_known_and_keeps_the_gateway() {
    let (mut state, now) = home_opened();
    let id = asked(&state.network.request()).unwrap().0;
    let mut sample = home_sample(2, ROUTER_MAC);
    let route =
        Route { network: v4("203.0.113.0"), prefix: 24, gateway: Some(v4("192.0.2.1")), interface: "en0".into() };
    sample.neighborhood.as_mut().unwrap().routes = vec![route];
    update(&mut state, Msg::Network { id, result: Ok(sample) }, now);
    tap(&mut state, KeyCode::Char('p'));
    press(&mut state, KeyCode::Char('u'), KeyModifiers::CONTROL);
    typed(&mut state, "203.0.113.9");
    tap(&mut state, KeyCode::Enter);
    let Some(Overlay::Confirm(confirm)) = &state.overlay else { panic!("P asks first: {:?}", state.overlay) };
    assert!(!confirm.message.contains("outside"), "{}", confirm.message);
    let crate::ConfirmPurpose::ScanPorts(request) = &confirm.purpose else { panic!("a port scan") };
    assert_eq!(request.gateway, Mac::parse(ROUTER_MAC));
}
