use super::*;

const ROUTER: &str = "00:27:22:4c:01:01";
const OTHER_ROUTER: &str = "00:16:3e:44:55:66";
const EARLIER: u64 = 1_800_000_000;
const LATER: u64 = EARLIER + 3_600;

fn address(text: &str) -> IpAddr {
    text.parse().unwrap()
}

fn mac(text: &str) -> Mac {
    Mac::parse(text).unwrap()
}

fn record(target: &str, gateway: Option<&str>, at: u64, open: &[u16]) -> PortScanRecord {
    PortScanRecord {
        target: target.into(),
        address: address(target),
        gateway: gateway.map(mac),
        choice: PortChoice::Custom("22,80,443".into()),
        speed: ScanSpeed::Fast,
        count: 3,
        at,
        open: open.to_vec(),
        closed: 3 - open.len(),
        filtered: 0,
        not_scanned: Vec::new(),
        ending: Ending::Done,
    }
}

#[test]
fn a_list_of_ports_and_ranges_reads_sorted_without_repeats() {
    assert_eq!(parse_ports("22,80,8000-8003"), Ok(vec![22, 80, 8000, 8001, 8002, 8003]));
    assert_eq!(parse_ports(" 443 22, 22 "), Ok(vec![22, 443]));
    assert_eq!(parse_ports("65535"), Ok(vec![65535]));
    assert_eq!(parse_ports(" , "), Err(PortListError::Empty));
    assert_eq!(parse_ports("0"), Err(PortListError::NotAPort("0".into())));
    assert_eq!(parse_ports("22,70000"), Err(PortListError::NotAPort("70000".into())));
    assert_eq!(parse_ports("ssh"), Err(PortListError::NotAPort("ssh".into())));
    assert_eq!(parse_ports("90-80"), Err(PortListError::Backwards("90-80".into())));
    assert_eq!(PortChoice::All.ports().map(|ports| ports.len()), Ok(usize::from(LAST_PORT)));
}

#[test]
fn ports_in_a_row_make_ranges_that_read_back() {
    let found = ranges(&[1, 2, 3, 5, 7, 8, 65535]);
    assert_eq!(describe(&found), "1-3, 5, 7-8, 65535");
    assert_eq!(found.iter().map(|range| range.count()).sum::<usize>(), 7);
    assert_eq!(ranges(&[]), Vec::new());
}

#[test]
fn the_common_ports_are_the_named_well_known_ones_and_a_few_registered_services() {
    let common = common_ports();
    assert!(common.windows(2).all(|pair| pair[0] < pair[1]), "in order");
    assert!(common.iter().all(|port| port_name(*port).is_some()), "every one has its IANA name");
    for port in [22, 25, 53, 80, 110, 143, 443, 445, 3306, 3389, 5432, 6379, 8080, 27017] {
        assert!(common.contains(&port), "{port} is common");
    }
    assert!(!common.contains(&62078), "an unnamed port is not");
    assert_eq!(port_name(22), Some("ssh"));
    assert_eq!(port_name(443), Some("https"));
    assert_eq!(port_name(62078), None);
}

#[test]
fn the_first_port_that_answers_is_the_canary_and_an_open_one_takes_over() {
    let mut watch = Watch::default();
    watch.record(21, PortAnswer::Filtered);
    assert_eq!(watch.canary(), None, "silence teaches nothing");
    watch.record(23, PortAnswer::Closed);
    assert_eq!(watch.canary(), Some(23));
    watch.record(80, PortAnswer::Open);
    assert_eq!(watch.canary(), Some(80), "an open port answers more surely");
    watch.record(443, PortAnswer::Open);
    assert_eq!(watch.canary(), Some(80), "the first open one stays");
}

