use std::time::Duration;

use super::*;
use crate::{How, ToastEvent};

const SECOND: Duration = Duration::from_secs(1);

fn alt(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::ALT)
}

fn finish_copy(state: &mut AppState, id: u64, now: Instant) -> Vec<Effect> {
    let done = Outcome::Done { done: Totals { files: 1, bytes: 200 }, skipped: 0 };
    update(state, Msg::Job { id, event: Event::Finished(done) }, now)
}

fn toasts(state: &AppState, now: Instant) -> Vec<(u64, ToastEvent)> {
    crate::visible_toasts(state, now).iter().map(|toast| (toast.job, toast.event)).collect()
}

fn job(state: &AppState, id: u64) -> &crate::Job {
    state.jobs.iter().find(|job| job.id == id).expect("the job is kept")
}

fn selected(state: &AppState) -> Option<u64> {
    let Some(Overlay::Jobs(list)) = state.overlay else { return None };
    crate::shown_jobs(&state.jobs).get(list.selected).map(|job| job.id)
}

fn search_home(state: &mut AppState, query: &str) -> (Side, u64) {
    let request = crate::SearchRequest {
        root: "/home".into(),
        query: query.into(),
        hidden: false,
        options: crate::SearchOptions::default(),
        skip: None,
    };
    let effects = crate::results::start_search(state, request);
    let [Effect::Search { side, generation, .. }] = effects[..] else { panic!("{effects:?}") };
    (side, generation)
}

#[test]
fn a_job_toasts_once_it_outlasts_a_second_and_stays_in_the_panel_once_done() {
    let (mut state, now) = setup_with(vec![file("a")]);
    let id = start_copy(&mut state, now);
    assert!(toasts(&state, now).is_empty(), "no toast for a job that may end at once");
    assert_eq!(state.jobs_wake, Some(now + SECOND), "the screen wakes when the toast is due");
    assert_eq!(toasts(&state, now + SECOND), vec![(id, ToastEvent::Started)]);

    finish_copy(&mut state, id, now + 2 * SECOND);
    assert_eq!(toasts(&state, now + 2 * SECOND), vec![(id, ToastEvent::Ended(How::Done))], "the end takes its place");
    assert!(job(&state, id).folded, "a finished job starts folded");
    assert_eq!(
        job(&state, id).reveal,
        Some(crate::Reveal::Folder { side: Side::Right, dir: "/tmp".into(), focus: Some("a".into()) })
    );

    update(&mut state, Msg::Tick, now + 8 * SECOND);
    assert!(state.toasts.is_empty(), "the done toast goes after six seconds");
    assert_eq!(ended_with(&state, JobKind::Copy), Some("Copied 1 file"), "the job stays until dismissed");
}

#[test]
fn a_job_that_ends_within_a_second_only_toasts_its_end() {
    let (mut state, now) = setup_with(vec![file("a")]);
    let id = start_copy(&mut state, now);
    finish_copy(&mut state, id, now + SECOND / 2);
    assert_eq!(toasts(&state, now + SECOND), vec![(id, ToastEvent::Ended(How::Done))]);
}

#[test]
fn with_job_notifications_off_a_finished_job_leaves_a_notice_and_no_toast() {
    let (mut state, now) = setup_with(vec![file("a")]);
    crate::actions::perform(&mut state, Action::ToggleJobNotifications, now);
    assert!(!state.settings.preferences.job_notifications);
    let id = start_copy(&mut state, now);
    finish_copy(&mut state, id, now + 2 * SECOND);
    assert!(state.toasts.is_empty());
    assert_eq!(state.notice.as_deref(), Some("Copied 1 file"));
    assert_eq!(ended_with(&state, JobKind::Copy), Some("Copied 1 file"));

    state.notice = None;
    let (side, generation) = search_home(&mut state, "*.log");
    update(&mut state, Msg::Tick, now);
    update(&mut state, Msg::Listed { side, generation, error: None }, now);
    assert_eq!(ended_with(&state, JobKind::Search), Some("Found 0 items"));
    assert!(state.notice.is_none(), "the results show how a search ended");
}

