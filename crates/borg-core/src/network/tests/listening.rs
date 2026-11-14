use super::*;
use crate::{ConfirmPurpose, How, JobKind, Value};

const PRINTER: &str = "192.0.2.31";
const PHONE: &str = "192.0.2.44";
const PRINTER_MAC: &str = "02:00:00:00:00:31";

fn tap(state: &mut AppState, code: KeyCode) -> Vec<Effect> {
    press(state, code, KeyModifiers::NONE)
}

fn typed(state: &mut AppState, text: &str) {
    for c in text.chars() {
        tap(state, KeyCode::Char(c));
    }
}

fn ip(text: &str) -> IpAddr {
    text.parse().unwrap()
}

fn form(state: &AppState) -> &ListenForm {
    match &state.overlay {
        Some(Overlay::Listen(form)) => form,
        other => panic!("the listen form is open: {other:?}"),
    }
}

fn started(effects: Vec<Effect>) -> (u64, ListenRequest) {
    effects
        .into_iter()
        .find_map(|effect| match effect {
            Effect::Listen { id, request } => Some((id, request)),
            _ => None,
        })
        .expect("listening starts")
}

fn event(state: &mut AppState, id: u64, event: ListenEvent, now: Instant) -> Vec<Effect> {
    update(state, Msg::Listen { id, event }, now)
}

/// What a minute on the home network says: the printer over mDNS and SSDP, and the phone, which
/// the tables list already, over DHCP.
fn chatter() -> Vec<Heard> {
    let printer = Announcement {
        address: Some(ip(PRINTER)),
        mac: Mac::parse(PRINTER_MAC),
        name: Some("laserbox.local".into()),
        model: Some("LaserBox 3000".into()),
        services: vec!["ipp".into(), "http".into()],
        ..Announcement::new(ListenProtocol::Mdns)
    };
    let upnp = Announcement {
        address: Some(ip(PRINTER)),
        kinds: vec!["Printer".into()],
        ..Announcement::new(ListenProtocol::Ssdp)
    };
    let phone = Announcement {
        address: Some(ip(PHONE)),
        name: Some("pocket".into()),
        client: Some("android-dhcp-14".into()),
        ..Announcement::new(ListenProtocol::Dhcp)
    };
    vec![Heard::Host(printer), Heard::Host(upnp), Heard::Host(phone)]
}

/// Starts a minute of listening on en0 and lets it hear the home network's chatter.
fn listened() -> (AppState, Instant, u64) {
    let (mut state, now) = home_opened();
    tap(&mut state, KeyCode::Char('h'));
    assert_eq!(form(&state).interfaces, ["en0"], "lo0 has no link to hear on");
    assert_eq!(form(&state).request().unwrap().seconds, Some(60), "a minute to start with");
    let (id, request) = started(tap(&mut state, KeyCode::Enter));
    assert_eq!(request, ListenRequest { interface: "en0".into(), seconds: Some(60), administrator: false });
    event(&mut state, id, ListenEvent::Started, now);
    let effects =
        event(&mut state, id, ListenEvent::Heard { heard: chatter(), at: HOME_READ_AT }, now + Duration::from_secs(5));
    assert!(saves(&effects), "the hosts heard are remembered");
    (state, now + Duration::from_secs(5), id)
}

fn job(state: &AppState, id: u64) -> &crate::Job {
    state.jobs.iter().find(|job| job.id == id).expect("the job is listed")
}

