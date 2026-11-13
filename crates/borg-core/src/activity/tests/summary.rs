//! The system summary above the process list: how it fits the pane, how its figures read, each
//! core's average, and S hiding it.

use super::*;
use std::time::Duration;

const SECOND: Duration = Duration::from_secs(1);

fn system(state: &mut AppState, sample: SystemSample, now: Instant) {
    state.activity.request(ActivityRequest::Sample);
    let id = state.activity.pending.as_ref().unwrap().0;
    receive(state, id, Ok(ActivityData::Sample(sample)), now);
}

#[test]
fn sixteen_cores_take_four_rows_of_bars_beside_two_columns_of_figures() {
    let layout = summary_layout(16, true, 64, 30).unwrap();
    assert_eq!(
        layout,
        SummaryLayout { cores: CoreRows::Bars { columns: 4, rows: 4 }, two_columns: true, figure_rows: 4 }
    );
    assert_eq!(layout.rows(), 8);
}

#[test]
fn bars_fill_their_rows_so_no_column_stands_half_empty() {
    assert_eq!(summary_layout(6, true, 64, 30).unwrap().cores, CoreRows::Bars { columns: 3, rows: 2 });
    assert_eq!(summary_layout(2, true, 64, 30).unwrap().cores, CoreRows::Bars { columns: 2, rows: 1 });
}

#[test]
fn a_short_pane_trades_the_bars_for_a_strip_then_drops_the_cores_then_the_summary() {
    assert_eq!(summary_layout(16, true, 64, 24).unwrap().cores, CoreRows::Bars { columns: 4, rows: 4 });
    assert_eq!(summary_layout(16, true, 64, 21).unwrap().cores, CoreRows::Strip { rows: 1 });
    assert_eq!(summary_layout(16, true, 64, 12).unwrap().cores, CoreRows::Hidden);
    assert_eq!(summary_layout(16, true, 64, 11), None);
}

#[test]
fn many_cores_show_as_a_strip_even_with_room_for_bars() {
    assert_eq!(summary_layout(64, true, 64, 60).unwrap().cores, CoreRows::Strip { rows: 2 });
    assert_eq!(summary_layout(0, true, 64, 60).unwrap().cores, CoreRows::Hidden, "no cores before the first sample");
}

#[test]
fn a_narrow_pane_stacks_the_figures_under_the_bars_and_a_very_narrow_one_keeps_the_overview() {
    assert_eq!(
        summary_layout(4, false, 40, 40).unwrap(),
        SummaryLayout { cores: CoreRows::Bars { columns: 2, rows: 2 }, two_columns: false, figure_rows: 7 }
    );
    assert_eq!(summary_layout(4, true, 40, 40).unwrap().figure_rows, 8, "the disk bar takes a row of its own");
    assert_eq!(summary_layout(4, true, 29, 40), None);
    assert!(summary_layout(8, true, 58, 26).unwrap().two_columns, "a 120-column terminal's pane has two columns");
}

#[test]
fn the_load_uptime_and_tasks_read_as_the_summary_shows_them() {
    assert_eq!(load_text(LoadAverage { one: 1416, five: 959, fifteen: 680 }), "14.16 9.59 6.80");
    assert_eq!(uptime_text(Duration::from_secs(16 * 86_400 + 50 * 60 + 18)), "16 days, 00:50:18");
    assert_eq!(uptime_text(Duration::from_secs(86_400 + 3_600)), "1 day, 01:00:00");
    assert_eq!(uptime_text(Duration::from_secs(59)), "00:00:59");
    assert_eq!(tasks_text(125, Some(Threads { total: 517, running: 3 })), "125 · 517 threads · 3 running");
    assert_eq!(tasks_text(1, Some(Threads { total: 1, running: 1 })), "1 · 1 thread · 1 running");
    assert_eq!(tasks_text(773, None), "773");
}

#[test]
fn each_core_is_averaged_like_the_system_and_starts_afresh_when_the_cores_change() {
    let (mut state, start) = state();
    open(&mut state);
    let mut cores = |cpu: Vec<u32>, now: Instant| {
        system(&mut state, SystemSample { cpu, ..Default::default() }, now);
        state.activity.core_cpu()
    };
    assert_eq!(cores(vec![8_000, 0], start), [8_000, 0]);
    let next = cores(vec![0, 8_000], start + SECOND);
    assert!(next[0] > 4_000 && next[0] < 8_000, "an idle second moves a busy core part of the way: {next:?}");
    assert!(next[1] > 0 && next[1] < 4_000, "a busy second moves an idle core part of the way: {next:?}");
    assert_eq!(cores(vec![1_000, 2_000, 3_000], start + SECOND * 2), [1_000, 2_000, 3_000]);
}

#[test]
fn s_hides_the_summary_and_gives_its_rows_back_to_the_list() {
    let (mut state, start) = state();
    open(&mut state);
    let disk = DiskActivity { read: 4_096, written: 0, busy: Some(500) };
    system(&mut state, SystemSample { cpu: vec![100; 4], disk: Some(disk), ..Default::default() }, start);
    let pane = Rect::new(0, 0, 60, 30);
    assert_eq!(state.activity.summary_rows(pane), 4);
    assert_eq!(chrome_rows(&state.activity, ActivityPage::Processes, pane), crate::screens::CHROME_ROWS + 4);
    assert_eq!(chrome_rows(&state.activity, ActivityPage::Files, pane), crate::screens::CHROME_ROWS);
    key(&mut state, KeyCode::Char('s'));
    assert!(state.activity.hide_summary);
    assert_eq!(state.activity.summary_rows(pane), 0);
    assert_eq!(chrome_rows(&state.activity, ActivityPage::Processes, pane), crate::screens::CHROME_ROWS);
    key(&mut state, KeyCode::Char('S'));
    assert!(!state.activity.hide_summary);
}
