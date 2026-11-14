use std::{
    net::{IpAddr, Ipv4Addr},
    time::Duration,
};

use serde_json::{Value, json};

use super::{
    HostFlag, NeighborsReport, ReportFormat, ReportHost, ReportPresence, ReportSource,
    text::{csv, table},
};
use crate::{Ending, Mac, NeighborState, PortChoice, PortRange, PortScanRecord, ScanNetwork, ScanSpeed};

const SCANNED_AT: u64 = 1_768_471_200;
const DAY: u64 = 86_400;
const ROUTER_MAC: &str = "00:27:22:4c:01:01";
const NAS_MAC: &str = "00:11:32:aa:00:60";
const PHONE_MAC: &str = "da:a1:19:12:34:56";

fn address(text: &str) -> IpAddr {
    text.parse().unwrap()
}

fn mac(text: &str) -> Mac {
    Mac::parse(text).unwrap()
}

fn network() -> ScanNetwork {
    ScanNetwork {
        interface: "en0".into(),
        subnet: Ipv4Addr::new(192, 0, 2, 0),
        prefix: 24,
        own: Ipv4Addr::new(192, 0, 2, 10),
        gateway: Some(address("192.0.2.1")),
        gateway_mac: Some(mac(ROUTER_MAC)),
    }
}

/// A Fast scan of the storage box that the host cut short after 500 of 720 ports.
fn limited() -> PortScanRecord {
    PortScanRecord {
        target: "nas.home.arpa".into(),
        address: address("192.0.2.60"),
        gateway: Some(mac(ROUTER_MAC)),
        choice: PortChoice::Custom("1-720".into()),
        speed: ScanSpeed::Fast,
        count: 720,
        at: SCANNED_AT,
        open: vec![22, 80, 445],
        closed: 400,
        filtered: 97,
        not_scanned: vec![PortRange { first: 501, last: 720 }],
        ending: Ending::RateLimited { port: 80 },
        network: Some(network()),
    }
}

fn quiet() -> PortScanRecord {
    PortScanRecord {
        target: "192.0.2.44".into(),
        address: address("192.0.2.44"),
        gateway: None,
        choice: PortChoice::Common,
        speed: ScanSpeed::Slow,
        count: 30,
        at: SCANNED_AT,
        open: Vec::new(),
        closed: 0,
        filtered: 30,
        not_scanned: Vec::new(),
        ending: Ending::Done,
        network: None,
    }
}

fn host(text: &str, mac_text: Option<&str>) -> ReportHost {
    ReportHost {
        address: address(text),
        mac: mac_text.map(mac),
        vendor: None,
        name: None,
        interface: "en0".into(),
        state: None,
        presence: ReportPresence::Listed,
        last_seen: None,
        flags: Vec::new(),
        ports: None,
        ping: None,
        port_scan: None,
    }
}

/// A router, the storage box with its scan, and a phone that went offline the day before.
fn neighbors() -> NeighborsReport {
    NeighborsReport {
        read_at: SCANNED_AT,
        networks: vec![network()],
        dns: vec![address("192.0.2.1")],
        hosts: vec![
            ReportHost {
                name: Some("router.home.arpa".into()),
                state: Some(NeighborState::Reachable),
                flags: vec![HostFlag::Gateway],
                ports: Some(vec![53, 80]),
                ping: Some(Duration::from_micros(1_200)),
                ..host("192.0.2.1", Some(ROUTER_MAC))
            },
            ReportHost {
                vendor: Some("Example Storage"),
                name: Some("nas.home.arpa".into()),
                state: Some(NeighborState::Stale),
                presence: ReportPresence::Answered,
                ports: Some(vec![22, 445]),
                ping: Some(Duration::from_micros(800)),
                port_scan: Some(limited()),
                ..host("192.0.2.60", Some(NAS_MAC))
            },
            ReportHost {
                presence: ReportPresence::Offline,
                last_seen: Some(SCANNED_AT - DAY),
                flags: vec![HostFlag::Randomized],
                ..host("192.0.2.77", Some(PHONE_MAC))
            },
        ],
    }
}

fn scan(record: PortScanRecord, format: ReportFormat) -> String {
    ReportSource::PortScan(Box::new(record)).render(format)
}