#[test]
fn a_silent_canary_gets_one_retry_and_then_stops_the_scan() {
    let mut watch = Watch::default();
    assert_eq!(watch.canary_answered(PortAnswer::Filtered), Canary::Fine, "no canary yet");
    watch.record(80, PortAnswer::Open);
    assert_eq!(watch.canary_answered(PortAnswer::Open), Canary::Fine);
    assert_eq!(watch.canary_answered(PortAnswer::Filtered), Canary::Retry);
    assert_eq!(watch.canary_answered(PortAnswer::Closed), Canary::Fine, "a refusal is an answer");
    assert_eq!(watch.canary_answered(PortAnswer::Filtered), Canary::Retry, "the miss before was forgotten");
    assert_eq!(watch.canary_answered(PortAnswer::Filtered), Canary::Stop { port: 80 });
}

#[test]
fn a_sudden_run_of_silence_after_answers_calls_the_canary() {
    let mut watch = Watch::default();
    let answered: Vec<bool> = (1..=SILENT_RUN as u16).map(|port| watch.record(port, PortAnswer::Closed)).collect();
    assert!(answered.iter().all(|sudden| !sudden));
    let silent: Vec<bool> =
        (1..=SILENT_RUN as u16).map(|port| watch.record(1_000 + port, PortAnswer::Filtered)).collect();
    assert_eq!(silent.iter().filter(|sudden| **sudden).count(), 1, "only once the run is whole: {silent:?}");
    assert_eq!(silent.last(), Some(&true));
    assert_eq!(watch.canary_answered(PortAnswer::Closed), Canary::Fine);
    assert!(!watch.record(2_000, PortAnswer::Filtered), "an answered canary forgets the run");
}

#[test]
fn a_host_that_was_quiet_from_the_start_never_looks_sudden() {
    let mut watch = Watch::default();
    watch.record(22, PortAnswer::Open);
    let quiet = (100..100 + SILENT_RUN as u16 * 3).map(|port| watch.record(port, PortAnswer::Filtered));
    assert!(quiet.collect::<Vec<_>>().iter().all(|sudden| !sudden));
    let mut scattered = Watch::default();
    let mixed = (1..=SILENT_RUN as u16 * 3).map(|port| {
        let answer = if port % 4 == 0 { PortAnswer::Closed } else { PortAnswer::Filtered };
        scattered.record(port, answer)
    });
    assert!(mixed.collect::<Vec<_>>().iter().all(|sudden| !sudden), "a few answers in the run keep it calm");
}

#[test]
fn the_tally_counts_answers_and_names_the_ports_never_scanned() {
    let mut tally = Tally::new(vec![443, 22, 80, 8000, 8001, 8002, 9000]);
    tally.record(0, PortAnswer::Open);
    tally.record(1, PortAnswer::Open);
    tally.record(2, PortAnswer::Closed);
    tally.record(6, PortAnswer::Filtered);
    tally.record(99, PortAnswer::Open);
    assert_eq!(tally.done(), 4);
    let result = tally.result(Ending::RateLimited { port: 22 });
    assert_eq!(result.open, vec![22, 443]);
    assert_eq!((result.closed, result.filtered), (1, 1));
    assert_eq!(result.not_scanned, vec![PortRange { first: 8000, last: 8002 }]);
    assert_eq!(result.ending, Ending::RateLimited { port: 22 });
}

#[test]
fn private_loopback_link_local_and_shared_addresses_are_local() {
    for local in ["10.1.2.3", "172.16.0.9", "192.168.1.20", "127.0.0.1", "169.254.7.7", "100.64.0.1", "100.127.255.254"]
    {
        assert!(is_private(address(local)), "{local}");
    }
    for local in ["::1", "fd12:3456::1", "fe80::1"] {
        assert!(is_private(address(local)), "{local}");
    }
    for outside in ["192.0.2.60", "198.51.100.4", "203.0.113.9", "100.128.0.1", "2001:db8::1"] {
        assert!(!is_private(address(outside)), "{outside}");
    }
}

