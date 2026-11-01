//! The memory map: regions, groups, open files, the page census and integrity findings.

use super::*;

#[test]
fn integrity_findings_replace_the_map_preview_their_evidence_and_go_back_with_esc() {
    let (mut state, now) = state();
    open(&mut state);
    sample(&mut state, vec![process(2, 10, 200)], now);
    state.activity.paused = true;
    key(&mut state, KeyCode::Enter);
    let id = state.activity.pending.as_ref().unwrap().0;
    let region = MemoryRegion { start: 0x10000, end: 0x20000, permissions: "r-x".into(), ..Default::default() };
    receive(&mut state, id, Ok(ActivityData::Regions(vec![region.clone()])), now);
    let effects = key(&mut state, KeyCode::Char('i'));
    assert!(matches!(
        &effects[..],
        [Effect::Activity { request: ActivityRequest::Integrity { admin: false, .. }, .. }]
    ));
    assert_eq!(state.activity.search_progress, Some((0, 0, 0)));
    let effects = on_key(&mut state, &KeyEvent::new(KeyCode::Char('c'), crossterm::event::KeyModifiers::CONTROL));
    assert!(matches!(&effects[..], [Effect::CancelMemorySearch]));

    let finding = |kind, address: Option<u64>| Finding {
        kind,
        severity: Severity::Alert,
        detail: "detail".into(),
        region: address.map(|_| region.clone()),
        address,
        length: 8,
    };
    let report = IntegrityReport {
        process: state.activity.selected.unwrap(),
        findings: vec![finding(FindingKind::CodeChanged, Some(0x11010)), finding(FindingKind::ProgramGone, None)],
        compared: 0x10000,
        uncompared: 0,
        scanned: 0,
        unbacked: 0,
        unreadable: 4096,
        complete: true,
    };
    let id = state.activity.pending.as_ref().unwrap().0;
    receive(&mut state, id, Ok(ActivityData::Integrity(report)), now);
    assert_eq!((state.activity.list, state.activity.page), (ActivityPage::Integrity, ActivityPage::Snapshot));
    assert!(state.activity.search_progress.is_none());
    assert!(state.activity.retry.is_some(), "unread memory can be checked again as administrator");
    assert_eq!(state.activity.marked(), Some(0x11010..0x11018));
    let effects = sync(&mut state, now + PREVIEW_DELAY);
    assert!(
        matches!(&effects[..], [Effect::Activity { request: ActivityRequest::Capture { region, seek: false, .. }, .. }] if region.start == 0x10ff0)
    );
    let later = now + PREVIEW_DELAY * 2;
    route_key(&mut state, &KeyCode::Down.into(), later);
    assert_eq!((state.activity.finding_cursor, state.activity.marked()), (1, None));
    assert!(sync(&mut state, later + PREVIEW_DELAY).is_empty(), "the whole process has nothing to capture");
    let effects = key(&mut state, KeyCode::Char('r'));
    assert!(matches!(&effects[..], [Effect::Activity { request: ActivityRequest::Integrity { .. }, .. }]));
    state.activity.pending = None;
    route_key(&mut state, &KeyCode::Esc.into(), later);
    assert_eq!(state.activity.list, ActivityPage::Regions);
}

#[test]
fn a_process_fills_memory_no_file_backs_or_it_may_write_unless_it_never_wrote_any() {
    let region = |permissions: &str, path: Option<&str>, dirty: Option<u64>, swap: Option<u64>| MemoryRegion {
        start: 0x1000,
        end: 0x5000,
        permissions: permissions.into(),
        path: path.map(Into::into),
        dirty,
        swap,
        ..Default::default()
    };
    assert!(region("rw-", None, Some(0x1000), Some(0)).own());
    assert!(region("rw-", Some("/app/borg"), Some(0x1000), Some(0)).own());
    assert!(!region("r-x", Some("/app/borg"), Some(0x1000), Some(0)).own());
    // The shared libraries' symbol tables: read-only, unnamed, never written.
    assert!(!region("r--", None, Some(0), Some(0)).own());
    assert!(region("rw-", None, Some(0), Some(0x1000)).own());
    // Linux says nothing about written pages.
    assert!(region("r--", None, None, None).own());
}

