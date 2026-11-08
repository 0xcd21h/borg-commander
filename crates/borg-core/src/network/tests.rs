use super::*;
use crate::{
    ActivityData, Appearance, AskKind, Elevation, InstallOffer, Installer, Msg, Overlay, PackageManager, ProcessInfo,
    ProcessKey, Settings, Startup, SystemSample, ThemeChoice, Tool, update,
};
use borg_term::ShellSignal;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
};

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
fn a_cyrillic_layout_sorts_and_closes_the_screen_and_types_into_the_filter() {
    let (mut state, _) = opened();
    press(&mut state, KeyCode::Char('т'), KeyModifiers::NONE);
    let names: Vec<String> = state.network.ports.iter().map(|row| state.network.name(row.pid)).collect();
    assert_eq!(names, ["devserver", "mdns", "postgres", "sshd"]);
    press(&mut state, KeyCode::Char('/'), KeyModifiers::NONE);
    press(&mut state, KeyCode::Char('т'), KeyModifiers::NONE);
    assert_eq!(state.network.filter, "т");
    press(&mut state, KeyCode::Esc, KeyModifiers::NONE);
    press(&mut state, KeyCode::Char('т'), KeyModifiers::ALT);
    assert!(!state.network.is_open());
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

fn activity_request(effects: &[Effect]) -> u64 {
    effects
        .iter()
        .find_map(|effect| match effect {
            Effect::Activity { id, .. } => Some(*id),
            _ => None,
        })
        .unwrap()
}

/// Answers the activity monitor's request with a few processes, one of them without sockets.
fn answer_activity(state: &mut AppState, effects: &[Effect], now: Instant) {
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
    update(state, Msg::Activity { id: activity_request(effects), result: Ok(sample) }, now);
}

#[test]
fn enter_shows_the_rows_process_in_the_activity_monitor() {
    let (mut state, now) = opened();
    let effects = press(&mut state, KeyCode::Enter, KeyModifiers::NONE);
    assert!(!state.network.is_open());
    assert!(state.activity.side.is_some());
    answer_activity(&mut state, &effects, now);
    assert_eq!(state.activity.process().map(|process| process.key.pid), Some(SSHD));
}

#[test]
fn esc_in_the_activity_monitor_goes_back_to_the_row_it_came_from() {
    let (mut state, now) = opened();
    press(&mut state, KeyCode::Char('3'), KeyModifiers::NONE);
    press(&mut state, KeyCode::Down, KeyModifiers::NONE);
    let chosen = state.network.connection().map(|(socket, _)| socket.key());
    let effects = press(&mut state, KeyCode::Enter, KeyModifiers::NONE);
    answer_activity(&mut state, &effects, now);
    let effects = press(&mut state, KeyCode::Esc, KeyModifiers::NONE);
    assert!(state.activity.side.is_none());
    assert!(state.network.is_open());
    assert!(requested(&effects).is_some());
    assert_eq!(state.network.page, NetworkPage::Connections);
    assert_eq!(state.network.connection().map(|(socket, _)| socket.key()), chosen);
    assert!(state.network.traffic.processes.values().any(|meter| !meter.history.is_empty()));
    press(&mut state, KeyCode::Esc, KeyModifiers::NONE);
    assert!(!state.network.is_open());
    assert!(state.activity.side.is_none());
}

#[test]
fn alt_m_opens_a_monitor_that_esc_closes() {
    let (mut state, now) = opened();
    let effects = press(&mut state, KeyCode::Char('m'), KeyModifiers::ALT);
    answer_activity(&mut state, &effects, now);
    press(&mut state, KeyCode::Esc, KeyModifiers::NONE);
    assert!(state.activity.side.is_none());
    assert!(!state.network.is_open());
}

#[test]
fn alt_n_in_the_activity_monitor_shows_the_process_traffic_and_esc_goes_back() {
    let (mut state, now) = state();
    let effects = press(&mut state, KeyCode::Char('m'), KeyModifiers::ALT);
    state.activity.reveal(BROWSER, false);
    answer_activity(&mut state, &effects, now);
    let id = requested(&press(&mut state, KeyCode::Char('n'), KeyModifiers::ALT)).unwrap();
    assert!(state.activity.side.is_none());
    assert_eq!(state.network.page, NetworkPage::Processes);
    update(&mut state, Msg::Network { id, result: Ok(sample(1)) }, now);
    assert_eq!(state.network.process().map(|row| row.pid), Some(Some(BROWSER)));
    let effects = press(&mut state, KeyCode::Esc, KeyModifiers::NONE);
    assert!(!state.network.is_open());
    answer_activity(&mut state, &effects, now);
    assert_eq!(state.activity.process().map(|process| process.key.pid), Some(BROWSER));
}

#[test]
fn a_process_off_the_network_says_so() {
    let (mut state, now) = state();
    let effects = press(&mut state, KeyCode::Char('m'), KeyModifiers::ALT);
    state.activity.reveal(1, false);
    answer_activity(&mut state, &effects, now);
    let id = requested(&press(&mut state, KeyCode::Char('n'), KeyModifiers::ALT)).unwrap();
    update(&mut state, Msg::Network { id, result: Ok(sample(1)) }, now);
    let process = tr!(process_control_subject, name = "process-1", pid = 1);
    assert_eq!(state.notice, Some(tr!(network_process_idle, process = process)));
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

fn capture_dialog(state: &AppState) -> &CaptureDialog {
    match &state.overlay {
        Some(Overlay::Capture(dialog)) => dialog,
        _ => panic!("the capture dialog is open"),
    }
}

fn progress(bytes: u64, packets: usize) -> CaptureProgress {
    CaptureProgress { bytes, packets, room: None }
}

/// Starts a capture that runs until it is stopped.
fn until_stopped(state: &mut AppState, now: Instant) -> u64 {
    press(state, KeyCode::Char('r'), KeyModifiers::NONE);
    press(state, KeyCode::Down, KeyModifiers::NONE);
    press(state, KeyCode::Down, KeyModifiers::NONE);
    press(state, KeyCode::Left, KeyModifiers::NONE);
    let (id, request) = started(update(state, Msg::Key(KeyCode::Enter.into()), now));
    assert_eq!(request.seconds, None);
    id
}

fn started(effects: Vec<Effect>) -> (u64, CaptureRequest) {
    match effects.into_iter().next() {
        Some(Effect::Capture { id, request }) => (id, request),
        _ => panic!("a capture starts"),
    }
}

#[test]
fn r_asks_what_to_capture_with_the_selected_row_first() {
    let (mut state, _) = opened();
    press(&mut state, KeyCode::Char('r'), KeyModifiers::NONE);
    let dialog = capture_dialog(&state);
    assert_eq!(dialog.scopes, [CaptureScope::Port { transport: Transport::Tcp, port: 22 }, CaptureScope::Everything]);
    assert_eq!(dialog.interfaces.first().map(String::as_str), Some("en0"), "the busiest interface first");
    assert_eq!(dialog.request().unwrap().filter, "tcp port 22");
    press(&mut state, KeyCode::Esc, KeyModifiers::NONE);
    assert!(state.overlay.is_none() && state.network.recording.is_none());

    press(&mut state, KeyCode::Char('3'), KeyModifiers::NONE);
    press(&mut state, KeyCode::Char('r'), KeyModifiers::NONE);
    let filter = capture_dialog(&state).request().unwrap().filter;
    assert_eq!(
        filter,
        "tcp and ((src host 192.0.2.10 and src port 50123 and dst host 203.0.113.9 and dst port 443) or \
         (src host 203.0.113.9 and src port 443 and dst host 192.0.2.10 and dst port 50123))",
        "the busiest connection, both ways"
    );
}

#[test]
fn the_dialog_changes_its_fields_and_starts_the_capture_in_the_panel_folder() {
    let (mut state, now) = opened();
    press(&mut state, KeyCode::Char('r'), KeyModifiers::NONE);
    press(&mut state, KeyCode::Down, KeyModifiers::NONE);
    press(&mut state, KeyCode::Down, KeyModifiers::NONE);
    press(&mut state, KeyCode::Left, KeyModifiers::NONE);
    press(&mut state, KeyCode::Down, KeyModifiers::NONE);
    press(&mut state, KeyCode::Char(' '), KeyModifiers::NONE);
    let (_, request) = started(update(&mut state, Msg::Key(KeyCode::Enter.into()), now));
    assert_eq!(request.seconds, None, "Left from the first length goes round to until stopped");
    assert_eq!(request.keep, Keep::Headers, "Space moves on from the start of each connection");
    assert_eq!(request.folder, state.panel(state.network.side.unwrap()).dir());
    assert!(state.network.recording.is_some());
}

#[test]
fn the_dialog_keeps_the_start_of_each_connection_until_keep_is_changed() {
    let (mut state, _) = opened();
    press(&mut state, KeyCode::Char('r'), KeyModifiers::NONE);
    assert_eq!(capture_dialog(&state).keep, Keep::ConnectionStart);
    for _ in 0..3 {
        press(&mut state, KeyCode::Down, KeyModifiers::NONE);
    }
    assert_eq!(capture_dialog(&state).selected(), CaptureField::Content);
    let mut kept = Vec::new();
    for code in [KeyCode::Left, KeyCode::Left, KeyCode::Right, KeyCode::Right] {
        press(&mut state, code, KeyModifiers::NONE);
        kept.push(capture_dialog(&state).keep);
    }
    assert_eq!(kept, [Keep::Whole, Keep::Headers, Keep::Whole, Keep::ConnectionStart], "Left from whole goes round");
}

#[test]
fn a_running_capture_tells_which_limit_stops_it_first_and_when() {
    const MIB: u64 = 1024 * 1024;
    let (mut state, now) = opened();
    let id = until_stopped(&mut state, now);
    let at = |seconds| now + Duration::from_secs(seconds);
    let ahead = |state: &AppState, seconds| state.network.recording.as_ref().unwrap().limit_ahead(at(seconds));
    update(&mut state, Msg::CaptureProgress { id, progress: progress(16 * MIB, 10) }, at(2));
    assert_eq!(ahead(&state, 2), None, "the rate settles first");
    update(&mut state, Msg::CaptureProgress { id, progress: progress(64 * MIB, 1_000) }, at(10));
    assert_eq!(ahead(&state, 10), Some((CaptureLimit::Size, 30)), "192 MiB to go at 6.4 MiB a second");
    let room = CaptureProgress { room: Some(32 * MIB), ..progress(64 * MIB, 1_000) };
    update(&mut state, Msg::CaptureProgress { id, progress: room }, at(10));
    assert_eq!(ahead(&state, 10), Some((CaptureLimit::Disk, 5)), "the disk fills before the size limit");
    update(&mut state, Msg::CaptureProgress { id, progress: progress(MIB, 100_000) }, at(10));
    assert_eq!(ahead(&state, 10), Some((CaptureLimit::Packets, 10)));
    update(&mut state, Msg::CaptureProgress { id, progress: progress(MIB / 16, 10) }, at(10));
    assert_eq!(ahead(&state, 10), None, "more than an hour ahead is too far to tell");

    let (mut state, now) = opened();
    press(&mut state, KeyCode::Char('r'), KeyModifiers::NONE);
    let (id, request) = started(update(&mut state, Msg::Key(KeyCode::Enter.into()), now));
    assert_eq!(request.seconds, Some(SHORTEST_CAPTURE));
    update(&mut state, Msg::CaptureProgress { id, progress: progress(64 * MIB, 1_000) }, at(4));
    let recording = state.network.recording.as_ref().unwrap();
    assert_eq!(recording.limit_ahead(at(4)), None, "the capture ends before a limit stops it");
}

#[test]
fn a_capture_a_limit_stopped_says_so_once_it_is_saved() {
    let (mut state, now) = opened();
    let id = until_stopped(&mut state, now);
    update(&mut state, Msg::CaptureLimited { id, limit: CaptureLimit::Size }, now);
    let recording = state.network.recording.as_ref().unwrap();
    assert!(recording.stopping && recording.limit_ahead(now).is_none(), "it is being saved");
    assert!(press(&mut state, KeyCode::Char('r'), KeyModifiers::NONE).is_empty(), "R has nothing left to stop");
    update(&mut state, Msg::CaptureDone { id, result: Ok(("/work/capture.pcap".into(), Capture::default())) }, now);
    assert_eq!(state.notice.as_deref(), Some("The capture stopped at 256 MiB, the most Borg saves."));

    for (keep_free_mib, stopped, refused) in [
        (None, "is full.", "is full, so the capture did not start."),
        (Some(512), "has less than 512 MiB free.", "has less than 512 MiB free, so the capture did not start."),
    ] {
        let (mut state, now) = opened();
        state.network.keep_free_mib = keep_free_mib;
        let folder = state.panel(state.network.side.unwrap()).dir().display().to_string();
        let id = until_stopped(&mut state, now);
        update(&mut state, Msg::CaptureLimited { id, limit: CaptureLimit::Disk }, now);
        let saved = Ok(("/work/capture.pcap".into(), Capture::default()));
        update(&mut state, Msg::CaptureDone { id, result: saved }, now);
        let notice = format!("The capture stopped because the disk holding {folder} {stopped}");
        assert_eq!(state.notice.as_deref(), Some(notice.as_str()));

        let (mut state, now) = opened();
        state.network.keep_free_mib = keep_free_mib;
        let id = until_stopped(&mut state, now);
        update(&mut state, Msg::CaptureDone { id, result: Err(CaptureFailure::NoRoom) }, now);
        let notice = format!("The disk holding {folder} {refused}");
        assert_eq!(state.notice.as_deref(), Some(notice.as_str()));
    }
}

#[test]
fn a_capture_keeps_free_only_the_space_the_settings_ask_for() {
    let (mut state, now) = opened();
    let id = until_stopped(&mut state, now);
    assert_eq!(state.network.recording.as_ref().unwrap().request.keep_free(), 0, "until the disk is full");
    assert_eq!(CaptureLimit::Disk.label(None), "a full disk");
    update(&mut state, Msg::CaptureDone { id, result: Err(CaptureFailure::Cancelled) }, now);

    state.network.keep_free_mib = Some(512);
    let id = until_stopped(&mut state, now);
    let request = &state.network.recording.as_ref().unwrap().request;
    assert_eq!((request.keep_free_mib, request.keep_free()), (Some(512), 512 * 1024 * 1024));
    assert_eq!(CaptureLimit::Disk.label(request.keep_free_mib), "512 MiB free");
    assert_eq!(CaptureLimit::Size.label(request.keep_free_mib), "256 MiB");
    update(&mut state, Msg::CaptureDone { id, result: Err(CaptureFailure::Cancelled) }, now);
}

#[test]
fn r_stops_a_capture_and_closing_the_screen_stops_it_too() {
    let (mut state, now) = opened();
    press(&mut state, KeyCode::Char('r'), KeyModifiers::NONE);
    let (id, _) = started(update(&mut state, Msg::Key(KeyCode::Enter.into()), now));
    update(&mut state, Msg::CaptureProgress { id, progress: progress(4_096, 12) }, now);
    assert_eq!(state.network.recording.as_ref().map(|recording| recording.packets), Some(12));
    assert_eq!(press(&mut state, KeyCode::Char('r'), KeyModifiers::NONE), [Effect::StopCapture { id }]);
    assert!(press(&mut state, KeyCode::Char('r'), KeyModifiers::NONE).is_empty(), "it is already stopping");

    let (mut state, now) = opened();
    press(&mut state, KeyCode::Char('r'), KeyModifiers::NONE);
    let (id, _) = started(update(&mut state, Msg::Key(KeyCode::Enter.into()), now));
    let effects = press(&mut state, KeyCode::Esc, KeyModifiers::NONE);
    assert!(!state.network.is_open());
    assert!(effects.contains(&Effect::StopCapture { id }));
    let path = PathBuf::from("/work/capture.pcap");
    update(&mut state, Msg::CaptureDone { id, result: Ok((path, Capture::default())) }, now);
    assert_eq!(state.notice.as_deref(), Some("Saved 0 packets to /work/capture.pcap"));
    assert!(state.network.recording.is_none());
}

#[test]
fn a_finished_capture_opens_in_the_viewer_on_its_process() {
    let (mut state, now) = opened();
    press(&mut state, KeyCode::Char('2'), KeyModifiers::NONE);
    let browser = state.network.processes.iter().position(|row| row.pid == Some(BROWSER)).unwrap();
    state.network.select(browser);
    press(&mut state, KeyCode::Char('r'), KeyModifiers::NONE);
    assert_eq!(capture_dialog(&state).scopes[0], CaptureScope::Process { pid: BROWSER });
    let (id, request) = started(update(&mut state, Msg::Key(KeyCode::Enter.into()), now));
    assert!(request.filter.is_empty(), "a process's capture takes the whole interface");
    let path = request.folder.join("capture.pcap");
    let effects = update(&mut state, Msg::CaptureDone { id, result: Ok((path.clone(), Capture::default())) }, now);
    assert!(
        effects.iter().any(|effect| matches!(effect, Effect::List { dir, .. } if *dir == request.folder)),
        "the panel lists the new file: {effects:?}"
    );
    let view = state.network.viewer.as_deref().unwrap();
    assert_eq!((view.filter.as_str(), view.path.as_ref()), ("pid:8239", Some(&path)));
    assert_eq!(view.interface.as_deref(), Some(request.interface.as_str()));

    press(&mut state, KeyCode::Esc, KeyModifiers::NONE);
    assert!(state.network.viewer.as_deref().is_some_and(|view| view.filter.is_empty()), "Esc clears the filter");
    let effects = press(&mut state, KeyCode::Esc, KeyModifiers::NONE);
    assert!(state.network.viewer.is_none() && state.network.is_open(), "then goes back to the live screen");
    assert!(requested(&effects).is_some() || state.network.pending.is_some(), "which samples again");
}

#[test]
fn a_failed_capture_says_why() {
    let (mut state, now) = opened();
    press(&mut state, KeyCode::Char('r'), KeyModifiers::NONE);
    let (id, _) = started(update(&mut state, Msg::Key(KeyCode::Enter.into()), now));
    let failure = CaptureFailure::Failed("tcpdump stopped".into());
    update(&mut state, Msg::CaptureDone { id, result: Err(failure) }, now);
    assert_eq!(state.notice.as_deref(), Some("tcpdump stopped"));
    assert!(state.network.viewer.is_none() && state.network.recording.is_none());
}

const KEY_LOG: &str = "/work/capture-20260101-120000.keylog";

/// What the effects type at the shell's prompt.
fn written(effects: &[Effect]) -> Vec<String> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::Write(bytes) => Some(String::from_utf8_lossy(bytes).into_owned()),
            _ => None,
        })
        .collect()
}

