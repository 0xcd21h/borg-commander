use super::*;
use crate::{
    Ending, Gauge, How, JobKind, PortScanEvent, PortScanForm, PortScanRequest, PortScanResult, PortScanView, Unit,
    Value, View,
};

const QUIET: Duration = Duration::from_secs(2);

fn tap(state: &mut AppState, code: KeyCode) -> Vec<Effect> {
    press(state, code, KeyModifiers::NONE)
}

fn typed(state: &mut AppState, text: &str) {
    for c in text.chars() {
        tap(state, KeyCode::Char(c));
    }
}

fn form(state: &AppState) -> &PortScanForm {
    match &state.overlay {
        Some(Overlay::PortScan(form)) => form,
        other => panic!("the port scan form is open: {other:?}"),
    }
}

fn confirm(state: &AppState) -> (&crate::Confirm, &PortScanRequest) {
    match &state.overlay {
        Some(Overlay::Confirm(confirm)) => match &confirm.purpose {
            crate::ConfirmPurpose::ScanPorts(request) => (confirm, request),
            other => panic!("the confirmation is for a port scan: {other:?}"),
        },
        other => panic!("a confirmation is open: {other:?}"),
    }
}

/// Opens the form on the storage box, the third neighbor in address order.
fn on_the_storage_box() -> (AppState, Instant) {
    let (mut state, now) = home_opened();
    tap(&mut state, KeyCode::Down);
    tap(&mut state, KeyCode::Down);
    tap(&mut state, KeyCode::Char('p'));
    assert_eq!(form(&state).target.text(), "192.0.2.60");
    (state, now)
}

fn started(effects: Vec<Effect>) -> (u64, PortScanRequest) {
    effects
        .into_iter()
        .find_map(|effect| match effect {
            Effect::ScanPorts { id, request } => Some((id, *request)),
            _ => None,
        })
        .expect("the scan starts")
}

fn finish(state: &mut AppState, id: u64, result: PortScanResult, now: Instant) -> Vec<Effect> {
    update(state, Msg::PortScan { id, event: PortScanEvent::Finished { result, at: HOME_READ_AT } }, now)
}

fn found(open: &[u16], closed: usize, ending: Ending) -> PortScanResult {
    PortScanResult { open: open.to_vec(), closed, filtered: 0, not_scanned: Vec::new(), ending }
}

#[test]
fn p_asks_which_ports_of_the_selected_neighbor_to_scan_and_a_typed_list_must_read() {
    let (mut state, _) = on_the_storage_box();
    assert_eq!(form(&state).field, crate::FormField::Ports, "the host is already there");
    tap(&mut state, KeyCode::Right);
    tap(&mut state, KeyCode::Right);
    assert_eq!(form(&state).kind, crate::PortsKind::List);
    tap(&mut state, KeyCode::Down);
    typed(&mut state, "90-80");
    tap(&mut state, KeyCode::Enter);
    assert_eq!(form(&state).error.as_deref(), Some("90-80 ends before it starts."));
    press(&mut state, KeyCode::Char('u'), KeyModifiers::CONTROL);
    assert_eq!(form(&state).error, None, "typing clears the complaint");
    typed(&mut state, "22,80,8000-8002");
    tap(&mut state, KeyCode::Down);
    tap(&mut state, KeyCode::Char(' '));
    tap(&mut state, KeyCode::Enter);
    let (confirm, request) = confirm(&state);
    assert_eq!(confirm.message, "Scan 5 ports on 192.0.2.60 in Slow mode?");
    assert!(confirm.accept, "a host on this network needs no warning");
    assert_eq!(request.ports, vec![22, 80, 8000, 8001, 8002]);
    assert_eq!(request.gateway, Mac::parse(ROUTER_MAC), "known behind this network's gateway");
}

#[test]
fn a_host_outside_the_private_ranges_is_named_as_such_and_no_is_the_default() {
    let (mut state, _) = on_the_storage_box();
    tap(&mut state, KeyCode::Up);
    press(&mut state, KeyCode::Char('u'), KeyModifiers::CONTROL);
    typed(&mut state, "203.0.113.9");
    tap(&mut state, KeyCode::Enter);
    let (confirm, request) = confirm(&state);
    assert!(confirm.message.starts_with("Scan 720 ports on 203.0.113.9 in Fast mode? 203.0.113.9 is outside"));
    assert!(!confirm.accept);
    assert_eq!(request.gateway, None);
}

