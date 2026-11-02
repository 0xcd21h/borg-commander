use super::*;
use crate::{Appearance, Msg, Settings, Startup, ThemeChoice, update};
use borg_term::ShellSignal;
use std::time::Instant;

fn found<'a>(programs: &'a [&'a str]) -> impl Fn(&str) -> bool + 'a {
    move |program| programs.contains(&program)
}

fn command(platform: Platform, root: bool, programs: &[&str]) -> Option<String> {
    Installer::detect(platform, root, found(programs)).map(|installer| installer.command(Tool::Tcpdump))
}

#[test]
fn each_package_manager_installs_tcpdump_with_sudo() {
    let commands = [
        ("apt-get", "sudo apt-get install tcpdump"),
        ("dnf", "sudo dnf install tcpdump"),
        ("pacman", "sudo pacman -S tcpdump"),
        ("zypper", "sudo zypper install tcpdump"),
        ("apk", "sudo apk add tcpdump"),
    ];
    for (manager, expected) in commands {
        assert_eq!(command(Platform::Linux, false, &[manager, "sudo"]).as_deref(), Some(expected), "{manager}");
    }
}

#[test]
fn root_needs_no_sudo_and_doas_stands_in_for_it() {
    assert_eq!(command(Platform::Linux, true, &["apt-get", "sudo"]).as_deref(), Some("apt-get install tcpdump"));
    assert_eq!(command(Platform::Linux, false, &["apk", "doas"]).as_deref(), Some("doas apk add tcpdump"));
    assert_eq!(command(Platform::Linux, false, &["apk", "sudo", "doas"]).as_deref(), Some("sudo apk add tcpdump"));
}

#[test]
fn the_system_manager_comes_before_homebrew() {
    assert_eq!(command(Platform::Linux, false, &["brew", "dnf", "sudo"]).as_deref(), Some("sudo dnf install tcpdump"));
    assert_eq!(command(Platform::Linux, false, &["brew", "dnf"]).as_deref(), Some("brew install tcpdump"));
    assert_eq!(command(Platform::Linux, false, &["dnf"]), None, "nothing gives dnf root");
}

#[test]
fn macos_installs_with_homebrew_as_you() {
    assert_eq!(command(Platform::MacOs, false, &["brew", "sudo"]).as_deref(), Some("brew install tcpdump"));
    assert_eq!(command(Platform::MacOs, false, &["apt-get", "sudo"]), None, "only Homebrew on macOS");
    assert_eq!(command(Platform::MacOs, true, &["brew"]), None, "Homebrew refuses root");
}

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

const APT: Installer = Installer { manager: PackageManager::Apt, elevation: Some(Elevation::Sudo) };

fn press(state: &mut AppState, code: KeyCode) -> Vec<Effect> {
    let now = state.epoch;
    update(state, Msg::Key(code.into()), now)
}

fn command_ends(state: &mut AppState, exit: i32) {
    let now = state.epoch;
    update(state, Msg::Signal(ShellSignal::CommandEnd { exit: Some(exit) }), now);
}

#[test]
fn without_an_installer_the_notice_says_tcpdump_is_missing() {
    let mut state = state();
    offer(&mut state, Tool::Tcpdump, None);
    assert!(state.overlay.is_none());
    assert_eq!(state.notice.as_deref(), Some(tr!(capture_missing)));
}

#[test]
fn enter_types_the_command_into_the_shell_and_its_end_says_how_it_went() {
    let mut state = state();
    state.shell.integrated = true;
    offer(&mut state, Tool::Tcpdump, Some(APT));
    assert_eq!(state.overlay, Some(Overlay::Install(InstallOffer { tool: Tool::Tcpdump, installer: APT })));
    assert!(press(&mut state, KeyCode::Char('x')).is_empty());
    assert!(state.overlay.is_some(), "other keys leave the dialog open");

    let effects = press(&mut state, KeyCode::Enter);
    assert!(matches!(effects.as_slice(), [Effect::Write(line)] if line == b"sudo apt-get install tcpdump\r"));
    assert_eq!((state.overlay.is_none(), state.view), (true, View::Shell));

    command_ends(&mut state, 100);
    assert_eq!(state.notice, Some(tr!(install_failed, program = "tcpdump", code = 100)));

    offer(&mut state, Tool::Tcpdump, Some(APT));
    press(&mut state, KeyCode::Enter);
    command_ends(&mut state, 0);
    assert_eq!(state.notice.as_deref(), Some(tr!(install_tcpdump_done)));
    assert!(state.installing.is_none());
}

#[test]
fn esc_cancels_and_a_busy_shell_types_nothing() {
    let mut state = state();
    offer(&mut state, Tool::Tcpdump, Some(APT));
    assert!(press(&mut state, KeyCode::Esc).is_empty());
    assert!(state.overlay.is_none());

    state.shell.running = true;
    offer(&mut state, Tool::Tcpdump, Some(APT));
    assert!(press(&mut state, KeyCode::Enter).is_empty());
    assert_eq!(state.notice.as_deref(), Some(tr!(shell_is_busy_finish_or_clear_the)));
    assert_eq!(state.view, View::Panels);
}

#[test]
fn another_machine_s_console_types_nothing() {
    let mut state = state();
    state.console = Console::Remote { machine: "server".into(), session: "main".into() };
    offer(&mut state, Tool::Tcpdump, Some(APT));
    let effects = press(&mut state, KeyCode::Enter);
    assert!(!effects.iter().any(|effect| matches!(effect, Effect::Write(_))), "{effects:?}");
    assert_eq!(state.notice, Some(tr!(install_host_console, program = "tcpdump")));
}