#[test]
fn maps_drill_into_the_list_pane_and_debounce_bounded_previews_without_changing_focus() {
    let (mut state, now) = state();
    open(&mut state);
    state.activity.set_grouped(false);
    sample(&mut state, vec![process(2, 10, 200)], now);
    state.activity.paused = true;
    let selected = state.activity.selected;
    let effects = key(&mut state, KeyCode::Enter);
    assert!(matches!(&effects[..], [Effect::Activity { request: ActivityRequest::Regions { .. }, .. }]));
    let region = |start, permissions: &str| MemoryRegion {
        start,
        end: start + 1_000_000,
        permissions: permissions.into(),
        name: format!("region-{start}"),
        resident: None,
        private: None,
        shared: None,
        swap: None,
        ..Default::default()
    };
    let id = state.activity.pending.as_ref().unwrap().0;
    receive(
        &mut state,
        id,
        Ok(ActivityData::Regions(vec![region(0x1000, "r--"), region(0x200000, "---"), region(0x400000, "rw-")])),
        now,
    );
    assert_eq!(state.activity.list, ActivityPage::Regions);
    assert_eq!(state.activity.page, ActivityPage::Snapshot);
    assert!(!state.activity.preview_focused);
    assert!(sync(&mut state, now + PREVIEW_DELAY / 2).is_empty());
    let effects = sync(&mut state, now + PREVIEW_DELAY);
    assert!(
        matches!(&effects[..], [Effect::Activity { request: ActivityRequest::Capture { region, admin: false, .. }, .. }] if region.start == 0x1000)
    );
    let stale = state.activity.pending.as_ref().unwrap().0;
    let later = now + PREVIEW_DELAY * 2;
    route_key(&mut state, &KeyCode::Down.into(), later);
    assert!(state.activity.snapshot.is_none());
    assert!(sync(&mut state, later + PREVIEW_DELAY).is_empty());
    assert!(state.activity.pending.is_none());
    route_key(&mut state, &KeyCode::Down.into(), later + PREVIEW_DELAY);
    let latest = later + PREVIEW_DELAY * 2;
    let effects = sync(&mut state, latest);
    assert!(
        matches!(&effects[..], [Effect::Activity { request: ActivityRequest::Capture { region, .. }, .. }] if region.start == 0x400000)
    );
    let current = state.activity.pending.as_ref().unwrap().0;
    let snapshot =
        |address| MemorySnapshot { process: selected.unwrap(), address, bytes: b"Borg memory".to_vec(), captured: 1 };
    receive(&mut state, stale, Ok(ActivityData::Snapshot(snapshot(0x1000))), latest);
    assert!(state.activity.snapshot.is_none());
    receive(&mut state, current, Ok(ActivityData::Snapshot(snapshot(0x400000))), latest);
    assert!(!state.activity.preview_focused);
    assert_eq!(state.activity.list, ActivityPage::Regions);
    route_key(&mut state, &KeyCode::Tab.into(), latest);
    assert!(state.activity.preview_focused);
    route_key(&mut state, &KeyCode::Down.into(), latest);
    assert_eq!(state.activity.region_cursor, 2);
    route_key(&mut state, &KeyCode::Esc.into(), latest);
    assert_eq!(state.activity.list, ActivityPage::Regions);
    route_key(&mut state, &KeyCode::Esc.into(), latest);
    assert_eq!(state.activity.list, ActivityPage::Processes);
    assert_eq!(state.activity.selected, selected);
    assert_eq!(state.activity.region_cursor, 2);
}