const TRUST: &str = "/run/borg/trust-7-1.pem";
const PROXY: &str = "127.0.0.1:8080";
const CURL: u32 = 7311;

/// Starts a capture from the dialog with the TLS keys choice `steps` to the right of the first.
fn choosing_tls_keys(state: &mut AppState, steps: usize, now: Instant) -> (u64, CaptureRequest) {
    press(state, KeyCode::Char('r'), KeyModifiers::NONE);
    while capture_dialog(state).selected() != CaptureField::TlsKeys {
        press(state, KeyCode::Down, KeyModifiers::NONE);
    }
    for _ in 0..steps {
        press(state, KeyCode::Right, KeyModifiers::NONE);
    }
    started(update(state, Msg::Key(KeyCode::Enter.into()), now))
}

/// Starts a capture with TLS keys from the dialog, and gives its id and what follows once its
/// key log is there.
fn keeping_keys(state: &mut AppState, now: Instant) -> (u64, Vec<Effect>) {
    let (id, request) = choosing_tls_keys(state, 1, now);
    assert_eq!(request.tls_keys, TlsKeys::Shell);
    (id, update(state, Msg::CaptureKeys { id, path: PathBuf::from(KEY_LOG), proxy: None }, now))
}

/// Starts a capture through Borg's proxy from the dialog, and gives its id and what follows
/// once the proxy listens.
fn proxying(state: &mut AppState, now: Instant) -> (u64, Vec<Effect>) {
    let (id, request) = choosing_tls_keys(state, 2, now);
    assert_eq!(request.tls_keys, TlsKeys::Proxy(Authority::Temporary));
    let proxy = ProxyEndpoint { address: address(PROXY), trust: PathBuf::from(TRUST) };
    (id, update(state, Msg::CaptureKeys { id, path: PathBuf::from(KEY_LOG), proxy: Some(proxy) }, now))
}