#[test]
fn a_name_is_looked_up_before_the_confirmation_and_one_not_found_says_so() {
    let (mut state, now) = on_the_storage_box();
    tap(&mut state, KeyCode::Up);
    press(&mut state, KeyCode::Char('u'), KeyModifiers::CONTROL);
    tap(&mut state, KeyCode::Enter);
    assert_eq!(form(&state).error.as_deref(), Some("Type a host name or an address."));
    typed(&mut state, "nas.home.arpa");
    let effects = tap(&mut state, KeyCode::Enter);
    assert!(effects.contains(&Effect::ResolveScanTarget("nas.home.arpa".into())));
    let stale = Msg::ScanTargetResolved { name: "printer.home.arpa".into(), result: Ok("192.0.2.31".parse().unwrap()) };
    update(&mut state, stale, now);
    assert_eq!(form(&state).resolving.as_deref(), Some("nas.home.arpa"), "another name's answer is ignored");
    let missing = Msg::ScanTargetResolved { name: "nas.home.arpa".into(), result: Err("no such host".into()) };
    update(&mut state, missing, now);
    assert_eq!(form(&state).error.as_deref(), Some("Could not find nas.home.arpa: no such host"));
    tap(&mut state, KeyCode::Enter);
    let answer = Msg::ScanTargetResolved { name: "nas.home.arpa".into(), result: Ok("192.0.2.60".parse().unwrap()) };
    update(&mut state, answer, now);
    let (confirm, request) = confirm(&state);
    assert_eq!(confirm.message, "Scan 720 ports on nas.home.arpa (192.0.2.60) in Fast mode?");
    assert_eq!(request.address, "192.0.2.60".parse::<IpAddr>().unwrap());
}

#[test]
fn yes_starts_a_job_that_outlives_the_screen_shows_progress_and_counts_at_quit() {
    let (mut state, now) = on_the_storage_box();
    tap(&mut state, KeyCode::Enter);
    let (id, request) = started(tap(&mut state, KeyCode::Enter));
    assert_eq!(request.ports.len(), 720);
    assert_eq!(state.notice.as_deref(), Some("Scanning ports of 192.0.2.60 in the background. Alt-J shows the job."));
    tap(&mut state, KeyCode::Esc);
    assert!(!state.network.is_open());
    let job = state.jobs.iter().find(|job| job.id == id).expect("the job runs on");
    assert_eq!(job.kind, JobKind::PortScan);
    update(&mut state, Msg::PortScan { id, event: PortScanEvent::Progress { done: 180, open: vec![22, 80] } }, now);
    assert_eq!(state.jobs[0].gauge, Gauge::of(180, 720, Unit::Ports));
    assert_eq!(state.jobs[0].details.last().map(|detail| &detail.value), Some(&Value::Number(2)));
    assert_eq!(state.jobs[0].fraction(), Some(0.25));
    tap(&mut state, KeyCode::F(10));
    let Some(Overlay::Confirm(quit)) = &state.overlay else { panic!("F10 asks") };
    assert_eq!(quit.message, "1 job is still running. Quit and cancel?");
    tap(&mut state, KeyCode::Esc);
    press(&mut state, KeyCode::Char('j'), KeyModifiers::ALT);
    let effects = tap(&mut state, KeyCode::Char('c'));
    assert_eq!(effects, vec![Effect::JobControl { id, command: crate::JobCommand::Cancel }]);
}

