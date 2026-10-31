//! Sampling processes, inspecting one, and filtering the list.

use super::*;

#[test]
fn samples_preserve_identity_selection_and_bound_history_without_inheriting_reused_pids() {
    let (mut state, now) = state();
    open(&mut state);
    sample(&mut state, vec![process(1, 10, 100), process(2, 10, 200)], now);
    assert_eq!(state.activity.selected, Some(ProcessKey { pid: 2, start: 10 }));
    key(&mut state, KeyCode::Down);
    for index in 0..80 {
        sample(&mut state, vec![process(1, 10, 100 + index), process(2, 10, 200)], now);
    }
    let key = ProcessKey { pid: 1, start: 10 };
    assert_eq!(state.activity.selected, Some(key));
    assert_eq!(state.activity.histories[&key].values.len(), HISTORY);
    assert_eq!(state.activity.histories[&key].peak, 179);
    assert_eq!(state.activity.growth(key), 79);
    sample(&mut state, vec![process(1, 20, 500)], now);
    assert!(!state.activity.histories.contains_key(&key));
    assert_eq!(state.activity.selected, Some(ProcessKey { pid: 1, start: 20 }));
    assert_eq!(state.activity.growth(ProcessKey { pid: 1, start: 20 }), 0);
}

#[test]
fn inspection_denial_keeps_monitoring_and_only_an_explicit_retry_requests_admin() {
    let (mut state, now) = state();
    open(&mut state);
    sample(&mut state, vec![process(2, 10, 200)], now);
    let effects = key(&mut state, KeyCode::Enter);
    assert!(matches!(&effects[..], [Effect::Activity { request: ActivityRequest::Regions { admin: false, .. }, .. }]));
    let id = state.activity.pending.as_ref().unwrap().0;
    receive(&mut state, id, Err("Process protected".into()), now);
    sample(&mut state, vec![process(2, 10, 210)], now);
    assert_eq!(state.activity.error.as_deref(), Some("Process protected"));
    assert!(state.activity.retry.is_some());
    let effects = key(&mut state, KeyCode::Char('a'));
    assert!(matches!(&effects[..], [Effect::Activity { request: ActivityRequest::Regions { admin: true, .. }, .. }]));
    key(&mut state, KeyCode::Esc);
    assert_eq!(state.activity.page, ActivityPage::Memory);
    assert!(state.activity.retry.is_none());
}

#[test]
fn memory_macos_protects_offers_no_administrator_and_neither_does_borg_running_as_one() {
    let (mut state, now) = state();
    open(&mut state);
    let mut protected = process(2, 10, 200);
    protected.protection = Some(MemoryProtection::System);
    sample(&mut state, vec![protected], now);
    key(&mut state, KeyCode::Enter);
    let fail = |state: &mut AppState, request: ActivityRequest| {
        state.activity.request(request);
        let id = state.activity.pending.as_ref().unwrap().0;
        receive(state, id, Err("refused".into()), now);
        state.activity.retry.is_some()
    };
    let process = ProcessKey { pid: 2, start: 10 };
    assert_eq!(state.activity.protection(), Some(MemoryProtection::System));
    assert!(!fail(&mut state, ActivityRequest::Integrity { process, admin: false }), "memory stays protected");
    assert!(fail(&mut state, ActivityRequest::Files { process, admin: false }), "files list as administrator");
    state.activity.sample.administrator = true;
    assert!(!fail(&mut state, ActivityRequest::Files { process, admin: false }), "Borg already is one");
}

#[test]
fn closing_pausing_and_disabling_leave_no_monitor_timer_or_stale_data_updates() {
    let (mut state, now) = state();
    open(&mut state);
    let id = state.activity.pending.as_ref().unwrap().0;
    receive(&mut state, id + 1, Ok(ActivityData::Sample(SystemSample::default())), now);
    assert!(state.activity.pending.is_some());
    receive(&mut state, id, Ok(ActivityData::Sample(SystemSample::default())), now);
    assert_eq!(wake_at(&state), Some(now + REFRESH));
    key(&mut state, KeyCode::Char(' '));
    assert!(wake_at(&state).is_none());
    key(&mut state, KeyCode::Esc);
    sync(&mut state, now);
    assert!(state.activity.pending.is_none());
    assert!(wake_at(&state).is_none());
    receive(&mut state, id, Ok(ActivityData::Sample(SystemSample { total: 999, ..Default::default() })), now);
    assert_eq!(state.activity.sample.total, 0);
    state.settings.preferences.system_plugin = false;
    assert!(open(&mut state).is_empty());
    assert!(state.overlay.is_none());
}

