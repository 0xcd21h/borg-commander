//! The system summary above the process list: how it fits the pane, how its figures read, each
//! core's average, and S hiding it.

use super::*;
use std::time::Duration;

const SECOND: Duration = Duration::from_secs(1);
/// A system that says how busy its disks are and counts its threads, as Linux does.
const LINUX: SummaryExtras = SummaryExtras { disk_busy: true, threads: true };
/// A system that does neither, as macOS does.
const MACOS: SummaryExtras = SummaryExtras { disk_busy: false, threads: false };

fn system(state: &mut AppState, sample: SystemSample, now: Instant) {
    state.activity.request(ActivityRequest::Sample);
    let id = state.activity.pending.as_ref().unwrap().0;
    receive(state, id, Ok(ActivityData::Sample(sample)), now);
}

#[test]
fn sixteen_cores_take_four_rows_of_bars_beside_two_columns_of_figures() {
    let layout = summary_layout(16, LINUX, 64, 30).unwrap();
    assert_eq!(
        layout,
        SummaryLayout { cores: CoreRows::Bars { columns: 4, rows: 4 }, two_columns: true, figure_rows: 5 }
    );
    assert_eq!(layout.rows(), 9);
    assert_eq!(summary_layout(16, MACOS, 64, 30).unwrap().figure_rows, 4);
}

#[test]
fn bars_fill_their_rows_so_no_column_stands_half_empty() {
    assert_eq!(summary_layout(6, LINUX, 64, 30).unwrap().cores, CoreRows::Bars { columns: 3, rows: 2 });
    assert_eq!(summary_layout(2, LINUX, 64, 30).unwrap().cores, CoreRows::Bars { columns: 2, rows: 1 });
}

#[test]
fn a_short_pane_trades_the_bars_for_a_strip_then_drops_the_cores_then_the_summary() {
    assert_eq!(summary_layout(16, LINUX, 64, 27).unwrap().cores, CoreRows::Bars { columns: 4, rows: 4 });
    assert_eq!(summary_layout(16, LINUX, 64, 26).unwrap().cores, CoreRows::Strip { rows: 1 });
    assert_eq!(summary_layout(16, LINUX, 64, 17).unwrap().cores, CoreRows::Hidden);
    assert_eq!(summary_layout(16, LINUX, 64, 14), None);
}

#[test]
fn many_cores_show_as_a_strip_even_with_room_for_bars() {
    assert_eq!(summary_layout(64, LINUX, 64, 60).unwrap().cores, CoreRows::Strip { rows: 2 });
    assert_eq!(summary_layout(0, LINUX, 64, 60).unwrap().cores, CoreRows::Hidden, "no cores before the first sample");
}

#[test]
fn a_narrow_pane_stacks_the_figures_under_the_bars_and_a_very_narrow_one_keeps_the_overview() {
    assert_eq!(
        summary_layout(4, MACOS, 40, 40).unwrap(),
        SummaryLayout { cores: CoreRows::Bars { columns: 2, rows: 2 }, two_columns: false, figure_rows: 7 }
    );
    assert_eq!(summary_layout(4, LINUX, 40, 40).unwrap().figure_rows, 9, "the disk bar and threads take rows");
    assert_eq!(summary_layout(4, LINUX, 29, 40), None);
    let wide = summary_layout(8, LINUX, 58, 26).unwrap();
    assert!(wide.two_columns, "a 120-column terminal's pane has two columns");
    assert_eq!(wide.cores, CoreRows::Bars { columns: 4, rows: 2 }, "and room for the cores' bars");
}

#[test]
fn the_load_reads_as_a_share_of_the_cores_with_its_trend() {
    let load = |one, fifteen| LoadAverage { one, five: (one + fifteen) / 2, fifteen };
    let busy = load_reading(load(1416, 680), 16).unwrap();
    assert_eq!(busy, LoadReading { share: 8850, trend: LoadTrend::Rising, waiting: None });
    assert_eq!(load_text(busy), "89%, rising");
    let calm = load_reading(load(300, 330), 4).unwrap();
    assert_eq!((calm.trend, load_text(calm)), (LoadTrend::Steady, "75%, steady".to_owned()));
    assert_eq!(load_reading(load(120, 290), 4).unwrap().trend, LoadTrend::Falling);
    assert_eq!(load_reading(load(1416, 680), 0), None, "no share before the cores are counted");
}

#[test]
fn a_load_above_the_cores_counts_the_processes_waiting_for_one() {
    let overloaded = load_reading(LoadAverage { one: 2214, five: 1890, fifteen: 1420 }, 16).unwrap();
    assert_eq!(overloaded.waiting, Some(7), "6.14 waiting rounds up");
    assert_eq!(load_text(overloaded), "138%, rising");
    assert_eq!(waiting_text(7), "~7 waiting");
    let full = load_reading(LoadAverage { one: 400, five: 400, fifteen: 400 }, 4).unwrap();
    assert_eq!(full.waiting, None, "every core busy is not yet a shortage");
    let barely = load_reading(LoadAverage { one: 401, five: 400, fifteen: 400 }, 4).unwrap();
    assert_eq!(barely.waiting, Some(1));
}

#[test]
fn the_uptime_and_threads_read_in_plain_units() {
    let time = |days: u64, hours: u64, minutes: u64, seconds: u64| {
        Duration::from_secs(((days * 24 + hours) * 60 + minutes) * 60 + seconds)
    };
    assert_eq!(uptime_text(time(16, 0, 50, 18)), "16 days");
    assert_eq!(uptime_text(time(3, 4, 12, 9)), "3 days, 4 hours");
    assert_eq!(uptime_text(time(1, 1, 0, 0)), "1 day, 1 hour");
    assert_eq!(uptime_text(time(0, 5, 2, 30)), "5 hours, 2 minutes");
    assert_eq!(uptime_text(time(0, 0, 12, 30)), "12 minutes");
    assert_eq!(uptime_text(time(0, 0, 0, 59)), "59 seconds");
    assert_eq!(threads_text(Threads { total: 517, running: 3 }), "517 · 3 running");
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
    assert_eq!(state.activity.summary_rows(pane), 5);
    assert_eq!(chrome_rows(&state.activity, ActivityPage::Processes, pane), crate::screens::CHROME_ROWS + 5);
    assert_eq!(chrome_rows(&state.activity, ActivityPage::Files, pane), crate::screens::CHROME_ROWS);
    key(&mut state, KeyCode::Char('s'));
    assert!(state.activity.hide_summary);
    assert_eq!(state.activity.summary_rows(pane), 0);
    assert_eq!(chrome_rows(&state.activity, ActivityPage::Processes, pane), crate::screens::CHROME_ROWS);
    key(&mut state, KeyCode::Char('S'));
    assert!(!state.activity.hide_summary);
}