fn saved(state: &mut AppState, id: u64, now: Instant) -> Vec<Effect> {
    update(state, Msg::CaptureDone { id, result: Ok((PathBuf::from("/work/capture.pcapng"), Capture::default())) }, now)
}

fn signal(state: &mut AppState, signal: ShellSignal, now: Instant) -> Vec<Effect> {
    update(state, Msg::Signal(signal), now)
}

#[test]
fn a_capture_with_tls_keys_sets_the_variable_in_the_shell_and_puts_it_back_at_the_end() {
    let (mut state, now) = opened();
    state.shell.integrated = true;
    let (id, effects) = keeping_keys(&mut state, now);
    assert_eq!(written(&effects), [format!("export SSLKEYLOGFILE='{KEY_LOG}'\r")]);
    assert_eq!(state.view, View::Shell, "the shell shows while the line runs");
    signal(&mut state, ShellSignal::CommandEnd { exit: Some(0) }, now);
    assert_eq!(state.view, View::Panels, "then the network screen is back");
    let recording = state.network.recording.as_ref().unwrap();
    assert_eq!(recording.key_log.as_deref(), Some(Path::new(KEY_LOG)));

    let effects = saved(&mut state, id, now);
    assert_eq!(written(&effects), ["unset SSLKEYLOGFILE\r"]);
    assert_eq!(state.view, View::Panels, "the viewer stays on screen");
    assert!(state.network.viewer.is_some());
    assert_eq!(state.notice.as_deref(), Some("Ran unset SSLKEYLOGFILE in Borg's shell."));
}