#[test]
fn a_finished_scan_opens_its_dialog_once_the_panels_show_and_the_keys_rest() {
    let (mut state, now) = on_the_storage_box();
    tap(&mut state, KeyCode::Enter);
    let (id, _) = started(tap(&mut state, KeyCode::Enter));
    tap(&mut state, KeyCode::Esc);
    let open: Vec<u16> = vec![22, 80, 443, 5000, 5001, 8080, 9000, 9090, 9200, 11211, 27017, 30000, 40000];
    let effects = finish(&mut state, id, found(&open, 720 - open.len(), Ending::Done), now);
    assert!(effects.contains(&Effect::SavePortScans));
    let ending = state.jobs[0].ending().map(|ending| (ending.how, ending.summary.as_str()));
    assert_eq!(ending, Some((How::Done, "Port scan done · 13 open")));
    assert_eq!(state.overlay, None, "the keys were just pressed");
    let rested = state.idle_since + QUIET;
    state.view = View::Shell;
    update(&mut state, Msg::Tick, rested);
    assert_eq!(state.overlay, None, "the shell keeps its keys");
    state.view = View::Panels;
    update(&mut state, Msg::Tick, rested);
    let Some(Overlay::PortScanDone(record)) = &state.overlay else { panic!("the dialog opens: {:?}", state.overlay) };
    assert_eq!(record.open, open);
    assert_eq!(state.port_scans.waiting(), 0);
    tap(&mut state, KeyCode::Enter);
    tap(&mut state, KeyCode::End);
    let Some(Overlay::PortScanResult(view)) = &state.overlay else { panic!("Enter shows the ports") };
    assert_eq!(view.top, open.len() - 1);
    tap(&mut state, KeyCode::Esc);
    assert_eq!(state.overlay, None);
    let scan = crate::scan_of(&state, "192.0.2.60".parse().unwrap()).expect("the host's details keep it");
    assert_eq!((scan.at, scan.count, scan.open.len()), (HOME_READ_AT, 720, open.len()));
    press(&mut state, KeyCode::Char('g'), KeyModifiers::ALT);
    let Some(Overlay::PortScanResult(view)) = &state.overlay else { panic!("Alt-G lists the ports again") };
    assert_eq!((view.record.open.len(), view.top), (open.len(), 0));
    tap(&mut state, KeyCode::Esc);
    press(&mut state, KeyCode::Char('j'), KeyModifiers::ALT);
    tap(&mut state, KeyCode::Char('o'));
    let Some(Overlay::PortScanResult(view)) = &state.overlay else { panic!("O in the jobs panel lists them") };
    assert_eq!(view.record.open, open);
}

#[test]
fn a_cancelled_scan_says_what_it_found_and_a_limited_one_still_reports() {
    let (mut state, now) = on_the_storage_box();
    tap(&mut state, KeyCode::Enter);
    let (id, _) = started(tap(&mut state, KeyCode::Enter));
    let mut cut = found(&[22], 99, Ending::Cancelled);
    cut.not_scanned = vec![crate::PortRange { first: 1_000, last: 1_619 }];
    let effects = finish(&mut state, id, cut, now);
    assert!(effects.contains(&Effect::SavePortScans), "what it found is kept");
    assert_eq!(state.notice.as_deref(), Some("Port scan of 192.0.2.60 cancelled. Open ports found: 1."));
    let ending = state.jobs[0].ending().map(|ending| (ending.how, ending.summary.as_str()));
    assert_eq!(ending, Some((How::Cancelled, "Port scan stopped · 1 open")));
    assert_eq!(state.port_scans.waiting(), 0, "no dialog for a scan you stopped");
    tap(&mut state, KeyCode::Char('p'));
    tap(&mut state, KeyCode::Enter);
    let (id, _) = started(tap(&mut state, KeyCode::Enter));
    let mut limited = found(&[22, 80], 98, Ending::RateLimited { port: 80 });
    limited.not_scanned = vec![crate::PortRange { first: 1_000, last: 1_619 }];
    finish(&mut state, id, limited, now);
    assert_eq!(state.port_scans.waiting(), 1);
    let limited = state.jobs.iter().find(|job| job.id == id).and_then(|job| job.ending());
    assert_eq!(
        limited.map(|ending| (ending.how, ending.summary.as_str())),
        Some((How::Failed, "Port scan limited by the host · 2 open"))
    );
    state.port_scans.writable = false;
    tap(&mut state, KeyCode::Char('p'));
    tap(&mut state, KeyCode::Enter);
    let (id, _) = started(tap(&mut state, KeyCode::Enter));
    let effects = finish(&mut state, id, found(&[], 720, Ending::Done), now);
    assert!(!effects.contains(&Effect::SavePortScans), "a file that failed to load is left alone");
}

#[test]
fn the_result_view_scrolls_within_the_open_ports() {
    let (mut state, now) = on_the_storage_box();
    tap(&mut state, KeyCode::Enter);
    let (id, _) = started(tap(&mut state, KeyCode::Enter));
    finish(&mut state, id, found(&[22, 80, 443], 717, Ending::Done), now);
    let record = state.port_scans.memory.last("192.0.2.60").cloned().unwrap();
    state.overlay = Some(Overlay::PortScanResult(Box::new(PortScanView { record, top: 0 })));
    tap(&mut state, KeyCode::PageDown);
    let Some(Overlay::PortScanResult(view)) = &state.overlay else { panic!("still open") };
    assert_eq!(view.top, 2);
    tap(&mut state, KeyCode::Up);
    tap(&mut state, KeyCode::Home);
    let Some(Overlay::PortScanResult(view)) = &state.overlay else { panic!("still open") };
    assert_eq!(view.top, 0);
}