fn page(format: ReportFormat) -> String {
    ReportSource::Neighbors(Box::new(neighbors())).render(format)
}

const LIMITED_SECTION: &str = "\
- **Scanned:** 2026-01-15 10:00 UTC
- **Mode:** Fast
- **Ports:** List 1-720 · 720 ports
- **Open:** 3
- **Closed:** 400
- **Filtered:** 97
- **Subnet:** 192.0.2.0/24 · this machine 192.0.2.10
- **Interface:** en0
- **Gateway:** 192.0.2.1 · 00:27:22:4c:01:01

{ports}Open ports

| Port | Service |
| --- | --- |
| 22 | ssh |
| 80 | http |
| 445 | microsoft-ds |

Port 80 answered earlier and then went silent, so Borg stopped: the host or a firewall seems to limit the scan. Not scanned: 220 ports. Slow mode may get through.

- **Not scanned:** 501-720
";

fn limited_section(heading: &str, ports: &str) -> String {
    format!("{heading}Port scan of nas.home.arpa (192.0.2.60)\n\n{}", LIMITED_SECTION.replace("{ports}", ports))
}

#[test]
fn a_scan_in_markdown_has_its_facts_its_open_ports_and_why_it_stopped() {
    assert_eq!(scan(limited(), ReportFormat::Markdown), limited_section("# ", "## "));
}

#[test]
fn a_scan_that_found_nothing_says_so_and_leaves_out_what_it_did_not_gather() {
    let text = scan(quiet(), ReportFormat::Markdown);
    assert_eq!(
        text,
        "\
# Port scan of 192.0.2.44

- **Scanned:** 2026-01-15 10:00 UTC
- **Mode:** Slow
- **Ports:** Common · 30 ports
- **Open:** 0
- **Closed:** 0
- **Filtered:** 30

## Open ports

No open ports.
"
    );
    let document: Value = serde_json::from_str(&scan(quiet(), ReportFormat::Json)).unwrap();
    assert_eq!(document.get("not_scanned"), None);
    assert_eq!(document.get("network"), None);
    assert_eq!(scan(quiet(), ReportFormat::Csv), "address,target,scanned_at,mode,port,service\n");
}

#[test]
fn a_scan_in_json_holds_everything_it_found() {
    let text = scan(limited(), ReportFormat::Json);
    assert!(text.ends_with("}\n"));
    let document: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        document,
        json!({
            "target": "nas.home.arpa",
            "address": "192.0.2.60",
            "scanned_at": "2026-01-15T10:00:00Z",
            "mode": "fast",
            "ports": {"custom": "1-720"},
            "ports_asked": 720,
            "open": [
                {"port": 22, "service": "ssh"},
                {"port": 80, "service": "http"},
                {"port": 445, "service": "microsoft-ds"},
            ],
            "closed": 400,
            "filtered": 97,
            "not_scanned": [{"first": 501, "last": 720}],
            "ending": {"rate_limited": {"port": 80}},
            "network": {
                "interface": "en0",
                "subnet": "192.0.2.0/24",
                "this_machine": "192.0.2.10",
                "gateway": "192.0.2.1",
                "gateway_mac": ROUTER_MAC,
            },
        })
    );
}

#[test]
fn a_scan_in_csv_has_a_row_for_each_open_port() {
    assert_eq!(
        scan(limited(), ReportFormat::Csv),
        "\
address,target,scanned_at,mode,port,service
192.0.2.60,nas.home.arpa,2026-01-15T10:00:00Z,fast,22,ssh
192.0.2.60,nas.home.arpa,2026-01-15T10:00:00Z,fast,80,http
192.0.2.60,nas.home.arpa,2026-01-15T10:00:00Z,fast,445,microsoft-ds
"
    );
}