#[test]
fn the_page_map_takes_a_census_of_the_region_and_opens_the_page_under_its_cursor() {
    const PAGE: u64 = 4096;
    let (mut state, now) = state();
    open(&mut state);
    state.activity.set_grouped(false);
    sample(&mut state, vec![process(2, 10, 200)], now);
    state.activity.paused = true;
    key(&mut state, KeyCode::Enter);
    let id = state.activity.pending.as_ref().unwrap().0;
    let start = 0x10000;
    let heap = MemoryRegion { start, end: start + 64 * PAGE, permissions: "rw-".into(), ..Default::default() };
    let guard = MemoryRegion { start: heap.end, end: heap.end + PAGE, permissions: "---".into(), ..Default::default() };
    let code = MemoryRegion { start: 0x400000, end: 0x401000, permissions: "r-x".into(), ..Default::default() };
    receive(&mut state, id, Ok(ActivityData::Regions(vec![heap.clone(), guard, code.clone()])), now);
    key(&mut state, KeyCode::Char('m'));
    let effects = sync(&mut state, now + PREVIEW_DELAY);
    let [Effect::Activity { id, request: ActivityRequest::Census { region, mapped, admin: false, .. } }] = &effects[..]
    else {
        panic!("expected a census, got {effects:?}");
    };
    assert_eq!(region.start, start);
    assert_eq!(*mapped, [(heap.start, heap.end), (code.start, code.end)], "only readable mappings count");
    let process = state.activity.selected.unwrap();
    let pages = [vec![PageClass::Zero; 32], vec![PageClass::Text; 32]].concat();
    let census = MemoryCensus { process, start, end: heap.end, page: PAGE, stride: 1, pages, residency: true };
    receive(&mut state, *id, Ok(ActivityData::Census(census)), now);
    assert!(state.activity.census.is_some() && !state.activity.preview_waiting());
    key(&mut state, KeyCode::Tab);
    assert!(state.activity.preview_focused);
    key(&mut state, KeyCode::Right);
    assert_eq!(state.activity.census_cursor, 1);
    key(&mut state, KeyCode::End);
    assert_eq!(state.activity.census_cursor, 63);
    let effects = key(&mut state, KeyCode::Enter);
    assert!(
        matches!(&effects[..], [Effect::Activity { request: ActivityRequest::Capture { region, seek: false, .. }, .. }] if region.start == start + 63 * PAGE),
        "{effects:?}"
    );
    assert!(!state.activity.map, "the page opens in bytes");
    key(&mut state, KeyCode::Char('m'));
    assert!(state.activity.map && state.activity.census.is_none(), "M brings the map back for a fresh census");
}

#[test]
fn groups_expand_without_reading_aggregate_ranges_and_tabs_keep_the_process() {
    let (mut state, now) = state();
    open(&mut state);
    sample(&mut state, vec![process(2, 10, 200)], now);
    let process = state.activity.selected.unwrap();
    key(&mut state, KeyCode::Enter);
    let id = state.activity.pending.as_ref().unwrap().0;
    let region = MemoryRegion {
        start: 0x1000,
        end: 0x2000,
        permissions: "rw-".into(),
        name: "MALLOC_SMALL".into(),
        resident: Some(1024),
        private: None,
        shared: None,
        swap: None,
        ..Default::default()
    };
    receive(
        &mut state,
        id,
        Ok(ActivityData::Regions(vec![
            region.clone(),
            MemoryRegion {
                start: 0x3000,
                end: 0x4000,
                permissions: "---".into(),
                name: "MALLOC guard".into(),
                ..region
            },
        ])),
        now,
    );
    assert_eq!(
        state.activity.region_rows(),
        vec![RegionRow::Group(MemoryGroup::Heap), RegionRow::Group(MemoryGroup::Reserved)]
    );
    assert!(state.activity.capture_request().is_none());
    key(&mut state, KeyCode::Enter);
    assert_eq!(state.activity.selected_region().unwrap().start, 0x1000);
    assert!(state.activity.capture_request().is_some());
    key(&mut state, KeyCode::Left);
    assert_eq!(state.activity.selected_group(), Some(MemoryGroup::Heap));
    assert!(state.activity.capture_request().is_none());
    let effects = key(&mut state, KeyCode::Char('2'));
    assert!(
        matches!(&effects[..], [Effect::Activity { request: ActivityRequest::Files { process: p, admin: false }, .. }] if *p == process)
    );
    let id = state.activity.pending.as_ref().unwrap().0;
    receive(&mut state, id, Ok(ActivityData::Files(ProcessFiles { entries: vec![], skipped: 1 })), now);
    assert_eq!(state.activity.list, ActivityPage::Files);
    assert_eq!(state.activity.page, ActivityPage::FileDetails);
    key(&mut state, KeyCode::Char('1'));
    assert_eq!(state.activity.list, ActivityPage::Regions);
    assert_eq!(state.activity.selected_group(), Some(MemoryGroup::Heap));
    assert_eq!(state.activity.selected, Some(process));
    assert!(key(&mut state, KeyCode::Char('2')).is_empty());
    assert_eq!(state.activity.files_skipped, 1);
}

