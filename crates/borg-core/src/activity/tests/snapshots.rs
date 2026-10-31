//! Capturing memory, comparing captures and moving through them.

use super::*;

#[test]
fn snapshots_compare_only_the_same_process_region_and_export_to_the_file_panel() {
    let (mut state, now) = state();
    open(&mut state);
    sample(&mut state, vec![process(2, 10, 200)], now);
    let process = state.activity.selected.unwrap();
    let region = MemoryRegion {
        start: 4096,
        end: 8192,
        permissions: "r--".into(),
        name: "heap".into(),
        resident: None,
        private: None,
        shared: None,
        swap: None,
        ..Default::default()
    };
    let request = ActivityRequest::Capture { process, region, admin: false, seek: false };
    for value in [1, 2] {
        state.activity.request(request.clone());
        let id = state.activity.pending.as_ref().unwrap().0;
        receive(
            &mut state,
            id,
            Ok(ActivityData::Snapshot(MemorySnapshot {
                process,
                address: 4096,
                bytes: vec![value; 64],
                captured: value.into(),
            })),
            now,
        );
    }
    assert_eq!(state.activity.previous.as_ref().unwrap().bytes[0], 1);
    assert_eq!(state.activity.snapshot.as_ref().unwrap().bytes[0], 2);
    state.activity.list = ActivityPage::Regions;
    state.activity.preview_focused = true;
    key(&mut state, KeyCode::Char('d'));
    assert!(state.activity.diff);
    assert!(
        matches!(&key(&mut state,KeyCode::Char('e'))[..],[Effect::ExportMemory {path,bytes}] if path.starts_with("/work") && bytes==&vec![2;64])
    );
    let pending = state.activity.request(request);
    assert_eq!(pending.len(), 1);
    let id = state.activity.pending.as_ref().unwrap().0;
    receive(
        &mut state,
        id,
        Ok(ActivityData::Snapshot(MemorySnapshot {
            process: ProcessKey { start: 11, ..process },
            address: 4096,
            bytes: vec![3],
            captured: 3,
        })),
        now,
    );
    assert!(state.activity.previous.is_none());
}

#[test]
fn zero_rows_fold_unless_the_compared_capture_or_the_selected_match_differs_there() {
    use SnapshotRow::{Bytes, Zeros};
    let (mut state, _) = state();
    let activity = &mut state.activity;
    let process = ProcessKey { pid: 2, start: 10 };
    let mut bytes = vec![0u8; 16 * 6];
    bytes[0] = 1;
    bytes[16 * 5] = 2;
    let snapshot = MemorySnapshot { process, address: 0x1000, bytes: bytes.clone(), captured: 1 };
    activity.snapshot = Some(snapshot.clone());
    assert_eq!(activity.snapshot_rows(16), [Bytes(0), Zeros { start: 16, len: 64 }, Bytes(80)]);
    let mut old = bytes;
    old[40] = 9;
    activity.previous = Some(MemorySnapshot { bytes: old, ..snapshot });
    activity.diff = true;
    assert_eq!(activity.snapshot_rows(16), [Bytes(0), Bytes(16), Bytes(32), Zeros { start: 48, len: 32 }, Bytes(80)]);
    activity.diff = false;
    activity.from_search = true;
    let region = MemoryRegion { start: 0x1000, end: 0x2000, permissions: "rw-".into(), ..Default::default() };
    let hit = MemoryHit { address: 0x1000 + 50, length: 2, encoding: "hex".into(), region, ..Default::default() };
    activity.search_report = Some(MemorySearchReport {
        process,
        hits: vec![hit],
        scanned: 0,
        total: 0,
        skipped: 0,
        next: None,
        cancelled: false,
        strings: None,
    });
    assert_eq!(activity.snapshot_rows(16), [Bytes(0), Zeros { start: 16, len: 32 }, Bytes(48), Bytes(64), Bytes(80)]);
}

#[test]
fn windows_move_through_a_region_and_recapture_reads_the_shown_window_again() {
    let (mut state, now) = state();
    open(&mut state);
    state.activity.set_grouped(false);
    sample(&mut state, vec![process(2, 10, 200)], now);
    state.activity.paused = true;
    key(&mut state, KeyCode::Enter);
    let id = state.activity.pending.as_ref().unwrap().0;
    let start = 0x10000;
    let window = SNAPSHOT_LIMIT as u64;
    let region = MemoryRegion { start, end: start + 4 * window, permissions: "rw-".into(), ..Default::default() };
    receive(&mut state, id, Ok(ActivityData::Regions(vec![region])), now);
    let capture = |effects: &[Effect]| match effects {
        [Effect::Activity { request: ActivityRequest::Capture { region, seek, .. }, .. }] => {
            Some((region.start, *seek))
        }
        _ => None,
    };
    assert_eq!(capture(&sync(&mut state, now + PREVIEW_DELAY)), Some((start, true)));
    let id = state.activity.pending.as_ref().unwrap().0;
    let shown = start + window;
    let process = state.activity.selected.unwrap();
    let bytes = vec![1; SNAPSHOT_LIMIT];
    receive(
        &mut state,
        id,
        Ok(ActivityData::Snapshot(MemorySnapshot { process, address: shown, bytes, captured: 1 })),
        now,
    );
    key(&mut state, KeyCode::Tab);
    assert!(state.activity.preview_focused);
    assert_eq!(capture(&key(&mut state, KeyCode::Char('>'))), Some((shown + window, true)));
    state.activity.pending = None;
    assert_eq!(capture(&key(&mut state, KeyCode::Char('<'))), Some((start, false)));
    state.activity.pending = None;
    assert_eq!(capture(&key(&mut state, KeyCode::Char('r'))), Some((shown, false)));
}

#[test]
fn scrolling_up_keeps_the_viewport_until_the_cursor_reaches_its_top() {
    let (mut state, now) = state();
    open(&mut state);
    sample(&mut state, (1..=100).map(|pid| process(pid, 1, 100)).collect(), now);
    for page in [ActivityPage::Processes, ActivityPage::Regions, ActivityPage::Search, ActivityPage::Files] {
        state.activity.page = page;
        state.activity.set_grouped(false);
        let region = MemoryRegion {
            start: 0,
            end: 1,
            permissions: "r--".into(),
            name: "heap".into(),
            resident: Some(1),
            private: None,
            shared: None,
            swap: None,
            ..Default::default()
        };
        state.activity.set_regions(vec![region.clone(); 100]);
        state.activity.files = (0..100)
            .map(|i| ProcessFile {
                descriptor: i.to_string(),
                access: "r".into(),
                kind: "REG".into(),
                name: "file".into(),
                size: None,
                offset: None,
            })
            .collect();
        state.activity.search_report = Some(MemorySearchReport {
            process: state.activity.selected.unwrap(),
            hits: vec![
                MemoryHit { address: 0, length: 1, encoding: "UTF-8".into(), region, ..Default::default() };
                100
            ],
            scanned: 100,
            total: 100,
            skipped: 0,
            next: None,
            cancelled: false,
            strings: None,
        });
        state.activity.move_by(isize::MAX, 10, 60);
        assert_eq!(state.activity.list_start(page, 10), 90);
        for _ in 0..9 {
            state.activity.move_by(-1, 10, 60);
            assert_eq!(state.activity.list_start(page, 10), 90);
        }
        state.activity.move_by(-1, 10, 60);
        assert_eq!(state.activity.list_start(page, 10), 89);
    }
}