#[test]
fn a_busy_shell_gets_the_variable_back_once_its_command_ends() {
    let (mut state, now) = opened();
    state.shell.integrated = true;
    let (id, _) = keeping_keys(&mut state, now);
    signal(&mut state, ShellSignal::CommandEnd { exit: Some(0) }, now);
    signal(&mut state, ShellSignal::CommandStart, now);
    assert!(written(&saved(&mut state, id, now)).is_empty());
    assert_eq!(state.notice.as_deref(), Some(tr!(capture_tls_still_set)));
    let effects = signal(&mut state, ShellSignal::CommandEnd { exit: Some(0) }, now);
    assert_eq!(written(&effects), ["unset SSLKEYLOGFILE\r"]);
    assert_eq!(state.notice.as_deref(), Some("Ran unset SSLKEYLOGFILE in Borg's shell."));
    assert!(written(&signal(&mut state, ShellSignal::CommandEnd { exit: Some(0) }, now)).is_empty(), "only once");
}

#[test]
fn without_the_shell_l_copies_the_line_for_another_terminal() {
    let (mut state, now) = opened();
    assert!(press(&mut state, KeyCode::Char('l'), KeyModifiers::NONE).is_empty(), "nothing to copy yet");
    state.shell.running = true;
    let (id, effects) = keeping_keys(&mut state, now);
    assert!(written(&effects).is_empty());
    assert_eq!(state.view, View::Panels);
    assert_eq!(state.notice.as_deref(), Some(tr!(capture_tls_not_set)));

    let prefix = format!("SSLKEYLOGFILE='{KEY_LOG}' ");
    assert_eq!(press(&mut state, KeyCode::Char('l'), KeyModifiers::NONE), [Effect::Copy(prefix.clone())]);
    assert_eq!(state.clipboard.latest(), Some(prefix.as_str()));
    assert_eq!(state.notice, Some(format!("Copied {}", prefix.trim_end())));
    state.shell.running = false;
    assert!(written(&saved(&mut state, id, now)).is_empty(), "Borg set nothing in its shell to put back");
}

