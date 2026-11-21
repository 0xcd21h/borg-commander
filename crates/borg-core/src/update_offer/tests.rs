use super::*;
use crate::{Appearance, Msg, Settings, Startup, ThemeChoice, update};
use borg_term::ShellSignal;

const BORG: &str = "/usr/local/bin/borg";
const BREW: &str = "brew upgrade borg-commander";

fn state() -> AppState {
    let now = Instant::now();
    let (mut state, _) = AppState::new(
        Startup::at("/work".into()),
        120,
        32,
        Settings::default(),
        Appearance::new(vec![ThemeChoice { id: "midnight".into(), name: "Midnight".into() }], 0),
        now,
    );
    state.settings.preferences.git_plugin = false;
    state
}

fn offer(manager: Option<&str>) -> UpdateOffer {
    UpdateOffer {
        version: "0.11.0".into(),
        current: "0.10.0".into(),
        program: BORG.into(),
        manager: manager.map(str::to_owned),
    }
}

fn tick(state: &mut AppState, now: Instant) {
    update(state, Msg::Tick, now);
}

/// The offer arrives and the keyboard rests long enough for it to open.
fn open(state: &mut AppState, offer: UpdateOffer) {
    let now = state.idle_since;
    update(state, Msg::UpdateAvailable(offer), now);
    tick(state, now + QUIET);
}

fn press(state: &mut AppState, code: KeyCode) -> Vec<Effect> {
    let now = state.idle_since;
    update(state, Msg::Key(code.into()), now)
}

fn command_ends(state: &mut AppState, exit: i32) {
    let now = state.idle_since;
    update(state, Msg::Signal(ShellSignal::CommandEnd { exit: Some(exit) }), now);
}

#[test]
fn the_command_is_borg_update_or_the_package_manager_s() {
    assert_eq!(offer(None).command(), "/usr/local/bin/borg update");
    assert_eq!(
        UpdateOffer { program: "/opt/my tools/borg".into(), ..offer(None) }.command(),
        "'/opt/my tools/borg' update"
    );
    assert_eq!(offer(Some(BREW)).command(), BREW);
}

#[test]
fn the_offer_opens_once_the_keyboard_rests() {
    let mut state = state();
    let start = state.epoch;
    update(&mut state, Msg::UpdateAvailable(offer(None)), start);
    assert_eq!(state.overlay, None, "it waits for a pause");
    assert_eq!(wake_at(&state), Some(start + QUIET));

    let typed = start + QUIET / 2;
    update(&mut state, Msg::Key(KeyCode::Down.into()), typed);
    tick(&mut state, start + QUIET);
    assert_eq!(state.overlay, None, "a key starts the pause again");
    tick(&mut state, typed + QUIET);
    assert_eq!(state.overlay, Some(Overlay::Update(offer(None))));
    assert_eq!(wake_at(&state), None);
}

#[test]
fn the_offer_waits_while_something_else_is_in_front() {
    let mut state = state();
    let start = state.epoch;
    let now = start + QUIET;
    update(&mut state, Msg::UpdateAvailable(offer(None)), start);

    state.overlay = Some(Overlay::TimeMachine);
    tick(&mut state, now);
    assert_eq!(state.overlay, Some(Overlay::TimeMachine));
    assert_eq!(wake_at(&state), None, "nothing to wake for while the dialog is open");
    state.overlay = None;

    state.view = View::Shell;
    tick(&mut state, now);
    assert_eq!(state.overlay, None);
    state.view = View::Panels;

    tick(&mut state, now);
    assert_eq!(state.overlay, Some(Overlay::Update(offer(None))));
}

#[test]
fn enter_types_the_command_into_the_shell_and_its_end_says_how_it_went() {
    let mut state = state();
    state.shell.integrated = true;
    open(&mut state, offer(None));
    let effects = press(&mut state, KeyCode::Enter);
    assert_eq!(effects, [Effect::Write(b"/usr/local/bin/borg update\r".to_vec())]);
    assert_eq!((state.overlay.is_none(), state.view), (true, View::Shell));

    command_ends(&mut state, 1);
    assert_eq!(state.notice, Some(tr!(update_failed, code = 1)));

    state.view = View::Panels;
    open(&mut state, offer(Some(BREW)));
    assert_eq!(press(&mut state, KeyCode::Enter), [Effect::Write(b"brew upgrade borg-commander\r".to_vec())]);
    command_ends(&mut state, 0);
    assert_eq!(state.notice.as_deref(), Some(tr!(update_finished)));
    state.notice = None;
    command_ends(&mut state, 0);
    assert_eq!(state.notice, None, "later commands say nothing");
}

#[test]
fn s_skips_the_version_n_turns_the_check_off_and_esc_leaves_it() {
    let mut state = state();
    open(&mut state, offer(None));
    assert!(press(&mut state, KeyCode::Char('x')).is_empty());
    assert_eq!(state.overlay, Some(Overlay::Update(offer(None))), "other keys do nothing");

    assert!(press(&mut state, KeyCode::Esc).is_empty());
    assert_eq!((state.overlay.is_none(), state.notice.is_none()), (true, true));

    open(&mut state, offer(None));
    assert_eq!(press(&mut state, KeyCode::Char('s')), [Effect::SkipUpdate("0.11.0".into())]);
    assert_eq!(state.overlay, None);
    assert_eq!(state.notice, Some(tr!(update_skipped, version = "0.11.0")));

    open(&mut state, offer(None));
    let effects = press(&mut state, KeyCode::Char('ы'));
    assert_eq!(effects, [Effect::SkipUpdate("0.11.0".into())], "S on a Russian layout");

    open(&mut state, offer(None));
    assert_eq!(press(&mut state, KeyCode::Char('n')), [Effect::StopUpdateChecks]);
    assert_eq!(state.overlay, None);
    assert_eq!(state.notice.as_deref(), Some(tr!(update_checks_off)));
}