#[test]
fn a_new_scan_of_a_host_replaces_its_last_and_another_gateway_is_another_host() {
    let mut memory = PortScanMemory::default();
    memory.note(record("192.0.2.60", Some(ROUTER), EARLIER, &[22]));
    memory.note(record("192.0.2.60", Some(OTHER_ROUTER), EARLIER, &[80]));
    memory.note(record("192.0.2.60", Some(ROUTER), LATER, &[22, 443]));
    assert_eq!(
        memory.find(address("192.0.2.60"), Some(mac(ROUTER))).map(|scan| scan.open.clone()),
        Some(vec![22, 443])
    );
    assert_eq!(memory.find(address("192.0.2.60"), Some(mac(OTHER_ROUTER))).map(|scan| scan.at), Some(EARLIER));
    assert_eq!(memory.find(address("192.0.2.60"), None), None);
    assert_eq!(memory.saved().scans.len(), 2);
    assert_eq!(memory.last(" 192.0.2.60 ").map(|scan| scan.at), Some(LATER), "the newest first");
}

#[test]
fn a_name_finds_its_last_scan_too() {
    let mut memory = PortScanMemory::default();
    memory.note(PortScanRecord { target: "nas.home.arpa".into(), ..record("192.0.2.60", None, EARLIER, &[22]) });
    assert_eq!(memory.last("NAS.home.arpa").map(|scan| scan.open.clone()), Some(vec![22]));
    assert_eq!(memory.last("192.0.2.60").map(|scan| scan.target.clone()), Some("nas.home.arpa".into()));
    assert_eq!(memory.last("printer.home.arpa"), None);
}

#[test]
fn the_stored_format_names_every_field_and_reads_back() {
    let mut memory = PortScanMemory::default();
    let limited = PortScanRecord {
        choice: PortChoice::Common,
        speed: ScanSpeed::Slow,
        count: 5,
        closed: 1,
        not_scanned: vec![PortRange { first: 8000, last: 8001 }],
        ending: Ending::RateLimited { port: 80 },
        ..record("192.0.2.60", Some(ROUTER), EARLIER, &[22, 80])
    };
    memory.note(limited);
    let json = serde_json::to_value(memory.saved()).unwrap();
    assert_eq!(
        json,
        serde_json::json!({
            "version": PORT_SCANS_FORMAT,
            "scans": [{
                "target": "192.0.2.60",
                "address": "192.0.2.60",
                "gateway": ROUTER,
                "choice": "common",
                "speed": "slow",
                "count": 5,
                "at": EARLIER,
                "open": [22, 80],
                "closed": 1,
                "filtered": 0,
                "not_scanned": [{"first": 8000, "last": 8001}],
                "ending": {"rate_limited": {"port": 80}},
            }],
        })
    );
    let read: SavedPortScans = serde_json::from_value(json).unwrap();
    assert_eq!(PortScanMemory::restore(read), Ok(memory));
    let listed = serde_json::to_value(PortChoice::Custom("22,80".into())).unwrap();
    assert_eq!(listed, serde_json::json!({"custom": "22,80"}));
}

#[test]
fn a_stored_file_that_does_not_add_up_is_refused() {
    let good = record("192.0.2.60", None, EARLIER, &[22]);
    let saved = |scans: Vec<PortScanRecord>| SavedPortScans { version: PORT_SCANS_FORMAT, scans };
    assert!(PortScanMemory::restore(saved(vec![good.clone()])).is_ok());
    let newer = SavedPortScans { version: PORT_SCANS_FORMAT + 1, scans: Vec::new() };
    assert!(PortScanMemory::restore(newer).is_err());
    let counted_wrong = PortScanRecord { count: 9, ..good.clone() };
    assert!(PortScanMemory::restore(saved(vec![counted_wrong])).is_err());
    let unsorted = PortScanRecord { open: vec![443, 22], closed: 1, ..good.clone() };
    assert!(PortScanMemory::restore(saved(vec![unsorted])).is_err());
    let crowded = vec![good; 257];
    assert!(PortScanMemory::restore(saved(crowded)).is_err());
}
