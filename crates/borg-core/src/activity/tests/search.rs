//! Searching a process's memory and ranking the strings found.

use super::*;

#[test]
fn strings_modes_search_without_a_query_and_the_mode_cycles_both_ways() {
    let (mut state, now) = state();
    open(&mut state);
    sample(&mut state, vec![process(2, 10, 200)], now);
    let send = |state: &mut AppState, code| update(state, Msg::Key(KeyEvent::from(code)), now);
    send(&mut state, KeyCode::Tab);
    send(&mut state, KeyCode::Char('/'));
    send(&mut state, KeyCode::Enter);
    assert!(state.activity.error.is_some(), "a text search needs a query");
    send(&mut state, KeyCode::Tab);
    let mode = |state: &AppState| state.activity.search_form.as_ref().unwrap().mode;
    send(&mut state, KeyCode::Left);
    assert_eq!(mode(&state), MemorySearchMode::Indicators);
    send(&mut state, KeyCode::Left);
    assert_eq!(mode(&state), MemorySearchMode::Strings);
    send(&mut state, KeyCode::Right);
    assert_eq!(mode(&state), MemorySearchMode::Indicators);
    let effects = send(&mut state, KeyCode::Enter);
    assert!(
        effects.iter().any(|e| matches!(e, Effect::Activity { request: ActivityRequest::Search { mode: MemorySearchMode::Indicators, query, .. }, .. } if query.is_empty())),
        "{effects:?}"
    );
}

#[test]
fn pane_focus_search_paste_and_snapshot_hits_never_reach_the_shell_or_live_memory() {
    let (mut state, now) = state();
    open(&mut state);
    sample(&mut state, vec![process(2, 10, 200)], now);
    let send = |state: &mut AppState, code| update(state, Msg::Key(KeyEvent::from(code)), now);
    send(&mut state, KeyCode::Tab);
    assert!(state.activity.preview_focused);
    assert_eq!(state.activity.page, ActivityPage::Memory);
    let process = state.activity.selected.unwrap();
    let snapshot = MemorySnapshot { process, address: 4096, bytes: b"--Borg--".to_vec(), captured: 1 };
    state.activity.snapshot = Some(snapshot.clone());
    send(&mut state, KeyCode::Char('/'));
    assert!(state.activity.search_form.is_some());
    let effects = update(&mut state, Msg::Paste("Borg".into()), now);
    assert!(!effects.iter().any(|e| matches!(e, Effect::Write(_))));
    send(&mut state, KeyCode::Tab);
    assert!(state.activity.preview_focused);
    assert_eq!(state.activity.search_form.as_ref().unwrap().selected, 1);
    state.activity.search_form.as_mut().unwrap().snapshot_only = true;
    let effects = send(&mut state, KeyCode::Enter);
    assert!(effects.iter().any(|e| matches!(e, Effect::Activity { request: ActivityRequest::Search { snapshot: Some(_), query, .. }, .. } if query == "Borg")));
    let id = state.activity.pending.as_ref().unwrap().0;
    update(&mut state, Msg::MemorySearchProgress { id: id + 1, scanned: 99, total: 100, hits: 2 }, now);
    assert_eq!(state.activity.search_progress, Some((0, 0, 0)));
    let region = MemoryRegion {
        start: 4096,
        end: 4104,
        permissions: "r--".into(),
        name: "Snapshot".into(),
        resident: None,
        private: None,
        shared: None,
        swap: None,
        ..Default::default()
    };
    receive(
        &mut state,
        id,
        Ok(ActivityData::Search(MemorySearchReport {
            process,
            hits: vec![MemoryHit { address: 4098, length: 4, encoding: "UTF-8".into(), region, ..Default::default() }],
            scanned: 8,
            total: 8,
            skipped: 0,
            next: None,
            cancelled: false,
            strings: None,
        })),
        now,
    );
    let effects = send(&mut state, KeyCode::Enter);
    assert!(!effects.iter().any(|e| matches!(e, Effect::Activity { request: ActivityRequest::Capture { .. }, .. })));
    assert_eq!(state.activity.page, ActivityPage::Snapshot);
    assert_eq!(state.activity.snapshot, Some(snapshot));
    send(&mut state, KeyCode::Esc);
    assert_eq!(state.activity.list, ActivityPage::Search);
    assert!(!state.activity.preview_focused);
    assert!(crate::actions::perform(&mut state, crate::Action::Delete, now).is_empty());
    assert!(state.overlay.is_none());
    send(&mut state, KeyCode::Esc);
    assert_eq!(state.activity.list, ActivityPage::Processes);
    send(&mut state, KeyCode::Esc);
    assert!(state.activity.side.is_none());
}