#[test]
fn a_capture_through_the_proxy_points_the_shell_at_it_and_puts_back_what_it_had() {
    let (mut state, now) = opened();
    state.shell.integrated = true;
    state.network.shell_variables.inherited = [("SSL_CERT_FILE", "/etc/ssl/cert.pem".into())].into();
    let (id, effects) = proxying(&mut state, now);
    let proxies = PROXY_VARIABLES.map(|name| format!("{name}='http://{PROXY}'"));
    let trusts = TRUST_VARIABLES.map(|name| format!("{name}='{TRUST}'"));
    assert_eq!(written(&effects), [format!("export {} {}\r", proxies.join(" "), trusts.join(" "))]);
    signal(&mut state, ShellSignal::CommandEnd { exit: Some(0) }, now);

    let effects = saved(&mut state, id, now);
    let restore = "export SSL_CERT_FILE='/etc/ssl/cert.pem'; \
                   unset HTTPS_PROXY https_proxy CURL_CA_BUNDLE REQUESTS_CA_BUNDLE NODE_EXTRA_CA_CERTS";
    assert_eq!(written(&effects), [format!("{restore}\r")]);
    assert_eq!(state.notice, Some(format!("Ran {restore} in Borg's shell.")));
}

#[test]
fn without_the_shell_l_copies_the_proxy_settings() {
    let (mut state, now) = opened();
    state.shell.running = true;
    let (id, effects) = proxying(&mut state, now);
    assert!(written(&effects).is_empty());
    assert_eq!(state.notice.as_deref(), Some(tr!(capture_proxy_not_set)));
    let copied = press(&mut state, KeyCode::Char('l'), KeyModifiers::NONE);
    let [Effect::Copy(prefix)] = copied.as_slice() else { panic!("one copy: {copied:?}") };
    assert!(prefix.starts_with(&format!("HTTPS_PROXY='http://{PROXY}' ")), "{prefix}");
    assert!(prefix.ends_with(&format!("NODE_EXTRA_CA_CERTS='{TRUST}' ")), "{prefix}");
    state.shell.running = false;
    assert!(written(&saved(&mut state, id, now)).is_empty(), "Borg set nothing in its shell to put back");
}