#[test]
fn at_most_three_toasts_show_and_a_newer_one_pushes_the_oldest_out() {
    let (mut state, now) = setup_with(vec![file("a")]);
    let ids: Vec<u64> = (0..4).map(|_| crate::jobs::begin(&mut state, JobKind::Delete, "a".into(), now).0).collect();
    for (step, id) in ids.iter().enumerate() {
        crate::jobs::end(&mut state, *id, How::Done, "Deleted 1 file".into(), now + SECOND / 4 * step as u32);
    }
    let shown: Vec<u64> = toasts(&state, now + SECOND).into_iter().map(|(id, _)| id).collect();
    assert_eq!(shown, ids[1..].to_vec());
}

#[test]
fn the_panel_keeps_the_twenty_newest_finished_jobs() {
    let (mut state, now) = setup();
    let ids: Vec<u64> = (0..22).map(|_| crate::jobs::begin(&mut state, JobKind::Delete, "a".into(), now).0).collect();
    for (step, id) in ids.iter().enumerate() {
        crate::jobs::end(&mut state, *id, How::Done, "Deleted 1 file".into(), now + Duration::from_millis(step as u64));
    }
    let kept: Vec<u64> = state.jobs.iter().map(|job| job.id).collect();
    assert_eq!(kept, ids[2..].to_vec());
}

#[test]
fn the_jobs_panel_folds_unfolds_and_steers_the_selected_job() {
    let (mut state, now) = setup_with(vec![file("a"), file("b")]);
    let first = start_copy(&mut state, now);
    let second = start_copy(&mut state, now);
    press(&mut state, now, alt('j'));
    assert_eq!(selected(&state), Some(first), "running jobs come oldest first");

    press(&mut state, now, key(KeyCode::Char(' ')));
    assert!(!job(&state, first).folded, "Space unfolds");
    press(&mut state, now, key(KeyCode::Enter));
    assert!(job(&state, first).folded, "Enter folds again");
    press(&mut state, now, key(KeyCode::Right));
    assert!(!job(&state, first).folded);
    press(&mut state, now, key(KeyCode::Left));
    assert!(job(&state, first).folded);
    press(&mut state, now, key(KeyCode::Char('*')));
    assert!(state.jobs.iter().all(|job| !job.folded), "* unfolds every job");
    press(&mut state, now, key(KeyCode::Char('*')));
    assert!(state.jobs.iter().all(|job| job.folded), "and folds them all once all are unfolded");

    press(&mut state, now, key(KeyCode::Down));
    assert_eq!(selected(&state), Some(second));
    let pause = press(&mut state, now, key(KeyCode::Char('p')));
    assert_eq!(pause, vec![Effect::JobControl { id: second, command: JobCommand::Pause(true) }]);
    let cancel = press(&mut state, now, key(KeyCode::Char('c')));
    assert_eq!(cancel, vec![Effect::JobControl { id: second, command: JobCommand::Cancel }]);
    press(&mut state, now, key(KeyCode::Esc));
    assert!(state.overlay.is_none());
}

#[test]
fn finished_jobs_open_their_folder_and_x_dismisses_them() {
    let (mut state, now) = setup_with(vec![file("a"), file("b")]);
    let first = start_copy(&mut state, now);
    let second = start_copy(&mut state, now);
    finish_copy(&mut state, first, now + SECOND);
    press(&mut state, now + SECOND, alt('j'));
    assert_eq!(selected(&state), Some(first), "Alt-J opens on the job of the newest toast");
    assert!(!job(&state, first).folded, "and unfolds it");
    assert!(state.toasts.is_empty(), "the panel takes the toasts' place");

    let effects = press(&mut state, now + SECOND, key(KeyCode::Char('o')));
    assert!(state.overlay.is_none(), "opening closes the panel");
    assert_eq!(state.active, Side::Right);
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::List { side: Side::Right, dir, .. } if dir == Path::new("/tmp")))
    );

    press(&mut state, now + SECOND, alt('j'));
    press(&mut state, now + SECOND, key(KeyCode::End));
    press(&mut state, now + SECOND, key(KeyCode::Char('x')));
    assert!(state.jobs.iter().all(|job| job.id != first), "X dismisses the finished job");
    let effects = press(&mut state, now + SECOND, key(KeyCode::Delete));
    assert_eq!(
        effects,
        vec![Effect::JobControl { id: second, command: JobCommand::Cancel }],
        "Delete cancels a running one"
    );

    let cancelled = Outcome::Cancelled { done: Totals::default() };
    update(&mut state, Msg::Job { id: second, event: Event::Finished(cancelled) }, now + SECOND);
    assert_eq!(job(&state, second).ending().map(|ending| ending.how), Some(How::Cancelled));
    press(&mut state, now + SECOND, key(KeyCode::Char('X')));
    assert!(state.jobs.is_empty(), "Shift-X dismisses every finished job");
}