#[test]
fn partial_search_offers_admin_retry_and_cancellation_keeps_results() {
    let (mut state, now) = state();
    open(&mut state);
    sample(&mut state, vec![process(2, 10, 200)], now);
    state.activity.preview_focused = true;
    state.activity.page = ActivityPage::Memory;
    key(&mut state, KeyCode::Char('/'));
    state.activity.search_form.as_mut().unwrap().input = crate::LineInput::new("Borg");
    key(&mut state, KeyCode::Enter);
    let id = state.activity.pending.as_ref().unwrap().0;
    let process = state.activity.selected.unwrap();
    receive(
        &mut state,
        id,
        Ok(ActivityData::Search(MemorySearchReport {
            process,
            hits: vec![],
            scanned: 128,
            total: 8192,
            skipped: 4096,
            next: Some(8192),
            cancelled: true,
            strings: None,
        })),
        now,
    );
    assert!(state.activity.retry.is_some());
    let effects = key(&mut state, KeyCode::Char('n'));
    assert!(matches!(
        &effects[..],
        [Effect::Activity { request: ActivityRequest::Search { start: Some(8192), .. }, .. }]
    ));
    let effects = on_key(&mut state, &KeyEvent::new(KeyCode::Char('c'), crossterm::event::KeyModifiers::CONTROL));
    assert!(matches!(&effects[..], [Effect::CancelMemorySearch]));
    assert!(state.activity.search_report.is_some());
}

#[test]
fn strings_read_like_words_or_filler_and_rank_indicators_and_memory_only_first() {
    assert_eq!(string_score("plain words"), 100);
    assert_eq!(string_score("getUserName"), 100, "camelCase reads as words");
    assert_eq!(string_score("x8Kq2!zP"), 0);
    assert!(string_score("UH-AWAVAT") < string_score("Hello, world"));
    for filler in ["AAAAAAAA", "@@@@@@@@", "ababababab", "%s %s %s"] {
        assert!(junk_string(filler), "{filler}");
    }
    assert!(!junk_string("0123456789abcdef"));
    let hit = |text: &str, indicator, in_file| MemoryHit {
        text: Some(text.into()),
        indicator,
        in_file: Some(in_file),
        ..Default::default()
    };
    let mut hits = vec![
        hit("x8Kq2!zP", None, false),
        hit("reading the config", None, true),
        hit("session started", None, false),
        hit("/usr/lib/libssl.so.3", Some(Indicator::Path), true),
        hit("https://shop.example.com/api", Some(Indicator::Url), false),
        hit("token=abc123", Some(Indicator::Secret), false),
    ];
    rank_strings(&mut hits);
    let order: Vec<_> = hits.iter().map(|hit| hit.text.as_deref().unwrap()).collect();
    assert_eq!(
        order,
        [
            "token=abc123",
            "https://shop.example.com/api",
            "/usr/lib/libssl.so.3",
            "session started",
            "x8Kq2!zP",
            "reading the config"
        ]
    );
}

#[test]
fn continued_strings_add_up_and_c_visits_each_copy() {
    let (mut state, now) = state();
    open(&mut state);
    sample(&mut state, vec![process(2, 10, 200)], now);
    state.activity.preview_focused = true;
    state.activity.page = ActivityPage::Memory;
    key(&mut state, KeyCode::Char('/'));
    state.activity.search_form.as_mut().unwrap().mode = MemorySearchMode::Strings;
    key(&mut state, KeyCode::Enter);
    let process = state.activity.selected.unwrap();
    let region = |start: u64| MemoryRegion {
        start,
        end: start + 0x1000,
        permissions: "rw-".into(),
        name: format!("heap {start:x}"),
        ..Default::default()
    };
    let report = |address: u64, next| {
        let hit = MemoryHit {
            address,
            length: 15,
            encoding: "ASCII".into(),
            region: region(address & !0xfff),
            text: Some("session started".into()),
            copies: 1,
            places: vec![(address, 0)],
            in_file: Some(false),
            ..Default::default()
        };
        let strings = StringsTally {
            found: 1,
            distinct: 1,
            memory_only: 1,
            regions: vec![region(address & !0xfff)],
            ..Default::default()
        };
        MemorySearchReport {
            process,
            hits: vec![hit],
            scanned: 0x1000,
            total: 0x2000,
            skipped: 0,
            next,
            cancelled: false,
            strings: Some(strings),
        }
    };
    let id = state.activity.pending.as_ref().unwrap().0;
    receive(&mut state, id, Ok(ActivityData::Search(report(0x10010, Some(0x20000)))), now);
    key(&mut state, KeyCode::Char('n'));
    let id = state.activity.pending.as_ref().unwrap().0;
    receive(&mut state, id, Ok(ActivityData::Search(report(0x20020, None))), now);
    let shown = state.activity.search_report.as_ref().unwrap();
    assert_eq!(shown.hits.len(), 1, "the same string from both parts is one result");
    assert_eq!((shown.hits[0].copies, shown.hits[0].places.clone()), (2, vec![(0x10010, 0), (0x20020, 1)]));
    let tally = shown.strings.as_ref().unwrap();
    assert_eq!((tally.found, tally.distinct, tally.regions.len()), (2, 1, 2));
    state.activity.preview_focused = false;
    route_key(&mut state, &KeyCode::Char('c').into(), now);
    let hit = &state.activity.search_report.as_ref().unwrap().hits[0];
    assert_eq!((hit.address, hit.region.start), (0x20020, 0x20000));
    assert_eq!(state.activity.marked(), Some(0x20020..0x2002f));
    route_key(&mut state, &KeyCode::Char('c').into(), now);
    assert_eq!(state.activity.search_report.as_ref().unwrap().hits[0].address, 0x10010);
}