#[test]
fn the_proxy_signs_with_the_ca_folder_the_config_names() {
    let (mut state, now) = opened();
    let folder = PathBuf::from("/work/dev-ca");
    state.network.authority = Authority::Folder(folder.clone());
    let (_, request) = choosing_tls_keys(&mut state, 2, now);
    assert_eq!(request.tls_keys, TlsKeys::Proxy(Authority::Folder(folder)));
    let (mut state, now) = opened();
    let (_, request) = choosing_tls_keys(&mut state, 3, now);
    assert_eq!(request.tls_keys, TlsKeys::Off, "Right goes round to Off");
}

#[test]
fn the_viewer_names_the_program_a_proxied_connection_was_for() {
    let (mut state, now) = opened();
    let (id, _) = proxying(&mut state, now);
    let proxied = Proxied { local: address("127.0.0.1:50001"), remote: address(PROXY), pid: CURL, name: "curl".into() };
    update(&mut state, Msg::CaptureProxied { id, proxied }, now);
    saved(&mut state, id, now);
    assert_eq!(state.network.viewer.as_deref().map(|view| view.name(Some(CURL))).as_deref(), Some("curl"));
}

const ASKS: CaptureAccess = CaptureAccess { administrator: false, elevate: true, tags: true };

#[test]
fn run_as_shows_where_borg_can_ask_and_an_administrator_capture_tags_processes_where_it_can() {
    let (mut state, now) = opened();
    press(&mut state, KeyCode::Char('r'), KeyModifiers::NONE);
    assert_eq!(capture_dialog(&state).fields().len(), 5, "no Run as where Borg cannot ask");
    press(&mut state, KeyCode::Esc, KeyModifiers::NONE);

    state.network.access = ASKS;
    press(&mut state, KeyCode::Char('r'), KeyModifiers::NONE);
    press(&mut state, KeyCode::End, KeyModifiers::NONE);
    assert_eq!(capture_dialog(&state).selected(), CaptureField::Access);
    assert_eq!(capture_dialog(&state).request().unwrap().extension(), "pcap");
    press(&mut state, KeyCode::Right, KeyModifiers::NONE);
    let (_, request) = started(update(&mut state, Msg::Key(KeyCode::Enter.into()), now));
    assert!(request.administrator && request.processes);
    assert_eq!(request.extension(), "pcapng");
    assert!(state.network.recording.as_ref().is_some_and(|recording| recording.waiting), "it waits for the password");

    let (mut state, now) = opened();
    state.network.access = CaptureAccess { administrator: true, elevate: false, tags: true };
    press(&mut state, KeyCode::Char('r'), KeyModifiers::NONE);
    let (_, request) = started(update(&mut state, Msg::Key(KeyCode::Enter.into()), now));
    assert!(!request.administrator && request.processes, "Borg running as administrator tags without asking");
}

