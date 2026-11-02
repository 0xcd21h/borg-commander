use super::*;
use crate::{
    ACK, PSH, SYN,
    capture::tests::{
        CLIENT, CLIENT_PORT, RESOLVER, SERVER, client_hello, dns, over_tcp, over_udp, pcap, pktap_of, tcp, udp,
    },
};

const BROWSER: u32 = 8239;
const WEB: u32 = 4100;
const VISITOR: [u8; 4] = [198, 51, 100, 7];
const WEB_PORT: u16 = 8080;
const VISITOR_PORT: u16 = 50_000;
const RESOLVER_PORT: u16 = 53_001;

fn address(octets: [u8; 4], port: u16) -> SocketAddr {
    SocketAddr::from((octets, port))
}

fn socket(local: SocketAddr, remote: Option<SocketAddr>, pid: u32) -> Socket {
    Socket { transport: Transport::Tcp, local, remote, state: None, pid: Some(pid), received: None, sent: None }
}

/// The browser asks for a name and opens a TLS connection, and a visitor reaches the web app
/// listening on every address.
fn pcap_frames() -> Vec<Vec<u8>> {
    let (browser, web) = (CLIENT_PORT, WEB_PORT);
    vec![
        over_udp(CLIENT, RESOLVER, udp(RESOLVER_PORT, 53, &dns("example.com", None))),
        over_udp(RESOLVER, CLIENT, udp(53, RESOLVER_PORT, &dns("example.com", Some(&[SERVER])))),
        over_tcp(CLIENT, SERVER, tcp(browser, 443, 1000, SYN, &[])),
        over_tcp(SERVER, CLIENT, tcp(443, browser, 5000, SYN | ACK, &[])),
        over_tcp(CLIENT, SERVER, tcp(browser, 443, 1001, PSH | ACK, &client_hello("example.com", 0))),
        over_tcp(VISITOR, CLIENT, tcp(VISITOR_PORT, web, 9000, SYN, &[])),
        over_tcp(CLIENT, VISITOR, tcp(web, VISITOR_PORT, 7000, SYN | ACK, &[])),
    ]
}

fn capture() -> Capture {
    Capture::read(&pcap(&pcap_frames())).unwrap()
}

fn view() -> TrafficView {
    let sockets = [
        socket(address(CLIENT, CLIENT_PORT), Some(address(SERVER, 443)), BROWSER),
        socket(address([0; 4], WEB_PORT), None, WEB),
    ];
    let names = [(BROWSER, "browser".to_owned()), (WEB, "web".to_owned())].into();
    TrafficView::new(capture(), &sockets, names, &HashMap::new(), None)
}

#[test]
fn conversations_belong_to_the_process_whose_socket_holds_them() {
    let view = view();
    let pids: Vec<Option<u32>> = view.talkers.iter().map(|talker| talker.pid).collect();
    assert_eq!(pids.len(), 3);
    assert!(pids.contains(&Some(BROWSER)) && pids.contains(&Some(WEB)) && pids.contains(&None));
    let web = view.capture.flows.iter().position(|flow| flow.server.port() == WEB_PORT).unwrap();
    assert_eq!(
        view.ends[web],
        Ends { pid: Some(WEB), local: address(CLIENT, WEB_PORT), remote: address(VISITOR, VISITOR_PORT) }
    );
}

#[test]
fn hosts_are_named_by_dns_answers_and_the_name_tls_asks_for() {
    let view = view();
    assert_eq!(view.host(IpAddr::from(SERVER)), Some("example.com"));
    assert_eq!(view.endpoint(address(SERVER, 443)), "example.com:443");
    let protocols: Vec<&str> = view.host_rows.iter().flat_map(|row| row.protocols.clone()).collect();
    assert!(protocols.contains(&"TLS") && protocols.contains(&"DNS"));
}

#[test]
fn proto_keeps_what_borg_recognized() {
    let mut view = view();
    view.set_filter("proto:dns");
    assert_eq!(view.len(TrafficPage::Connections), 1);
    assert_eq!(view.len(TrafficPage::Packets), 2);
    assert_eq!(view.totals[TrafficPage::Packets.index()], 7, "the total counts every packet");
    view.set_filter("proto:tls");
    assert_eq!(view.flows.len(), 1);
    assert_eq!(view.capture.flows[view.flows[0]].server_name.as_deref(), Some("example.com"));
}

#[test]
fn enter_goes_from_a_process_to_its_connections_and_packets_and_back() {
    let mut view = view();
    let browser = view.talkers.iter().position(|talker| talker.pid == Some(BROWSER)).unwrap();
    view.select(browser);
    assert!(view.drill());
    assert_eq!((view.page, view.filter.as_str()), (TrafficPage::Connections, "pid:8239"));
    assert_eq!(view.len(TrafficPage::Connections), 1);
    assert!(view.drill());
    assert_eq!(view.page, TrafficPage::Packets);
    assert_eq!(view.filter, format!("host:{} port:{CLIENT_PORT}", IpAddr::from(SERVER)));
    assert_eq!(view.len(TrafficPage::Packets), 3);
    assert!(!view.drill(), "a packet has nothing under it");
    assert!(view.back());
    assert_eq!((view.page, view.filter.as_str()), (TrafficPage::Connections, "pid:8239"));
    assert!(view.back());
    assert_eq!((view.page, view.cursor()), (TrafficPage::Processes, browser));
    assert!(!view.back());
}