#[test]
fn alt_g_opens_what_the_newest_finished_job_made() {
    let (mut state, now) = setup_with(vec![file("a")]);
    press(&mut state, now, alt('g'));
    assert_eq!(state.notice.as_deref(), Some("No finished job to open"));

    let id = start_copy(&mut state, now);
    finish_copy(&mut state, id, now + SECOND);
    let effects = press(&mut state, now + SECOND, alt('g'));
    assert_eq!(state.active, Side::Right);
    assert!(effects.iter().any(|effect| matches!(effect, Effect::List { side: Side::Right, .. })));
}

#[test]
fn a_search_into_results_is_a_job_that_ends_with_what_it_found() {
    let (mut state, now) = setup();
    let (side, generation) = search_home(&mut state, "*.log");
    update(&mut state, Msg::Tick, now);
    let search = state.jobs.iter().find(|job| job.kind == JobKind::Search).map(|job| job.id).expect("a search job");
    assert!(job(&state, search).running());
    assert_eq!(job(&state, search).subject, "*.log in /home");

    let mut found = file("app.log");
    found.source = Some("/home/app.log".into());
    update(&mut state, Msg::Batch { side, generation, entries: vec![found] }, now);
    update(&mut state, Msg::Listed { side, generation, error: None }, now);
    assert_eq!(ended_with(&state, JobKind::Search), Some("Found 1 item"));

    state.reload_all();
    update(&mut state, Msg::Tick, now);
    let searches = state.jobs.iter().filter(|job| job.kind == JobKind::Search).count();
    assert_eq!(searches, 1, "a refresh of the results is no job of its own");
}

#[test]
fn the_jobs_panel_stops_a_search_and_quitting_stops_the_running_ones() {
    let (mut state, now) = setup();
    search_home(&mut state, "*.log");
    update(&mut state, Msg::Tick, now);
    press(&mut state, now, alt('j'));
    let effects = press(&mut state, now, key(KeyCode::Char('c')));
    assert_eq!(effects, vec![Effect::CancelSearch(Side::Left)]);
    assert_eq!(ended_with(&state, JobKind::Search), Some("Search stopped"));
    press(&mut state, now, key(KeyCode::Esc));

    search_home(&mut state, "*.txt");
    update(&mut state, Msg::Tick, now);
    press(&mut state, now, key(KeyCode::F(10)));
    let effects = press(&mut state, now, key(KeyCode::Char('y')));
    assert!(effects.contains(&Effect::CancelSearch(Side::Left)), "quit stops the search too: {effects:?}");
}

#[test]
fn folder_sizes_are_a_job_the_panel_can_stop() {
    let (mut state, now) = setup();
    press(&mut state, now, alt('i'));
    assert!(runs(&state, JobKind::Sizes));
    press(&mut state, now, alt('j'));
    let effects = press(&mut state, now, key(KeyCode::Char('c')));
    assert_eq!(effects, vec![Effect::CancelMeasure]);
    assert!(state.sizing.is_none());
    assert_eq!(ended_with(&state, JobKind::Sizes), Some(borg_i18n::tr!(background_sizes_stopped)));
}

#[test]
fn a_question_that_comes_while_a_command_is_typed_waits_until_alt_j() {
    let (mut state, now) = setup_with(vec![file("a")]);
    let id = start_copy(&mut state, now);
    update(&mut state, Msg::Observe(ShellView { prompt: PromptState::Typed, ..idle_prompt() }), now);
    let ask = Ask::Conflict { source: "/home/a".into(), target: "/tmp/a".into(), sizes: (200, 10), newer: true };
    update(&mut state, Msg::Job { id, event: Event::Asking(ask) }, now);
    assert!(!state.question_shows(), "the question waits behind a toast");
    assert_eq!(toasts(&state, now), vec![(id, ToastEvent::Asking)]);
    assert_eq!(state.jobs_wake, Some(now + crate::update_offer::QUIET));

    press(&mut state, now, alt('j'));
    assert!(matches!(state.overlay, Some(Overlay::Question(question)) if question.id == id));
    assert!(state.toasts.is_empty(), "the toast goes once the question shows");
}