#[test]
fn a_refused_capture_asks_to_run_as_administrator_and_yes_runs_it_so() {
    let (mut state, now) = opened();
    state.network.access = ASKS;
    press(&mut state, KeyCode::Char('r'), KeyModifiers::NONE);
    let (id, refused) = started(update(&mut state, Msg::Key(KeyCode::Enter.into()), now));
    update(&mut state, Msg::CaptureDone { id, result: Err(CaptureFailure::Denied) }, now);
    let Some(Overlay::Confirm(confirm)) = &state.overlay else { panic!("Borg asks first: {:?}", state.overlay) };
    assert!(confirm.message.contains(&refused.interface), "{}", confirm.message);
    assert!(state.network.recording.is_none());

    let (id, request) = started(press(&mut state, KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(request, CaptureRequest { administrator: true, processes: true, ..refused });
    let recording = state.network.recording.as_ref().unwrap();
    assert!(recording.waiting);

    let later = now + Duration::from_secs(20);
    update(&mut state, Msg::CaptureProgress { id, progress: progress(24, 0) }, later);
    let recording = state.network.recording.as_ref().unwrap();
    assert!(!recording.waiting);
    assert_eq!(recording.started, later, "its seconds count from the password");

    update(&mut state, Msg::CaptureDone { id, result: Err(CaptureFailure::Denied) }, later);
    assert!(state.overlay.is_none(), "an administrator's capture is not asked about again");
    assert_eq!(state.notice.as_deref(), Some(tr!(capture_denied, interface = request.interface).as_str()));
}

#[test]
fn a_refusal_where_borg_cannot_ask_says_so_and_a_dismissed_password_says_nothing() {
    let (mut state, now) = opened();
    press(&mut state, KeyCode::Char('r'), KeyModifiers::NONE);
    let (id, request) = started(update(&mut state, Msg::Key(KeyCode::Enter.into()), now));
    update(&mut state, Msg::CaptureDone { id, result: Err(CaptureFailure::Denied) }, now);
    assert!(state.overlay.is_none());
    assert_eq!(state.notice.as_deref(), Some(tr!(capture_denied, interface = request.interface).as_str()));

    state.notice = None;
    state.network.access = ASKS;
    press(&mut state, KeyCode::Char('r'), KeyModifiers::NONE);
    let (id, _) = started(update(&mut state, Msg::Key(KeyCode::Enter.into()), now));
    update(&mut state, Msg::CaptureDone { id, result: Err(CaptureFailure::Denied) }, now);
    press(&mut state, KeyCode::Esc, KeyModifiers::NONE);
    assert!(state.overlay.is_none() && state.network.recording.is_none(), "No leaves it");
    press(&mut state, KeyCode::Char('r'), KeyModifiers::NONE);
    let (id, _) = started(update(&mut state, Msg::Key(KeyCode::Enter.into()), now));
    update(&mut state, Msg::CaptureDone { id, result: Err(CaptureFailure::Cancelled) }, now);
    assert!(state.notice.is_none() && state.network.recording.is_none());
}

#[test]
fn a_capture_without_tcpdump_offers_to_install_it() {
    let (mut state, now) = opened();
    press(&mut state, KeyCode::Char('r'), KeyModifiers::NONE);
    let (id, _) = started(update(&mut state, Msg::Key(KeyCode::Enter.into()), now));
    let installer = Installer { manager: PackageManager::Dnf, elevation: Some(Elevation::Sudo) };
    update(&mut state, Msg::CaptureDone { id, result: Err(CaptureFailure::Missing(Some(installer))) }, now);
    let offer = InstallOffer { tool: Tool::Tcpdump, installer };
    assert_eq!(state.overlay, Some(Overlay::Install(offer)));
    assert!(state.network.recording.is_none() && state.notice.is_none());
}

#[test]
fn an_opened_capture_file_shows_its_hosts() {
    let (mut state, now) = state();
    let path = PathBuf::from("/work/trace.pcap");
    let effects = update(&mut state, Msg::CaptureOpened { path: path.clone(), result: Ok(Capture::default()) }, now);
    assert!(state.network.is_open());
    assert!(requested(&effects).is_some(), "the screen opens and samples");
    let view = state.network.viewer.as_deref().unwrap();
    assert_eq!((view.page, view.path.as_ref()), (TrafficPage::Hosts, Some(&path)));
    press(&mut state, KeyCode::Enter, KeyModifiers::NONE);
    assert!(!state.network.details_focused, "Enter on an empty list leaves the keys with it");
}

#[test]
fn f3_shows_a_conversations_stream_or_a_packets_bytes_as_hex_rows() {
    use crate::{
        ACK, Content, PSH, SYN, Shown,
        capture::tests::{CLIENT, CLIENT_PORT, SERVER, over_tcp, pcap, tcp},
    };

    let (request, answer) = (b"GET / HTTP/1.1\r\n", b"HTTP/1.1 200 OK\r\n");
    let frames = [
        over_tcp(CLIENT, SERVER, tcp(CLIENT_PORT, 80, 1000, SYN, &[])),
        over_tcp(SERVER, CLIENT, tcp(80, CLIENT_PORT, 5000, SYN | ACK, &[])),
        over_tcp(CLIENT, SERVER, tcp(CLIENT_PORT, 80, 1001, PSH | ACK, request)),
        over_tcp(SERVER, CLIENT, tcp(80, CLIENT_PORT, 5001, PSH | ACK, answer)),
    ];
    let capture = Capture::read(&pcap(&frames)).unwrap();
    let (mut state, now) = state();
    update(&mut state, Msg::CaptureOpened { path: PathBuf::from("/work/web.pcap"), result: Ok(capture) }, now);
    let f3 = |state: &mut AppState| press(state, KeyCode::F(3), KeyModifiers::NONE);
    let shown = |state: &AppState| state.viewer.as_ref().map(|viewer| viewer.shown.clone());
    let pager = |state: &AppState| match &state.viewer.as_ref().unwrap().content {
        Content::Text(pager) => pager.clone(),
        Content::Image(_) => panic!("expected bytes"),
    };

    f3(&mut state);
    assert_eq!(shown(&state), None, "a host has no bytes of its own");
    press(&mut state, KeyCode::Char('3'), KeyModifiers::NONE);
    f3(&mut state);
    assert_eq!(shown(&state), Some(Shown::Bytes("192.168.1.20:51000 → 93.184.216.34:80".into())));
    assert!(pager(&state).hex());
    assert_eq!(pager(&state).len(), (request.len() + answer.len()) as u64, "both sides' turns, one after another");
    assert_eq!(pager(&state).rows()[0], "0000  47 45 54 20 2f 20 48 54 54 50 2f 31 2e 31 0d 0a  GET / HTTP/1.1..");
    press(&mut state, KeyCode::Esc, KeyModifiers::NONE);
    assert!(state.viewer.is_none() && state.network.viewer.is_some(), "Esc goes back to the traffic");

    press(&mut state, KeyCode::Char('4'), KeyModifiers::NONE);
    f3(&mut state);
    assert_eq!(shown(&state), Some(Shown::Bytes("Packet 1".into())));
    assert!(pager(&state).hex());
    press(&mut state, KeyCode::Esc, KeyModifiers::NONE);
    press(&mut state, KeyCode::Char('1'), KeyModifiers::NONE);
    f3(&mut state);
    assert_eq!(shown(&state), None, "a process has no bytes of its own");
}

#[test]
fn an_agent_capture_takes_the_interface_the_dialog_would() {
    let network = measured("");
    let postgres = CaptureScope::Port { transport: Transport::Tcp, port: 5432 };
    assert_eq!(capture_interfaces(&network, postgres)[0], "lo0", "a port only this machine reaches is on loopback");
    let ssh = CaptureScope::Connection {
        transport: Transport::Tcp,
        local: address("127.0.0.1:50000"),
        remote: address("127.0.0.1:22"),
    };
    assert_eq!(capture_interfaces(&network, ssh)[0], "lo0");
    assert_eq!(capture_interfaces(&network, CaptureScope::Everything)[0], "en0", "the busiest otherwise");
}

#[test]
fn an_agent_capture_asks_with_what_the_dialog_shows() {
    let request = CaptureRequest {
        interface: "en0".into(),
        seconds: Some(10),
        keep: Keep::Headers,
        filter: String::new(),
        folder: PathBuf::from("/work"),
        administrator: false,
        processes: false,
        tls_keys: TlsKeys::Off,
        keep_free_mib: None,
    };
    let ask = capture_ask(CaptureScope::Process { pid: BROWSER }, Some("browser"), &request, None);
    assert_eq!((ask.kind, ask.summary.as_str()), (AskKind::Capture, "capture on en0, 10 s"));
    let lines: Vec<&str> = ask.body.iter().map(|line| line.text.as_str()).collect();
    assert_eq!(
        lines[..5],
        [
            "Traffic: All traffic, shown for browser · PID 8239",
            "Interface: en0",
            "Duration: 10 s",
            "Keep: Headers, first 128 bytes of each packet",
            "Saved as a .pcap file in /work",
        ]
    );
    let port = CaptureScope::Port { transport: Transport::Udp, port: 53 };
    let ask = capture_ask(port, None, &CaptureRequest { keep: Keep::Whole, ..request.clone() }, None);
    assert_eq!(ask.body[0].text, "Traffic: UDP port 53 and its connections");
    assert_eq!(ask.body[3].text, "Keep: Whole packets");
    let ask = capture_ask(port, None, &CaptureRequest { keep: Keep::ConnectionStart, ..request }, None);
    assert_eq!(ask.body[3].text, "Keep: First 1 MiB of each connection");
}