const SHORT_LIVED: u32 = 9001;

/// The same traffic caught on macOS's `pktap`: a process the screen never saw made the TLS
/// connection, and the name lookup's question and answer carry different processes.
fn tagged_view() -> TrafficView {
    let frames: Vec<(Vec<u8>, Option<u32>)> = pcap_frames()
        .into_iter()
        .enumerate()
        .map(|(index, frame)| {
            let pid = match index {
                0 => Some(WEB),
                1 => Some(BROWSER),
                2..=4 => Some(SHORT_LIVED),
                _ => None,
            };
            (frame, pid)
        })
        .collect();
    let processes = [(WEB, "web"), (BROWSER, "brows"), (SHORT_LIVED, "fetch")];
    let capture = Capture::read(&pktap_of(&frames, &processes)).unwrap();
    let sockets = [
        socket(address(CLIENT, CLIENT_PORT), Some(address(SERVER, 443)), BROWSER),
        socket(address([0; 4], WEB_PORT), None, WEB),
    ];
    let names = [(BROWSER, "browser".to_owned()), (WEB, "web".to_owned())].into();
    TrafficView::new(capture, &sockets, names, &HashMap::new(), None)
}

#[test]
fn a_packets_own_process_wins_over_the_socket_that_held_its_conversation() {
    let view = tagged_view();
    let tls = view.capture.flows.iter().position(|flow| flow.server.port() == 443).unwrap();
    assert_eq!(view.ends[tls].pid, Some(SHORT_LIVED), "the packets say who sent them");
    assert_eq!(view.ends[tls].local, address(CLIENT, CLIENT_PORT), "the socket still tells which end is here");
    assert_eq!(view.name(Some(SHORT_LIVED)), "fetch", "a process the screen never saw is named by the capture");
    assert_eq!(view.name(Some(BROWSER)), "browser", "the screen's name comes before the capture's shorter one");
    let web = view.capture.flows.iter().position(|flow| flow.server.port() == WEB_PORT).unwrap();
    assert_eq!(view.ends[web].pid, Some(WEB), "untagged packets keep the socket's process");
}

#[test]
fn a_process_filter_keeps_exactly_the_packets_that_process_sent_or_received() {
    let mut view = tagged_view();
    view.set_filter(&format!("pid:{BROWSER}"));
    assert_eq!(view.packets, [1], "only the answer the browser received");
    view.set_filter(&format!("pid:{WEB}"));
    assert_eq!(view.packets, [0, 5, 6], "the question, and the visitor's packets through the web socket");
    view.set_filter("fetch");
    assert_eq!(view.packets, [2, 3, 4]);
    let packet = view.capture.packets[1].clone();
    assert_eq!(view.packet_pid(&packet), Some(BROWSER));
}

#[test]
fn the_report_sums_up_who_talked_to_whom() {
    let mut view = view();
    view.interface = Some("en0".into());
    let expected = "\
Capture on en0 · packets: 7 · bytes: 504 · seconds: 0.0
Protocols by packet: TCP 4, DNS 2, TLS 1

Processes: 3
PID   PROCESS          PACKETS  BYTES  HOSTS  CONNECTIONS
8239  browser          3        238    1      1
-     Unknown process  2        158    1      1
4100  web              2        108    1      1

Hosts: 3
ADDRESS        NAME         PACKETS  BYTES  PROTOCOLS  PROCESSES
93.184.216.34  example.com  3        238    TLS        browser (PID 8239)
192.168.1.1    -            2        158    DNS        Unknown process
198.51.100.7   -            2        108    TCP        web (PID 4100)

Connections: 3, busiest first
PROTOCOL  PID   PROCESS          LOCAL               REMOTE              HOST         ASKED        PACKETS  SENT  RECEIVED  FIRST
TLS       8239  browser          192.168.1.20:51000  93.184.216.34:443   example.com  example.com  3        184   54        3
DNS       -     Unknown process  192.168.1.20:53001  192.168.1.1:53      -            example.com  2        71    87        1
TCP       4100  web              192.168.1.20:8080   198.51.100.7:50000  -            -            2        54    54        6
";
    assert_eq!(view.report(10), expected);
}

#[test]
fn the_report_follows_the_filter_and_the_limit() {
    let mut view = view();
    view.set_filter("proto:dns");
    let report = view.report(10);
    assert!(report.contains("Filter proto:dns keeps 2 of the packets\nProtocols by packet: DNS 2\n"), "{report}");
    assert!(report.contains("\nConnections: 1, busiest first\n"), "{report}");
    view.set_filter("");
    let report = view.report(1);
    assert_eq!(report.matches("… 2 more").count(), 3, "{report}");
}

#[test]
fn a_packet_report_gives_its_connection_layers_and_bytes() {
    let view = view();
    let report = view.packet_report(5).unwrap();
    assert!(report.starts_with("Packet 5 of 7: TLS ClientHello example.com\n"), "{report}");
    assert!(
        report.contains(&format!(
            "Connection: TLS {} → {} · browser (PID {BROWSER}) · packet 3 of 3 in it · previous 4 · next -\n",
            address(CLIENT, CLIENT_PORT),
            address(SERVER, 443)
        )),
        "{report}"
    );
    assert!(report.contains("\nTLS       ClientHello · example.com\n"), "{report}");
    assert!(report.contains("\nBytes:\n0000  "), "{report}");
    assert_eq!(view.packet_report(0), None);
    assert_eq!(view.packet_report(8), None);
}
