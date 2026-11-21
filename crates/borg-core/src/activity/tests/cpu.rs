//! CPU use averaged over the last few seconds, and hiding the processes that used none.

use super::*;
use crate::activity::cpu::CPU_WINDOW;
use std::time::Duration;

const SECOND: Duration = Duration::from_secs(1);
/// What Linux shows for a lightly busy process in a second it got one 10 ms tick.
const TICK: u32 = 90;

fn busy(pid: u32, cpu: Option<u32>) -> ProcessInfo {
    ProcessInfo { cpu, ..process(pid, 10, 100) }
}

fn sorted_by_cpu(state: &mut AppState) {
    key(state, KeyCode::Char('c'));
    assert_eq!(state.activity.sort, ActivitySort::Cpu);
    assert!(state.activity.reverse, "the busiest first");
}

fn listed(state: &AppState) -> Vec<u32> {
    state.activity.rows.iter().map(|row| state.activity.sample.processes[row.index].key.pid).collect()
}

#[test]
fn the_cpu_order_holds_still_while_single_samples_jump() {
    let (mut state, start) = state();
    open(&mut state);
    sorted_by_cpu(&mut state);
    for second in 0..20u32 {
        let flicker = if second % 2 == 0 { 3 * TICK } else { 0 };
        let now = start + SECOND * second;
        sample(&mut state, vec![busy(1, Some(flicker)), busy(2, Some(TICK)), busy(3, Some(0))], now);
        assert_eq!(listed(&state), [1, 2, 3], "second {second}");
        let shown: Vec<_> =
            listed(&state).into_iter().map(|pid| state.activity.cpu(ProcessKey { pid, start: 10 })).collect();
        assert!(shown.windows(2).all(|pair| pair[0] >= pair[1]), "the column agrees with the order: {shown:?}");
    }
    let flicker = state.activity.cpu(ProcessKey { pid: 1, start: 10 }).unwrap();
    assert!(flicker > TICK && flicker < 3 * TICK, "a flickering process shows its average: {flicker}");
}

#[test]
fn the_average_follows_a_process_and_forgets_it_once_it_ends() {
    let (mut state, start) = state();
    open(&mut state);
    let first = ProcessKey { pid: 1, start: 10 };
    sample(&mut state, vec![busy(1, None)], start);
    assert_eq!(state.activity.cpu(first), None, "nothing to average before CPU is measured");
    sample(&mut state, vec![busy(1, Some(10_000))], start + SECOND);
    assert_eq!(state.activity.cpu(first), Some(10_000), "the first reading starts the average");
    sample(&mut state, vec![busy(1, Some(0))], start + SECOND + CPU_WINDOW);
    let faded = state.activity.cpu(first).unwrap();
    assert!((3_600..=3_800).contains(&faded), "a window later, a third is left: {faded}");
    sample(&mut state, vec![busy(1, Some(0))], start + SECOND + CPU_WINDOW * 10);
    assert_eq!(state.activity.cpu(first), Some(0));
    sample(&mut state, vec![busy(2, Some(500))], start + SECOND * 60);
    assert_eq!(state.activity.cpu(first), None, "an ended process drops out");
    sample(&mut state, vec![ProcessInfo { cpu: Some(200), ..process(2, 20, 100) }], start + SECOND * 61);
    assert_eq!(state.activity.cpu(ProcessKey { pid: 2, start: 20 }), Some(200), "a reused PID starts afresh");
}

#[test]
fn the_overview_cpu_is_averaged_like_the_processes() {
    let (mut state, start) = state();
    open(&mut state);
    let mut system = |cores: Vec<u32>, now: Instant| {
        state.activity.request(ActivityRequest::Sample);
        let id = state.activity.pending.as_ref().unwrap().0;
        receive(&mut state, id, Ok(ActivityData::Sample(SystemSample { cpu: cores, ..Default::default() })), now);
        state.activity.system_cpu()
    };
    assert_eq!(system(Vec::new(), start), None);
    assert_eq!(system(vec![8_000, 4_000], start), Some(6_000));
    let next = system(vec![0, 0], start + SECOND).unwrap();
    assert!(next > 3_000 && next < 6_000, "one idle second moves it part of the way: {next}");
}

#[test]
fn h_hides_processes_with_no_cpu_use_in_the_window_and_shows_them_again() {
    let (mut state, start) = state();
    open(&mut state);
    sample(&mut state, vec![busy(1, Some(0)), busy(2, Some(500)), busy(3, None), busy(4, Some(TICK))], start);
    key(&mut state, KeyCode::Char('h'));
    assert!(state.activity.hide_idle);
    assert_eq!(listed(&state), [2, 3, 4], "a process not measured yet stays");
    assert_eq!(state.activity.idle_hidden(), 1);
    let mut now = start;
    while state.activity.rows.len() > 2 {
        now += SECOND;
        assert!(now - start < CPU_WINDOW * 10, "a process that stopped using CPU is hidden in time");
        sample(&mut state, vec![busy(1, Some(0)), busy(2, Some(500)), busy(3, Some(300)), busy(4, Some(0))], now);
    }
    assert!(now - start > CPU_WINDOW, "it shows its last use for a few seconds");
    assert_eq!(listed(&state), [2, 3]);
    assert_eq!(state.activity.idle_hidden(), 2);
    key(&mut state, KeyCode::Char('/'));
    key(&mut state, KeyCode::Char('2'));
    assert_eq!(listed(&state), [2], "the filter and hiding the idle add up");
    key(&mut state, KeyCode::Esc);
    key(&mut state, KeyCode::Char('h'));
    assert_eq!(listed(&state), [1, 2, 3, 4]);
    assert_eq!(state.activity.idle_hidden(), 0);
}

#[test]
fn the_tree_keeps_the_processes_a_busy_one_descends_from() {
    let (mut state, start) = state();
    open(&mut state);
    let child = |pid, parent, cpu| ProcessInfo { parent: Some(parent), ..busy(pid, Some(cpu)) };
    sample(
        &mut state,
        vec![busy(1, Some(0)), child(2, 1, 0), child(3, 2, 400), child(4, 1, 0), busy(5, Some(0))],
        start,
    );
    key(&mut state, KeyCode::Char('t'));
    key(&mut state, KeyCode::Char('h'));
    assert_eq!(listed(&state), [1, 2, 3]);
    let depths: Vec<_> = state.activity.rows.iter().map(|row| row.depth).collect();
    assert_eq!(depths, [0, 1, 2], "the busy process keeps its place in the tree");
    assert_eq!(state.activity.idle_hidden(), 2);
}

#[test]
fn revealing_an_idle_process_shows_the_idle_ones_again() {
    let (mut state, start) = state();
    open(&mut state);
    sample(&mut state, vec![busy(1, Some(0)), busy(2, Some(500))], start);
    key(&mut state, KeyCode::Char('h'));
    assert_eq!(listed(&state), [2]);
    state.activity.reveal(1, false);
    sample(&mut state, vec![busy(1, Some(0)), busy(2, Some(500))], start + SECOND);
    assert!(!state.activity.hide_idle);
    assert_eq!(state.activity.selected.map(|key| key.pid), Some(1));
}
