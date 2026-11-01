//! Sending signals and priority changes from the processes list.

use super::*;

fn press(state: &mut AppState, code: KeyCode) -> Vec<Effect> {
    update(state, Msg::Key(KeyEvent::from(code)), state.epoch)
}

fn dialog(state: &AppState) -> Option<&ControlDialog> {
    match &state.overlay {
        Some(crate::Overlay::ProcessControl(dialog)) => Some(dialog),
        _ => None,
    }
}

fn answer(state: &mut AppState, outcome: ControlOutcome, now: Instant) {
    let (id, request) = state.activity.pending.clone().unwrap();
    let ActivityRequest::Control { process, control, .. } = request else { panic!("a control request: {request:?}") };
    receive(state, id, Ok(ActivityData::Control(ControlReport { process, control, outcome })), now);
}

#[test]
fn k_opens_the_signals_for_the_selected_process_and_a_letter_sends_one() {
    let (mut state, now) = state();
    open(&mut state);
    sample(&mut state, vec![process(7, 10, 100)], now);
    key(&mut state, KeyCode::Char('k'));
    let opened = dialog(&state).unwrap();
    assert_eq!(
        (opened.process, opened.name.as_str(), opened.nice),
        (ProcessKey { pid: 7, start: 10 }, "worker-7", Some(0))
    );
    let effects = press(&mut state, KeyCode::Char('s'));
    assert!(state.overlay.is_none());
    let stop = ProcessControl::Signal(ProcessSignal::Stop);
    assert!(matches!(
        &effects[..],
        [Effect::Activity { request: ActivityRequest::Control { control, admin: false, .. }, .. }] if *control == stop
    ));
    answer(&mut state, ControlOutcome::Done, now);
    assert_eq!(state.notice.as_deref(), Some("Sent STOP to worker-7 (7)"));
    assert!(state.activity.error.is_none());
    assert!(state.activity.next.is_none(), "a fresh sample shows the change at once");
}

#[test]
fn arrows_and_enter_pick_a_choice_and_esc_sends_nothing() {
    let (mut state, now) = state();
    open(&mut state);
    sample(&mut state, vec![process(7, 10, 100)], now);
    key(&mut state, KeyCode::Char('k'));
    assert!(press(&mut state, KeyCode::Esc).is_empty());
    assert!(state.overlay.is_none());
    key(&mut state, KeyCode::Char('k'));
    press(&mut state, KeyCode::End);
    press(&mut state, KeyCode::Up);
    let effects = press(&mut state, KeyCode::Enter);
    let lower = ProcessControl::Priority(PRIORITY_STEP);
    assert!(matches!(
        &effects[..],
        [Effect::Activity { request: ActivityRequest::Control { control, .. }, .. }] if *control == lower
    ));
    answer(&mut state, ControlOutcome::Done, now);
    assert_eq!(state.notice.as_deref(), Some("worker-7 (7) now runs at nice 5"));
}

#[test]
fn priority_steps_stop_at_the_limits_and_wait_for_a_known_priority() {
    let with_nice =
        |nice| ControlDialog { process: ProcessKey { pid: 7, start: 10 }, name: "w".into(), nice, selected: 0 };
    assert_eq!(with_nice(Some(17)).control(ControlChoice::LowerPriority), Some(ProcessControl::Priority(NICEST)));
    assert_eq!(with_nice(Some(NICEST)).control(ControlChoice::LowerPriority), None);
    assert_eq!(with_nice(Some(-18)).control(ControlChoice::RaisePriority), Some(ProcessControl::Priority(LEAST_NICE)));
    assert_eq!(with_nice(Some(LEAST_NICE)).control(ControlChoice::RaisePriority), None);
    assert_eq!(with_nice(None).control(ControlChoice::RaisePriority), None);

    let (mut state, now) = state();
    open(&mut state);
    let mut unknown = process(7, 10, 100);
    unknown.nice = None;
    sample(&mut state, vec![unknown], now);
    key(&mut state, KeyCode::Char('k'));
    assert!(press(&mut state, KeyCode::Char('r')).is_empty(), "an unknown priority cannot be raised");
    assert!(dialog(&state).is_some(), "the dialog stays open for another choice");
}

#[test]
fn a_refusal_offers_the_administrator_and_a_vanished_process_does_not() {
    let (mut state, now) = state();
    open(&mut state);
    sample(&mut state, vec![process(7, 10, 100)], now);
    key(&mut state, KeyCode::Char('k'));
    press(&mut state, KeyCode::Char('t'));
    answer(&mut state, ControlOutcome::Denied, now);
    assert_eq!(state.activity.error.as_deref(), Some("Only the owner of worker-7 (7) or an administrator can do that"));
    let effects = key(&mut state, KeyCode::Char('a'));
    let terminate = ProcessControl::Signal(ProcessSignal::Terminate);
    assert!(matches!(
        &effects[..],
        [Effect::Activity { request: ActivityRequest::Control { control, admin: true, .. }, .. }] if *control == terminate
    ));
    answer(&mut state, ControlOutcome::Gone, now);
    assert_eq!(state.activity.error.as_deref(), Some("worker-7 (7) has already ended"));
    assert!(state.activity.retry.is_none());
}

#[test]
fn borg_running_as_administrator_or_its_own_process_offer_nothing_more() {
    let (mut state, now) = state();
    open(&mut state);
    state.activity.request(ActivityRequest::Sample);
    let id = state.activity.pending.as_ref().unwrap().0;
    let sample =
        SystemSample { processes: vec![process(7, 10, 100)], administrator: true, own: Some(7), ..Default::default() };
    receive(&mut state, id, Ok(ActivityData::Sample(sample)), now);
    key(&mut state, KeyCode::Char('k'));
    assert!(state.overlay.is_none(), "Borg sends no signals to itself");
    assert!(state.activity.error.is_some());

    state.activity.sample.own = None;
    state.activity.error = None;
    key(&mut state, KeyCode::Char('k'));
    press(&mut state, KeyCode::Char('k'));
    answer(&mut state, ControlOutcome::Denied, now);
    assert!(state.activity.retry.is_none(), "an administrator has no one higher to ask");
}

#[test]
fn k_types_into_the_filter_while_it_is_open() {
    let (mut state, now) = state();
    open(&mut state);
    sample(&mut state, vec![process(7, 10, 100)], now);
    key(&mut state, KeyCode::Char('/'));
    key(&mut state, KeyCode::Char('k'));
    assert!(state.overlay.is_none());
    assert_eq!(state.activity.filter, "k");
}