#[test]
fn tree_filter_handles_cycles_and_keeps_only_matching_processes() {
    let (mut state, now) = state();
    open(&mut state);
    let mut a = process(1, 10, 100);
    let mut b = process(2, 10, 200);
    a.parent = Some(2);
    b.parent = Some(1);
    sample(&mut state, vec![a, b], now);
    key(&mut state, KeyCode::Char('t'));
    assert_eq!(state.activity.rows.len(), 2);
    key(&mut state, KeyCode::Char('/'));
    key(&mut state, KeyCode::Char('2'));
    key(&mut state, KeyCode::Enter);
    assert_eq!(state.activity.rows.len(), 1);
    assert_eq!(state.activity.selected.unwrap().pid, 2);
    let effects = update(&mut state, Msg::Paste("rm -rf /".into()), now);
    assert!(!effects.iter().any(|e| matches!(e, Effect::Write(_))));
}

#[test]
fn finishing_filter_during_initial_sample_keeps_focus_and_selects_when_data_arrives() {
    let (mut state, now) = state();
    open(&mut state);
    for code in [KeyCode::Char('/'), KeyCode::Char('2'), KeyCode::Enter] {
        route_key(&mut state, &code.into(), now);
    }
    assert!(!state.activity.filtering);
    assert!(!state.activity.preview_focused);
    route_key(&mut state, &KeyCode::Enter.into(), now);
    sample(&mut state, vec![process(2, 10, 200)], now);
    assert!(state.activity.process().is_some());
    route_key(&mut state, &KeyCode::Enter.into(), now);
    assert_eq!(state.activity.list, ActivityPage::Regions);
    route_key(&mut state, &KeyCode::Char('/').into(), now);
    assert!(state.activity.search_form.is_some());
}

#[test]
fn changing_process_cancels_its_inspection_and_clears_previous_memory() {
    let (mut state, now) = state();
    open(&mut state);
    sample(&mut state, vec![process(2, 10, 200), process(3, 10, 100)], now);
    let process = state.activity.selected.unwrap();
    let request = ActivityRequest::Regions { process, admin: false };
    state.activity.request(request.clone());
    let id = state.activity.pending.as_ref().unwrap().0;
    state.activity.error = Some("Access denied".into());
    state.activity.retry = Some(request);
    state.activity.snapshot = Some(MemorySnapshot { process, address: 100, bytes: vec![1], captured: 1 });
    route_key(&mut state, &KeyCode::Down.into(), now);
    assert_eq!(state.activity.selected.unwrap().pid, 3);
    assert!(state.activity.pending.is_none());
    assert!(state.activity.snapshot.is_none());
    assert!(state.activity.error.is_none() && state.activity.retry.is_none());
    receive(&mut state, id, Err("Late reply".into()), now);
    assert!(state.activity.error.is_none());
}

#[test]
fn the_process_filter_types_moves_keeps_and_clears() {
    let (mut state, now) = state();
    open(&mut state);
    sample(&mut state, vec![process(21, 1, 300), process(22, 1, 200), process(3, 1, 100)], now);
    for code in [KeyCode::Char('/'), KeyCode::Char('2')] {
        key(&mut state, code);
    }
    assert_eq!(state.activity.rows.len(), 2);
    key(&mut state, KeyCode::Down);
    assert!(state.activity.filtering, "arrows move without leaving the filter");
    assert_eq!(state.activity.selected.unwrap().pid, 22);
    key(&mut state, KeyCode::Enter);
    assert!(!state.activity.filtering && state.activity.filter == "2");
    key(&mut state, KeyCode::Esc);
    assert!(state.activity.filter.is_empty() && state.activity.side.is_some(), "Esc clears before closing");
    assert_eq!(state.activity.rows.len(), 3);
    for code in [KeyCode::Char('/'), KeyCode::Char('3'), KeyCode::Esc] {
        key(&mut state, code);
    }
    assert!(!state.activity.filtering && state.activity.filter.is_empty());
    key(&mut state, KeyCode::Esc);
    assert!(state.activity.side.is_none());
}