fn texts(job: &crate::Job) -> Vec<String> {
    job.details
        .iter()
        .filter_map(|detail| match &detail.value {
            Value::Text(text) => Some(text.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn h_asks_where_and_for_how_long_and_the_hosts_heard_join_the_map_with_their_names() {
    let (state, _, id) = listened();
    let network = &state.network;
    assert_eq!(job(&state, id).kind, JobKind::Listen);
    assert_eq!(presence_of(network, PRINTER), Some(Presence::Heard), "only listening knows the printer");
    assert_eq!(presence_of(network, PHONE), Some(Presence::Listed));
    let printer = network.neighborhood.neighbors.iter().find(|neighbor| neighbor.address == ip(PRINTER)).unwrap();
    assert_eq!(printer.mac, Mac::parse(PRINTER_MAC));
    assert_eq!(network.neighbor_name(printer).as_deref(), Some("laserbox.local"));
    assert_eq!(network.host_name(ip(PHONE)), Some("pocket"), "an announced name beats the vendor");
    let announced = network.announced(ip(PRINTER)).unwrap();
    assert_eq!(announced.kinds, ["Printer"]);
    assert_eq!(announced.protocols, [ListenProtocol::Mdns, ListenProtocol::Ssdp]);
    assert_eq!(
        texts(job(&state, id)),
        ["en0", "2 hosts", "192.0.2.31 laserbox.local · mDNS", "192.0.2.44 pocket · DHCP"]
    );
    assert_eq!(job(&state, id).gauge.done, 5, "five seconds of sixty");
}

#[test]
fn the_filter_finds_heard_hosts_by_state_name_model_and_protocol() {
    let (mut state, _, _) = listened();
    for (filter, expected) in [
        ("state:heard", vec![PRINTER]),
        ("laserbox", vec![PRINTER]),
        ("printer", vec![PRINTER]),
        ("proto:dhcp", vec![PHONE]),
        ("pocket", vec![PHONE]),
    ] {
        tap(&mut state, KeyCode::Char('/'));
        tap(&mut state, KeyCode::Esc);
        tap(&mut state, KeyCode::Char('/'));
        typed(&mut state, filter);
        tap(&mut state, KeyCode::Enter);
        assert_eq!(shown_neighbors(&state.network), expected, "{filter}");
    }
}

#[test]
fn h_again_stops_listening_and_the_job_says_what_it_heard() {
    let (mut state, now, id) = listened();
    tap(&mut state, KeyCode::Esc);
    tap(&mut state, KeyCode::Esc);
    assert!(!state.network.is_open(), "listening goes on with the screen closed");
    let later = Announcement { address: Some(ip("192.0.2.70")), ..Announcement::new(ListenProtocol::Arp) };
    event(&mut state, id, ListenEvent::Heard { heard: vec![Heard::Host(later)], at: HOME_READ_AT + 9 }, now);
    assert_eq!(state.network.listening.as_ref().map(|listening| listening.heard.len()), Some(3));
    press(&mut state, KeyCode::Char('n'), KeyModifiers::ALT);
    tap(&mut state, KeyCode::Char('5'));
    let effects = tap(&mut state, KeyCode::Char('h'));
    assert_eq!(effects, vec![Effect::JobControl { id, command: crate::JobCommand::Cancel }]);
    event(&mut state, id, ListenEvent::Finished(Err(CaptureFailure::Cancelled)), now);
    let ending = job(&state, id).ending().map(|ending| (ending.how, ending.summary.clone()));
    assert_eq!(ending, Some((How::Cancelled, "Listening stopped · heard 3 hosts".into())));
    assert!(state.network.listening.is_none());
    assert_eq!(job(&state, id).reveal, Some(crate::Reveal::Neighbors));
    tap(&mut state, KeyCode::Char('h'));
    assert!(matches!(state.overlay, Some(Overlay::Listen(_))), "H asks again once it stopped");
}

#[test]
fn a_listen_that_ran_its_time_is_done_and_events_for_another_job_are_ignored() {
    let (mut state, now, id) = listened();
    let stray = Announcement { address: Some(ip("192.0.2.80")), ..Announcement::new(ListenProtocol::Arp) };
    event(&mut state, id + 7, ListenEvent::Heard { heard: vec![Heard::Host(stray)], at: HOME_READ_AT }, now);
    assert_eq!(presence_of(&state.network, "192.0.2.80"), None);
    event(&mut state, id, ListenEvent::Finished(Ok(())), now + Duration::from_secs(60));
    let ending = job(&state, id).ending().map(|ending| (ending.how, ending.summary.clone()));
    assert_eq!(ending, Some((How::Done, "Listening done · heard 2 hosts".into())));
}

#[test]
fn a_refused_listen_asks_to_run_as_administrator_and_yes_runs_it_so() {
    let (mut state, now) = home_opened();
    state.network.access = CaptureAccess { administrator: false, elevate: true, tags: true };
    tap(&mut state, KeyCode::Char('h'));
    assert_eq!(form(&state).fields().len(), 3, "Run as shows where Borg can ask");
    let (id, _) = started(tap(&mut state, KeyCode::Enter));
    event(&mut state, id, ListenEvent::Finished(Err(CaptureFailure::Denied)), now);
    let Some(Overlay::Confirm(confirm)) = &state.overlay else { panic!("Borg asks first: {:?}", state.overlay) };
    assert!(matches!(confirm.purpose, ConfirmPurpose::Listen(_)));
    assert!(confirm.message.contains("en0"), "{}", confirm.message);
    assert!(state.jobs.iter().all(|job| job.id != id), "the refused job goes away");
    let (_, request) = started(tap(&mut state, KeyCode::Char('y')));
    assert!(request.administrator);

    let (mut state, now) = home_opened();
    tap(&mut state, KeyCode::Char('h'));
    assert_eq!(form(&state).fields().len(), 2, "no Run as where Borg cannot ask");
    let (id, _) = started(tap(&mut state, KeyCode::Enter));
    event(&mut state, id, ListenEvent::Finished(Err(CaptureFailure::Denied)), now);
    let ending = job(&state, id).ending().map(|ending| (ending.how, ending.summary.clone()));
    assert_eq!(
        ending,
        Some((How::Failed, "No permission to listen on en0. See Listening for neighbors in the guide.".into()))
    );
}

#[test]
fn listening_without_tcpdump_offers_to_install_it() {
    let (mut state, now) = home_opened();
    tap(&mut state, KeyCode::Char('h'));
    let (id, _) = started(tap(&mut state, KeyCode::Enter));
    let installer = Installer { manager: PackageManager::Dnf, elevation: Some(Elevation::Sudo) };
    event(&mut state, id, ListenEvent::Finished(Err(CaptureFailure::Missing(Some(installer)))), now);
    let Some(Overlay::Install(offer)) = &state.overlay else { panic!("Borg offers tcpdump: {:?}", state.overlay) };
    assert_eq!(offer.tool, Tool::Tcpdump);
    assert!(state.jobs.iter().all(|job| job.id != id));
}

#[test]
fn heard_hosts_are_remembered_for_the_next_visit_until_f8_forgets_them() {
    let (mut state, now, id) = listened();
    event(&mut state, id, ListenEvent::Finished(Ok(())), now);
    let saved = state.network.memory.saved();
    state.network.memory = NeighborMemory::restore(saved).unwrap();
    state.network.hearing = Hearing::default();
    let next = asked(&state.network.request()).unwrap().0;
    update(&mut state, Msg::Network { id: next, result: Ok(home_sample(2, ROUTER_MAC)) }, now);
    assert_eq!(presence_of(&state.network, PRINTER), Some(Presence::Offline(HOME_READ_AT)), "not heard yet this time");
    let heard = state
        .network
        .neighbor_rows()
        .find(|row| state.network.neighborhood.neighbors[row.neighbor].address == ip(PRINTER));
    assert!(heard.is_some_and(|row| row.heard), "the row still says listening heard it");
    assert_eq!(state.network.host_name(ip(PRINTER)), Some("laserbox.local"));
    assert_eq!(state.network.last_heard(ip(PRINTER)), Some(HOME_READ_AT));

    let row = state.network.neighbors.iter().position(|entry| match entry {
        NeighborEntry::Host(row) => state.network.neighborhood.neighbors[row.neighbor].address == ip(PHONE),
        _ => false,
    });
    state.network.select(row.unwrap());
    tap(&mut state, KeyCode::F(8));
    assert_eq!(
        state.notice.as_deref(),
        Some("Borg forgets only hosts that are offline or that only listening heard."),
        "the tables still list the phone"
    );
    let row = state.network.neighbors.iter().position(|entry| match entry {
        NeighborEntry::Host(row) => state.network.neighborhood.neighbors[row.neighbor].address == ip(PRINTER),
        _ => false,
    });
    state.network.select(row.unwrap());
    tap(&mut state, KeyCode::F(8));
    let Some(Overlay::Confirm(confirm)) = &state.overlay else { panic!("F8 asks first: {:?}", state.overlay) };
    assert!(!confirm.accept, "No is the default");
    let effects = tap(&mut state, KeyCode::Char('y'));
    assert!(saves(&effects));
    assert_eq!(presence_of(&state.network, PRINTER), None);
    assert_eq!(state.network.announced(ip(PRINTER)), None);
}

#[test]
fn the_report_names_heard_hosts_and_what_they_announced() {
    let (state, _, _) = listened();
    let report = crate::report::NeighborsReport::of(&state);
    let printer = report.hosts.iter().find(|host| host.address == ip(PRINTER)).unwrap();
    assert_eq!(printer.presence, crate::report::ReportPresence::Heard);
    assert_eq!(printer.name.as_deref(), Some("laserbox.local"));
    assert_eq!(printer.heard, Some(HOME_READ_AT));
    let services = printer.announced.as_ref().map(|announced| announced.services.clone());
    assert_eq!(services, Some(vec!["ipp".into(), "http".into()]));
}
