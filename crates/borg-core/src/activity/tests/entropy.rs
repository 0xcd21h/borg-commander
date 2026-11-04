//! The entropy scan of every process: the dialog, hits as they come in, stopping, going on,
//! the administrator retry, and landing on a hit's pages.

use super::*;
use borg_view::entropy::{Entropy, Tally};
use crossterm::event::KeyModifiers;

const PAGE: u64 = 4096;

fn region(start: u64, pages: u64, permissions: &str) -> MemoryRegion {
    MemoryRegion { start, end: start + pages * PAGE, permissions: permissions.into(), ..Default::default() }
}

fn hit(pid: u32, region: MemoryRegion, high: u32) -> EntropyHit {
    let tally = Tally { blocks: 10, high, highest: Entropy::HIGH, first_high: Some(region.start + 3 * PAGE) };
    EntropyHit {
        process: ProcessKey { pid, start: 10 },
        name: format!("worker-{pid}"),
        kind: HitKind::of(&region),
        region,
        tally,
    }
}

fn found(hits: Vec<EntropyHit>) -> EntropyReport {
    EntropyReport { scanned: 1, hits, ..Default::default() }
}

/// The monitor open on three processes, with a scan of their code started from the dialog.
fn scanning() -> (AppState, Instant, u64) {
    let (mut state, now) = state();
    open(&mut state);
    sample(&mut state, vec![process(2, 10, 200), process(3, 10, 100), process(4, 10, 50)], now);
    state.activity.paused = true;
    key(&mut state, KeyCode::Char('e'));
    assert!(matches!(state.overlay, Some(crate::Overlay::EntropyScan(EntropyDialog { results: false, .. }))));
    let effects = crate::overlays::on_key(&mut state, &KeyCode::Char('c').into(), now);
    let [Effect::EntropyScan { id, scope: EntropyScope::Code, processes, admin: false }] = &effects[..] else {
        panic!("expected a scan, got {effects:?}");
    };
    assert_eq!(processes.len(), 3, "every sampled process");
    assert_eq!((state.activity.list, state.activity.page), (ActivityPage::Entropy, ActivityPage::EntropyDetails));
    (state, now, *id)
}

#[test]
fn hits_rank_code_first_and_densest_first_and_the_cursor_stays_on_its_hit() {
    let (mut state, _, id) = scanning();
    let own = region(0x10000, 4, "rw-");
    receive_entropy(
        &mut state.activity,
        id,
        Ok(found(vec![hit(2, own.clone(), 9), hit(3, region(0x2000, 2, "r-x"), 2)])),
        false,
    );
    let scan = state.activity.entropy.as_ref().unwrap();
    assert_eq!(scan.report.hits[0].kind, HitKind::Code, "code ranks above memory the process filled");
    assert!(scan.running);
    key(&mut state, KeyCode::Down);
    assert_eq!(state.activity.entropy.as_ref().unwrap().selected().unwrap().region, own);

    let packed = hit(4, region(0x4000, 2, "r-x"), 8);
    assert!(packed.packed() && !hit(4, region(0x4000, 2, "r-x"), 6).packed());
    receive_entropy(&mut state.activity, id, Ok(found(vec![packed])), true);
    let scan = state.activity.entropy.as_ref().unwrap();
    let order: Vec<_> = scan.report.hits.iter().map(|hit| hit.process.pid).collect();
    assert_eq!(order, [4, 3, 2], "the larger share of dense pages first");
    assert_eq!(scan.selected().unwrap().region, own, "the cursor follows its hit");
    assert_eq!((scan.report.scanned, scan.running), (2, false));
    receive_entropy(&mut state.activity, id + 1, Ok(found(vec![hit(9, own.clone(), 9)])), true);
    assert_eq!(state.activity.entropy.as_ref().unwrap().report.hits.len(), 3, "another scan's parts are ignored");

    let mut report = EntropyReport::default();
    report.merge(found((0..HIT_LIMIT as u32 + 5).map(|pid| hit(pid, own.clone(), 1)).collect()));
    assert_eq!((report.hits.len(), report.dropped), (HIT_LIMIT, 5));
}