#[test]
fn a_regular_open_file_previews_under_its_details_scrolls_and_opens_in_the_viewer() {
    let (mut state, now) = state();
    open(&mut state);
    sample(&mut state, vec![process(2, 10, 200)], now);
    let send = |state: &mut AppState, code| update(state, Msg::Key(KeyEvent::from(code)), now);
    send(&mut state, KeyCode::Char('2'));
    let id = state.activity.pending.as_ref().unwrap().0;
    let file = |descriptor: &str, kind: &str, name: &str| ProcessFile {
        descriptor: descriptor.into(),
        access: "r".into(),
        kind: kind.into(),
        name: name.into(),
        size: Some(2400),
        offset: Some(0),
    };
    let entries = vec![file("0", "CHR", "/dev/ttys001"), file("3", "REG", "/home/me/notes.txt")];
    receive(&mut state, id, Ok(ActivityData::Files(ProcessFiles { entries, skipped: 0 })), now);
    let peeks = |effects: &[Effect]| {
        effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::Peek { aside: crate::Aside::OpenFile, path, .. } => Some(path.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert!(peeks(&send(&mut state, KeyCode::Char('x'))).is_empty(), "a terminal has nothing to preview");
    let path = PathBuf::from("/home/me/notes.txt");
    assert_eq!(peeks(&send(&mut state, KeyCode::Down)), vec![path.clone()]);
    assert!(peeks(&send(&mut state, KeyCode::Down)).is_empty(), "asked once while it loads");
    assert!(state.activity.file_preview_loading(), "the load is kept without a quick view");
    let text = (0..100).map(|n| format!("line {n}\n")).collect::<String>();
    let pager = borg_view::Pager::for_path(std::sync::Arc::new(text.into_bytes()), &path, 40, 10).unwrap();
    update(
        &mut state,
        Msg::Peeked(crate::Peeked {
            aside: crate::Aside::OpenFile,
            path: path.clone(),
            at: None,
            peek: crate::Peek::Text(pager),
        }),
        now,
    );
    let offset = |state: &AppState| match &state.activity.file_preview {
        Some(crate::Peeked { peek: crate::Peek::Text(pager), .. }) => pager.offset(),
        _ => panic!("no text preview"),
    };
    assert_eq!(offset(&state), 0);
    assert!(!state.activity.file_preview_loading());
    send(&mut state, KeyCode::Tab);
    send(&mut state, KeyCode::Down);
    assert!(offset(&state) > 0, "the focused preview scrolls");
    assert_eq!(state.activity.file_cursor, 1, "the selection stays");
    assert!(matches!(&send(&mut state, KeyCode::F(3))[..], [Effect::View(shown)] if *shown == path));
    send(&mut state, KeyCode::Tab);
    send(&mut state, KeyCode::Up);
    assert!(state.activity.file_preview.is_none());
}

#[test]
fn regions_group_by_purpose_rank_by_footprint_and_collapse_library_segments_per_file() {
    let (mut state, now) = state();
    open(&mut state);
    let mut program = process(2, 10, 200);
    program.executable = Some("/app/borg".into());
    sample(&mut state, vec![program], now);
    key(&mut state, KeyCode::Enter);
    let region = |start: u64, name: &str, path: Option<&str>, dirty: u64| MemoryRegion {
        start,
        end: start + 0x4000,
        permissions: "r-x".into(),
        name: path.map_or(name.into(), |p| format!("{name} · {p}")),
        resident: Some(0x4000),
        dirty: Some(dirty),
        swap: Some(0),
        path: path.map(Into::into),
        ..Default::default()
    };
    let regions = vec![
        region(0x1000, "__TEXT", Some("/app/borg"), 0),
        region(0x10000, "__TEXT", Some("/usr/lib/libc.dylib"), 0),
        region(0x20000, "__DATA", Some("/usr/lib/libc.dylib"), 64),
        region(0x30000, "__TEXT", Some("/usr/lib/libz.dylib"), 128),
        MemoryRegion { permissions: "rw-".into(), ..region(0x40000, "MALLOC_SMALL", None, 4096) },
        MemoryRegion { permissions: "---".into(), ..region(0x50000, "STACK GUARD", None, 0) },
    ];
    let id = state.activity.pending.as_ref().unwrap().0;
    receive(&mut state, id, Ok(ActivityData::Regions(regions.clone())), now);
    let groups: Vec<_> = state.activity.region_tree().iter().map(|item| item.row).collect();
    assert_eq!(
        groups,
        [MemoryGroup::Heap, MemoryGroup::Libraries, MemoryGroup::Program, MemoryGroup::Reserved].map(RegionRow::Group)
    );
    assert_eq!(MemoryTotals::of(&state.activity.regions).footprint, Some(4096 + 64 + 128));
    let libraries = state.activity.region_tree()[1].clone();
    assert_eq!((libraries.files, libraries.totals.regions, libraries.totals.footprint), (2, 3, Some(192)));
    key(&mut state, KeyCode::Down);
    key(&mut state, KeyCode::Right);
    assert_eq!(state.activity.selected_file().map(|f| state.activity.file_name(f)), Some("libz.dylib"));
    key(&mut state, KeyCode::Down);
    key(&mut state, KeyCode::Enter);
    assert_eq!(state.activity.selected_region().unwrap().kind(), "__TEXT");
    assert_eq!(state.activity.region_tree()[state.activity.region_cursor].depth, 2);
    key(&mut state, KeyCode::Down);
    assert_eq!(state.activity.selected_region().unwrap().kind(), "__DATA");
    let id =
        state.activity.request(ActivityRequest::Regions { process: state.activity.selected.unwrap(), admin: false });
    let id = match &id[..] {
        [Effect::Activity { id, .. }] => *id,
        _ => unreachable!(),
    };
    receive(&mut state, id, Ok(ActivityData::Regions(regions)), now);
    assert_eq!(state.activity.selected_region().unwrap().start, 0x20000);
    key(&mut state, KeyCode::Left);
    assert_eq!(state.activity.selected_file().map(|f| state.activity.file_name(f)), Some("libc.dylib"));
    assert!(state.activity.capture_request().is_none());
    key(&mut state, KeyCode::Left);
    assert_eq!(state.activity.selected_group(), Some(MemoryGroup::Libraries));
    assert_eq!(state.activity.region_rows().len(), 4);
    key(&mut state, KeyCode::Char('g'));
    assert_eq!(state.activity.region_rows().len(), 6);
    let tagged = |name: &str| MemoryRegion { name: name.into(), permissions: "rw-".into(), ..Default::default() };
    assert_eq!(tagged("Memory Tag 253").group(None), MemoryGroup::Heap);
    assert_eq!(tagged("Memory Tag 22").group(None), MemoryGroup::Other);
}
