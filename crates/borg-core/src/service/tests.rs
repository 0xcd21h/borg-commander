use super::*;
use crate::{Action, Appearance, Msg, Settings, Startup, ThemeChoice, update};
use borg_term::ShellSignal;
use std::time::Instant;

const BORG: &str = "/usr/local/bin/borg";

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

fn answer(state: &mut AppState, status: ServiceStatus) {
    let now = state.epoch;
    update(state, Msg::Service(ServiceCheck { status, program: BORG.into(), root: false }), now);
}

fn press(state: &mut AppState, code: KeyCode) -> Vec<Effect> {
    let now = state.epoch;
    update(state, Msg::Key(code.into()), now)
}

fn command_ends(state: &mut AppState, exit: i32) {
    let now = state.epoch;
    update(state, Msg::Signal(ShellSignal::CommandEnd { exit: Some(exit) }), now);
}

fn offer(step: ServiceStep, program: &str) -> ServiceOffer {
    ServiceOffer { step, program: program.into(), root: false }
}

#[test]
fn installing_runs_install_enable_and_start_each_once_the_last_succeeded() {
    let install = offer(ServiceStep::Install, BORG);
    let line = "/usr/local/bin/borg service install && /usr/local/bin/borg service enable && /usr/local/bin/borg service start";
    assert_eq!(String::from_utf8_lossy(&install.line().line()), line);
    assert_eq!(
        install.rows(),
        [
            "/usr/local/bin/borg service install &&",
            "/usr/local/bin/borg service enable &&",
            "/usr/local/bin/borg service start"
        ]
    );
    assert_eq!(install.rows().join(" "), line, "the dialog shows the very command it runs");
    assert_eq!(offer(ServiceStep::Start, "borg").rows(), ["borg service enable &&", "borg service start"]);
    assert_eq!(offer(ServiceStep::Start, "/opt/my tools/borg").rows()[1], "'/opt/my tools/borg' service start");
}

#[test]
fn the_menu_asks_the_app_and_its_answer_opens_the_dialog_or_says_why_not() {
    let mut state = state();
    let now = state.epoch;
    assert_eq!(crate::actions::perform(&mut state, Action::BackgroundService, now), [Effect::CheckService]);

    answer(&mut state, ServiceStatus::Missing);
    assert_eq!(state.overlay, Some(Overlay::Service(offer(ServiceStep::Install, BORG))));
    assert!(press(&mut state, KeyCode::Char('x')).is_empty());
    assert!(press(&mut state, KeyCode::Esc).is_empty());
    assert!(state.overlay.is_none());

    answer(&mut state, ServiceStatus::Stopped);
    assert_eq!(state.overlay, Some(Overlay::Service(offer(ServiceStep::Start, BORG))));
    state.overlay = None;

    answer(&mut state, ServiceStatus::Running);
    assert_eq!((state.overlay.is_none(), state.notice.as_deref()), (true, Some(tr!(service_running))));
    answer(&mut state, ServiceStatus::Unmanaged);
    assert_eq!((state.overlay.is_none(), state.notice.as_deref()), (true, Some(tr!(service_unmanaged))));
}

#[test]
fn enter_types_the_command_into_the_shell_and_its_end_says_how_it_went() {
    let mut state = state();
    state.shell.integrated = true;
    answer(&mut state, ServiceStatus::Stopped);
    let effects = press(&mut state, KeyCode::Enter);
    let typed = b"/usr/local/bin/borg service enable && /usr/local/bin/borg service start\r";
    assert!(matches!(effects.as_slice(), [Effect::Write(line)] if line == typed), "{effects:?}");
    assert_eq!((state.overlay.is_none(), state.view), (true, View::Shell));

    command_ends(&mut state, 1);
    assert_eq!(state.notice, Some(tr!(service_failed, code = 1)));

    answer(&mut state, ServiceStatus::Missing);
    press(&mut state, KeyCode::Enter);
    command_ends(&mut state, 0);
    assert_eq!(state.notice.as_deref(), Some(tr!(service_done)));
    state.notice = None;
    command_ends(&mut state, 0);
    assert_eq!(state.notice, None, "later commands say nothing");
}

#[test]
fn a_busy_shell_or_another_machine_s_console_types_nothing() {
    let mut state = state();
    state.shell.running = true;
    answer(&mut state, ServiceStatus::Missing);
    assert!(press(&mut state, KeyCode::Enter).is_empty());
    assert_eq!(state.notice.as_deref(), Some(tr!(shell_is_busy_finish_or_clear_the)));
    assert_eq!(state.view, View::Panels);

    state.shell.running = false;
    answer(&mut state, ServiceStatus::Missing);
    state.console = Console::Remote { machine: "server".into(), session: "main".into() };
    let effects = press(&mut state, KeyCode::Enter);
    assert!(!effects.iter().any(|effect| matches!(effect, Effect::Write(_))), "{effects:?}");
    assert_eq!(state.notice.as_deref(), Some(tr!(service_host_console)));
}

#[test]
fn an_answer_that_comes_while_another_dialog_is_open_leaves_that_one() {
    let mut state = state();
    state.overlay = Some(Overlay::TimeMachine);
    answer(&mut state, ServiceStatus::Missing);
    assert_eq!(state.overlay, Some(Overlay::TimeMachine));
}