#[test]
fn the_neighbors_in_markdown_list_every_host_and_then_their_port_scans() {
    let expected = format!(
        "\
# Neighbors

- **Read at:** 2026-01-15 10:00 UTC
- **Subnet:** 192.0.2.0/24 · this machine 192.0.2.10
- **Interface:** en0
- **Gateway:** 192.0.2.1 · 00:27:22:4c:01:01
- **DNS:** 192.0.2.1

| Address | MAC | Vendor | Name | State | Ports | Ping | Note |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 192.0.2.1 | 00:27:22:4c:01:01 |  | router.home.arpa | REACHABLE | 53, 80 | 2 ms | Gateway |
| 192.0.2.60 | 00:11:32:aa:00:60 | Example Storage | nas.home.arpa | STALE | 22, 445 | 1 ms |  |
| 192.0.2.77 | da:a1:19:12:34:56 |  |  | Offline |  |  | Randomized, Seen 2026-01-14 10:00 UTC |

## Port scans

{}",
        limited_section("### ", "#### ")
    );
    assert_eq!(page(ReportFormat::Markdown), expected);
}

#[test]
fn the_neighbors_in_json_name_each_host_s_flags_and_keep_its_scan() {
    let document: Value = serde_json::from_str(&page(ReportFormat::Json)).unwrap();
    assert_eq!(document["read_at"], "2026-01-15T10:00:00Z");
    assert_eq!(document["networks"][0]["subnet"], "192.0.2.0/24");
    assert_eq!(document["dns"], json!(["192.0.2.1"]));
    let hosts = document["hosts"].as_array().unwrap();
    assert_eq!(hosts.len(), 3);
    assert_eq!(
        hosts[0],
        json!({
            "address": "192.0.2.1",
            "mac": ROUTER_MAC,
            "name": "router.home.arpa",
            "interface": "en0",
            "state": "REACHABLE",
            "presence": "listed",
            "flags": ["gateway"],
            "ports": [53, 80],
            "ping_ms": 2,
        })
    );
    assert_eq!(hosts[1]["vendor"], "Example Storage");
    assert_eq!(hosts[1]["port_scan"]["open"][2], json!({"port": 445, "service": "microsoft-ds"}));
    assert_eq!(hosts[2]["presence"], "offline");
    assert_eq!(hosts[2]["last_seen"], "2026-01-14T10:00:00Z");
    assert_eq!(hosts[2].get("ports"), None, "nothing is made up for what was not gathered");
    assert_eq!(hosts[2].get("state"), None);
}

#[test]
fn the_neighbors_in_csv_have_a_row_for_each_host() {
    assert_eq!(
        page(ReportFormat::Csv),
        "\
address,mac,vendor,name,interface,state,presence,last_seen,ports,ping_ms,flags,port_scan_at,port_scan_mode,port_scan_open
192.0.2.1,00:27:22:4c:01:01,,router.home.arpa,en0,REACHABLE,listed,,53 80,2,gateway,,,
192.0.2.60,00:11:32:aa:00:60,Example Storage,nas.home.arpa,en0,STALE,answered,,22 445,1,,2026-01-15T10:00:00Z,fast,22 80 445
192.0.2.77,da:a1:19:12:34:56,,,en0,,offline,2026-01-14T10:00:00Z,,,randomized,,,
"
    );
}

#[test]
fn csv_quotes_commas_quotes_and_line_breaks_and_markdown_escapes_pipes() {
    let rows = vec![vec!["a, \"b\"".to_owned(), "two\nlines".to_owned(), "plain".to_owned()]];
    assert_eq!(csv(&["x", "y", "z"], &rows), "x,y,z\n\"a, \"\"b\"\"\",\"two\nlines\",plain\n");
    let rows = vec![vec!["a|b".to_owned(), "two\nlines".to_owned()]];
    assert_eq!(table(&["x", "y"], &rows), "| x | y |\n| --- | --- |\n| a\\|b | two lines |");
}

#[test]
fn the_file_name_says_what_the_report_is_about_and_the_day() {
    assert_eq!(ReportSource::PortScan(Box::new(limited())).stem(), "port-scan-192.0.2.60-2026-01-15");
    let six = PortScanRecord { address: address("2001:db8::5"), ..limited() };
    assert_eq!(ReportSource::PortScan(Box::new(six)).stem(), "port-scan-2001-db8--5-2026-01-15");
    assert_eq!(ReportSource::Neighbors(Box::new(neighbors())).stem(), "neighbors-2026-01-15");
}
