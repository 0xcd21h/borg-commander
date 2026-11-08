//! The process tree: folding its branches, and the selected process's family.

use super::*;

fn child(pid: u32, parent: u32, ram: u64) -> ProcessInfo {
    ProcessInfo { parent: Some(parent), ..process(pid, 1, ram) }
}

/// 1 started 2 and 4, 2 started 3, and 5 stands alone, listed by RAM as 1, 2, 3, 4, 5.
fn family_sample() -> Vec<ProcessInfo> {
    vec![process(1, 1, 500), child(2, 1, 400), child(3, 2, 300), child(4, 1, 200), process(5, 1, 100)]
}

fn shown(state: &AppState) -> Vec<(u32, usize, Branch)> {
    let activity = &state.activity;
    activity.rows.iter().map(|row| (activity.sample.processes[row.index].key.pid, row.depth, row.branch)).collect()
}

fn tree_of(processes: Vec<ProcessInfo>) -> (AppState, Instant) {
    let (mut state, now) = state();
    open(&mut state);
    sample(&mut state, processes, now);
    key(&mut state, KeyCode::Char('t'));
    (state, now)
}

fn select(state: &mut AppState, pid: u32) {
    key(state, KeyCode::Home);
    while state.activity.selected.map(|key| key.pid) != Some(pid) {
        key(state, KeyCode::Down);
    }
}

#[test]
fn left_folds_a_branch_away_and_counts_it_and_right_unfolds_it() {
    let (mut state, _) = tree_of(family_sample());
    assert_eq!(
        shown(&state),
        [(1, 0, Branch::Open), (2, 1, Branch::Open), (3, 2, Branch::Leaf), (4, 1, Branch::Leaf), (5, 0, Branch::Leaf)]
    );
    select(&mut state, 2);
    key(&mut state, KeyCode::Left);
    assert_eq!(shown(&state)[1], (2, 1, Branch::Folded(1)));
    assert_eq!(shown(&state).len(), 4);
    assert_eq!(state.activity.selected.unwrap().pid, 2);
    key(&mut state, KeyCode::Right);
    assert_eq!(shown(&state)[1], (2, 1, Branch::Open));
    assert_eq!(state.activity.selected.unwrap().pid, 2, "unfolding stays on the process");
    key(&mut state, KeyCode::Right);
    assert_eq!(state.activity.selected.unwrap().pid, 3, "Right on an open branch steps into it");
}

#[test]
fn left_on_a_leaf_folds_the_process_that_started_it_and_selects_that_one() {
    let (mut state, _) = tree_of(family_sample());
    select(&mut state, 3);
    key(&mut state, KeyCode::Left);
    assert_eq!(state.activity.selected.unwrap().pid, 2);
    assert_eq!(shown(&state)[1], (2, 1, Branch::Folded(1)));
    key(&mut state, KeyCode::Left);
    assert_eq!(state.activity.selected.unwrap().pid, 1);
    assert_eq!(shown(&state), [(1, 0, Branch::Folded(3)), (5, 0, Branch::Leaf)], "the fold counts every level");
    key(&mut state, KeyCode::Left);
    assert_eq!(state.activity.selected.unwrap().pid, 1, "a root has nothing above it to fold");
}

#[test]
fn folds_last_through_new_samples_and_end_with_their_process() {
    let (mut state, now) = tree_of(family_sample());
    select(&mut state, 2);
    key(&mut state, KeyCode::Left);
    sample(&mut state, family_sample(), now);
    assert_eq!(shown(&state)[1], (2, 1, Branch::Folded(1)));
    let without_two = family_sample().into_iter().filter(|process| process.key.pid != 2).collect();
    sample(&mut state, without_two, now);
    sample(&mut state, family_sample(), now);
    assert_eq!(shown(&state)[1], (2, 1, Branch::Open), "a new process 2 starts unfolded");
}

#[test]
fn the_flat_list_and_a_filter_do_not_fold() {
    let (mut state, _) = tree_of(family_sample());
    key(&mut state, KeyCode::Char('t'));
    select(&mut state, 2);
    key(&mut state, KeyCode::Left);
    assert_eq!(shown(&state).len(), 5);
    assert!(shown(&state).iter().all(|&(_, depth, branch)| depth == 0 && branch == Branch::Leaf));
    key(&mut state, KeyCode::Char('t'));
    for code in [KeyCode::Char('/'), KeyCode::Char('2'), KeyCode::Enter, KeyCode::Left] {
        key(&mut state, code);
    }
    assert!(shown(&state).iter().all(|&(_, _, branch)| branch == Branch::Leaf));
    key(&mut state, KeyCode::Esc);
    assert_eq!(shown(&state).len(), 5, "nothing was folded while filtered");
}

#[test]
fn the_family_runs_from_the_first_process_down_to_all_the_selected_one_started_in_pid_order() {
    let mut processes = family_sample();
    processes.extend([child(8, 3, 10), child(6, 3, 10), child(7, 6, 10)]);
    let (mut state, _) = tree_of(processes);
    select(&mut state, 3);
    let family: Vec<(u32, usize, Kin)> = state
        .activity
        .family()
        .iter()
        .map(|relative| (state.activity.sample.processes[relative.index].key.pid, relative.depth, relative.kin))
        .collect();
    assert_eq!(
        family,
        [
            (1, 0, Kin::Ancestor),
            (2, 1, Kin::Ancestor),
            (3, 2, Kin::Selected),
            (6, 3, Kin::Descendant),
            (7, 4, Kin::Descendant),
            (8, 3, Kin::Descendant),
        ]
    );
}

#[test]
fn a_loop_of_parents_ends_the_family() {
    let (mut state, _) = tree_of(vec![child(9, 10, 200), child(10, 9, 100)]);
    select(&mut state, 9);
    let pids: Vec<u32> = state
        .activity
        .family()
        .iter()
        .map(|relative| state.activity.sample.processes[relative.index].key.pid)
        .collect();
    assert_eq!(pids, [10, 9]);
    assert_eq!(shown(&state).len(), 2, "the tree lists both processes once");
}