#[test]
fn a_stopped_scan_goes_on_with_what_it_did_not_reach_and_unread_processes_go_to_the_administrator() {
    let (mut state, now, id) = scanning();
    let control_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
    let effects = route_key(&mut state, &control_c, now);
    assert!(matches!(&effects[..], [Effect::StopEntropyScan]));
    let (three, four) = (ProcessKey { pid: 3, start: 10 }, ProcessKey { pid: 4, start: 10 });
    let report = EntropyReport { scanned: 1, unreadable: vec![three], remaining: vec![four], ..Default::default() };
    receive_entropy(&mut state.activity, id, Ok(report), true);
    assert!(route_key(&mut state, &control_c, now).is_empty(), "nothing runs to stop");

    let effects = key(&mut state, KeyCode::Char('n'));
    let [Effect::EntropyScan { id, processes, admin: false, .. }] = &effects[..] else { panic!("{effects:?}") };
    assert_eq!(processes, &[four]);
    receive_entropy(&mut state.activity, *id, Ok(found(Vec::new())), true);

    let effects = key(&mut state, KeyCode::Char('a'));
    let [Effect::EntropyScan { id, processes, admin: true, .. }] = &effects[..] else { panic!("{effects:?}") };
    assert_eq!(processes, &[three]);
    let scan = state.activity.entropy.as_ref().unwrap();
    assert!(scan.running && scan.report.unreadable.is_empty());
    receive_entropy(&mut state.activity, *id, Err("cancelled".into()), true);
    let scan = state.activity.entropy.as_ref().unwrap();
    assert_eq!(scan.report.unreadable, [three], "a failed helper leaves them to try again");
    assert_eq!(state.activity.error.as_deref(), Some("cancelled"));
    assert_eq!(scan.report.scanned, 2);

    state.activity.sample.administrator = true;
    assert!(key(&mut state, KeyCode::Char('a')).is_empty(), "an administrator already read what it could");
    let effects = key(&mut state, KeyCode::Char('r'));
    assert!(matches!(&effects[..], [Effect::EntropyScan { admin: false, .. }]), "R scans again");
    assert_eq!(state.activity.entropy.as_ref().unwrap().report, EntropyReport::default());
}

#[test]
fn enter_on_a_hit_opens_its_page_map_at_the_first_dense_page_and_esc_goes_back() {
    let (mut state, now, id) = scanning();
    let start = 0x40000;
    let code = MemoryRegion { path: Some("/opt/tool".into()), ..region(start, 64, "r-x") };
    receive_entropy(&mut state.activity, id, Ok(found(vec![hit(3, code.clone(), 9)])), true);
    let effects = key(&mut state, KeyCode::Enter);
    assert!(
        matches!(&effects[..], [Effect::Activity { request: ActivityRequest::Regions { process, .. }, .. }] if process.pid == 3),
        "{effects:?}"
    );
    assert_eq!(state.activity.selected.map(|process| process.pid), Some(3));
    assert_eq!(state.activity.list, ActivityPage::Regions);

    let id = state.activity.pending.as_ref().unwrap().0;
    let heap = region(0x10000, 4, "rw-");
    receive(&mut state, id, Ok(ActivityData::Regions(vec![heap, code.clone()])), now);
    assert_eq!(state.activity.selected_region(), Some(&code), "the hit's region, its group and file opened");
    assert!(state.activity.map && state.activity.preview_focused);
    let effects = sync(&mut state, now + PREVIEW_DELAY);
    let [Effect::Activity { id, request: ActivityRequest::Census { region, .. } }] = &effects[..] else {
        panic!("expected a census, got {effects:?}");
    };
    assert_eq!(region.start, start);
    let process = ProcessKey { pid: 3, start: 10 };
    let pages = vec![CensusPage::unread(PageClass::Code); 32];
    let census = MemoryCensus { process, start, end: code.end, page: PAGE, stride: 2, pages, residency: false };
    receive(&mut state, *id, Ok(ActivityData::Census(census)), now);
    assert_eq!(state.activity.census_cursor, 1, "the page holding the first dense page sampled");

    key(&mut state, KeyCode::Esc);
    key(&mut state, KeyCode::Esc);
    assert_eq!((state.activity.list, state.activity.page), (ActivityPage::Entropy, ActivityPage::EntropyDetails));
    assert!(!state.activity.map);
    key(&mut state, KeyCode::Esc);
    assert_eq!(state.activity.list, ActivityPage::Processes);
    key(&mut state, KeyCode::Char('e'));
    let effects = crate::overlays::on_key(&mut state, &KeyCode::Char('l').into(), now);
    assert!(effects.is_empty());
    assert_eq!(state.activity.list, ActivityPage::Entropy, "L shows the last hits again");
}

#[test]
fn a_hit_whose_process_ended_says_so() {
    let (mut state, now, id) = scanning();
    receive_entropy(&mut state.activity, id, Ok(found(vec![hit(7, region(0x1000, 1, "r-x"), 9)])), true);
    sample(&mut state, vec![process(2, 10, 200)], now);
    assert!(key(&mut state, KeyCode::Enter).is_empty());
    assert_eq!(state.activity.list, ActivityPage::Entropy);
    assert!(state.activity.error.as_deref().is_some_and(|error| error.contains("worker-7")));
}
