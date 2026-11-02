use super::*;
use crate::{
    ActivityData, Appearance, Msg, ProcessInfo, ProcessKey, Settings, Startup, SystemSample, ThemeChoice, update,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::net::SocketAddr;

const SSHD: u32 = 301;
const POSTGRES: u32 = 812;
const DEVSERVER: u32 = 900;
const MDNS: u32 = 44;
const BROWSER: u32 = 8239;
const SECONDS: f64 = 2.0;

fn address(text: &str) -> SocketAddr {
    text.parse().unwrap()
}

fn tcp(local: &str, remote: Option<&str>, state: TcpState, pid: Option<u32>, counts: (u64, u64)) -> Socket {
    Socket {
        transport: Transport::Tcp,
        local: address(local),
        remote: remote.map(address),
        state: Some(state),
        pid,
        received: Some(counts.0),
        sent: Some(counts.1),
    }
}

fn udp(local: &str, remote: Option<&str>, pid: u32, counts: Option<(u64, u64)>) -> Socket {
    Socket {
        transport: Transport::Udp,
        local: address(local),
        remote: remote.map(address),
        state: None,
        pid: Some(pid),
        received: counts.map(|(received, _)| received),
        sent: counts.map(|(_, sent)| sent),
    }
}

fn interface(name: &str, kind: InterfaceKind, received: u64, sent: u64) -> Interface {
    Interface {
        name: name.into(),
        kind,
        up: true,
        addresses: Vec::new(),
        mac: None,
        mtu: None,
        received,
        sent,
        packets_received: 0,
        packets_sent: 0,
        errors: 0,
    }
}

/// A machine with a public SSH server, a database for itself, a server on one address, mDNS,
/// and a browser downloading over TCP and UDP; `step` scales the bytes moved so far.
fn sample(step: u64) -> NetworkSample {
    let sockets = vec![
        tcp("0.0.0.0:22", None, TcpState::Listen, Some(SSHD), (0, 0)),
        tcp("192.0.2.10:22", Some("198.51.100.7:50000"), TcpState::Established, Some(SSHD), (1000 * step, 500 * step)),
        tcp("127.0.0.1:5432", None, TcpState::Listen, Some(POSTGRES), (0, 0)),
        tcp("[::1]:5432", None, TcpState::Listen, Some(POSTGRES), (0, 0)),
        tcp("192.0.2.10:8080", None, TcpState::Listen, Some(DEVSERVER), (0, 0)),
        udp("0.0.0.0:5353", None, MDNS, Some((100 * step, 100 * step))),
        tcp(
            "192.0.2.10:50123",
            Some("203.0.113.9:443"),
            TcpState::Established,
            Some(BROWSER),
            (10_000 * step, 1000 * step),
        ),
        udp("192.0.2.10:62556", Some("203.0.113.9:443"), BROWSER, None),
        tcp("192.0.2.10:50100", Some("203.0.113.5:443"), TcpState::TimeWait, None, (0, 0)),
    ];
    let names =
        [(SSHD, "sshd"), (POSTGRES, "postgres"), (DEVSERVER, "devserver"), (MDNS, "mdns"), (BROWSER, "browser")]
            .into_iter()
            .map(|(pid, name)| (pid, name.to_owned()))
            .collect();
    let interfaces = vec![
        interface("lo0", InterfaceKind::Loopback, 50_000 * step, 50_000 * step),
        interface("en0", InterfaceKind::WiFi, 100_000 * step, 20_000 * step),
    ];
    NetworkSample { sockets, interfaces, names }
}

fn measured(filter: &str) -> Network {
    measured_with(filter, HashMap::new())
}

fn measured_with(filter: &str, hosts: HashMap<IpAddr, Option<String>>) -> Network {
    Network::measured(&sample(1), sample(3), SECONDS, hosts, filter)
}

fn remotes(network: &Network) -> Vec<(Option<u32>, Option<SocketAddr>)> {
    network
        .connections
        .iter()
        .map(|row| {
            let socket = &network.sample.sockets[row.socket];
            (socket.pid, socket.remote)
        })
        .collect()
}

#[test]
fn ports_say_who_can_reach_them() {
    let network = measured("");
    let ports: Vec<(Transport, u16, Exposure)> =
        network.ports.iter().map(|row| (row.transport, row.port, row.exposure)).collect();
    assert_eq!(
        ports,
        [
            (Transport::Tcp, 22, Exposure::Everywhere),
            (Transport::Udp, 5353, Exposure::Everywhere),
            (Transport::Tcp, 5432, Exposure::ThisMachine),
            (Transport::Tcp, 8080, Exposure::Addresses),
        ]
    );
    let postgres = &network.ports[2];
    assert_eq!(postgres.addresses, [IpAddr::from([127, 0, 0, 1]), "::1".parse::<IpAddr>().unwrap()]);
}

#[test]
fn a_port_counts_the_connections_that_came_in_through_it() {
    let network = measured("");
    assert_eq!(network.ports[0].connections, 1);
    assert_eq!(network.ports[0].rate, Rate { down: 1000, up: 500 });
    assert_eq!(network.ports[3].connections, 0);
}

#[test]
fn rates_are_bytes_moved_per_second_between_samples() {
    let network = measured("");
    let browser = network.processes.iter().find(|row| row.pid == Some(BROWSER)).unwrap();
    assert_eq!(browser.rate, Rate { down: 10_000, up: 1000 });
    assert_eq!(browser.connections, 2);
    assert!(browser.counted);
    let en0 = network.traffic.interface("en0");
    assert_eq!(en0, Rate { down: 100_000, up: 20_000 });
    assert_eq!(network.total(), en0);
}

#[test]
fn a_count_that_went_down_is_all_new() {
    let mut traffic = Traffic::default();
    traffic.take(&sample(3), 0.0);
    traffic.take(&sample(1), SECONDS);
    let socket = sample(1).sockets[6].key();
    assert_eq!(traffic.socket(&socket), Rate { down: 5000, up: 500 });
}

#[test]
fn the_first_sample_only_records_counts() {
    let mut traffic = Traffic::default();
    traffic.take(&sample(5), 0.0);
    assert_eq!(traffic.process(Some(BROWSER)), Rate::default());
    assert_eq!(traffic.interface("en0"), Rate::default());
}

#[test]
fn a_process_without_counted_bytes_says_so() {
    let mut uncounted = sample(1);
    for socket in &mut uncounted.sockets {
        (socket.received, socket.sent) = (None, None);
    }
    let network = Network::measured(&uncounted, uncounted.clone(), SECONDS, HashMap::new(), "");
    assert!(network.processes.iter().all(|row| !row.counted));
}

#[test]
fn connections_list_the_busiest_first() {
    let network = measured("");
    assert_eq!(
        remotes(&network)[..2],
        [(Some(BROWSER), Some(address("203.0.113.9:443"))), (Some(SSHD), Some(address("198.51.100.7:50000")))]
    );
    assert_eq!(network.connections.len(), 4);
}

#[test]
fn the_filter_matches_ports_and_services() {
    assert_eq!(measured("port:443").connections.len(), 3);
    assert_eq!(measured("port:https").connections.len(), 3);
    assert_eq!(measured("port:ssh").ports.len(), 1);
}

#[test]
fn the_filter_combines_negated_fields() {
    let network = measured("proc:BROW !proto:udp");
    assert_eq!(remotes(&network), [(Some(BROWSER), Some(address("203.0.113.9:443")))]);
}

#[test]
fn the_filter_finds_listening_tcp_and_counts_what_it_hides() {
    let network = measured("state:listen");
    assert_eq!(network.ports.len(), 3);
    assert_eq!(network.totals[NetworkPage::Ports.index()], 4);
}

#[test]
fn the_filter_finds_hosts_by_name() {
    let hosts = HashMap::from([(IpAddr::from([203, 0, 113, 9]), Some("cdn.example.net".to_owned()))]);
    let network = measured_with("host:example", hosts);
    assert_eq!(network.connections.len(), 2);
    assert_eq!(network.host(IpAddr::from([203, 0, 113, 9])), Some("cdn.example.net"));
}

#[test]
fn a_plain_word_searches_every_field() {
    assert_eq!(measured("8239").processes.len(), 1);
    assert_eq!(measured("wi-fi").interfaces.len(), 1);
}

fn state() -> (AppState, Instant) {
    let now = Instant::now();
    let (mut state, _) = AppState::new(
        Startup::at("/work".into()),
        120,
        32,
        Settings::default(),
        Appearance::new(vec![ThemeChoice { id: "midnight".into(), name: "Midnight".into() }], 0),
        now,
    );
    state.settings.preferences.git_plugin = false;
    (state, now)
}

fn press(state: &mut AppState, code: KeyCode, modifiers: KeyModifiers) -> Vec<Effect> {
    update(state, Msg::Key(KeyEvent::new(code, modifiers)), state.epoch)
}

fn requested(effects: &[Effect]) -> Option<u64> {
    effects.iter().find_map(|effect| match effect {
        Effect::Network { id } => Some(*id),
        _ => None,
    })
}

/// Opens the screen with Alt-N and gives it two samples, the second `SECONDS` later.
fn opened() -> (AppState, Instant) {
    let (mut state, now) = state();
    let id = requested(&press(&mut state, KeyCode::Char('n'), KeyModifiers::ALT)).unwrap();
    assert!(state.network.is_open());
    update(&mut state, Msg::Network { id, result: Ok(sample(1)) }, now);
    let later = now + Duration::from_secs_f64(SECONDS);
    let id = requested(&state.network.request()).unwrap();
    update(&mut state, Msg::Network { id, result: Ok(sample(3)) }, later);
    (state, later)
}

#[test]
fn alt_n_opens_the_screen_and_shows_the_samples() {
    let (state, _) = opened();
    assert!(state.network.loaded);
    assert_eq!(state.network.page, NetworkPage::Ports);
    assert_eq!(state.network.ports.len(), 4);
    assert!(state.monitor_open());
}

#[test]
fn an_answer_to_an_older_request_is_ignored() {
    let (mut state, now) = state();
    let id = requested(&press(&mut state, KeyCode::Char('n'), KeyModifiers::ALT)).unwrap();
    update(&mut state, Msg::Network { id: id + 1, result: Ok(sample(1)) }, now);
    assert!(!state.network.loaded);
    update(&mut state, Msg::Network { id, result: Err("netstat failed".into()) }, now);
    assert_eq!(state.network.error.as_deref(), Some("netstat failed"));
}

#[test]
fn the_screen_needs_the_system_plugin() {
    let (mut state, _) = state();
    state.settings.preferences.system_plugin = false;
    assert_eq!(requested(&press(&mut state, KeyCode::Char('n'), KeyModifiers::ALT)), None);
    assert!(!state.network.is_open());
    assert!(state.notice.is_some());
}

#[test]
fn remote_addresses_are_resolved_once() {
    let (mut state, now) = state();
    let id = requested(&press(&mut state, KeyCode::Char('n'), KeyModifiers::ALT)).unwrap();
    let effects = update(&mut state, Msg::Network { id, result: Ok(sample(1)) }, now);
    let mut asked: Vec<IpAddr> = effects
        .iter()
        .find_map(|effect| match effect {
            Effect::ResolveHosts(addresses) => Some(addresses.clone()),
            _ => None,
        })
        .unwrap();
    asked.sort();
    let expected: Vec<IpAddr> =
        ["198.51.100.7", "203.0.113.5", "203.0.113.9"].iter().map(|address| address.parse().unwrap()).collect();
    assert_eq!(asked, expected);
    update(&mut state, Msg::HostNames(asked.iter().map(|address| (*address, None)).collect()), now);
    let id = requested(&state.network.request()).unwrap();
    let effects = update(&mut state, Msg::Network { id, result: Ok(sample(2)) }, now);
    assert!(!effects.iter().any(|effect| matches!(effect, Effect::ResolveHosts(_))));
}

#[test]
fn digits_switch_pages_and_letters_sort() {
    let (mut state, _) = opened();
    press(&mut state, KeyCode::Char('3'), KeyModifiers::NONE);
    assert_eq!(state.network.page, NetworkPage::Connections);
    press(&mut state, KeyCode::Char('1'), KeyModifiers::NONE);
    press(&mut state, KeyCode::Char('n'), KeyModifiers::NONE);
    let names: Vec<String> = state.network.ports.iter().map(|row| state.network.name(row.pid)).collect();
    assert_eq!(names, ["devserver", "mdns", "postgres", "sshd"]);
    press(&mut state, KeyCode::Char('n'), KeyModifiers::NONE);
    assert_eq!(state.network.name(state.network.ports[0].pid), "sshd");
    press(&mut state, KeyCode::Char('4'), KeyModifiers::NONE);
    let before = state.network.order();
    press(&mut state, KeyCode::Char('p'), KeyModifiers::NONE);
    assert_eq!(state.network.order(), before);
}

#[test]
fn the_cursor_stays_on_its_row_when_the_order_changes() {
    let (mut state, _) = opened();
    press(&mut state, KeyCode::Down, KeyModifiers::NONE);
    press(&mut state, KeyCode::Down, KeyModifiers::NONE);
    assert_eq!(state.network.port().map(|row| row.port), Some(5432));
    press(&mut state, KeyCode::Char('n'), KeyModifiers::NONE);
    assert_eq!(state.network.port().map(|row| row.port), Some(5432));
}

#[test]
fn the_cursor_follows_the_busiest_row_until_it_is_moved() {
    let (mut state, now) = opened();
    press(&mut state, KeyCode::Char('3'), KeyModifiers::NONE);
    assert_eq!(state.network.connection().and_then(|(socket, _)| socket.pid), Some(BROWSER));
    press(&mut state, KeyCode::Down, KeyModifiers::NONE);
    let id = requested(&state.network.request()).unwrap();
    update(&mut state, Msg::Network { id, result: Ok(sample(9)) }, now + Duration::from_secs(1));
    assert_eq!(state.network.connection().and_then(|(socket, _)| socket.pid), Some(SSHD));
}

#[test]
fn typing_filters_and_esc_steps_back() {
    let (mut state, _) = opened();
    press(&mut state, KeyCode::Char('3'), KeyModifiers::NONE);
    press(&mut state, KeyCode::Char('/'), KeyModifiers::NONE);
    for c in "sshd".chars() {
        press(&mut state, KeyCode::Char(c), KeyModifiers::NONE);
    }
    assert_eq!(state.network.connections.len(), 1);
    press(&mut state, KeyCode::Enter, KeyModifiers::NONE);
    assert!(!state.network.filtering);
    assert_eq!(state.network.filter, "sshd");
    press(&mut state, KeyCode::Esc, KeyModifiers::NONE);
    assert!(state.network.filter.is_empty());
    assert_eq!(state.network.connections.len(), 4);
    press(&mut state, KeyCode::Esc, KeyModifiers::NONE);
    assert!(!state.network.is_open());
}

#[test]
fn ctrl_c_copies_what_reaches_the_row() {
    let (mut state, _) = opened();
    let effects = press(&mut state, KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert!(effects.contains(&Effect::Copy("localhost:22".into())));
    press(&mut state, KeyCode::End, KeyModifiers::NONE);
    let effects = press(&mut state, KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert!(effects.contains(&Effect::Copy("192.0.2.10:8080".into())));
    press(&mut state, KeyCode::Char('3'), KeyModifiers::NONE);
    let effects = press(&mut state, KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert!(effects.contains(&Effect::Copy("203.0.113.9:443".into())));
}

#[test]
fn enter_shows_the_rows_process_in_the_activity_monitor() {
    let (mut state, now) = opened();
    let effects = press(&mut state, KeyCode::Enter, KeyModifiers::NONE);
    assert!(!state.network.is_open());
    assert!(state.activity.side.is_some());
    let id = effects
        .iter()
        .find_map(|effect| match effect {
            Effect::Activity { id, .. } => Some(*id),
            _ => None,
        })
        .unwrap();
    let process = |pid: u32| ProcessInfo {
        key: ProcessKey { pid, start: 1 },
        parent: None,
        name: format!("process-{pid}"),
        executable: None,
        cpu: Some(0),
        resident: Some(1),
        virtual_bytes: Some(1),
        read: Some(0),
        written: Some(0),
        protection: None,
        nice: Some(0),
        stopped: false,
    };
    let processes = vec![process(1), process(MDNS), process(SSHD), process(BROWSER)];
    let sample = ActivityData::Sample(SystemSample { processes, ..SystemSample::default() });
    update(&mut state, Msg::Activity { id, result: Ok(sample) }, now);
    assert_eq!(state.activity.process().map(|process| process.key.pid), Some(SSHD));
}

#[test]
fn a_row_without_a_process_has_nothing_to_show() {
    let (mut state, _) = opened();
    press(&mut state, KeyCode::Char('3'), KeyModifiers::NONE);
    press(&mut state, KeyCode::Char('/'), KeyModifiers::NONE);
    for c in "state:time".chars() {
        press(&mut state, KeyCode::Char(c), KeyModifiers::NONE);
    }
    press(&mut state, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(state.network.selected_pid(), None);
    press(&mut state, KeyCode::Char('k'), KeyModifiers::NONE);
    assert!(state.network.is_open());
    assert_eq!(state.notice.as_deref(), Some(tr!(network_no_process)));
}
