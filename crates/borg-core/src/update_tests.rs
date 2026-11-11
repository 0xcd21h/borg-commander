use std::{
    path::{Path, PathBuf},
    time::Instant,
};

use borg_term::{Foreground, PromptState, ShellSignal, TermMode};
use borg_vfs::{Entry, EntryKind};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use borg_jobs::{Ask, Decision, Event, Outcome, Progress, Spec, Totals};

use crate::{
    Action, AppState, Effect, JobCommand, ListingMode, Msg, Overlay, PanelSetup, Settings, ShellView, Side, SortKey,
    Startup, ThemeChoice, View, menus, state::REVEAL_DELAY, update,
};

const COLS: u16 = 80;
const ROWS: u16 = 24;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

#[test]
fn remote_mount_stays_distinct_from_local_paths_and_blocks_local_changes() {
    let (mut state, now) = setup();
    let local = state.panel(Side::Left).dir().to_path_buf();
    let other = state.panel(Side::Right).dir().to_path_buf();
    let effects = crate::mount_remote(&mut state, Side::Left, "prod".into(), "remote-id".into(), "Deploy".into());
    assert!(matches!(&effects[..], [Effect::List { side: Side::Left, dir, .. }] if dir == Path::new("/")));
    assert_eq!(state.panel(Side::Left).destination(), Path::new("/"));
    assert_eq!(state.panel(Side::Right).dir(), other);
    assert!(!state.console_owns_panel());
    assert_eq!(
        crate::actions::perform(&mut state, Action::MirrorRemote, now),
        vec![Effect::MirrorRemote { machine: "prod".into(), session: "remote-id".into() }]
    );
    let Effect::List { generation, .. } = effects[0] else { panic!("remote list effect") };
    update(&mut state, Msg::Batch { side: Side::Left, generation, entries: vec![file("readme.md")] }, now);
    update(&mut state, Msg::Listed { side: Side::Left, generation, error: None }, now);
    state.panel_mut(Side::Left).move_by(1);
    assert_eq!(state.panel(Side::Left).entries().len(), 1);
    assert!(state.panel(Side::Left).hovered_entry().is_some(), "remote file should be selected");
    assert_eq!(
        crate::actions::perform(&mut state, Action::View, now),
        vec![Effect::RemoteView {
            machine: "prod".into(),
            session: "remote-id".into(),
            path: PathBuf::from("/readme.md")
        }]
    );
    assert!(crate::actions::perform(&mut state, Action::Delete, now).is_empty());
    assert!(state.notice.as_deref().unwrap_or_default().contains("not available"));
    let effect = crate::unmount_remote(&mut state, Side::Left).unwrap();
    assert!(matches!(effect, Effect::List { side: Side::Left, dir, .. } if dir == local));
}

#[test]
fn remote_session_removal_confirms_the_exact_target_and_restores_mounted_panels() {
    let (mut state, now) = setup();
    let host = state.active_panel().dir().to_path_buf();
    crate::mount_remote(&mut state, Side::Left, "server".into(), "junk".into(), "Junk".into());
    state.remote_names.set("server-key", None, "Lab").unwrap();
    state.remote_names.set("server-key", Some("junk"), "Scratch").unwrap();
    crate::actions::perform(&mut state, Action::BrowseRemotes, now);
    update(
        &mut state,
        Msg::RemoteCatalog {
            choices: vec![
                crate::RemoteChoice {
                    machine: "server".into(),
                    session: Some("default".into()),
                    name: "Default".into(),
                    ..Default::default()
                },
                crate::RemoteChoice {
                    machine: "server".into(),
                    identity: "server-key".into(),
                    session: Some("junk".into()),
                    name: "Junk".into(),
                    host_label: Some("Lab".into()),
                    session_label: Some("Scratch".into()),
                    ..Default::default()
                },
            ],
            complete: true,
        },
        now,
    );
    update(&mut state, Msg::Key(key(KeyCode::Delete)), now);
    assert!(
        matches!(&state.overlay, Some(Overlay::Remotes(browser)) if browser.error.as_deref().is_some_and(|error| error.contains("Default")))
    );
    update(&mut state, Msg::Key(key(KeyCode::Down)), now);
    update(&mut state, Msg::Key(key(KeyCode::Delete)), now);
    assert!(
        matches!(&state.overlay, Some(Overlay::Confirm(confirm)) if !confirm.accept && confirm.message.contains("Scratch") && confirm.message.contains("Lab"))
    );
    assert_eq!(update(&mut state, Msg::Key(key(KeyCode::Enter)), now), vec![Effect::OpenRemotes]);
    assert!(state.panel(Side::Left).remote_mount.is_some());
    update(&mut state, Msg::Key(key(KeyCode::F(8))), now);
    assert_eq!(
        update(&mut state, Msg::Key(key(KeyCode::Char('y'))), now),
        vec![Effect::DeleteRemoteSession { machine: "server".into(), session: "junk".into() }]
    );
    assert!(
        matches!(&state.overlay, Some(Overlay::Remotes(browser)) if browser.deleting == Some(("server".into(), "junk".into())))
    );
    update(
        &mut state,
        Msg::RemoteSessionDeleted {
            machine: "server".into(),
            session: "junk".into(),
            result: Err("Host unavailable".into()),
        },
        now,
    );
    assert!(state.panel(Side::Left).remote_mount.is_some());
    update(&mut state, Msg::Key(key(KeyCode::Delete)), now);
    update(&mut state, Msg::Key(key(KeyCode::Char('y'))), now);
    state.console = crate::Console::Remote { machine: "server".into(), session: "junk".into() };
    state.parked_host = Some(crate::ShellStatus::new(host.clone()));
    let effects = update(
        &mut state,
        Msg::RemoteSessionDeleted { machine: "server".into(), session: "junk".into(), result: Ok(()) },
        now,
    );
    assert!(effects.contains(&Effect::CloseRemoteConsole { machine: "server".into(), session: "junk".into() }));
    assert!(effects.contains(&Effect::OpenRemotes));
    assert!(state.panel(Side::Left).remote_mount.is_none());
    assert_eq!(state.panel(Side::Left).dir(), host);
    assert_eq!(state.console, crate::Console::Host);
    assert!(
        matches!(&state.overlay, Some(Overlay::Remotes(browser)) if browser.deleting.is_none() && browser.choices.len() == 1)
    );
}

#[test]
fn remote_browser_shows_incoming_clients_without_turning_them_into_hosts() {
    let (mut state, now) = setup();
    crate::actions::perform(&mut state, Action::BrowseRemotes, now);
    update(&mut state, Msg::RemoteCatalog { choices: Vec::new(), complete: true }, now);
    let clients = vec![
        crate::RemoteClient {
            name: "laptop".into(),
            fingerprint: "first-key".into(),
            connections: Some(3),
            key_changed: None,
        },
        crate::RemoteClient {
            name: "workstation".into(),
            fingerprint: "second-key".into(),
            connections: None,
            key_changed: None,
        },
    ];
    update(&mut state, Msg::RemoteClients(clients.clone()), now);
    assert!(
        matches!(&state.overlay, Some(Overlay::Remotes(browser)) if browser.show_clients && browser.clients.as_ref() == Some(&clients) && browser.choices.is_empty())
    );
    assert!(update(&mut state, Msg::Key(key(KeyCode::Enter)), now).is_empty());
    assert!(update(&mut state, Msg::Key(key(KeyCode::Char('m'))), now).is_empty());
    update(&mut state, Msg::Key(key(KeyCode::Down)), now);
    update(&mut state, Msg::RemoteClients(vec![clients[1].clone(), clients[0].clone()]), now);
    assert!(matches!(&state.overlay, Some(Overlay::Remotes(browser)) if browser.client_selected == 0));
    update(&mut state, Msg::Key(key(KeyCode::Tab)), now);
    assert!(matches!(&state.overlay, Some(Overlay::Remotes(browser)) if !browser.show_clients));
    update(&mut state, Msg::Key(key(KeyCode::Tab)), now);
    assert_eq!(update(&mut state, Msg::Key(key(KeyCode::Char('r'))), now), vec![Effect::OpenRemotes]);
    update(&mut state, Msg::Key(key(KeyCode::Esc)), now);
    assert!(state.overlay.is_none());
}

#[test]
fn alt_r_reloads_the_active_panel_and_its_menu_names_the_key() {
    let (mut state, now) = setup();
    state.active = Side::Right;
    let right = state.panel(Side::Right).dir().to_path_buf();
    let effects = update(&mut state, Msg::Key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::ALT)), now);
    assert!(matches!(&effects[..], [Effect::List { side: Side::Right, dir, .. }] if *dir == right), "{effects:?}");
    let rescan = |menu: &crate::menu::Menu| {
        menu.entries.iter().find_map(|entry| match entry {
            crate::menu::MenuEntry::Item(item) if matches!(item.action, Action::Rescan(_)) => Some(item.hint),
            _ => None,
        })
    };
    let menus = menus(&state);
    assert_eq!((rescan(&menus[0]), rescan(&menus[5])), (Some(""), Some("M-r")));
}

#[test]
fn remote_browser_opens_switches_and_mirrors_named_sessions() {
    let (mut state, now) = setup();
    assert_eq!(
        crate::keymap::lookup(
            &KeyEvent::new(KeyCode::Char('h'), KeyModifiers::ALT),
            crate::keymap::Context { view: View::Panels, line_empty: true, at_prompt: false },
        ),
        Some(Action::BrowseRemotes)
    );
    assert_eq!(
        crate::keymap::lookup(
            &KeyEvent::new(KeyCode::Char('R'), KeyModifiers::ALT | KeyModifiers::SHIFT),
            crate::keymap::Context { view: View::Panels, line_empty: true, at_prompt: false },
        ),
        Some(Action::BackResults)
    );
    assert_eq!(
        crate::keymap::lookup(
            &KeyEvent::new(KeyCode::Char('M'), KeyModifiers::ALT | KeyModifiers::SHIFT),
            crate::keymap::Context { view: View::Panels, line_empty: true, at_prompt: false },
        ),
        Some(Action::MirrorRemote)
    );
    let menu = &menus(&state)[3];
    assert_eq!(menu.title, "Remote");
    assert_eq!(menu.item(0).map(|item| item.action), Some(Action::BrowseRemotes));
    assert_eq!(menu.item(1).map(|item| item.enabled), Some(false));
    let host = state.panel(Side::Left).dir().to_path_buf();
    state.settings.preferences.docker_console = false;
    assert_eq!(
        update(&mut state, Msg::Key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::ALT)), now),
        vec![Effect::OpenRemotes]
    );
    assert!(matches!(state.overlay, Some(Overlay::Remotes(_))));
    assert_eq!(state.overlay_fade.target(), 1.0, "the remote dialog must become visible");
    let choices = vec![
        crate::RemoteChoice {
            machine: "first".into(),
            session: Some("one".into()),
            name: "Default".into(),
            ..Default::default()
        },
        crate::RemoteChoice {
            machine: "second".into(),
            session: Some("two".into()),
            name: "Work".into(),
            ..Default::default()
        },
    ];
    update(&mut state, Msg::RemoteCatalog { choices: choices.clone(), complete: true }, now);
    assert!(
        matches!(state.overlay, Some(Overlay::Remotes(ref browser)) if !browser.loading && browser.choices == choices)
    );
    assert_eq!(
        update(&mut state, Msg::Key(key(KeyCode::Enter)), now),
        vec![Effect::MountRemote {
            side: Side::Left,
            machine: "first".into(),
            session: "one".into(),
            name: "Default".into()
        }]
    );
    crate::mount_remote(&mut state, Side::Left, "first".into(), "one".into(), "Default".into());
    state.overlay = Some(Overlay::Remotes(crate::RemoteBrowser {
        side: Side::Left,
        choices: choices.clone(),
        selected: 1,
        loading: false,
        connecting: false,
        error: None,
        clients: None,
        show_clients: false,
        client_selected: 0,
        deleting: None,
        forgetting: None,
        updating: None,
    }));
    assert_eq!(
        update(&mut state, Msg::Key(key(KeyCode::Enter)), now),
        vec![Effect::MountRemote {
            side: Side::Left,
            machine: "second".into(),
            session: "two".into(),
            name: "Work".into()
        }]
    );
    crate::mount_remote(&mut state, Side::Left, "second".into(), "two".into(), "Work".into());
    assert_eq!(state.panel(Side::Left).remote_mount.as_ref().unwrap().host, host);
    state.overlay = Some(Overlay::Remotes(crate::RemoteBrowser {
        side: Side::Left,
        choices,
        selected: 1,
        loading: false,
        connecting: false,
        error: None,
        clients: None,
        show_clients: false,
        client_selected: 0,
        deleting: None,
        forgetting: None,
        updating: None,
    }));
    assert_eq!(
        update(&mut state, Msg::Key(key(KeyCode::Char('m'))), now),
        vec![Effect::MirrorRemote { machine: "second".into(), session: "two".into() }]
    );
    assert!(matches!(&state.overlay, Some(Overlay::Remotes(browser)) if browser.connecting && browser.selected == 1));
    assert!(update(&mut state, Msg::Key(key(KeyCode::Down)), now).is_empty());
    assert_eq!(update(&mut state, Msg::Key(key(KeyCode::Esc)), now), vec![Effect::CancelRemoteConnection]);
    update(&mut state, Msg::RemoteMirrorConnected(Err("late failure".into())), now);
    assert!(
        matches!(&state.overlay, Some(Overlay::Remotes(browser)) if !browser.connecting && browser.error.is_none())
    );
    update(&mut state, Msg::Key(key(KeyCode::Char('m'))), now);
    update(&mut state, Msg::RemoteMirrorConnected(Err("connection timed out".into())), now);
    assert!(
        matches!(&state.overlay, Some(Overlay::Remotes(browser)) if !browser.connecting && browser.error.as_deref() == Some("connection timed out") && browser.selected == 1)
    );
    update(&mut state, Msg::Key(key(KeyCode::Char('m'))), now);
    assert!(matches!(&state.overlay, Some(Overlay::Remotes(browser)) if browser.connecting && browser.error.is_none()));
    update(&mut state, Msg::RemoteMirrorConnected(Ok(())), now);
    assert!(state.overlay.is_none());
    assert_eq!(
        crate::actions::perform(&mut state, Action::MirrorRemote, now),
        vec![Effect::MirrorRemote { machine: "second".into(), session: "two".into() }]
    );
    assert!(
        matches!(&state.overlay, Some(Overlay::Remotes(browser)) if browser.connecting && browser.choices[0].machine == "second")
    );
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

#[test]
fn remote_navigation_shows_spinner_before_first_chunk_and_until_completion() {
    let (mut state, now) = setup();
    let effects = update(
        &mut state,
        Msg::MountRemote { side: Side::Left, machine: "server".into(), session: "id".into(), name: "Work".into() },
        now,
    );
    let Effect::List { generation, .. } = effects[0] else { panic!("listing") };
    let shown = now + crate::SPINNER_DELAY + std::time::Duration::from_millis(1);
    update(&mut state, Msg::Tick, shown);
    assert!(state.busy.shown_since(crate::Task::Listing(Side::Left), shown).is_some());
    update(&mut state, Msg::Batch { side: Side::Left, generation, entries: vec![file("first")] }, shown);
    assert!(state.busy.shown_since(crate::Task::Listing(Side::Left), shown).is_some());
    update(&mut state, Msg::Listed { side: Side::Left, generation, error: None }, shown);
    assert!(state.busy.since(crate::Task::Listing(Side::Left)).is_none());
}

fn shift(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::SHIFT)
}

fn entry(name: &str, kind: EntryKind) -> Entry {
    Entry {
        source: None,
        missing: false,
        usage: None,
        name: name.into(),
        kind,
        size: 0,
        modified: None,
        executable: false,
        permissions: 0o644,
    }
}

fn dir(name: &str) -> Entry {
    entry(name, EntryKind::Dir)
}

fn file(name: &str) -> Entry {
    entry(name, EntryKind::File)
}

fn idle_prompt() -> ShellView {
    ShellView {
        prompt: PromptState::Empty,
        command_rows: 1,
        foreground: Foreground::Shell,
        mode: TermMode::empty(),
        raw_input: false,
        line: borg_term::TypedLine::default(),
    }
}

#[test]
fn shell_exit_defaults_to_no_and_only_yes_closes_borg() {
    let (mut state, now) = setup();
    let request = || Msg::Signal(ShellSignal::ExitRequested { token: "42-77".into() });
    assert!(update(&mut state, request(), now).is_empty());
    assert!(matches!(
        state.overlay,
        Some(Overlay::Confirm(crate::Confirm { purpose: crate::ConfirmPurpose::ShellExit { .. }, accept: false, .. }))
    ));
    assert_eq!(press(&mut state, now, key(KeyCode::Enter)), vec![Effect::CancelShellExit { token: "42-77".into() }]);
    assert!(state.overlay.is_none());
    update(&mut state, request(), now);
    assert_eq!(press(&mut state, now, key(KeyCode::Esc)), vec![Effect::CancelShellExit { token: "42-77".into() }]);
    update(&mut state, request(), now);
    assert!(press(&mut state, now, key(KeyCode::Char('y'))).contains(&Effect::Quit));
    crate::actions::perform(&mut state, Action::Quit, now);
    let before = state.overlay.clone();
    assert_eq!(update(&mut state, request(), now), vec![Effect::CancelShellExit { token: "42-77".into() }]);
    assert_eq!(state.overlay, before, "another modal is preserved");
}

#[test]
fn shell_exit_confirmation_tracks_its_host_shell_when_another_console_finishes() {
    let (mut state, now) = setup();
    state.console = crate::Console::Remote { machine: "server".into(), session: "default".into() };
    state.shell.running = true;
    update(
        &mut state,
        Msg::ParkedSignal { container: None, signal: ShellSignal::ExitRequested { token: "123".into() } },
        now,
    );
    let before = state.overlay.clone();
    update(&mut state, Msg::Signal(ShellSignal::CommandEnd { exit: Some(0) }), now);
    assert_eq!(state.overlay, before);
    update(&mut state, Msg::ParkedSignal { container: None, signal: ShellSignal::CommandEnd { exit: Some(1) } }, now);
    assert!(state.overlay.is_none());
}

#[test]
fn exit_in_a_remote_shell_asks_first_and_a_confirmed_close_is_no_error() {
    let (mut state, now) = setup();
    let mount =
        Msg::MountRemote { side: Side::Left, machine: "server".into(), session: "default".into(), name: "Work".into() };
    update(&mut state, mount, now);
    assert_eq!(state.console, crate::Console::Remote { machine: "server".into(), session: "default".into() });
    let exit = |token: &str| Msg::RemoteConsoleOutput {
        echoed: true,
        machine: "server".into(),
        session: "default".into(),
        view: idle_prompt(),
        signals: vec![ShellSignal::ExitRequested { token: token.into() }],
    };
    update(&mut state, exit("7"), now);
    assert!(matches!(&state.overlay, Some(Overlay::Confirm(confirm))
        if confirm.purpose == crate::ConfirmPurpose::RemoteShellExit { token: "7".into() } && !confirm.accept));
    assert_eq!(writes(&press(&mut state, now, key(KeyCode::Enter))), "7:no\n", "No is the default");
    update(&mut state, exit("8"), now);
    assert_eq!(writes(&press(&mut state, now, key(KeyCode::Char('y')))), "8:yes\n");
    let closed =
        Msg::RemoteConsoleClosed { machine: "server".into(), session: "default".into(), error: "Shell exited".into() };
    update(&mut state, closed, now);
    assert_eq!(state.console, crate::Console::Host);
    assert!(state.notice.as_deref().is_some_and(|notice| notice.contains("ended")));
    update(&mut state, Msg::Key(key(KeyCode::Char('a'))), now);
    assert_eq!(state.console, crate::Console::Host, "the closed shell is not reopened by typing");
}

fn text(viewer: &crate::Viewer) -> &borg_view::Pager {
    match &viewer.content {
        crate::Content::Text(pager) => pager,
        crate::Content::Image(_) => panic!("expected a text viewer"),
    }
}

fn quiet() -> Settings {
    Settings {
        preferences: crate::Preferences { git_plugin: false, ..crate::Preferences::DEFAULT },
        ..Settings::default()
    }
}

fn setup_with(entries: Vec<Entry>) -> (AppState, Instant) {
    setup_from(entries, quiet())
}

fn setup_from(entries: Vec<Entry>, settings: Settings) -> (AppState, Instant) {
    let now = Instant::now();
    let themes = ["midnight", "classic", "paper"].map(|id| ThemeChoice { id: id.into(), name: id.into() }).to_vec();
    let startup = Startup {
        panels: [PanelSetup::at(PathBuf::from("/home")), PanelSetup::at(PathBuf::from("/tmp"))],
        active: Side::Left,
        split: crate::EVEN_SPLIT,
    };
    let (mut state, effects) = AppState::new(startup, COLS, ROWS, settings, crate::Appearance::new(themes, 0), now);
    for effect in effects {
        let Effect::List { side, generation, .. } = effect else { continue };
        update(&mut state, Msg::Batch { side, generation, entries: entries.clone() }, now);
        update(&mut state, Msg::Listed { side, generation, error: None }, now);
    }
    update(&mut state, Msg::Observe(idle_prompt()), now);
    update(&mut state, Msg::Signal(ShellSignal::PromptStart), now);
    state.settle();
    (state, now)
}

fn setup() -> (AppState, Instant) {
    setup_with(vec![dir("me")])
}

fn press(state: &mut AppState, now: Instant, event: KeyEvent) -> Vec<Effect> {
    update(state, Msg::Key(event), now)
}

fn type_text(state: &mut AppState, now: Instant, text: &str) {
    text.chars().for_each(|c| {
        press(state, now, key(KeyCode::Char(c)));
    });
}

fn writes(effects: &[Effect]) -> String {
    let bytes: Vec<u8> = effects
        .iter()
        .filter_map(|e| match e {
            Effect::Write(bytes) | Effect::WriteConcealed(bytes) => Some(bytes.clone()),
            _ => None,
        })
        .flatten()
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

fn hovered(state: &AppState) -> Option<&str> {
    state.active_panel().hovered_entry().map(|e| e.name.as_str())
}

fn marked(state: &AppState) -> Vec<&str> {
    state.active_panel().marked().map(|e| e.name.as_str()).collect()
}

#[test]
fn typing_goes_to_shell_after_syncing_directory() {
    let (mut state, now) = setup();
    press(&mut state, now, key(KeyCode::Down));
    let effects = press(&mut state, now, key(KeyCode::Enter));
    let Some(Effect::List { side, generation, dir }) = effects.first().cloned() else { panic!("expected listing") };
    assert_eq!(dir, Path::new("/home/me"));
    update(&mut state, Msg::Batch { side, generation, entries: vec![] }, now);
    state.settle();

    let effects = press(&mut state, now, key(KeyCode::Char('l')));
    assert_eq!(writes(&effects), " cd '/home/me'\rl");
    assert!(effects.contains(&Effect::WriteConcealed(b" cd '/home/me'\r".to_vec())), "the cd stays out of sight");
    assert_eq!(state.view, View::Panels);
}

#[test]
fn shells_announcing_silent_cd_change_folder_without_a_typed_command() {
    let (mut state, now) = setup();
    for signal in [ShellSignal::PromptStart, ShellSignal::SilentCd] {
        update(&mut state, Msg::Signal(signal), now);
    }
    press(&mut state, now, key(KeyCode::Down));
    let effects = press(&mut state, now, key(KeyCode::Enter));
    let Some(Effect::List { side, generation, .. }) = effects.first().cloned() else { panic!("expected listing") };
    update(&mut state, Msg::Batch { side, generation, entries: vec![] }, now);
    state.settle();

    let effects = press(&mut state, now, ctrl('o'));
    assert!(matches!(
        effects.as_slice(),
        [Effect::ChangeShellDir { dir, typed }] if dir == Path::new("/home/me") && typed == b" cd '/home/me'\r"
    ));
    assert_eq!(state.shell.cwd, Path::new("/home/me"));

    press(&mut state, now, ctrl('o'));
    let Effect::List { side, generation, .. } = state.navigate(Side::Left, PathBuf::from("/home"), None) else {
        panic!("expected listing")
    };
    update(&mut state, Msg::Batch { side, generation, entries: vec![] }, now);
    state.settle();
    update(&mut state, Msg::Signal(ShellSignal::PromptStart), now);
    let effects = press(&mut state, now, ctrl('o'));
    assert_eq!(writes(&effects), " cd '/home'\r", "a prompt without the announcement falls back to typing");
}

#[test]
fn esc_closes_what_is_open_before_digits_act_as_function_keys() {
    let (mut state, now) = setup();
    press(&mut state, now, ctrl('q'));
    assert!(state.aside.is_some());
    press(&mut state, now, key(KeyCode::Esc));
    assert!(state.aside.is_none(), "Esc closes the quick view");
    assert!(!state.function_prefix(now) && state.notice.is_none());
    let effects = press(&mut state, now, key(KeyCode::Char('1')));
    assert_eq!(writes(&effects), "1", "a digit after a closing Esc is typed");
    assert!(state.overlay.is_none());

    let (mut state, now) = setup();
    press(&mut state, now, key(KeyCode::Esc));
    assert!(state.function_prefix(now) && state.notice.is_none(), "a plain Esc waits quietly for a digit");
    press(&mut state, now, key(KeyCode::Char('9')));
    assert!(state.overlay.is_some(), "Esc 9 opens the menus like F9");

    let (mut state, now) = setup();
    press(&mut state, now, key(KeyCode::Esc));
    press(&mut state, now, key(KeyCode::Esc));
    assert!(!state.function_prefix(now), "a second Esc cancels");
    assert!(!state.function_prefix(now + std::time::Duration::from_secs(2)));
}

#[test]
fn enter_on_typed_line_runs_command_and_reveals_shell() {
    let (mut state, now) = setup();
    state.shell.observed.prompt = PromptState::Typed;
    let effects = press(&mut state, now, key(KeyCode::Enter));
    assert_eq!(writes(&effects), "\r");
    assert_eq!(state.view, View::Shell);
    assert_ne!(state.pace(now), Some(crate::Pace::Smooth));
}

#[test]
fn keys_typed_before_echo_still_count_as_typed() {
    let (mut state, now) = setup();
    type_text(&mut state, now, "ls");
    update(&mut state, Msg::Observe(idle_prompt()), now);
    let effects = press(&mut state, now, key(KeyCode::Enter));
    assert_eq!(writes(&effects), "\r");
    assert_eq!(state.view, View::Shell);
}

#[test]
fn echo_releases_pending_keystrokes() {
    let (mut state, now) = setup();
    press(&mut state, now, key(KeyCode::Char('x')));
    press(&mut state, now, key(KeyCode::Backspace));
    update(&mut state, Msg::ShellOutput(idle_prompt()), now);
    let effects = press(&mut state, now, key(KeyCode::Tab));
    assert!(effects.is_empty());
    assert_eq!(state.active, Side::Right);
}

#[test]
fn tab_switches_panels_only_on_empty_line() {
    let (mut state, now) = setup();
    press(&mut state, now, key(KeyCode::Tab));
    assert_eq!(state.active, Side::Right);
    state.shell.observed.prompt = PromptState::Typed;
    let effects = press(&mut state, now, key(KeyCode::Tab));
    assert_eq!(writes(&effects), "\t");
    assert_eq!(state.active, Side::Right);
}

#[test]
fn full_screen_programs_capture_every_key() {
    let (mut state, now) = setup();
    let captured = ShellView { foreground: Foreground::Program, mode: TermMode::ALT_SCREEN, ..idle_prompt() };
    update(&mut state, Msg::Observe(captured), now);
    assert_eq!(state.effective_view(), View::Shell);
    let effects = press(&mut state, now, ctrl('o'));
    assert_eq!(writes(&effects), "\u{f}");
    assert_eq!(state.view, View::Panels);
}

#[test]
fn ctrl_o_toggles_shell_view() {
    let (mut state, now) = setup();
    press(&mut state, now, ctrl('o'));
    assert_eq!(state.view, View::Shell);
    press(&mut state, now, ctrl('o'));
    assert_eq!(state.view, View::Panels);
}

#[test]
fn panel_follows_shell_directory_changes() {
    let (mut state, now) = setup();
    let signal = ShellSignal::Cwd { host: String::new(), path: PathBuf::from("/var") };
    let effects = update(&mut state, Msg::Signal(signal), now);
    assert!(matches!(effects.as_slice(), [Effect::List { side: Side::Left, dir, .. }] if dir == Path::new("/var")));
    assert_eq!(state.shell.cwd, Path::new("/var"));
}

#[test]
fn finished_command_refreshes_panels() {
    let (mut state, now) = setup();
    update(&mut state, Msg::Signal(ShellSignal::CommandStart), now);
    let effects = update(&mut state, Msg::Signal(ShellSignal::CommandEnd { exit: Some(0) }), now);
    assert_eq!(effects.iter().filter(|e| matches!(e, Effect::List { .. })).count(), 2);
}

#[test]
fn shells_without_integration_never_look_busy() {
    let (mut state, now) = setup();
    state.shell.integrated = false;
    type_text(&mut state, now, "x");
    press(&mut state, now, key(KeyCode::Enter));
    assert!(!state.shell.running);
}

#[test]
fn theme_picker_previews_saves_and_escape_restores() {
    let (mut state, now) = setup();
    update(&mut state, Msg::Key(key(KeyCode::F(9))), now);
    let options = menus(&state).len() - 2;
    state.overlay = None;

    crate::actions::perform(&mut state, Action::PickTheme, now);
    press(&mut state, now, key(KeyCode::Down));
    assert_eq!(state.appearance.current_id(), Some("classic"));
    assert_eq!(press(&mut state, now, key(KeyCode::Enter)), vec![Effect::SaveConfig]);

    crate::actions::perform(&mut state, Action::PickTheme, now);
    press(&mut state, now, key(KeyCode::End));
    press(&mut state, now, key(KeyCode::Esc));
    assert_eq!(state.appearance.current_id(), Some("classic"));
    assert!(state.overlay.is_none());
    assert!(options > 0);
}

#[test]
fn insert_and_shift_arrows_mark_files() {
    let (mut state, now) = setup_with(vec![file("a"), file("b"), file("c")]);
    press(&mut state, now, key(KeyCode::Down));
    press(&mut state, now, key(KeyCode::Insert));
    assert_eq!(marked(&state), ["a"]);
    assert_eq!(hovered(&state), Some("b"));

    press(&mut state, now, shift(KeyCode::Down));
    press(&mut state, now, shift(KeyCode::Down));
    assert_eq!(marked(&state), ["a", "b", "c"]);

    press(&mut state, now, ctrl('t'));
    assert_eq!(marked(&state), ["a", "b"]);
}

#[test]
fn plus_opens_pattern_prompt_and_selects_matches() {
    let (mut state, now) = setup_with(vec![file("a.rs"), file("b.rs"), file("notes.md")]);
    press(&mut state, now, key(KeyCode::Char('+')));
    assert!(matches!(state.overlay, Some(Overlay::Prompt(_))));

    press(&mut state, now, ctrl('u'));
    type_text(&mut state, now, "*.rs");
    press(&mut state, now, key(KeyCode::Enter));
    assert!(state.overlay.is_none());
    assert_eq!(marked(&state), ["a.rs", "b.rs"]);

    press(&mut state, now, key(KeyCode::Char('*')));
    assert_eq!(marked(&state), ["notes.md"]);

    press(&mut state, now, key(KeyCode::Char('-')));
    press(&mut state, now, key(KeyCode::Enter));
    assert!(marked(&state).is_empty());
}

#[test]
fn copy_prompts_for_destination_and_starts_a_job() {
    let (mut state, now) = setup_with(vec![file("a"), file("b c")]);
    press(&mut state, now, key(KeyCode::Down));
    press(&mut state, now, key(KeyCode::Insert));
    press(&mut state, now, key(KeyCode::Insert));
    press(&mut state, now, key(KeyCode::F(5)));
    let Some(Overlay::Prompt(prompt)) = &state.overlay else { panic!("expected prompt") };
    assert_eq!(prompt.input.text(), "/tmp");
    assert_eq!(prompt.label, "Copy 2 items to");

    let effects = press(&mut state, now, key(KeyCode::Enter));
    let sources = vec![PathBuf::from("/home/a"), PathBuf::from("/home/b c")];
    assert_eq!(
        effects,
        vec![Effect::StartJob {
            id: 0,
            spec: Spec::Copy { sources, target: PathBuf::from("/tmp"), optimize_hdd: false }
        }]
    );
    assert_eq!(state.jobs.len(), 1);
    assert_eq!(state.jobs[0].shown_on, Side::Right);
    assert_eq!(state.view, View::Panels);
    assert_eq!(state.pace(now + std::time::Duration::from_secs(1)), Some(crate::Pace::Ambient));
}

#[test]
fn hard_drive_copy_option_is_per_job_and_supports_keyboard_and_mouse() {
    let (mut state, now) = setup_with(vec![file("a")]);
    press(&mut state, now, key(KeyCode::Down));
    press(&mut state, now, key(KeyCode::F(5)));
    press(&mut state, now, key(KeyCode::Tab));
    press(&mut state, now, key(KeyCode::Char(' ')));
    let Some(Overlay::Prompt(prompt)) = &state.overlay else { panic!("expected prompt") };
    assert!(prompt.option_focused);
    assert_eq!(prompt.input.text(), "/tmp");
    assert!(matches!(prompt.purpose, crate::PromptPurpose::Copy { optimize_hdd: true, .. }));
    press(&mut state, now, key(KeyCode::BackTab));
    press(&mut state, now, ctrl('u'));
    type_text(&mut state, now, "/tmp/backup");
    let effects = press(&mut state, now, key(KeyCode::Enter));
    assert!(
        matches!(&effects[..], [Effect::StartJob { spec: Spec::Copy { target, optimize_hdd: true, .. }, .. }] if target == &PathBuf::from("/tmp/backup"))
    );
    press(&mut state, now, key(KeyCode::F(5)));
    let Some(Overlay::Prompt(prompt)) = &state.overlay else { panic!("expected prompt") };
    assert!(!prompt.option_focused);
    assert!(matches!(prompt.purpose, crate::PromptPurpose::Copy { optimize_hdd: false, .. }));
    update(&mut state, Msg::Pointer(crate::Pointer::PromptField { option: true }), now);
    let effects = press(&mut state, now, key(KeyCode::Enter));
    assert!(matches!(&effects[..], [Effect::StartJob { spec: Spec::Copy { optimize_hdd: true, .. }, .. }]));
}

fn start_copy(state: &mut AppState, now: Instant) -> u64 {
    press(state, now, key(KeyCode::Down));
    press(state, now, key(KeyCode::F(5)));
    let effects = press(state, now, key(KeyCode::Enter));
    let Some(Effect::StartJob { id, .. }) = effects.first() else { panic!("expected a job") };
    *id
}

#[test]
fn job_progress_conflicts_and_completion() {
    let (mut state, now) = setup_with(vec![file("a")]);
    let id = start_copy(&mut state, now);
    let progress = Progress {
        done: Totals { files: 0, bytes: 50 },
        total: Totals { files: 1, bytes: 200 },
        current: PathBuf::from("/home/a"),
    };
    update(&mut state, Msg::Job { id, event: Event::Progress(progress) }, now);
    assert_eq!(state.jobs[0].fraction(), Some(0.25));

    let ask = Ask::Conflict { source: "/home/a".into(), target: "/tmp/a".into(), sizes: (200, 10), newer: true };
    update(&mut state, Msg::Job { id, event: Event::Asking(ask) }, now);
    assert!(matches!(state.overlay, Some(Overlay::Question(question)) if question.id == id));
    let effects = press(&mut state, now, key(KeyCode::Char('k')));
    assert_eq!(effects, vec![Effect::JobControl { id, command: JobCommand::Answer(Decision::KeepBoth) }]);
    assert!(state.overlay.is_none());

    let done = Outcome::Done { done: Totals { files: 1, bytes: 200 }, skipped: 0 };
    let effects = update(&mut state, Msg::Job { id, event: Event::Finished(done) }, now);
    assert!(state.jobs.is_empty());
    assert_eq!(state.notice.as_deref(), Some("Copied 1 file"));
    assert_eq!(effects.iter().filter(|e| matches!(e, Effect::List { .. })).count(), 2);
}

#[test]
fn questions_wait_for_open_dialogs_and_esc_cancels_the_job() {
    let (mut state, now) = setup_with(vec![file("a")]);
    let id = start_copy(&mut state, now);
    press(&mut state, now, key(KeyCode::F(7)));
    let failure = Ask::Failure { path: "/home/a".into(), message: "denied".into() };
    update(&mut state, Msg::Job { id, event: Event::Asking(failure) }, now);
    assert!(matches!(state.overlay, Some(Overlay::Prompt(_))));
    press(&mut state, now, key(KeyCode::Esc));
    assert!(matches!(state.overlay, Some(Overlay::Question(_))));
    let effects = press(&mut state, now, key(KeyCode::Esc));
    assert_eq!(effects, vec![Effect::JobControl { id, command: JobCommand::Answer(Decision::Cancel) }]);
}

#[test]
fn jobs_list_pauses_and_cancels_and_quitting_asks_first() {
    let (mut state, now) = setup_with(vec![file("a")]);
    let id = start_copy(&mut state, now);
    crate::actions::perform(&mut state, Action::Jobs, now);
    assert_eq!(
        press(&mut state, now, key(KeyCode::Char('p'))),
        vec![Effect::JobControl { id, command: JobCommand::Pause(true) }]
    );
    assert!(state.jobs[0].paused);
    assert_eq!(
        press(&mut state, now, key(KeyCode::Char('c'))),
        vec![Effect::JobControl { id, command: JobCommand::Cancel }]
    );
    press(&mut state, now, key(KeyCode::Esc));

    assert!(press(&mut state, now, key(KeyCode::F(10))).is_empty());
    assert!(matches!(state.overlay, Some(Overlay::Confirm(_))));
    let effects = press(&mut state, now, key(KeyCode::Char('y')));
    assert_eq!(effects, vec![Effect::JobControl { id, command: JobCommand::Cancel }, Effect::Quit]);
}

#[test]
fn slow_operations_reveal_the_shell_and_return_afterwards() {
    let (mut state, now) = setup_with(vec![file("a")]);
    press(&mut state, now, key(KeyCode::F(7)));
    type_text(&mut state, now, "slow");
    press(&mut state, now, key(KeyCode::Enter));
    assert_eq!(state.wake_at(), Some(now + crate::SPINNER_DELAY), "the footer spinner is due first");
    update(&mut state, Msg::Tick, now + crate::SPINNER_DELAY);
    assert_eq!(state.wake_at(), Some(now + REVEAL_DELAY));

    update(&mut state, Msg::Tick, now + REVEAL_DELAY);
    assert_eq!(state.view, View::Shell);

    let effects = update(&mut state, Msg::Signal(ShellSignal::CommandEnd { exit: Some(0) }), now + REVEAL_DELAY);
    assert_eq!(state.view, View::Panels);
    assert_eq!(effects.iter().filter(|e| matches!(e, Effect::List { .. })).count(), 2);
}

#[test]
fn failed_operations_leave_a_notice() {
    let (mut state, now) = setup_with(vec![file("a")]);
    press(&mut state, now, key(KeyCode::F(7)));
    type_text(&mut state, now, "taken");
    press(&mut state, now, key(KeyCode::Enter));
    update(&mut state, Msg::Signal(ShellSignal::CommandEnd { exit: Some(1) }), now);
    assert_eq!(state.notice.as_deref(), Some("mkdir failed (exit 1) · Ctrl-O shows the output"));
}

#[test]
fn delete_confirms_and_can_be_cancelled() {
    let (mut state, now) = setup_with(vec![file("a")]);
    press(&mut state, now, key(KeyCode::Down));
    press(&mut state, now, key(KeyCode::F(8)));
    let Some(Overlay::Confirm(confirm)) = &state.overlay else { panic!("expected confirm") };
    assert_eq!(confirm.message, "Delete \"a\" permanently?");
    assert!(press(&mut state, now, key(KeyCode::Char('n'))).is_empty());

    press(&mut state, now, key(KeyCode::F(8)));
    let effects = press(&mut state, now, key(KeyCode::Char('y')));
    let spec = Spec::Delete { sources: vec![PathBuf::from("/home/a")] };
    assert_eq!(effects, vec![Effect::StartJob { id: 0, spec }]);
    assert_eq!(state.jobs[0].shown_on, Side::Left);
}

#[test]
fn mkdir_focuses_the_new_directory_after_it_is_created() {
    let (mut state, now) = setup_with(vec![file("a")]);
    press(&mut state, now, key(KeyCode::F(7)));
    type_text(&mut state, now, "fresh/inner");
    let effects = press(&mut state, now, key(KeyCode::Enter));
    assert_eq!(writes(&effects), " mkdir -p './fresh/inner'\r");

    let effects = update(&mut state, Msg::Signal(ShellSignal::CommandEnd { exit: Some(0) }), now);
    let Some(Effect::List { side, generation, .. }) =
        effects.iter().find(|e| matches!(e, Effect::List { side: Side::Left, .. })).cloned()
    else {
        panic!("expected relisting of active panel")
    };
    update(&mut state, Msg::Batch { side, generation, entries: vec![file("a"), dir("fresh")] }, now);
    update(&mut state, Msg::Listed { side, generation, error: None }, now);
    state.settle();
    assert_eq!(hovered(&state), Some("fresh"));
}

fn list_from(state: &mut AppState, now: Instant, effects: Vec<Effect>, entries: &[Entry]) {
    let Some(Effect::List { side, generation, .. }) = effects.into_iter().find(|e| matches!(e, Effect::List { .. }))
    else {
        panic!("expected a listing")
    };
    update(state, Msg::Batch { side, generation, entries: entries.to_vec() }, now);
    update(state, Msg::Listed { side, generation, error: None }, now);
}

#[test]
fn leaving_a_folder_shows_its_parent_scrolled_as_it_was() {
    const FOLDERS: usize = 60;
    let folders: Vec<Entry> = (0..FOLDERS).map(|i| dir(&format!("d{i:02}"))).collect();
    let (mut state, now) = setup_with(folders.clone());
    let rows = state.active_panel().rows();
    for _ in 0..rows * 2 {
        press(&mut state, now, key(KeyCode::Down));
    }
    for _ in 0..rows / 2 {
        press(&mut state, now, key(KeyCode::Up));
    }
    let view = |state: &AppState| (hovered(state).map(str::to_owned), state.active_panel().scroll());
    let before = view(&state);
    assert!(before.1 > 0, "the listing is scrolled with the cursor mid-screen");

    for leave in [key(KeyCode::Enter), key(KeyCode::Left), KeyEvent::new(KeyCode::PageUp, KeyModifiers::CONTROL)] {
        let effects = press(&mut state, now, key(KeyCode::Enter));
        list_from(&mut state, now, effects, &folders);
        assert_eq!(state.active_panel().scroll(), 0, "a folder entered opens at the top");
        let effects = press(&mut state, now, leave);
        list_from(&mut state, now, effects, &folders);
        assert_eq!(view(&state), before, "{leave:?} returns to the same screen");
    }
}

#[test]
fn operations_wait_for_an_idle_shell() {
    let (mut state, now) = setup_with(vec![file("a")]);
    state.shell.running = true;
    press(&mut state, now, key(KeyCode::F(7)));
    type_text(&mut state, now, "later");
    assert!(press(&mut state, now, key(KeyCode::Enter)).is_empty());
    assert!(state.notice.is_some());
}

#[test]
fn compress_offers_archive_name_and_detects_format() {
    let (mut state, now) = setup_with(vec![file("a"), file("b")]);
    press(&mut state, now, key(KeyCode::Down));
    crate::actions::perform(&mut state, Action::Compress, now);
    let Some(Overlay::Prompt(prompt)) = &state.overlay else { panic!("expected prompt") };
    assert_eq!(prompt.input.text(), "a.tar.gz");

    press(&mut state, now, ctrl('u'));
    type_text(&mut state, now, "a.zip");
    let effects = press(&mut state, now, key(KeyCode::Enter));
    assert_eq!(writes(&effects), " zip -r -q './a.zip' './a'\r");
}

#[test]
fn menu_bar_navigates_and_runs_actions() {
    let (mut state, now) = setup_with(vec![file("a")]);
    press(&mut state, now, key(KeyCode::F(9)));
    assert!(matches!(state.overlay, Some(Overlay::Menu(cursor)) if cursor.menu == 0));

    press(&mut state, now, key(KeyCode::Down));
    press(&mut state, now, key(KeyCode::Enter));
    assert_eq!(state.active_panel().mode(), ListingMode::Brief);

    press(&mut state, now, key(KeyCode::F(9)));
    press(&mut state, now, key(KeyCode::Left));
    assert!(matches!(state.overlay, Some(Overlay::Menu(cursor)) if menus(&state)[cursor.menu].title == "Search"));
    press(&mut state, now, key(KeyCode::Esc));
    assert!(state.overlay.is_none());
}

#[test]
fn toggling_preferences_saves_config_and_changes_layout() {
    let (mut state, now) = setup();
    let rows_with_status = state.active_panel().rows();
    let effects = crate::actions::perform(&mut state, Action::ToggleStatusLine, now);
    assert_eq!(effects, vec![Effect::SaveConfig]);
    assert_eq!(state.active_panel().rows(), rows_with_status + 1);
}

#[test]
fn split_nudges_apply_immediately_and_clamp() {
    let (mut state, now) = setup();
    press(&mut state, now, KeyEvent::new(KeyCode::Right, KeyModifiers::ALT | KeyModifiers::SHIFT));
    assert!(state.split_target() > crate::EVEN_SPLIT);
    assert_ne!(state.pace(now), Some(crate::Pace::Smooth));

    (0..100).for_each(|_| {
        crate::actions::perform(&mut state, Action::NudgeSplit(10), now);
    });
    let layout = state.layout(now + std::time::Duration::from_secs(1));
    assert!(layout.panels[1].width >= 16);
}

#[test]
fn brief_mode_uses_left_and_right_to_change_columns() {
    let entries: Vec<Entry> = (0..60).map(|i| file(&format!("f{i:02}"))).collect();
    let (mut state, now) = setup_with(entries);
    crate::actions::perform(&mut state, Action::SetListing(Side::Left, ListingMode::Brief), now);
    let rows = state.active_panel().rows();
    press(&mut state, now, key(KeyCode::Right));
    assert_eq!(state.active_panel().cursor(), rows);
    assert_eq!(state.active_panel().dir(), Path::new("/home"));
}

#[test]
fn sorting_from_menu_reorders_panel() {
    let (mut state, now) = setup_with(vec![file("b.md"), file("a.rs")]);
    crate::actions::perform(&mut state, Action::SetSort(Side::Left, SortKey::Extension), now);
    let names: Vec<_> = state.active_panel().entries().iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["b.md", "a.rs"]);
    crate::actions::perform(&mut state, Action::ToggleReverse(Side::Left), now);
    let names: Vec<_> = state.active_panel().entries().iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["a.rs", "b.md"]);
}

#[test]
fn swap_panels_exchanges_directories() {
    let (mut state, now) = setup();
    press(&mut state, now, ctrl('u'));
    assert_eq!(state.panel(Side::Left).dir(), Path::new("/tmp"));
    assert_eq!(state.panel(Side::Right).dir(), Path::new("/home"));
}

fn alt_o() -> KeyEvent {
    KeyEvent::new(KeyCode::Char('o'), KeyModifiers::ALT)
}

fn listings(effects: &[Effect]) -> Vec<(Side, PathBuf)> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::List { side, dir, .. } => Some((*side, dir.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn alt_o_opens_this_panels_folder_in_the_other_panel_in_place_of_the_preview() {
    let (mut state, now) = setup();
    press(&mut state, now, ctrl('q'));
    let effects = press(&mut state, now, alt_o());
    assert_eq!(listings(&effects), [(Side::Right, PathBuf::from("/home"))]);
    assert_eq!((state.active, state.aside), (Side::Left, None));
    list(&mut state, now, &effects, vec![dir("me")]);
    assert_eq!(state.panel(Side::Right).dir(), Path::new("/home"));
}

#[test]
fn alt_o_goes_to_the_shell_while_a_command_is_typed() {
    let (mut state, now) = setup();
    type_text(&mut state, now, "ls");
    let effects = press(&mut state, now, alt_o());
    assert_eq!((listings(&effects), writes(&effects)), (Vec::new(), "\x1bo".to_owned()));
}

#[test]
fn alt_o_from_search_results_opens_the_folder_the_search_started_from() {
    let (mut state, now) = setup();
    let request = crate::SearchRequest {
        root: "/var/log".into(),
        query: "*.log".into(),
        hidden: false,
        options: crate::SearchOptions::default(),
        skip: None,
    };
    crate::results::start_search(&mut state, request);
    assert_eq!(listings(&press(&mut state, now, alt_o())), [(Side::Right, PathBuf::from("/var/log"))]);
    assert!(state.panel(Side::Left).is_results(), "the results stay");
}

#[test]
fn alt_o_from_a_collection_or_a_container_explains_why_nothing_opens() {
    let (mut state, now) = setup();
    crate::results::collection(&mut state, "Bundle", Side::Left);
    assert!(listings(&press(&mut state, now, alt_o())).is_empty());
    assert_eq!(state.notice.as_deref(), Some("A collection has no folder to open in the other panel."));

    let (mut state, now) = setup();
    let host = PathBuf::from("/home");
    state.panel_mut(Side::Left).mount = Some(crate::Mount { id: "c0ffee".into(), name: "web".into(), host });
    assert!(listings(&press(&mut state, now, alt_o())).is_empty());
    assert_eq!(state.notice.as_deref(), Some("Container and remote folders open in one panel only."));

    let (mut state, now) = setup();
    crate::mount_remote(&mut state, Side::Left, "prod".into(), "remote-id".into(), "Deploy".into());
    assert!(listings(&press(&mut state, now, alt_o())).is_empty());
    assert_eq!(state.notice.as_deref(), Some("Container and remote folders open in one panel only."));
}

#[test]
fn alt_o_leaves_a_container_or_a_remote_machine_in_the_other_panel() {
    let (mut state, now) = setup();
    let host = PathBuf::from("/tmp");
    state.panel_mut(Side::Right).mount = Some(crate::Mount { id: "c0ffee".into(), name: "web".into(), host });
    assert_eq!(listings(&press(&mut state, now, alt_o())), [(Side::Right, PathBuf::from("/home"))]);
    assert!(state.panel(Side::Right).mount.is_none());

    let (mut state, now) = setup();
    crate::mount_remote(&mut state, Side::Right, "prod".into(), "remote-id".into(), "Deploy".into());
    assert_eq!(listings(&press(&mut state, now, alt_o())).last(), Some(&(Side::Right, PathBuf::from("/home"))));
    assert!(state.panel(Side::Right).remote_mount.is_none());
    assert_eq!(state.panel(Side::Right).destination(), Path::new("/home"));
}

#[test]
fn results_in_the_other_panel_come_back_with_alt_shift_r_after_alt_o() {
    let (mut state, now) = setup();
    state.active = Side::Right;
    crate::results::collection(&mut state, "Bundle", Side::Right);
    press(&mut state, now, key(KeyCode::Tab));
    let effects = press(&mut state, now, alt_o());
    list(&mut state, now, &effects, vec![dir("me")]);
    assert!(!state.panel(Side::Right).is_results());
    press(&mut state, now, key(KeyCode::Tab));
    press(&mut state, now, KeyEvent::new(KeyCode::Char('R'), KeyModifiers::ALT | KeyModifiers::SHIFT));
    assert!(state.panel(Side::Right).is_results());
}

#[test]
fn alt_o_leaves_an_editor_in_the_other_panel_alone() {
    let (mut state, now) = setup_with(vec![file("notes.txt")]);
    press(&mut state, now, key(KeyCode::Tab));
    press(&mut state, now, key(KeyCode::Down));
    let path = PathBuf::from("/tmp/notes.txt");
    let slot = crate::EditorSlot::Panel(Side::Right);
    let alt_e = KeyEvent::new(KeyCode::Char('e'), KeyModifiers::ALT);
    assert_eq!(press(&mut state, now, alt_e), [Effect::OpenEditor { path: path.clone(), slot }]);
    let text = String::from("hello");
    let stamp = crate::FileStamp { len: text.len() as u64, modified: None, inode: 1 };
    update(&mut state, Msg::EditorOpened(slot, Ok(crate::EditorFile { path, text, stamp })), now);
    press(&mut state, now, key(KeyCode::F(6)));
    assert_eq!(state.active, Side::Left);
    assert!(listings(&press(&mut state, now, alt_o())).is_empty());
    assert_eq!(state.notice.as_deref(), Some("The other panel holds an editor. Close it to open the folder there."));
    assert_eq!(state.panel(Side::Right).dir(), Path::new("/tmp"));
}

#[test]
fn enter_on_regular_file_opens_viewer() {
    let (mut state, now) = setup_with(vec![file("log.txt")]);
    press(&mut state, now, key(KeyCode::Down));
    let effects = press(&mut state, now, key(KeyCode::Enter));
    assert_eq!(effects, vec![Effect::View { path: PathBuf::from("/home/log.txt"), at: None }]);
}

#[test]
fn alt_e_edits_the_hovered_file_in_its_own_panel() {
    let (mut state, now) = setup_with(vec![dir("src"), file("log.txt")]);
    let alt_e = KeyEvent::new(KeyCode::Char('e'), KeyModifiers::ALT);
    press(&mut state, now, key(KeyCode::Down));
    assert_eq!(press(&mut state, now, alt_e), Vec::new(), "a folder opens in no editor");
    press(&mut state, now, key(KeyCode::Down));
    let effects = press(&mut state, now, alt_e);
    let slot = crate::EditorSlot::Panel(state.active);
    assert_eq!(effects, vec![Effect::OpenEditor { path: PathBuf::from("/home/log.txt"), slot }]);
}

#[test]
fn alt_enter_inserts_every_marked_name() {
    let (mut state, now) = setup_with(vec![file("a b"), file("c")]);
    press(&mut state, now, key(KeyCode::Down));
    press(&mut state, now, key(KeyCode::Insert));
    press(&mut state, now, key(KeyCode::Insert));
    let effects = press(&mut state, now, KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT));
    assert_eq!(writes(&effects), "'a b' 'c' ");
}

/// Lists `entries` into the folder `effects` asked to list.
fn list(state: &mut AppState, now: Instant, effects: &[Effect], entries: Vec<Entry>) {
    let Some(&Effect::List { side, generation, .. }) = effects.iter().find(|e| matches!(e, Effect::List { .. })) else {
        panic!("expected a listing in {effects:?}")
    };
    update(state, Msg::Batch { side, generation, entries }, now);
    update(state, Msg::Listed { side, generation, error: None }, now);
}

#[test]
fn alt_enter_on_the_parent_row_types_the_parent_folders_path() {
    let (mut state, now) = setup_with(vec![dir("my notes")]);
    press(&mut state, now, key(KeyCode::Down));
    let effects = press(&mut state, now, key(KeyCode::Enter));
    list(&mut state, now, &effects, vec![file("a b")]);
    press(&mut state, now, key(KeyCode::Home));
    let alt_enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT);
    assert_eq!(writes(&press(&mut state, now, alt_enter)), " cd '/home/my notes'\r'/home' ");

    press(&mut state, now, key(KeyCode::Down));
    press(&mut state, now, key(KeyCode::Insert));
    press(&mut state, now, key(KeyCode::Up));
    assert_eq!(writes(&press(&mut state, now, alt_enter)), "'a b' ", "marked names win over the parent row");
}

#[test]
fn alt_enter_on_the_parent_row_of_a_containers_root_types_nothing() {
    let (mut state, now) = setup();
    let host = PathBuf::from("/home");
    state.panel_mut(Side::Left).mount = Some(crate::Mount { id: "c0ffee".into(), name: "web".into(), host });
    let effect = state.navigate(Side::Left, PathBuf::from("/"), None);
    list(&mut state, now, &[effect], vec![dir("etc")]);
    press(&mut state, now, key(KeyCode::Home));
    assert_eq!(writes(&press(&mut state, now, KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT))), "");
}

#[test]
fn ctrl_insert_copies_marked_names_and_alt_shift_insert_their_paths() {
    let (mut state, now) = setup_with(vec![file("a b"), file("c")]);
    press(&mut state, now, key(KeyCode::Down));
    press(&mut state, now, key(KeyCode::Insert));
    press(&mut state, now, key(KeyCode::Insert));
    let effects = press(&mut state, now, KeyEvent::new(KeyCode::Insert, KeyModifiers::CONTROL));
    assert_eq!(effects, vec![Effect::Copy("a b\nc".into())]);
    let shift_alt = KeyModifiers::ALT | KeyModifiers::SHIFT;
    let effects = press(&mut state, now, KeyEvent::new(KeyCode::Insert, shift_alt));
    assert_eq!(effects, vec![Effect::Copy("/home/a b\n/home/c".into())]);
    assert_eq!(state.clipboard.entries().collect::<Vec<_>>(), ["/home/a b\n/home/c", "a b\nc"]);
}

#[test]
fn ctrl_v_pastes_borg_copies_into_its_fields_but_stays_the_shells_at_the_prompt() {
    let (mut state, now) = setup_with(vec![file("notes.txt")]);
    press(&mut state, now, key(KeyCode::Down));
    press(&mut state, now, KeyEvent::new(KeyCode::Insert, KeyModifiers::CONTROL));
    let effects = press(&mut state, now, KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL));
    assert_eq!(writes(&effects), "\x16", "the shell's quoted insert");
    press(&mut state, now, key(KeyCode::F(7)));
    press(&mut state, now, KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL));
    let Some(Overlay::Prompt(prompt)) = &state.overlay else { panic!("expected the folder prompt") };
    assert_eq!(prompt.input.text(), "notes.txt");
}

/// Runs the viewer's search as the app does, and hands back what it found.
fn run_viewer_search(state: &mut AppState, now: Instant, effects: &[Effect]) {
    let [Effect::FindInViewer { id, query, from, direction, count }] = effects else { panic!("{effects:?}") };
    let crate::Content::Text(pager) = &state.viewer.as_ref().unwrap().content else { panic!() };
    let found = crate::find_in(&*pager.source(), query, *from, *direction, *count, &|_| true);
    update(state, Msg::FoundInViewer { id: *id, found: found.map_err(|error| error.to_string()) }, now);
}

#[test]
fn the_viewer_finds_in_the_background_marks_matches_and_steps_both_ways() {
    use std::sync::Arc;

    let (mut state, now) = setup();
    let content: Vec<u8> = (1..=100).flat_map(|i| format!("row {i}\n").into_bytes()).collect();
    let (width, height) = crate::viewer_size(state.cols, state.rows);
    let pager = borg_view::Pager::new(Arc::new(content), width, height).unwrap_or_else(|e| panic!("{e}"));
    state.opening = Some(PathBuf::from("/home/log"));
    update(&mut state, Msg::Opened(crate::Viewer::new(PathBuf::from("/home/log"), crate::Content::Text(pager))), now);
    let current = |state: &AppState| text(state.viewer.as_ref().unwrap()).current_match();
    let summary = |state: &AppState| crate::find_bar_summary(&state.viewer.as_ref().unwrap().find);

    press(&mut state, now, key(KeyCode::Char('/')));
    let mut effects = Vec::new();
    for c in "row 5".chars() {
        effects = press(&mut state, now, key(KeyCode::Char(c)));
    }
    assert!(
        text(state.viewer.as_ref().unwrap()).rich_rows()[4].iter().any(|run| run.mark == borg_view::Mark::Match),
        "the page marks row 5 while the search runs"
    );
    run_viewer_search(&mut state, now, &effects);
    assert_eq!(current(&state), Some(24..29));
    assert_eq!(summary(&state).as_deref(), Some("1 of 11"), "row 5 and rows 50 to 59");
    let effects = press(&mut state, now, key(KeyCode::Enter));
    run_viewer_search(&mut state, now, &effects);
    let top = format!("row {}", 50 - height / 3);
    assert_eq!(text(state.viewer.as_ref().unwrap()).rows()[0], top, "row 50 shows a third of the page down");
    assert_eq!(summary(&state).as_deref(), Some("2 of 11"));
    let effects = press(&mut state, now, key(KeyCode::Up));
    run_viewer_search(&mut state, now, &effects);
    assert_eq!(current(&state), Some(24..29), "Up steps back");
    let effects = press(&mut state, now, key(KeyCode::Up));
    run_viewer_search(&mut state, now, &effects);
    assert_eq!(summary(&state).as_deref(), Some("11 of 11 · from the other end"));

    assert!(press(&mut state, now, key(KeyCode::Esc)).is_empty());
    let viewer = state.viewer.as_ref().unwrap();
    assert!(!viewer.find.open && !text(viewer).shows_matches(), "Esc closes the bar and takes the marks away");
    assert_eq!(state.find_recent, ["row 5"]);
    assert_eq!(current(&state), Some(397..402), "the match stepped to, row 59, stays marked");
    let effects = press(&mut state, now, key(KeyCode::Char('n')));
    assert!(text(state.viewer.as_ref().unwrap()).shows_matches(), "n marks the matches again");
    let stale = effects.clone();
    run_viewer_search(&mut state, now, &effects);
    assert_eq!(summary(&state).as_deref(), Some("1 of 11 · from the other end"), "n goes on from row 59");
    run_viewer_search(&mut state, now, &stale);
    assert_eq!(current(&state), Some(24..29), "an answer the viewer no longer waits for changes nothing");

    let effects = press(&mut state, now, shift(KeyCode::Char('N')));
    assert!(matches!(effects[..], [Effect::FindInViewer { direction: crate::Direction::Backward, .. }]));
    assert_eq!(press(&mut state, now, key(KeyCode::Esc)), [Effect::StopFindInViewer]);
    assert!(state.viewer.is_some(), "Esc stops a running search before it closes the viewer");
    press(&mut state, now, key(KeyCode::Down));
    assert_eq!(summary(&state), None, "a key that moves the page clears what the last step said");
    assert!(press(&mut state, now, key(KeyCode::Esc)).is_empty());
    assert!(state.viewer.is_none());
}

#[test]
fn h_shows_the_viewers_file_as_hex_rows_that_scroll_and_find_and_the_preview_keeps_them() {
    use borg_view::Mark;
    use std::sync::Arc;

    let (mut state, now) = setup_with(vec![file("log")]);
    let content: Vec<u8> = (1..=200).flat_map(|i| format!("row {i}\n").into_bytes()).collect();
    press(&mut state, now, key(KeyCode::Down));
    let effects = press(&mut state, now, ctrl('q'));
    let Some((path, at)) = peeked_at(&effects) else { panic!("{effects:?}") };
    let (width, height) = crate::viewer_size(state.cols, state.rows);
    let pager = borg_view::Pager::new(Arc::new(content), width, height).unwrap();
    let peeked = crate::Peeked { aside: crate::Aside::QuickView, path, at, peek: crate::Peek::Text(pager) };
    update(&mut state, Msg::Peeked(peeked), now);
    press(&mut state, now, key(KeyCode::F(3)));
    let shown = |state: &AppState| text(state.viewer.as_ref().unwrap()).clone();

    press(&mut state, now, key(KeyCode::Char('h')));
    assert!(shown(&state).hex());
    assert_eq!(shown(&state).rows()[0], "0000  72 6f 77 20 31 0a 72 6f 77 20 32 0a 72 6f 77 20  row 1.row 2.row ");
    press(&mut state, now, key(KeyCode::Down));
    assert_eq!(shown(&state).offset(), 16, "a row is 16 bytes in 80 columns");
    press(&mut state, now, key(KeyCode::PageDown));
    assert_eq!(shown(&state).offset(), 16 * height as u64);
    press(&mut state, now, key(KeyCode::End));
    assert_eq!(shown(&state).percent(), 100);
    press(&mut state, now, key(KeyCode::Home));
    press(&mut state, now, key(KeyCode::Char('w')));
    assert!(shown(&state).hex() && shown(&state).offset() == 0, "w leaves the hex rows");

    press(&mut state, now, key(KeyCode::Char('/')));
    let mut effects = Vec::new();
    for c in "row 150".chars() {
        effects = press(&mut state, now, key(KeyCode::Char(c)));
    }
    run_viewer_search(&mut state, now, &effects);
    let found = shown(&state);
    assert_eq!(found.current_match(), Some(1084..1091));
    assert_eq!(found.offset(), 1072 - 16 * (height as u64 / 3), "a third of the page shows above the match's row");
    let current: String = found
        .rich_rows()
        .iter()
        .flatten()
        .filter(|run| run.mark == Mark::Current)
        .map(|run| run.text.as_str())
        .collect();
    assert_eq!(current, "72 6f 77 20row 31 35 30150", "the match is marked in hex and in text, over two rows");
    assert_eq!(crate::find_bar_summary(&state.viewer.as_ref().unwrap().find).as_deref(), Some("1 of 1"));
    press(&mut state, now, key(KeyCode::Esc));

    press(&mut state, now, shift(KeyCode::Char('H')));
    assert!(!shown(&state).hex());
    assert_eq!(shown(&state).rows()[0], "row 134", "the line the top row starts in, at byte 960");
    press(&mut state, now, key(KeyCode::Char('р')));
    assert!(shown(&state).hex(), "the key in the same place on a Russian keyboard");
    press(&mut state, now, key(KeyCode::Esc));
    assert!(state.viewer.is_none());
    let Some(crate::Peeked { peek: crate::Peek::Text(preview), .. }) = &state.peeked else { panic!("preview") };
    assert!(preview.hex(), "the quick view keeps the hex rows");
    assert_eq!(preview.offset(), 944);
}

#[test]
fn a_binary_file_opens_as_hex_rows_and_h_shows_its_text() {
    use std::sync::Arc;

    let (mut state, now) = setup();
    let path = PathBuf::from("/home/app.bin");
    let (width, height) = crate::viewer_size(state.cols, state.rows);
    let bytes = b"\x7fELF\x02\x01\x01\x00\x00\x00text\n".to_vec();
    let pager = borg_view::Pager::for_path(Arc::new(bytes), &path, width, height).unwrap_or_else(|e| panic!("{e}"));
    state.opening = Some(path.clone());
    update(&mut state, Msg::Opened(crate::Viewer::new(path, crate::Content::Text(pager))), now);
    let shown = |state: &AppState| text(state.viewer.as_ref().unwrap()).clone();

    assert!(shown(&state).hex(), "a zero byte near the start opens the file as hex rows");
    let row = format!("0000  7f 45 4c 46 02 01 01 00 00 00 74 65 78 74 0a{:5}.ELF......text.", "");
    assert_eq!(shown(&state).rows()[0], row);
    press(&mut state, now, key(KeyCode::Char('h')));
    assert!(!shown(&state).hex(), "h shows the text");
    press(&mut state, now, key(KeyCode::Char('h')));
    assert_eq!(shown(&state).rows()[0], row);
}

/// A binary file of zeros with random blocks at `high`, of `blocks` blocks, as the viewer opens it.
fn packed_file(state: &mut AppState, now: Instant, blocks: u64, high: &[u64]) -> (Vec<u8>, Vec<Effect>) {
    use borg_view::entropy::BLOCK;
    let mut noise = 0x2545_f491_4f6c_dd1du64;
    let bytes: Vec<u8> = (0..blocks * BLOCK)
        .map(|at| {
            noise ^= noise << 13;
            noise ^= noise >> 7;
            noise ^= noise << 17;
            if high.contains(&(at / BLOCK)) { noise as u8 } else { 0 }
        })
        .collect();
    let path = PathBuf::from("/home/packed.bin");
    let (width, height) = crate::viewer_size(state.cols, state.rows);
    let source = std::sync::Arc::new(bytes.clone());
    let pager = borg_view::Pager::for_path(source, &path, width, height).unwrap_or_else(|e| panic!("{e}"));
    state.opening = Some(path.clone());
    let effects = update(state, Msg::Opened(crate::Viewer::new(path, crate::Content::Text(pager))), now);
    (bytes, effects)
}

#[test]
fn hex_rows_read_the_entropy_of_the_whole_file_and_jump_between_its_high_runs() {
    use borg_view::{
        HexLayout,
        entropy::{BLOCK, Profile},
    };

    let (mut state, now) = setup();
    let len = 13 * BLOCK;
    state.resize(HexLayout::full(len).width() as u16, 30, now);
    let (bytes, effects) = packed_file(&mut state, now, 13, &[1, 4, 5]);
    let [Effect::ProfileViewer { id }] = effects[..] else { panic!("{effects:?}") };
    let shown = |state: &AppState| text(state.viewer.as_ref().unwrap()).clone();
    assert!(shown(&state).hex());
    assert!(
        shown(&state).rows()[0].starts_with("0000  00 00 00 00 00 00 00 00  "),
        "the strip takes a column, so 16 bytes no longer fit: {:?}",
        shown(&state).rows()[0]
    );
    press(&mut state, now, key(KeyCode::Char(']')));
    assert_eq!(shown(&state).offset(), 0, "nothing to jump to before the entropy was read");

    let profile = Profile::read(&bytes, &|| true).unwrap().unwrap();
    update(&mut state, Msg::ViewerProfiled { id: id + 1, profile: Ok(profile.clone()) }, now);
    assert_eq!(state.viewer.as_ref().unwrap().profile(), None, "a read the viewer did not ask for is dropped");
    update(&mut state, Msg::ViewerProfiled { id, profile: Ok(profile.clone()) }, now);
    assert_eq!(state.viewer.as_ref().unwrap().profile(), Some(&profile));

    press(&mut state, now, key(KeyCode::Char(']')));
    assert_eq!(shown(&state).offset(), BLOCK);
    press(&mut state, now, key(KeyCode::Char(']')));
    assert_eq!(shown(&state).offset(), 4 * BLOCK, "the run of two blocks starts at the fifth");
    press(&mut state, now, key(KeyCode::Char(']')));
    assert_eq!(shown(&state).offset(), 4 * BLOCK, "no run after the last");
    press(&mut state, now, key(KeyCode::Char('[')));
    assert_eq!(shown(&state).offset(), BLOCK);

    let effects = update(&mut state, Msg::Pointer(crate::Pointer::EntropyStrip { cell: 1, cells: 4 }), now);
    assert!(effects.is_empty());
    assert_eq!(shown(&state).offset(), len / 4, "a click on the strip scrolls to where its cell starts");

    assert!(press(&mut state, now, key(KeyCode::Char('h'))).is_empty());
    assert!(!shown(&state).hex());
    assert!(press(&mut state, now, key(KeyCode::Char('h'))).is_empty(), "the entropy is read once");
    assert!(press(&mut state, now, key(KeyCode::Esc)).is_empty());
}

#[test]
fn closing_the_viewer_stops_reading_entropy_and_a_failed_read_says_why() {
    let (mut state, now) = setup();
    let (_, effects) = packed_file(&mut state, now, 2, &[1]);
    assert!(matches!(effects[..], [Effect::ProfileViewer { .. }]));
    assert_eq!(press(&mut state, now, key(KeyCode::Esc)), [Effect::StopProfileViewer]);

    let (_, effects) = packed_file(&mut state, now, 2, &[1]);
    let [Effect::ProfileViewer { id }] = effects[..] else { panic!("{effects:?}") };
    update(&mut state, Msg::ViewerProfiled { id, profile: Err("Input/output error".into()) }, now);
    assert_eq!(state.notice.as_deref(), Some("viewer: Input/output error"));
    assert!(state.viewer.is_some(), "the hex rows stay");
    assert!(press(&mut state, now, key(KeyCode::Char('h'))).is_empty());
    assert!(press(&mut state, now, key(KeyCode::Char('h'))).is_empty(), "a read that failed is not tried again");
}

#[test]
fn a_binary_file_previews_as_hex_rows_and_h_switches_the_quick_view_and_the_viewer_it_opens() {
    use std::sync::Arc;

    let (mut state, now) = setup_with(vec![file("app.bin")]);
    press(&mut state, now, key(KeyCode::Down));
    let effects = press(&mut state, now, ctrl('q'));
    let Some((path, at)) = peeked_at(&effects) else { panic!("{effects:?}") };
    let Some(Effect::Peek { size: (width, height), .. }) = effects.iter().find(|e| matches!(e, Effect::Peek { .. }))
    else {
        panic!("{effects:?}")
    };
    let bytes = b"\x7fELF\x02\x01\x01\x00\x00\x00text\n".to_vec();
    let pager = borg_view::Pager::for_path(Arc::new(bytes), &path, *width, *height).unwrap_or_else(|e| panic!("{e}"));
    let peeked = crate::Peeked { aside: crate::Aside::QuickView, path, at, peek: crate::Peek::Text(pager) };
    update(&mut state, Msg::Peeked(peeked), now);
    let preview = |state: &AppState| match &state.peeked {
        Some(crate::Peeked { peek: crate::Peek::Text(pager), .. }) => pager.clone(),
        peeked => panic!("{peeked:?}"),
    };

    assert!(preview(&state).hex(), "a zero byte near the start previews the file as hex rows");
    assert!(preview(&state).rows()[0].starts_with("0000  7f 45 4c 46"));
    press(&mut state, now, key(KeyCode::Tab));
    press(&mut state, now, key(KeyCode::Char('h')));
    assert!(!preview(&state).hex(), "h in the quick view shows the text");
    press(&mut state, now, key(KeyCode::Char('р')));
    assert!(preview(&state).hex(), "the key in the same place on a Russian keyboard");
    press(&mut state, now, key(KeyCode::Char('h')));

    press(&mut state, now, key(KeyCode::F(3)));
    assert!(!text(state.viewer.as_ref().unwrap()).hex(), "the viewer opens on the text the quick view showed");
    press(&mut state, now, key(KeyCode::Char('h')));
    press(&mut state, now, key(KeyCode::Esc));
    assert!(state.viewer.is_none());
    assert!(preview(&state).hex(), "the quick view gets the hex rows the viewer showed");
}

/// A contents search for `query` that found its first match in `/home/log` at `line`.
fn found_in_log(state: &mut AppState, now: Instant, query: &borg_view::Query, line: crate::LineMatch) {
    found_in(state, now, query, vec![("log", line)]);
}

/// A contents search for `query` that found its first match in each file of `/home` named, at its line.
fn found_in(state: &mut AppState, now: Instant, query: &borg_view::Query, files: Vec<(&str, crate::LineMatch)>) {
    let request = crate::SearchRequest {
        root: "/home".into(),
        query: "*".into(),
        hidden: false,
        options: crate::SearchOptions {
            kind: crate::SearchKind::Files,
            contents: Some(Box::new(query.clone())),
            ..Default::default()
        },
        skip: None,
    };
    let effects = crate::results::start_search(state, request);
    let [Effect::Search { side, generation, .. }] = effects[..] else { panic!("{effects:?}") };
    let found = files
        .into_iter()
        .map(|(name, line)| {
            let mut found = file(name);
            found.source = Some(Path::new("/home").join(name));
            (found, line)
        })
        .collect();
    update(state, Msg::Found { side, generation, found }, now);
    update(state, Msg::Listed { side, generation, error: None }, now);
}

#[test]
fn a_contents_search_lists_files_with_their_first_match_and_f3_lands_on_it_with_the_find_bar_ready() {
    use std::sync::Arc;

    let (mut state, now) = setup();
    let query = borg_view::Query { text: "row 5".into(), ..Default::default() };
    let line =
        crate::LineMatch { line: 5, offset: 24, before: String::new(), found: "row 5".into(), after: String::new() };
    found_in_log(&mut state, now, &query, line.clone());
    let results = state.active_panel().results.as_ref().unwrap();
    assert_eq!(results.title(), "Search: * containing \"row 5\"");
    assert_eq!(results.line(Path::new("/home/log")), Some(&line));
    assert_eq!(state.active_panel().hovered_entry().map(|entry| entry.identity()), Some(Path::new("/home/log")));

    let at = Some(crate::FirstMatch { query: query.clone(), offset: 24 });
    assert_eq!(press(&mut state, now, key(KeyCode::F(3))), [Effect::View { path: PathBuf::from("/home/log"), at }]);
    let content: Vec<u8> = (1..=100).flat_map(|i| format!("row {i}\n").into_bytes()).collect();
    let (width, height) = crate::viewer_size(state.cols, state.rows);
    let pager = borg_view::Pager::new(Arc::new(content), width, height).unwrap_or_else(|e| panic!("{e}"));
    let effects = update(
        &mut state,
        Msg::Opened(crate::Viewer::new(PathBuf::from("/home/log"), crate::Content::Text(pager))),
        now,
    );
    assert!(matches!(&effects[..], [Effect::FindInViewer { query: asked, from: 24, .. }] if *asked == query));
    let viewer = state.viewer.as_ref().unwrap();
    assert!(viewer.find.open && viewer.find.input.text() == "row 5", "the find bar is ready on the query");
    run_viewer_search(&mut state, now, &effects);
    assert_eq!(text(state.viewer.as_ref().unwrap()).current_match(), Some(24..29));
    assert_eq!(crate::find_bar_summary(&state.viewer.as_ref().unwrap().find).as_deref(), Some("1 of 11"));
    let effects = press(&mut state, now, key(KeyCode::Enter));
    run_viewer_search(&mut state, now, &effects);
    assert_eq!(text(state.viewer.as_ref().unwrap()).current_match(), Some(334..339), "Enter steps on to row 50");
}

#[test]
fn a_rules_search_lists_files_with_the_rules_they_match_and_f3_lands_on_the_evidence() {
    use borg_rules::{Evidence, Level, Problem, RuleMatch};

    let (mut state, now) = setup();
    let request = crate::SearchRequest {
        root: "/home".into(),
        query: "*".into(),
        hidden: false,
        options: crate::SearchOptions { kind: crate::SearchKind::Rules, ..Default::default() },
        skip: None,
    };
    let effects = crate::results::start_search(&mut state, request);
    let [Effect::Search { side, generation, .. }] = effects[..] else { panic!("{effects:?}") };
    let hit = |rule: &str, bytes: &[u8]| RuleMatch {
        rule: rule.into(),
        namespace: borg_rules::BUILT_IN.into(),
        level: Level::Warning,
        description: None,
        false_positives: None,
        evidence: Some(Evidence { string: "$a".into(), offset: 40, length: bytes.len() as u64, bytes: bytes.to_vec() }),
    };
    let mut script = file("install.sh");
    script.source = Some(PathBuf::from("/home/install.sh"));
    let mut packed = file("tool");
    packed.source = Some(PathBuf::from("/home/tool"));
    let found = vec![
        (script, vec![hit("decode_and_run", b"base64 -d | sh\nrm -f x")]),
        (packed, vec![hit("upx_packed", b"UPX!\x00\x01")]),
    ];
    update(&mut state, Msg::RuleHits { side, generation, found }, now);
    let left_out = vec![Problem { file: "/rules/broken.yar".into(), message: "syntax error".into() }];
    let progress = crate::SearchProgress { rules: 16, left_out, ..Default::default() };
    update(&mut state, Msg::SearchProgress { side, generation, progress }, now);
    assert_eq!(state.notice.as_deref(), Some("Left out /rules/broken.yar: syntax error"));
    update(&mut state, Msg::Listed { side, generation, error: None }, now);

    let results = state.active_panel().results.as_ref().unwrap();
    assert!(results.by_rules());
    assert_eq!(results.title(), "Rule matches: *");
    let at = results.first_match(Path::new("/home/install.sh"));
    let text = borg_view::Query { text: "base64 -d | sh".into(), case: true, words: false, regex: false };
    assert_eq!(at, Some(crate::FirstMatch { query: text, offset: 40 }), "text up to its line break");
    let bytes = results.first_match(Path::new("/home/tool")).map(|at| at.query).unwrap();
    assert_eq!(
        (bytes.text.as_str(), bytes.regex),
        (r"(?-u:\x55\x50\x58\x21\x00\x01)", true),
        "bytes that are not text"
    );
    let search = bytes.compile().unwrap().unwrap();
    assert_eq!(search.in_line(b"..UPX!\x00\x01.."), [std::ops::Range { start: 2, end: 8 }]);

    let at = results.first_match(Path::new("/home/install.sh"));
    assert_eq!(
        press(&mut state, now, key(KeyCode::F(3))),
        [Effect::View { path: PathBuf::from("/home/install.sh"), at }]
    );
}

#[test]
fn a_search_for_packed_programs_shows_their_code_entropy_and_f3_opens_the_first_high_block_as_hex() {
    use borg_view::entropy::{BLOCK, Entropy, Tally};

    let (mut state, now) = setup();
    let request = crate::SearchRequest {
        root: "/home".into(),
        query: "*".into(),
        hidden: false,
        options: crate::SearchOptions { kind: crate::SearchKind::Packed, ..Default::default() },
        skip: None,
    };
    let effects = crate::results::start_search(&mut state, request);
    let [Effect::Search { side, generation, .. }] = effects[..] else { panic!("{effects:?}") };
    let noise = borg_view::entropy::Entropy::of(&(0..=255u8).cycle().take(4096).collect::<Vec<_>>());
    let mut tally = Tally::default();
    (1..5).for_each(|block| tally.add(block * BLOCK, if block == 1 { Entropy::default() } else { noise }));
    assert!(crate::looks_packed(&tally));
    let mut tool = file("tool");
    tool.source = Some(PathBuf::from("/home/tool"));
    update(&mut state, Msg::PackedHits { side, generation, found: vec![(tool, tally)] }, now);
    update(&mut state, Msg::Listed { side, generation, error: None }, now);

    let results = state.active_panel().results.as_ref().unwrap();
    assert!(results.by_entropy() && !results.by_rules());
    assert_eq!(results.title(), "Packed executables: *");
    assert_eq!(results.first_high(Path::new("/home/tool")), Some(2 * BLOCK));
    assert_eq!(results.first_match(Path::new("/home/tool")), None, "no query to find");

    let path = PathBuf::from("/home/tool");
    assert_eq!(press(&mut state, now, key(KeyCode::F(3))), [Effect::View { path: path.clone(), at: None }]);
    let bytes: Vec<u8> = (0..BLOCK * 6).map(|at| (at % 7) as u8).collect();
    let (width, height) = crate::viewer_size(state.cols, state.rows);
    let pager = borg_view::Pager::new(std::sync::Arc::new(bytes), width, height).unwrap();
    update(&mut state, Msg::Opened(crate::Viewer::new(path, crate::Content::Text(pager))), now);
    let viewer = state.viewer.as_ref().unwrap();
    assert!(text(viewer).hex(), "shown as hex rows");
    assert_eq!(text(viewer).offset(), 2 * BLOCK, "from the first high block");
}

#[test]
fn f4_on_a_contents_match_opens_the_editor_on_it_with_the_find_bar_ready() {
    let (mut state, now) = setup();
    let query = borg_view::Query { text: "retry".into(), words: true, ..Default::default() };
    let text: String = (1..=100)
        .map(|i| match i {
            70 => "retry once\r\n".to_owned(),
            90 => "then retry again\r\n".to_owned(),
            _ => format!("retrying {i}\r\n"),
        })
        .collect();
    let offset = text.find("retry once").unwrap() as u64;
    let line =
        crate::LineMatch { line: 70, offset, before: String::new(), found: "retry".into(), after: " once".into() };
    found_in_log(&mut state, now, &query, line);

    let path = PathBuf::from("/home/log");
    let slot = crate::EditorSlot::Screen;
    assert_eq!(press(&mut state, now, key(KeyCode::F(4))), [Effect::OpenEditor { path: path.clone(), slot }]);
    let stamp = crate::FileStamp { len: text.len() as u64, modified: None, inode: 1 };
    let file = crate::EditorFile { path: path.clone(), text, stamp };
    update(&mut state, Msg::EditorOpened(slot, Ok(file.clone())), now);
    let editor = state.editor.as_ref().unwrap();
    assert!(editor.find.open && editor.find.input.text() == "retry" && editor.find.words, "the bar takes the query");
    assert_eq!(editor.buffer.selected_text().as_deref(), Some("retry"));
    assert_eq!(editor.buffer.position(), (69, 5), "the offset counted the line endings the editor holds as \\n");
    let rows = usize::from(ROWS - crate::EDITOR_CHROME);
    assert_eq!(editor.top, 69 - rows / 3, "a third of the screen shows above the match, as in the viewer");
    assert_eq!(crate::find_bar_summary(&editor.find).as_deref(), Some("1 of 2"), "retrying is no whole word");
    press(&mut state, now, key(KeyCode::Enter));
    assert_eq!(state.editor.as_ref().unwrap().buffer.position(), (89, 10), "Enter steps on to line 90");

    press(&mut state, now, key(KeyCode::F(10)));
    assert!(state.editor.is_none());
    crate::editor::open_at(&mut state, path.clone(), slot, query, offset);
    crate::editor::open(&mut state, path, slot);
    update(&mut state, Msg::EditorOpened(slot, Ok(file)), now);
    let editor = state.editor.as_ref().unwrap();
    assert!(!editor.find.open && editor.buffer.cursor() == 0, "a plain open after it starts at the top");
}

#[test]
fn a_contents_match_the_editor_leaves_to_the_viewer_opens_there_on_the_match() {
    use std::sync::Arc;

    let (mut state, now) = setup();
    let path = PathBuf::from("/home/huge.log");
    let query = borg_view::Query { text: "needle".into(), ..Default::default() };
    let slot = crate::EditorSlot::Screen;
    crate::editor::open_at(&mut state, path.clone(), slot, query.clone(), 9);
    let effects = update(&mut state, Msg::EditorOpened(slot, Err(crate::OpenError::TooLarge(path.clone()))), now);
    let at = Some(crate::FirstMatch { query: query.clone(), offset: 9 });
    assert_eq!(effects, [Effect::View { path: path.clone(), at }]);
    let (width, height) = crate::viewer_size(state.cols, state.rows);
    let pager = borg_view::Pager::new(Arc::new(b"haystack\nneedle\n".to_vec()), width, height).unwrap();
    let effects = update(&mut state, Msg::Opened(crate::Viewer::new(path, crate::Content::Text(pager))), now);
    assert!(matches!(&effects[..], [Effect::FindInViewer { query: asked, from: 9, .. }] if *asked == query));
    run_viewer_search(&mut state, now, &effects);
    assert_eq!(text(state.viewer.as_ref().unwrap()).current_match(), Some(9..15));
}

/// Where the quick view's request in `effects` opens a file, if it asks for one.
fn peeked_at(effects: &[Effect]) -> Option<(PathBuf, Option<crate::FirstMatch>)> {
    effects.iter().find_map(|effect| match effect {
        Effect::Peek { aside: crate::Aside::QuickView, path, at, .. } => Some((path.clone(), at.clone())),
        _ => None,
    })
}

#[test]
fn ctrl_q_on_contents_results_previews_each_file_on_its_match_with_the_query_marked() {
    use borg_view::Mark;
    use std::sync::Arc;

    const HAY: usize = 400;
    let (mut state, now) = setup();
    let query = borg_view::Query { text: "needle".into(), ..Default::default() };
    let deep = "hay\n".repeat(HAY) + "a needle beside a needle\n" + &"hay\n".repeat(HAY);
    let offset = deep.find("needle").unwrap() as u64;
    let line = |line, offset| crate::LineMatch {
        line,
        offset,
        before: String::new(),
        found: "needle".into(),
        after: String::new(),
    };
    found_in(&mut state, now, &query, vec![("log", line(HAY as u64 + 1, offset)), ("notes", line(1, 2))]);

    let effects = press(&mut state, now, ctrl('q'));
    let at = crate::FirstMatch { query: query.clone(), offset };
    assert_eq!(peeked_at(&effects), Some((PathBuf::from("/home/log"), Some(at.clone()))));
    let Some(Effect::Peek { path, size, .. }) =
        effects.into_iter().find(|effect| matches!(effect, Effect::Peek { .. }))
    else {
        unreachable!()
    };
    let mut pager = borg_view::Pager::new(Arc::new(deep.into_bytes()), size.0, size.1).unwrap();
    crate::land_on_match(&mut pager, &at, &|_| true).unwrap();
    let row = size.1 / 3;
    assert_eq!(pager.rows()[row], "a needle beside a needle", "a third of the preview shows above the match");
    let marks: Vec<_> = pager.rich_rows()[row]
        .iter()
        .filter(|run| run.mark != Mark::None)
        .map(|run| (run.text.as_str(), run.mark))
        .collect();
    assert_eq!(marks, [("needle", Mark::Current), ("needle", Mark::Match)], "as the viewer's find bar marks them");

    let top = crate::Peeked {
        aside: crate::Aside::QuickView,
        path: path.clone(),
        at: None,
        peek: crate::Peek::Text(pager.clone()),
    };
    update(&mut state, Msg::Peeked(top), now);
    assert!(state.peeked.is_none(), "a preview of the file from its top is not the one asked for");
    let landed =
        crate::Peeked { aside: crate::Aside::QuickView, path, at: Some(at.clone()), peek: crate::Peek::Text(pager) };
    update(&mut state, Msg::Peeked(landed), now);
    assert!(peeked_at(&update(&mut state, Msg::Tick, now)).is_none() && state.peeked.is_some());

    press(&mut state, now, key(KeyCode::F(3)));
    assert!(state.viewer.as_ref().is_some_and(|viewer| viewer.find.open), "F3 takes the preview to the viewer");
    press(&mut state, now, key(KeyCode::Esc));
    press(&mut state, now, key(KeyCode::Esc));
    let Some(crate::Peeked { at: back, peek: crate::Peek::Text(pager), .. }) = &state.peeked else { panic!("preview") };
    assert_eq!(back.as_ref(), Some(&at));
    assert!(pager.shows_matches() && pager.current_match().is_none(), "the preview marks the search's matches again");

    let next = crate::FirstMatch { query, offset: 2 };
    let effects = press(&mut state, now, key(KeyCode::Down));
    assert_eq!(peeked_at(&effects), Some((PathBuf::from("/home/notes"), Some(next))), "the preview follows the cursor");
    let effects = press(&mut state, now, key(KeyCode::Esc));
    assert!(!state.active_panel().is_results());
    assert!(peeked_at(&effects).is_some_and(|(_, at)| at.is_none()), "the folder's files open at the top");
    press(&mut state, now, key(KeyCode::Esc));
    assert_eq!(state.aside, None, "Esc closes the quick view");
}

#[test]
fn alt_e_on_results_edits_beside_them_and_closing_gives_back_the_results_and_quick_view() {
    let (mut state, now) = setup();
    let query = borg_view::Query { text: "retry".into(), ..Default::default() };
    let text = "first line\nretry once\n".to_owned();
    let offset = text.find("retry").unwrap() as u64;
    let line =
        crate::LineMatch { line: 2, offset, before: String::new(), found: "retry".into(), after: " once".into() };
    found_in_log(&mut state, now, &query, line);
    let effects = press(&mut state, now, ctrl('q'));
    let Some((shown, at)) = peeked_at(&effects) else { panic!("{effects:?}") };
    let pager = borg_view::Pager::new(std::sync::Arc::new(text.clone().into_bytes()), 30, 10).unwrap();
    let peek = crate::Peek::Text(pager);
    update(&mut state, Msg::Peeked(crate::Peeked { aside: crate::Aside::QuickView, path: shown, at, peek }), now);

    let alt_e = KeyEvent::new(KeyCode::Char('e'), KeyModifiers::ALT);
    let path = PathBuf::from("/home/log");
    let slot = crate::EditorSlot::Panel(Side::Right);
    assert_eq!(press(&mut state, now, alt_e), [Effect::OpenEditor { path: path.clone(), slot }]);
    let stamp = crate::FileStamp { len: text.len() as u64, modified: None, inode: 1 };
    let file = crate::EditorFile { path, text, stamp };
    update(&mut state, Msg::EditorOpened(slot, Ok(file.clone())), now);
    assert_eq!((state.active, crate::editor::focused(&state)), (Side::Right, Some(slot)));
    assert!(state.panel(Side::Left).is_results() && state.aside.is_none(), "the editor takes the quick view's place");
    let editor = state.docked[1].as_ref().unwrap();
    assert!(editor.find.open && editor.buffer.selected_text().as_deref() == Some("retry"), "on the match, as F4");

    press(&mut state, now, key(KeyCode::F(6)));
    assert_eq!((state.active, crate::editor::focused(&state)), (Side::Left, None), "F6 goes to the results");
    press(&mut state, now, ctrl('q'));
    assert_eq!(state.notice.as_deref(), Some("The other panel holds an editor; close it to show a preview"));
    press(&mut state, now, key(KeyCode::Tab));
    assert_eq!(crate::editor::focused(&state), Some(slot), "Tab goes back to the editor");

    press(&mut state, now, key(KeyCode::Esc));
    let effects = press(&mut state, now, key(KeyCode::Esc));
    assert!(state.docked[1].is_none(), "the first Esc closes the find bar and the second the editor");
    assert_eq!((state.active, state.aside), (Side::Left, Some(crate::Aside::QuickView)));
    let at = crate::FirstMatch { query, offset };
    let back = Some((PathBuf::from("/home/log"), Some(at)));
    assert_eq!(peeked_at(&effects), back, "the quick view comes back and reads the file the editor saved");

    press(&mut state, now, ctrl('q'));
    press(&mut state, now, alt_e);
    update(&mut state, Msg::EditorOpened(slot, Ok(file)), now);
    press(&mut state, now, key(KeyCode::F(10)));
    assert!(state.docked[1].is_none(), "F10 closes it too");
    assert_eq!((state.active, state.aside), (Side::Left, None), "with no quick view before, none comes back");
}

#[test]
fn alt_e_on_results_asks_about_unsaved_changes_beside_them_and_replaces_a_saved_file() {
    let (mut state, now) = setup();
    let query = borg_view::Query { text: "retry".into(), ..Default::default() };
    let line = |offset| crate::LineMatch {
        line: 1,
        offset,
        before: String::new(),
        found: "retry".into(),
        after: String::new(),
    };
    found_in(&mut state, now, &query, vec![("a.log", line(0)), ("b.log", line(6))]);
    press(&mut state, now, ctrl('q'));
    let alt_e = KeyEvent::new(KeyCode::Char('e'), KeyModifiers::ALT);
    let slot = crate::EditorSlot::Panel(Side::Right);
    let open = |state: &mut AppState, name: &str, text: &str| {
        let stamp = crate::FileStamp { len: text.len() as u64, modified: None, inode: 1 };
        let file = crate::EditorFile { path: Path::new("/home").join(name), text: text.into(), stamp };
        update(state, Msg::EditorOpened(slot, Ok(file)), now);
    };
    press(&mut state, now, alt_e);
    open(&mut state, "a.log", "retry\n");
    press(&mut state, now, key(KeyCode::F(6)));
    press(&mut state, now, key(KeyCode::Down));
    let path = PathBuf::from("/home/b.log");
    assert_eq!(press(&mut state, now, alt_e), [Effect::OpenEditor { path, slot }], "a saved file gives way");
    open(&mut state, "b.log", "again retry\n");
    assert_eq!(state.docked[1].as_ref().unwrap().buffer.selected_text().as_deref(), Some("retry"));

    press(&mut state, now, key(KeyCode::Esc));
    press(&mut state, now, key(KeyCode::Char('!')));
    press(&mut state, now, key(KeyCode::F(6)));
    press(&mut state, now, key(KeyCode::Up));
    assert!(press(&mut state, now, alt_e).is_empty(), "unsaved changes are not replaced");
    assert_eq!(state.active, Side::Right, "the editor with changes comes forward");
    let dialog = crate::editor::dialog(state.docked[1].as_ref().unwrap()).unwrap();
    assert_eq!(dialog.message.as_deref(), Some("Save the changes to b.log?"));
    press(&mut state, now, key(KeyCode::Char('d')));
    assert!(state.docked[1].is_none(), "Discard closes it");
    assert_eq!(
        (state.active, state.aside),
        (Side::Left, Some(crate::Aside::QuickView)),
        "as the first Alt-E left them"
    );
}

#[test]
fn landing_on_a_contents_match_in_markdown_shows_the_source_where_the_offset_was_counted() {
    let (mut state, now) = setup();
    let path = PathBuf::from("/home/plan.md");
    let query = borg_view::Query { text: "release".into(), words: true, ..Default::default() };
    let effects = crate::viewer::open_at(&mut state, path.clone(), query.clone(), 17);
    let at = Some(crate::FirstMatch { query, offset: 17 });
    assert_eq!(effects, [Effect::View { path: path.clone(), at }], "the load lands on the match");
    let source = b"# Plan\n\nShip the release.\nThen send the Release notes.\n".to_vec();
    let (width, height) = crate::viewer_size(state.cols, state.rows);
    let pager = borg_view::Pager::for_path(std::sync::Arc::new(source), &path, width, height).unwrap();
    assert!(pager.formatted());
    let effects = update(&mut state, Msg::Opened(crate::Viewer::new(path, crate::Content::Text(pager))), now);
    run_viewer_search(&mut state, now, &effects);
    let viewer = state.viewer.as_ref().unwrap();
    assert!(!text(viewer).formatted());
    assert_eq!(text(viewer).current_match(), Some(17..24));
    assert_eq!(crate::find_bar_summary(&viewer.find).as_deref(), Some("1 of 2"));
}

#[test]
fn viewer_keys_scroll_wrap_and_close() {
    use std::sync::Arc;

    let (mut state, now) = setup();
    let content: Vec<u8> = (1..=100).flat_map(|i| format!("row {i}\n").into_bytes()).collect();
    let (width, height) = crate::viewer_size(state.cols, state.rows);
    let pager = borg_view::Pager::new(Arc::new(content), width, height).unwrap_or_else(|e| panic!("{e}"));
    state.opening = Some(PathBuf::from("/home/log"));
    update(&mut state, Msg::Opened(crate::Viewer::new(PathBuf::from("/home/log"), crate::Content::Text(pager))), now);

    press(&mut state, now, key(KeyCode::End));
    let last = state.viewer.as_ref().and_then(|v| text(v).rows().last().cloned());
    assert_eq!(last.as_deref(), Some("row 100"));

    press(&mut state, now, key(KeyCode::Char('w')));
    assert!(state.viewer.as_ref().is_some_and(|v| !text(v).wrapped()));

    let effects = press(&mut state, now, key(KeyCode::Char('l')));
    assert!(effects.is_empty(), "viewer must not leak keys to the shell");
    press(&mut state, now, key(KeyCode::Esc));
    assert!(state.viewer.is_none());
}

fn secret() -> ThemeChoice {
    ThemeChoice { id: "cube".into(), name: "Cube".into() }
}

fn chars(state: &mut AppState, now: Instant, text: &str) -> Vec<Effect> {
    text.chars().flat_map(|c| press(state, now, key(KeyCode::Char(c)))).collect()
}

#[test]
fn startup_opens_panels_immediately() {
    let (state, now) = setup_from(vec![dir("me")], Settings::default());
    assert_eq!(state.view, View::Panels);
    assert_eq!(state.wake_at(), Some(now + state.settings.screensaver_after));
}

#[test]
fn hidden_2048_modal_owns_input_and_retains_the_board_when_closed() {
    let (mut state, now) = setup_with(vec![file("a")]);
    press(&mut state, now, key(KeyCode::F(1)));
    for digit in "2048".chars() {
        let effects = press(&mut state, now, key(KeyCode::Char(digit)));
        assert!(writes(&effects).is_empty());
    }
    assert!(matches!(state.overlay, Some(Overlay::Game2048)));
    assert!(state.game2048.is_some());
    for code in [KeyCode::Left, KeyCode::Up, KeyCode::Char('u'), KeyCode::Char('r'), KeyCode::Enter] {
        assert!(writes(&press(&mut state, now, key(code))).is_empty());
    }
    assert!(update(&mut state, Msg::Paste("echo do-not-run".into()), now).is_empty());
    let saved = state.game2048.as_ref().unwrap().saved();
    press(&mut state, now, key(KeyCode::Esc));
    assert!(state.overlay.is_none());
    assert_eq!(state.game2048.as_ref().unwrap().saved(), saved);
    press(&mut state, now, key(KeyCode::F(1)));
    type_text(&mut state, now, "2048");
    assert!(matches!(state.overlay, Some(Overlay::Game2048)));
    assert_eq!(state.game2048.as_ref().unwrap().saved(), saved);
}

#[test]
fn typing_borg_in_about_unlocks_the_secret_theme_once() {
    let (mut state, now) = setup();
    state.appearance = state.appearance.clone().with_secret(Some(secret()), Vec::new());
    press(&mut state, now, key(KeyCode::F(1)));
    assert!(matches!(state.overlay, Some(Overlay::About(_))));
    assert_eq!(state.pace(now + std::time::Duration::from_secs(1)), Some(crate::Pace::Ambient));

    assert_eq!(chars(&mut state, now, crate::UNLOCK_WORD), vec![Effect::SaveConfig]);
    assert_eq!(state.appearance.unlocked(), ["cube"]);
    assert!(chars(&mut state, now, crate::UNLOCK_WORD).is_empty());
    assert_eq!(state.appearance.choices().iter().filter(|c| c.id == "cube").count(), 1);

    press(&mut state, now, key(KeyCode::Esc));
    assert!(state.overlay.is_none());
}

#[test]
fn konami_code_in_about_starts_rain_for_a_while() {
    let (mut state, now) = setup();
    press(&mut state, now, key(KeyCode::F(1)));
    [
        KeyCode::Up,
        KeyCode::Up,
        KeyCode::Down,
        KeyCode::Down,
        KeyCode::Left,
        KeyCode::Right,
        KeyCode::Left,
        KeyCode::Right,
    ]
    .into_iter()
    .for_each(|code| {
        press(&mut state, now, key(code));
    });
    chars(&mut state, now, "ba");
    assert!(state.raining(now));
    let dry = now + crate::RAIN_DURATION;
    update(&mut state, Msg::Tick, dry);
    assert!(!state.raining(dry));
}

#[test]
fn screensaver_starts_after_idle_and_wakes_on_a_key() {
    let mut settings = quiet();
    settings.preferences.screensaver = true;
    let (mut state, now) = setup_from(vec![dir("me")], settings);
    let due = now + state.settings.screensaver_after;
    assert_eq!(state.wake_at(), Some(due));
    update(&mut state, Msg::Tick, due);
    assert!(state.screensaver);
    assert_eq!(state.saver_started, due);
    assert!(press(&mut state, due, key(KeyCode::Down)).is_empty());
    assert!(!state.screensaver);
    assert_eq!(state.active_panel().cursor(), 0);
}

#[test]
fn overlays_open_and_close_without_transitions() {
    let (mut state, now) = setup();
    press(&mut state, now, key(KeyCode::F(1)));
    assert_eq!(state.overlay_fade.value(now), 1.0);
    press(&mut state, now, key(KeyCode::Esc));
    assert!(state.leaving.is_none());
    assert_eq!(state.overlay_fade.value(now), 0.0);
}

fn image_viewer(state: &mut AppState, name: &str, now: Instant) {
    let path = state.active_panel().dir().join(name);
    state.opening = Some(path.clone());
    update(state, loaded(path), now);
}

fn loaded(path: PathBuf) -> Msg {
    let picture = borg_gfx::Picture::from_rgba(1, 1, vec![0, 0, 0, 255]).unwrap_or_else(|e| panic!("{e}"));
    let image = crate::Image { picture, original: (1, 1), format: "png", bytes: 1 };
    Msg::Opened(crate::Viewer::new(path, crate::Content::Image(image)))
}

#[test]
fn image_viewer_steps_through_pictures_in_the_folder() {
    let (mut state, now) = setup_with(vec![file("a.png"), file("b.txt"), file("c.jpg"), file("d.gif")]);
    image_viewer(&mut state, "a.png", now);

    let next = press(&mut state, now, key(KeyCode::Right));
    assert_eq!(next, vec![Effect::View { path: PathBuf::from("/home/c.jpg"), at: None }]);
    assert!(matches!(state.active_panel().hovered(), Some(crate::Row::Entry(entry)) if entry.name == "c.jpg"));

    image_viewer(&mut state, "d.gif", now);
    assert!(press(&mut state, now, key(KeyCode::Right)).is_empty());
    assert_eq!(
        press(&mut state, now, key(KeyCode::Left)),
        vec![Effect::View { path: PathBuf::from("/home/c.jpg"), at: None }]
    );

    press(&mut state, now, key(KeyCode::Esc));
    assert!(state.viewer.is_none());
}

#[test]
fn a_picture_loaded_after_the_viewer_gave_it_up_stays_away() {
    let (mut state, now) = setup_with(vec![file("a.png"), file("b.jpg"), file("c.gif")]);
    let path = |name: &str| PathBuf::from("/home").join(name);
    let shown = |state: &AppState| state.viewer.as_ref().and_then(|viewer| viewer.path().map(PathBuf::from));
    image_viewer(&mut state, "a.png", now);

    press(&mut state, now, key(KeyCode::Down));
    press(&mut state, now, key(KeyCode::Esc));
    assert!(state.viewer.is_none());
    update(&mut state, loaded(path("b.jpg")), now);
    assert_eq!(shown(&state), None, "Esc while it loaded gave it up");
    update(&mut state, Msg::OpenFailed { path: path("b.jpg"), notice: "unreadable".into() }, now);
    assert_eq!(state.notice, None);

    image_viewer(&mut state, "a.png", now);
    press(&mut state, now, key(KeyCode::Down));
    press(&mut state, now, key(KeyCode::Down));
    update(&mut state, loaded(path("c.gif")), now);
    update(&mut state, loaded(path("b.jpg")), now);
    assert_eq!(shown(&state), Some(path("c.gif")), "an earlier load that arrives late is dropped");
    press(&mut state, now, key(KeyCode::Esc));

    assert_eq!(press(&mut state, now, key(KeyCode::F(3))), vec![Effect::View { path: path("c.gif"), at: None }]);
    press(&mut state, now, key(KeyCode::Esc));
    update(&mut state, loaded(path("c.gif")), now);
    assert_eq!(shown(&state), None, "Esc in the panels gives up a file still opening");
}

fn ctrl_key(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

fn hovered_name(state: &AppState) -> Option<String> {
    state.active_panel().hovered_entry().map(|entry| entry.name.clone())
}

#[test]
fn quick_find_jumps_cycles_marks_and_remembers() {
    let (mut state, now) =
        setup_with(vec![file("app.js"), file("main.rs"), file("next.config.js"), file("package.json"), file("README")]);
    press(&mut state, now, ctrl_key('s'));
    assert!(matches!(state.overlay, Some(Overlay::Find(_))));

    type_text(&mut state, now, "js");
    assert_eq!(hovered_name(&state).as_deref(), Some("app.js"));
    let Some(Overlay::Find(find)) = state.overlay.clone() else { panic!("find closed") };
    assert_eq!(crate::find_summary(&state, &find), (1, 3));

    press(&mut state, now, key(KeyCode::Down));
    assert_eq!(hovered_name(&state).as_deref(), Some("next.config.js"));
    press(&mut state, now, key(KeyCode::Down));
    press(&mut state, now, key(KeyCode::Down));
    assert_eq!(hovered_name(&state).as_deref(), Some("app.js"));
    press(&mut state, now, key(KeyCode::Up));
    assert_eq!(hovered_name(&state).as_deref(), Some("package.json"));

    press(&mut state, now, ctrl_key('a'));
    assert_eq!(state.active_panel().marked_count(), 3);

    press(&mut state, now, key(KeyCode::Enter));
    assert!(state.overlay.is_none());
    assert_eq!(hovered_name(&state).as_deref(), Some("package.json"));

    press(&mut state, now, ctrl_key('s'));
    press(&mut state, now, ctrl_key('s'));
    let Some(Overlay::Find(find)) = state.overlay.clone() else { panic!("find closed") };
    assert_eq!(find.input.text(), "js");
}

#[test]
fn quick_find_needs_an_empty_command_line() {
    let (mut state, now) = setup_with(vec![file("a")]);
    state.shell.observed.prompt = PromptState::Typed;
    let effects = press(&mut state, now, ctrl_key('s'));
    assert!(state.overlay.is_none());
    assert_eq!(writes(&effects), "\u{13}");
}

#[test]
fn repeated_shell_directory_reports_do_not_pull_panels_back() {
    let (mut state, now) = setup();
    let shell_dir = state.shell.cwd.clone();
    press(&mut state, now, key(KeyCode::Tab));
    press(&mut state, now, key(KeyCode::Down));
    let effects = press(&mut state, now, key(KeyCode::Enter));
    assert!(matches!(effects.first(), Some(Effect::List { .. })));
    let entered = state.active_panel().destination().to_path_buf();

    let again = ShellSignal::Cwd { path: shell_dir, host: String::new() };
    assert!(update(&mut state, Msg::Signal(again), now).is_empty());
    assert_eq!(state.active_panel().destination(), entered);
}

#[test]
fn alt_digits_stand_in_for_function_keys() {
    let (mut state, now) = setup();
    press(&mut state, now, KeyEvent::new(KeyCode::Char('9'), KeyModifiers::ALT));
    assert!(matches!(state.overlay, Some(Overlay::Menu(_))));
    press(&mut state, now, key(KeyCode::Esc));
    let effects = press(&mut state, now, KeyEvent::new(KeyCode::Char('0'), KeyModifiers::ALT));
    assert!(effects.is_empty());
    assert!(matches!(state.overlay, Some(Overlay::Confirm(_))));
    assert_eq!(press(&mut state, now, key(KeyCode::Char('y'))), vec![Effect::Quit]);
}

#[test]
fn panels_come_back_when_a_full_screen_program_exits() {
    let (mut state, now) = setup();
    state.shell.integrated = true;
    type_text(&mut state, now, "vim");
    update(&mut state, Msg::Observe(ShellView { prompt: PromptState::Typed, ..idle_prompt() }), now);
    press(&mut state, now, key(KeyCode::Enter));
    assert_eq!(state.view, View::Shell);

    let editing = ShellView { foreground: Foreground::Program, mode: TermMode::ALT_SCREEN, ..idle_prompt() };
    update(&mut state, Msg::ShellOutput(editing), now);
    update(&mut state, Msg::ShellOutput(idle_prompt()), now);
    update(&mut state, Msg::Signal(ShellSignal::CommandEnd { exit: Some(0) }), now);
    assert_eq!(state.view, View::Panels);

    type_text(&mut state, now, "ls");
    update(&mut state, Msg::Observe(ShellView { prompt: PromptState::Typed, ..idle_prompt() }), now);
    press(&mut state, now, key(KeyCode::Enter));
    update(&mut state, Msg::Signal(ShellSignal::CommandEnd { exit: Some(0) }), now);
    assert_eq!(state.view, View::Panels, "commands return to panels by default");
}

#[test]
fn a_full_screen_program_typed_before_the_last_command_ended_still_brings_the_panels_back() {
    let (mut state, now) = setup();
    state.settings.preferences.after_command = crate::AfterCommand::ShowShell;
    state.shell.integrated = true;
    type_text(&mut state, now, "make");
    update(&mut state, Msg::Observe(ShellView { prompt: PromptState::Typed, ..idle_prompt() }), now);
    press(&mut state, now, key(KeyCode::Enter));
    press(&mut state, now, KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL));
    assert_eq!(state.view, View::Panels);
    type_text(&mut state, now, "vim");
    update(&mut state, Msg::Observe(ShellView { prompt: PromptState::Typed, ..idle_prompt() }), now);
    press(&mut state, now, key(KeyCode::Enter));

    for signal in [ShellSignal::CommandEnd { exit: Some(0) }, ShellSignal::PromptStart, ShellSignal::InputStart] {
        update(&mut state, Msg::Signal(signal), now);
    }
    let editing =
        ShellView { foreground: Foreground::Program, mode: TermMode::ALT_SCREEN, raw_input: true, ..idle_prompt() };
    update(&mut state, Msg::ShellOutput(editing), now);
    update(&mut state, Msg::ShellOutput(idle_prompt()), now);
    update(&mut state, Msg::Signal(ShellSignal::CommandEnd { exit: Some(0) }), now);
    assert_eq!(state.view, View::Panels);
}

#[test]
fn alt_a_types_folder_paths_onto_the_command_line() {
    let (mut state, now) = setup();
    let effects = press(&mut state, now, KeyEvent::new(KeyCode::Char('a'), KeyModifiers::ALT));
    assert_eq!(writes(&effects), "'/home' ");
    let effects = press(&mut state, now, KeyEvent::new(KeyCode::Char('A'), KeyModifiers::ALT | KeyModifiers::SHIFT));
    assert_eq!(writes(&effects), "'/tmp' ");
}

fn peeks(effects: &[Effect]) -> Vec<(crate::Aside, PathBuf)> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::Peek { aside, path, .. } => Some((*aside, path.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn quick_view_asks_for_the_hovered_file_once_and_follows_the_cursor() {
    let (mut state, now) = setup_with(vec![file("a.txt"), file("b.txt")]);
    press(&mut state, now, key(KeyCode::Home));
    let effects = press(&mut state, now, ctrl('q'));
    assert_eq!(state.aside, Some(crate::Aside::QuickView));
    assert_eq!(peeks(&effects), vec![(crate::Aside::QuickView, PathBuf::from("/"))], "the parent row shows its folder");
    assert!(peeks(&update(&mut state, Msg::Tick, now)).is_empty(), "a pending request is not repeated");

    let effects = press(&mut state, now, key(KeyCode::End));
    assert_eq!(peeks(&effects), vec![(crate::Aside::QuickView, PathBuf::from("/home/b.txt"))]);

    let stale = crate::Peeked {
        aside: crate::Aside::QuickView,
        path: PathBuf::from("/home/a.txt"),
        at: None,
        peek: crate::Peek::Text(borg_view::Pager::new(std::sync::Arc::new(b"old".to_vec()), 20, 5).unwrap()),
    };
    update(&mut state, Msg::Peeked(stale), now);
    assert!(state.peeked.is_none(), "results for a file the cursor left are dropped");

    let fresh = crate::Peeked {
        aside: crate::Aside::QuickView,
        path: PathBuf::from("/home/b.txt"),
        at: None,
        peek: crate::Peek::Text(borg_view::Pager::new(std::sync::Arc::new(b"new".to_vec()), 20, 5).unwrap()),
    };
    update(&mut state, Msg::Peeked(fresh), now);
    assert!(state.peeked.is_some());
    assert!(peeks(&update(&mut state, Msg::Tick, now)).is_empty());
    press(&mut state, now, key(KeyCode::Up));
    assert!(state.peeked.is_none(), "the previous file must disappear while the next preview loads");
}

#[test]
fn info_replaces_quick_view_and_toggles_off() {
    let (mut state, now) = setup_with(vec![file("a.txt")]);
    press(&mut state, now, ctrl('q'));
    let effects = press(&mut state, now, ctrl('l'));
    assert_eq!(state.aside, Some(crate::Aside::Info));
    assert!(peeks(&effects).iter().all(|(aside, _)| *aside == crate::Aside::Info));
    assert!(!peeks(&effects).is_empty());
    press(&mut state, now, ctrl('l'));
    assert_eq!(state.aside, None);
    assert!(state.peeked.is_none());
}

#[test]
fn tab_stays_on_the_listing_while_the_other_side_shows_a_preview() {
    let (mut state, now) = setup();
    press(&mut state, now, ctrl('q'));
    press(&mut state, now, key(KeyCode::Tab));
    assert_eq!(state.active, Side::Left);
}

#[test]
fn shift_enter_inserts_a_newline_without_opening_an_overlay_or_running() {
    for (mode, expected) in [(TermMode::BRACKETED_PASTE, "\x1b[200~\n\x1b[201~"), (TermMode::empty(), "\x16\n")] {
        let (mut state, now) = setup();
        update(&mut state, Msg::Observe(ShellView { mode, ..idle_prompt() }), now);
        let before = state.layout(now).panels[0].height;
        let effects = press(&mut state, now, shift(KeyCode::Enter));
        assert_eq!(writes(&effects), expected);
        assert!(state.overlay.is_none());
        assert!(!state.shell.running);
        assert!(!state.shell.line_empty());
        assert_eq!(state.view, View::Panels);
        update(
            &mut state,
            Msg::Observe(ShellView { mode, command_rows: 2, prompt: PromptState::Typed, ..idle_prompt() }),
            now,
        );
        assert_eq!(state.layout(now).panels[0].height, before - 1);
        assert_eq!(writes(&press(&mut state, now, ctrl('j'))), expected);
        let effects = press(&mut state, now, key(KeyCode::Enter));
        assert_eq!(writes(&effects), "\r");
        assert_eq!(state.view, View::Shell);
    }
}

#[test]
fn newline_shortcuts_are_forwarded_to_busy_programs() {
    let (mut state, now) = setup();
    update(&mut state, Msg::Observe(ShellView { foreground: Foreground::Program, ..idle_prompt() }), now);
    assert_eq!(writes(&press(&mut state, now, ctrl('j'))), "\n");
    assert_eq!(writes(&press(&mut state, now, shift(KeyCode::Enter))), "\r");
    assert!(state.overlay.is_none());
}

#[test]
fn menu_bar_reopens_where_it_was_left() {
    let (mut state, now) = setup();
    press(&mut state, now, key(KeyCode::F(9)));
    press(&mut state, now, key(KeyCode::Right));
    press(&mut state, now, key(KeyCode::Right));
    press(&mut state, now, key(KeyCode::Down));
    let Some(Overlay::Menu(left_at)) = state.overlay else { panic!("menu must be open") };
    press(&mut state, now, key(KeyCode::Esc));
    assert!(state.overlay.is_none());
    press(&mut state, now, key(KeyCode::F(9)));
    assert_eq!(state.overlay, Some(Overlay::Menu(left_at)));
}

#[test]
fn escape_then_digit_works_as_a_function_key() {
    let (mut state, now) = setup();
    assert!(press(&mut state, now, key(KeyCode::Esc)).is_empty());
    press(&mut state, now, key(KeyCode::Char('9')));
    assert!(matches!(state.overlay, Some(Overlay::Menu(_))), "Esc 9 opens the menu bar like F9");
    press(&mut state, now, key(KeyCode::Esc));
    assert!(state.overlay.is_none());

    press(&mut state, now, key(KeyCode::Esc));
    let later = now + std::time::Duration::from_secs(5);
    let effects = press(&mut state, later, key(KeyCode::Char('9')));
    assert_eq!(writes(&effects), "9", "a slow digit is just a digit");
}

#[test]
fn escape_reaches_the_shell_when_the_line_has_text() {
    let (mut state, now) = setup();
    type_text(&mut state, now, "ls");
    press(&mut state, now, key(KeyCode::Esc));
    let effects = press(&mut state, now, key(KeyCode::Char('b')));
    assert_eq!(writes(&effects), "\x1bb");
}

#[test]
fn mouse_selects_marks_and_opens_panel_rows() {
    use crate::Pointer;
    let (mut state, now) = setup_with(vec![dir("me"), file("log.txt")]);
    update(&mut state, Msg::Pointer(Pointer::Panel { side: Side::Right, index: 2, open: false, mark: true }), now);
    assert_eq!(state.active, Side::Right);
    assert_eq!(state.active_panel().hovered_entry().unwrap().name, "log.txt");
    assert_eq!(state.active_panel().marked_count(), 1);
    let effects =
        update(&mut state, Msg::Pointer(Pointer::Panel { side: Side::Right, index: 1, open: true, mark: false }), now);
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::List { side: Side::Right, dir, .. } if dir == Path::new("/tmp/me")))
    );
}

#[test]
fn mouse_menus_respect_disabled_items_and_modal_dialogs() {
    use crate::Pointer;
    let (mut state, now) = setup();
    update(&mut state, Msg::Pointer(Pointer::Menu(1)), now);
    update(&mut state, Msg::Pointer(Pointer::MenuItem(0)), now);
    assert!(matches!(state.overlay, Some(Overlay::Menu(_))), "View is disabled on the parent row");
    update(&mut state, Msg::Pointer(Pointer::MenuItem(6)), now);
    assert!(matches!(state.overlay, Some(Overlay::Prompt(_))), "Make directory opens its prompt");
    update(&mut state, Msg::Pointer(Pointer::Action(Action::Quit)), now);
    assert!(matches!(state.overlay, Some(Overlay::Prompt(_))));
}

#[test]
fn console_history_shortcuts_scroll_without_writing_to_the_shell() {
    use borg_term::Scroll;
    let (mut state, now) = setup();
    press(&mut state, now, ctrl('o'));
    assert_eq!(press(&mut state, now, shift(KeyCode::PageUp)), vec![Effect::ScrollConsole(Scroll::PageUp)]);
    assert_eq!(press(&mut state, now, shift(KeyCode::End)), vec![Effect::ScrollConsole(Scroll::Bottom)]);
    let captured = ShellView { mode: TermMode::ALT_SCREEN, ..idle_prompt() };
    update(&mut state, Msg::Observe(captured), now);
    assert_eq!(writes(&press(&mut state, now, shift(KeyCode::PageUp))), "\x1b[5;2~");
}

#[test]
fn moving_a_cell_cursor_does_not_schedule_pixel_animation_frames() {
    let mut settings = quiet();
    settings.motion = crate::Motion::LOCAL;
    let (mut state, now) = setup_from(vec![dir("me"), file("notes.txt")], settings);
    let later = now + std::time::Duration::from_secs(1);
    press(&mut state, later, key(KeyCode::Down));
    assert_eq!(state.pace(later), None);
    assert_eq!(state.active_panel().cursor(), 1);
}

#[test]
fn preview_focus_scrolls_document_and_tab_returns_to_listing() {
    let (mut state, now) = setup_with(vec![file("a.rs")]);
    press(&mut state, now, key(KeyCode::End));
    press(&mut state, now, ctrl('q'));
    let path = state.active_panel().dir().join("a.rs");
    let text = (0..100).map(|n| format!("let value_{n} = {n};\n")).collect::<String>();
    let pager = borg_view::Pager::for_path(std::sync::Arc::new(text.into_bytes()), &path, 40, 10).unwrap();
    update(
        &mut state,
        Msg::Peeked(crate::Peeked { aside: crate::Aside::QuickView, path, at: None, peek: crate::Peek::Text(pager) }),
        now,
    );
    press(&mut state, now, key(KeyCode::Tab));
    assert!(state.preview_focused);
    let selected = state.active_panel().cursor();
    assert!(press(&mut state, now, key(KeyCode::PageDown)).is_empty());
    let Some(crate::Peeked { peek: crate::Peek::Text(pager), .. }) = &state.peeked else { panic!("preview missing") };
    assert!(pager.offset() > 0);
    assert_eq!(state.active_panel().cursor(), selected);
    press(&mut state, now, key(KeyCode::Tab));
    assert!(!state.preview_focused);
    assert_eq!(state.active, Side::Left);
}

#[test]
fn preview_and_viewer_share_position_wrap_and_markdown_mode() {
    for focused in [false, true] {
        let (mut state, now) = setup_with(vec![file("guide.md")]);
        press(&mut state, now, key(KeyCode::End));
        press(&mut state, now, ctrl('q'));
        let path = state.active_panel().dir().join("guide.md");
        let source = (0..100).map(|n| format!("# Heading {n}\n\n")).collect::<String>();
        let mut pager = borg_view::Pager::for_path(std::sync::Arc::new(source.into_bytes()), &path, 40, 10).unwrap();
        pager.toggle_source().unwrap();
        pager.toggle_wrap().unwrap();
        pager.scroll_down(30).unwrap();
        let offset = pager.offset();
        update(
            &mut state,
            Msg::Peeked(crate::Peeked {
                aside: crate::Aside::QuickView,
                path,
                at: None,
                peek: crate::Peek::Text(pager),
            }),
            now,
        );
        if focused {
            press(&mut state, now, key(KeyCode::Tab));
        }
        let effects = press(&mut state, now, key(KeyCode::F(3)));
        assert!(
            !effects.iter().any(|effect| matches!(effect, Effect::View { .. })),
            "F3 must reuse the loaded document"
        );
        let pager = text(state.viewer.as_ref().unwrap());
        assert_eq!(pager.offset(), offset);
        assert!(!pager.formatted());
        assert!(!pager.wrapped());
        press(&mut state, now, key(KeyCode::PageDown));
        let offset = text(state.viewer.as_ref().unwrap()).offset();
        press(&mut state, now, key(KeyCode::Esc));
        let Some(crate::Peeked { peek: crate::Peek::Text(pager), .. }) = &state.peeked else {
            panic!("preview missing")
        };
        assert_eq!(pager.offset(), offset);
        assert!(!pager.formatted());
        assert!(!pager.wrapped());
    }
}

#[test]
fn settings_window_toggles_and_saves_without_closing() {
    let (mut state, now) = setup();
    crate::actions::perform(&mut state, Action::Settings, now);
    assert_eq!(state.overlay, Some(Overlay::Settings { selected: 0 }));
    let before = state.settings.preferences.show_hidden;
    let effects = press(&mut state, now, key(KeyCode::Char(' ')));
    assert_ne!(state.settings.preferences.show_hidden, before);
    assert!(effects.contains(&Effect::SaveConfig));
    assert_eq!(state.overlay, Some(Overlay::Settings { selected: 0 }));
    press(&mut state, now, key(KeyCode::Down));
    let before = state.settings.preferences.status_line;
    let effects = update(&mut state, Msg::Pointer(crate::Pointer::Setting(1)), now);
    assert_ne!(state.settings.preferences.status_line, before);
    assert!(effects.contains(&Effect::SaveConfig));
    press(&mut state, now, key(KeyCode::Esc));
    assert!(state.overlay.is_none());
}

#[test]
fn multiline_prompt_grows_and_leaves_room_for_panels() {
    let (mut state, now) = setup();
    let initial = state.layout(now);
    update(&mut state, Msg::ShellOutput(ShellView { command_rows: 4, ..idle_prompt() }), now);
    assert_eq!(state.layout(now).prompt.height, 4);
    assert_eq!(state.active_panel().rows(), initial.panel_rows() - 3);
    update(&mut state, Msg::ShellOutput(ShellView { command_rows: u16::MAX, ..idle_prompt() }), now);
    assert!(state.layout(now).prompt.height <= state.rows / 2);
    update(&mut state, Msg::ShellOutput(idle_prompt()), now);
    assert_eq!(state.layout(now), initial);
}

#[test]
fn screensaver_does_not_cover_a_running_program_or_spin_an_overdue_timer() {
    let (mut state, now) = setup();
    state.shell.running = true;
    let due = now + state.settings.screensaver_after;
    update(&mut state, Msg::Tick, due);
    assert!(!state.screensaver);
    assert!(state.wake_at().is_none());
    let effects = update(&mut state, Msg::Signal(ShellSignal::CommandEnd { exit: Some(0) }), due);
    assert_eq!(state.wake_at(), Some(due + crate::SPINNER_DELAY));
    for effect in effects {
        if let Effect::List { side, generation, .. } = effect {
            update(&mut state, Msg::Batch { side, generation, entries: vec![file("notes.txt")] }, due);
            update(&mut state, Msg::Listed { side, generation, error: None }, due);
        }
    }
    assert_eq!(state.wake_at(), Some(due + state.settings.screensaver_after));
}

#[test]
fn command_end_without_a_start_marker_still_refreshes_panels_for_older_shells() {
    let (mut state, now) = setup();
    state.shell.integrated = true;
    state.shell.running = false;
    let effects = update(&mut state, Msg::Signal(ShellSignal::CommandEnd { exit: Some(0) }), now);
    assert_eq!(effects.iter().filter(|effect| matches!(effect, Effect::List { .. })).count(), 2);
    assert_eq!(state.view, View::Panels);
}

#[test]
fn git_plugin_enables_scans_ignores_stale_results_and_disables_all_actions() {
    let (mut state, now) = setup_with(vec![file("a.rs")]);
    state.settings.preferences.git_plugin = true;
    let effects = update(&mut state, Msg::Tick, now);
    let (generation, dir) = effects
        .iter()
        .find_map(|effect| match effect {
            Effect::GitScan { side: Side::Left, generation, dir } => Some((*generation, dir.clone())),
            _ => None,
        })
        .unwrap();
    let repository = borg_git::Repository::parse(dir.clone(), b"## main\0 M a.rs\0").unwrap();
    update(
        &mut state,
        Msg::GitStatus {
            side: Side::Left,
            generation: generation + 1,
            dir: dir.clone(),
            result: Ok(Some(repository.clone())),
        },
        now,
    );
    assert!(state.active_panel().git_repository().is_none());
    update(
        &mut state,
        Msg::GitStatus { side: Side::Left, generation, dir: dir.clone(), result: Ok(Some(repository.clone())) },
        now,
    );
    assert!(state.active_panel().git_repository().is_some());
    assert!(state.wake_at().is_some());
    let due = now + std::time::Duration::from_secs(5);
    assert!(
        update(&mut state, Msg::Tick, due)
            .iter()
            .any(|effect| matches!(effect, Effect::GitScan { side: Side::Left, .. }))
    );
    state.overlay = Some(Overlay::Plugins { selected: 0 });
    let effects = press(&mut state, now, key(KeyCode::Char(' ')));
    assert!(effects.contains(&Effect::SaveConfig));
    assert!(!state.settings.preferences.git_plugin);
    assert!(state.active_panel().git_repository().is_none());
    assert!(menus(&state).iter().all(|menu| menu.title != "Git"));
    assert!(crate::actions::perform(&mut state, Action::Git(borg_git::Tool::Stage), now).is_empty());
    update(&mut state, Msg::GitStatus { side: Side::Left, generation, dir, result: Ok(Some(repository)) }, now);
    assert!(state.active_panel().git_repository().is_none());
    assert!(!update(&mut state, Msg::Tick, due).iter().any(|effect| matches!(effect, Effect::GitScan { .. })));
}

#[test]
fn git_tools_use_marked_files_and_refresh_status_after_index_changes() {
    let (mut state, now) = setup_with(vec![file("a.rs"), file("b.rs")]);
    state.settings.preferences.git_plugin = true;
    update(&mut state, Msg::Tick, now);
    state.panels[0].git.repository =
        Some(borg_git::Repository::parse("/home".into(), b"## main\0 M a.rs\0?? b.rs\0").unwrap());
    state.panels[0].invert_marks();
    let effects = crate::actions::perform(&mut state, Action::Git(borg_git::Tool::Stage), now);
    let [Effect::GitTool { id, paths, .. }] = effects.as_slice() else { panic!("expected Git operation") };
    assert_eq!(paths, &[PathBuf::from("/home/a.rs"), PathBuf::from("/home/b.rs")]);
    assert!(crate::actions::perform(&mut state, Action::Git(borg_git::Tool::Stage), now).is_empty());
    let effects = update(&mut state, Msg::GitFinished { id: *id, result: Ok(None) }, now);
    assert!(state.plugins.git_task.is_none());
    assert!(effects.iter().any(|effect| matches!(effect, Effect::GitScan { side: Side::Left, .. })));
    assert!(effects.iter().any(|effect| matches!(effect, Effect::List { side: Side::Left, .. })));
    let effects = press(&mut state, now, ctrl('u'));
    assert_eq!(
        effects.iter().filter(|effect| matches!(effect, Effect::GitScan { .. })).count(),
        2,
        "swapping panels must replace requests sent to their former sides"
    );
}

#[test]
fn the_git_menu_does_not_change_the_default_menu_for_the_right_panel() {
    let (mut state, now) = setup();
    state.settings.preferences.git_plugin = true;
    state.active = Side::Right;
    press(&mut state, now, key(KeyCode::F(9)));
    let Some(Overlay::Menu(cursor)) = state.overlay else { panic!("menu missing") };
    assert_eq!(menus(&state)[cursor.menu].title, "Right");
}

#[test]
fn explicit_console_stays_open_after_commands_interrupts_and_full_screen_programs() {
    let (mut state, now) = setup();
    state.shell.integrated = true;
    press(&mut state, now, ctrl('o'));
    for exit in [0, 1, 130] {
        type_text(&mut state, now, "echo console");
        press(&mut state, now, key(KeyCode::Enter));
        update(&mut state, Msg::Signal(ShellSignal::CommandEnd { exit: Some(exit) }), now);
        update(&mut state, Msg::ShellOutput(idle_prompt()), now);
        assert_eq!(state.view, View::Shell);
    }
    type_text(&mut state, now, "cancelled input");
    let effects = press(&mut state, now, ctrl('c'));
    assert_eq!(writes(&effects), "\x03");
    update(&mut state, Msg::Signal(ShellSignal::CommandEnd { exit: Some(130) }), now);
    assert_eq!(state.view, View::Shell);
    type_text(&mut state, now, "vim");
    press(&mut state, now, key(KeyCode::Enter));
    update(
        &mut state,
        Msg::ShellOutput(ShellView { foreground: Foreground::Program, mode: TermMode::ALT_SCREEN, ..idle_prompt() }),
        now,
    );
    press(&mut state, now, key(KeyCode::Enter));
    update(&mut state, Msg::ShellOutput(idle_prompt()), now);
    update(&mut state, Msg::Signal(ShellSignal::CommandEnd { exit: Some(0) }), now);
    assert_eq!(state.view, View::Shell);
    press(&mut state, now, ctrl('o'));
    type_text(&mut state, now, "echo panels");
    press(&mut state, now, key(KeyCode::Enter));
    update(&mut state, Msg::Signal(ShellSignal::CommandEnd { exit: Some(0) }), now);
    assert_eq!(state.view, View::Panels);
}

#[test]
fn explicit_view_change_during_a_command_wins_over_automatic_return() {
    let (mut state, now) = setup();
    state.shell.integrated = true;
    type_text(&mut state, now, "sleep 10");
    press(&mut state, now, key(KeyCode::Enter));
    press(&mut state, now, ctrl('o'));
    press(&mut state, now, ctrl('o'));
    update(&mut state, Msg::Signal(ShellSignal::CommandEnd { exit: Some(130) }), now);
    assert_eq!(state.view, View::Shell);
}

#[test]
fn option_arrows_edit_shell_words_and_keep_application_key_sequences_intact() {
    let (mut state, now) = setup();
    press(&mut state, now, ctrl('o'));
    for (code, expected) in [(KeyCode::Left, "\x1bb"), (KeyCode::Right, "\x1bf")] {
        let effects = press(&mut state, now, KeyEvent::new(code, KeyModifiers::ALT));
        assert_eq!(writes(&effects), expected);
    }
    state.shell.running = true;
    update(
        &mut state,
        Msg::Observe(ShellView { foreground: Foreground::Program, mode: TermMode::ALT_SCREEN, ..idle_prompt() }),
        now,
    );
    let effects = press(&mut state, now, KeyEvent::new(KeyCode::Left, KeyModifiers::ALT));
    assert_eq!(writes(&effects), "\x1b[1;3D");
}

#[test]
fn background_sizes_update_real_paths_without_changing_panels_cursor_or_marks() {
    let (mut state, now) = setup_with(vec![dir("me"), dir("other"), file("file")]);
    press(&mut state, now, key(KeyCode::Down));
    press(&mut state, now, key(KeyCode::Insert));
    let hovered = state.active_panel().hovered_entry().unwrap().name.clone();
    let marked = state.active_panel().marked_count();
    let effects = press(&mut state, now, KeyEvent::new(KeyCode::Char('i'), KeyModifiers::ALT));
    let scan = state.sizing.as_ref().unwrap().clone();
    assert!(effects.iter().any(|e| matches!(e, Effect::MeasureSizes { root, .. } if root == Path::new("/home"))));
    assert!(!state.active_panel().is_results());
    assert!(state.overlay.is_none());
    let mut measured = dir("me");
    measured.source = Some("/home/me".into());
    measured.size = 42000;
    measured.usage = Some(borg_vfs::FolderUsage { files: 30, ..Default::default() });
    update(&mut state, Msg::Sized { id: scan.id, entries: vec![measured] }, now);
    assert_eq!(state.panels[0].entries().iter().find(|e| e.name == "me").unwrap().size, 42000);
    assert!(state.panels[1].entries().iter().find(|e| e.name == "me").unwrap().usage.is_none());
    assert_eq!(state.active_panel().hovered_entry().unwrap().name, hovered);
    assert_eq!(state.active_panel().marked_count(), marked);
    press(&mut state, now, key(KeyCode::Tab));
    assert_eq!(state.active, Side::Right);
    assert!(state.sizing.is_some());
    update(
        &mut state,
        Msg::SizeProgress { id: scan.id, progress: crate::SearchProgress { visited: 80, ..Default::default() } },
        now,
    );
    let effects = update(&mut state, Msg::SizesFinished { id: scan.id, error: None }, now);
    assert!(state.sizing.is_none());
    assert_eq!(state.active, Side::Right);
    assert!(effects.iter().all(|e| matches!(e, Effect::List { .. })));
    assert!(state.notice.as_ref().unwrap().contains("80 scanned"));
}

#[test]
fn stopping_background_sizes_rejects_late_replies_and_new_runs_get_new_ids() {
    let (mut state, now) = setup();
    press(&mut state, now, KeyEvent::new(KeyCode::Char('i'), KeyModifiers::ALT));
    let id = state.sizing.as_ref().unwrap().id;
    assert!(matches!(
        &press(&mut state, now, KeyEvent::new(KeyCode::Char('i'), KeyModifiers::ALT))[..],
        [Effect::CancelMeasure]
    ));
    assert!(state.sizing.is_none());
    press(&mut state, now, KeyEvent::new(KeyCode::Char('i'), KeyModifiers::ALT));
    let next = state.sizing.as_ref().unwrap().id;
    assert_ne!(id, next);
    update(
        &mut state,
        Msg::SizeProgress { id, progress: crate::SearchProgress { visited: 999, ..Default::default() } },
        now,
    );
    update(&mut state, Msg::SizesFinished { id, error: None }, now);
    assert_eq!(state.sizing.as_ref().unwrap().id, next);
    assert_eq!(state.sizing.as_ref().unwrap().progress.visited, 0);
    let mut measured = dir("me");
    measured.source = Some("/home/me".into());
    measured.size = 999;
    measured.usage = Some(Default::default());
    update(&mut state, Msg::Sized { id, entries: vec![measured] }, now);
    assert!(state.active_panel().entries()[0].usage.is_none());
    press(&mut state, now, ctrl('o'));
    assert_eq!(writes(&press(&mut state, now, KeyEvent::new(KeyCode::Char('i'), KeyModifiers::ALT))), "\x1bi");
    assert_eq!(writes(&press(&mut state, now, ctrl(' '))), "\0");
    assert!(state.sizing.is_some());
}

#[test]
fn measured_sizes_survive_relisting_and_stopping_until_a_finished_scan_saves_them() {
    let (mut state, now) = setup();
    let alt_i = KeyEvent::new(KeyCode::Char('i'), KeyModifiers::ALT);
    press(&mut state, now, alt_i);
    let id = state.sizing.as_ref().unwrap().id;
    let mut measured = dir("me");
    measured.source = Some("/home/me".into());
    measured.size = 4_096_000;
    measured.usage = Some(borg_vfs::FolderUsage { files: 3, partial: true, ..Default::default() });
    update(&mut state, Msg::Sized { id, entries: vec![measured] }, now);
    let size = |state: &AppState| state.active_panel().entries().iter().find(|e| e.name == "me").map(|e| e.size);
    let relist = |state: &mut AppState| {
        let Effect::List { side, generation, .. } = state.reload(Side::Left) else { panic!("expected listing") };
        update(state, Msg::Batch { side, generation, entries: vec![dir("me")] }, now);
        update(state, Msg::Listed { side, generation, error: None }, now);
    };
    assert_eq!(size(&state), Some(4_096_000));
    relist(&mut state);
    assert_eq!(size(&state), Some(4_096_000), "a folder listed again during the scan keeps its total");
    press(&mut state, now, alt_i);
    assert!(state.sizing.is_none());
    relist(&mut state);
    assert_eq!(size(&state), Some(4_096_000), "stopping keeps completed and running totals");

    press(&mut state, now, alt_i);
    let id = state.sizing.as_ref().unwrap().id;
    update(&mut state, Msg::SizesFinished { id, error: None }, now);
    relist(&mut state);
    assert_ne!(size(&state), Some(4_096_000), "a finished scan leaves totals to its saved cache");
}

#[test]
fn starting_a_file_job_stops_background_sizes_before_modifying_the_filesystem() {
    let (mut state, now) = setup();
    press(&mut state, now, KeyEvent::new(KeyCode::Char('i'), KeyModifiers::ALT));
    let effects =
        crate::jobs::start(&mut state, crate::JobKind::Delete, Spec::Delete { sources: vec!["/home/me".into()] }, now);
    assert!(state.sizing.is_none());
    assert!(matches!(&effects[..], [Effect::CancelMeasure, Effect::StartJob { .. }]));
}

#[test]
fn the_docker_menu_toggles_the_view_and_offers_only_actions_that_change_the_selection() {
    use borg_docker::{Action as Docker, Container, State};
    let (mut state, now) = setup();
    let enabled = |state: &AppState, label: &str| {
        let menus = crate::menus(state);
        let menu = menus.iter().find(|m| m.title == "Docker").expect("a Docker menu");
        menu.entries.iter().any(|e| matches!(e, crate::MenuEntry::Item(i) if i.label == label && i.enabled))
    };
    assert!(enabled(&state, "Containers") && !enabled(&state, "Stop…"), "closed, it only opens the view");
    crate::actions::perform(&mut state, Action::Docker, now);
    let Some((id, crate::DockerRequest::List)) = state.docker.pending.clone() else {
        panic!("opening lists containers")
    };
    let running = Container {
        id: "c1".into(),
        name: "web".into(),
        image: "nginx".into(),
        command: String::new(),
        created: String::new(),
        state: State::Running,
        status: "Up 1 minute".into(),
        ports: String::new(),
        networks: String::new(),
        project: None,
        service: None,
        project_dir: None,
        config_files: Vec::new(),
    };
    let listed = crate::DockerData::Listed { context: None, containers: vec![running] };
    update(&mut state, Msg::Docker { id, result: Ok(listed) }, now);
    press(&mut state, now, key(KeyCode::Down));
    assert_eq!(state.docker.container().map(|c| c.name.as_str()), Some("web"));
    assert!(!enabled(&state, "Start") && enabled(&state, "Stop…") && enabled(&state, "Pause"));
    crate::actions::perform(&mut state, Action::DockerAct(Docker::Stop), now);
    assert!(matches!(state.overlay, Some(Overlay::Confirm(_))), "stopping from the menu asks first");
    state.overlay = None;
    crate::actions::perform(&mut state, Action::Docker, now);
    assert!(!state.docker.is_open(), "the menu item closes the view again");
}

/// A panel switched to a running container's filesystem, as Docker → Files leaves it.
fn browsing_container(now: Instant) -> AppState {
    use borg_docker::{Container, State};
    let (mut state, _) = setup_with(vec![file("notes.txt")]);
    crate::actions::perform(&mut state, Action::Docker, now);
    let Some((id, crate::DockerRequest::List)) = state.docker.pending.clone() else {
        panic!("opening lists containers")
    };
    let web = Container {
        id: "c1".into(),
        name: "shop-web-1".into(),
        image: "nginx".into(),
        command: String::new(),
        created: String::new(),
        state: State::Running,
        status: "Up 1 minute".into(),
        ports: String::new(),
        networks: String::new(),
        project: None,
        service: None,
        project_dir: None,
        config_files: Vec::new(),
    };
    update(
        &mut state,
        Msg::Docker { id, result: Ok(crate::DockerData::Listed { context: None, containers: vec![web] }) },
        now,
    );
    press(&mut state, now, key(KeyCode::Down));
    let effects = press(&mut state, now, key(KeyCode::Char('f')));
    assert!(!state.docker.is_open(), "browsing files leaves the Docker view");
    let Some(Effect::List { side: Side::Left, generation, dir }) = effects.first().cloned() else {
        panic!("the panel lists the container's root: {effects:?}")
    };
    assert_eq!(dir, Path::new("/"));
    let etc = Entry { source: Some("/etc".into()), ..dir_entry("etc") };
    let motd = Entry { source: Some("/motd".into()), ..file("motd") };
    update(&mut state, Msg::Batch { side: Side::Left, generation, entries: vec![etc, motd] }, now);
    update(&mut state, Msg::Listed { side: Side::Left, generation, error: None }, now);
    state
}

fn dir_entry(name: &str) -> Entry {
    dir(name)
}

fn file_request(effects: &[Effect]) -> Option<(String, crate::FileOp)> {
    effects.iter().find_map(|effect| match effect {
        Effect::Docker { request: crate::DockerRequest::Files { container, op }, .. } => {
            Some((container.clone(), op.clone()))
        }
        _ => None,
    })
}

#[test]
fn a_container_panel_copies_with_the_host_panel_and_leaves_through_its_root() {
    use crate::FileOp;
    let now = Instant::now();
    let mut state = browsing_container(now);
    assert_eq!(state.panel(Side::Left).mount.as_ref().map(|m| m.name.as_str()), Some("shop-web-1"));
    assert!(state.panel(Side::Left).hovered().is_some_and(|row| matches!(row, crate::Row::Parent)), "the root has ..");

    press(&mut state, now, key(KeyCode::Down));
    press(&mut state, now, key(KeyCode::Down));
    let effects = crate::actions::perform(&mut state, Action::Copy, now);
    assert!(effects.is_empty());
    let Some(Overlay::Prompt(prompt)) = state.overlay.clone() else { panic!("copy asks for the destination") };
    assert_eq!(prompt.input.text(), "/tmp");
    let effects = press(&mut state, now, key(KeyCode::Enter));
    let copy = effects.iter().find_map(|e| match e {
        Effect::StartDockerCopy { id, container, inbound, sources, target } => {
            Some((*id, container.clone(), *inbound, sources.clone(), target.clone()))
        }
        _ => None,
    });
    let Some((id, container, inbound, sources, target)) = copy else { panic!("copying out starts a job: {effects:?}") };
    assert_eq!((container.as_str(), inbound, sources, target), ("c1", false, vec!["/motd".into()], "/tmp".into()));
    assert!(state.jobs.iter().any(|job| job.id == id && job.shown_on == Side::Right), "a copy job, shown on the host");
    let done = borg_jobs::Outcome::Done { done: borg_jobs::Totals { files: 1, bytes: 0 }, skipped: 0 };
    let effects = update(&mut state, Msg::Job { id, event: borg_jobs::Event::Finished(done) }, now);
    assert_eq!(state.notice.as_deref(), Some("Copied 1 file"));
    assert!(effects.iter().any(|e| matches!(e, Effect::List { side: Side::Right, .. })), "both panels refresh");

    let effects = crate::actions::perform(&mut state, Action::View, now);
    let Some((_, FileOp::Fetch { path })) = file_request(&effects) else { panic!("viewing copies the file out") };
    assert_eq!(path, Path::new("/motd"));
    let id = state.docker.files.last().unwrap().id;
    let effects =
        update(&mut state, Msg::Docker { id, result: Ok(crate::DockerData::Fetched("/run/motd".into())) }, now);
    assert_eq!(effects, [Effect::View { path: "/run/motd".into(), at: None }]);

    crate::actions::perform(&mut state, Action::Delete, now);
    let effects = press(&mut state, now, key(KeyCode::Enter));
    assert_eq!(file_request(&effects).map(|(_, op)| op), Some(FileOp::Delete { paths: vec!["/motd".into()] }));
    crate::actions::perform(&mut state, Action::Mkdir, now);
    type_text(&mut state, now, "cache");
    let effects = press(&mut state, now, key(KeyCode::Enter));
    assert_eq!(file_request(&effects).map(|(_, op)| op), Some(FileOp::Mkdir { path: "/cache".into() }));

    crate::actions::perform(&mut state, Action::QuickView, now);
    assert_eq!(state.notice.as_deref(), Some("Not available inside a container"));
    assert!(state.aside.is_none());
    assert!(crate::update::prepare_shell(&mut state).is_empty(), "the host shell does not follow into a container");

    press(&mut state, now, key(KeyCode::Tab));
    press(&mut state, now, key(KeyCode::Down));
    let effects = crate::actions::perform(&mut state, Action::Copy, now);
    assert!(effects.is_empty());
    let effects = press(&mut state, now, key(KeyCode::Enter));
    let inbound = effects.iter().find_map(|e| match e {
        Effect::StartDockerCopy { inbound, target, .. } => Some((*inbound, target.clone())),
        _ => None,
    });
    assert_eq!(inbound, Some((true, "/".into())), "copying into the container");

    press(&mut state, now, key(KeyCode::Tab));
    press(&mut state, now, key(KeyCode::Home));
    let effects = press(&mut state, now, key(KeyCode::Enter));
    assert!(state.panel(Side::Left).mount.is_none(), ".. at the root leaves the container");
    assert!(matches!(effects.first(), Some(Effect::List { side: Side::Left, dir, .. }) if dir == Path::new("/home")));
    assert!(effects.contains(&Effect::UseConsole(crate::Console::Host)), "and the command line is the host's again");
}

#[test]
fn a_container_shell_takes_keys_until_alt_c_and_stays_open_until_it_exits() {
    use crate::ShellRequest;
    let now = Instant::now();
    let (mut state, _) = setup_with(vec![file("notes.txt")]);
    crate::actions::perform(&mut state, Action::Docker, now);
    let Some((id, crate::DockerRequest::List)) = state.docker.pending.clone() else {
        panic!("opening lists containers")
    };
    let web = borg_docker::Container {
        id: "c1".into(),
        name: "shop-web-1".into(),
        image: "nginx".into(),
        command: String::new(),
        created: String::new(),
        state: borg_docker::State::Running,
        status: "Up 1 minute".into(),
        ports: String::new(),
        networks: String::new(),
        project: Some("shop".into()),
        service: Some("web".into()),
        project_dir: None,
        config_files: Vec::new(),
    };
    update(
        &mut state,
        Msg::Docker { id, result: Ok(crate::DockerData::Listed { context: None, containers: vec![web] }) },
        now,
    );
    press(&mut state, now, key(KeyCode::Down));
    let effects = press(&mut state, now, key(KeyCode::Char('e')));
    let [Effect::DockerShell(ShellRequest::Open { container, size, .. })] = effects.as_slice() else {
        panic!("E opens a shell: {effects:?}")
    };
    assert_eq!(container, "c1");
    assert!(size.cols > 0 && size.rows > 0);
    assert!(state.docker.typing(), "the shell takes the keys");

    let input = |effects: Vec<Effect>| match effects.as_slice() {
        [Effect::DockerShell(ShellRequest::Input { bytes, .. })] => bytes.clone(),
        other => panic!("expected input for the shell: {other:?}"),
    };
    assert_eq!(input(press(&mut state, now, key(KeyCode::Char('x')))), b"x", "letters reach the shell, not Stop");
    assert_eq!(input(press(&mut state, now, key(KeyCode::Char('1')))), b"1", "digits reach the shell too");
    assert_eq!(input(press(&mut state, now, key(KeyCode::F(1)))), b"\x1bOP", "and function keys");
    assert_eq!(input(press(&mut state, now, key(KeyCode::Esc))), b"\x1b");
    assert_eq!(input(update(&mut state, Msg::Paste("ls".into()), now)), b"ls");
    assert!(state.docker.is_open() && state.overlay.is_none());

    press(&mut state, now, KeyEvent::new(KeyCode::Char('c'), KeyModifiers::ALT));
    assert!(!state.docker.typing() && state.docker.is_open(), "Alt-C returns to the list");
    assert!(state.docker.visible_shell().is_some(), "the shell stays on its tab");
    press(&mut state, now, KeyEvent::new(KeyCode::Char('2'), KeyModifiers::ALT));
    assert!(state.docker.typing(), "Alt-2 returns to the shell");
    assert!(press(&mut state, now, KeyEvent::new(KeyCode::Char('1'), KeyModifiers::ALT)).is_empty());
    assert!(state.docker.visible_shell().is_none() && !state.docker.typing(), "Alt-1 leaves it for the overview");
    press(&mut state, now, key(KeyCode::Esc));
    assert!(!state.docker.is_open());
    assert_eq!(state.docker.shells.len(), 1, "closing the view keeps the shell running");

    crate::actions::perform(&mut state, Action::Quit, now);
    let Some(Overlay::Confirm(confirm)) = state.overlay.take() else { panic!("quitting asks first") };
    assert_eq!(confirm.message, "A container shell is open. Quit and close it?");

    crate::actions::perform(&mut state, Action::Docker, now);
    press(&mut state, now, KeyEvent::new(KeyCode::Char('2'), KeyModifiers::ALT));
    assert!(state.docker.typing(), "Alt-2 returns to the open shell from the list");
    press(&mut state, now, ctrl('o'));
    assert_eq!(state.view, View::Shell, "Ctrl-O still shows the host console");
    update(&mut state, Msg::DockerShellClosed { container: "c1".into() }, now);
    assert!(state.docker.shells.is_empty() && !state.docker.typing());
    assert_eq!(state.notice.as_deref(), Some("The shell in web closed"));
}

#[test]
fn the_command_line_waits_for_a_host_panel_unless_it_follows_containers() {
    use crate::{Console, ShellRequest};
    let now = Instant::now();
    let mut state = browsing_container(now);
    assert_eq!(state.console, Console::Container("c1".into()), "on by default");
    crate::actions::perform(&mut state, Action::ToggleDockerConsole, now);
    update(&mut state, Msg::Tick, now);
    assert_eq!(state.console, Console::Host, "off, the host keeps the command line");
    update(&mut state, Msg::DockerShellClosed { container: "c1".into() }, now);
    type_text(&mut state, now, "ls");
    let effects = press(&mut state, now, key(KeyCode::Enter));
    assert!(effects.is_empty(), "Enter runs nothing on the host under a container's files");
    assert!(state.notice.as_deref().is_some_and(|n| n.starts_with("This command line runs on the client")));
    assert!(!state.console_owns_panel() && state.view == View::Panels);

    crate::actions::perform(&mut state, Action::ToggleDockerConsole, now);
    let effects = update(&mut state, Msg::Tick, now);
    let opened = effects.iter().find_map(|e| match e {
        Effect::DockerShell(ShellRequest::Open { container, prompt, workdir, .. }) => {
            Some((container.clone(), prompt.clone(), workdir.clone()))
        }
        _ => None,
    });
    assert_eq!(
        opened,
        Some(("c1".into(), "shop-web-1".into(), Some("/".into()))),
        "a shell opens in the panel's folder"
    );
    assert!(effects.contains(&Effect::UseConsole(Console::Container("c1".into()))));
    assert!(state.console_owns_panel() && state.console_container() == Some("shop-web-1"));
    assert_eq!(
        state.parked_host.as_ref().map(|host| host.cwd.as_path()),
        Some(Path::new("/home")),
        "the host shell waits"
    );

    update(&mut state, Msg::Signal(ShellSignal::PromptStart), now);
    type_text(&mut state, now, "cd etc");
    let effects = press(&mut state, now, key(KeyCode::Enter));
    assert!(matches!(effects.last(), Some(Effect::Write(bytes)) if bytes == b"\r"), "Enter runs in the container");
    assert_eq!(state.view, View::Shell);
    update(&mut state, Msg::Signal(ShellSignal::Cwd { host: "box".into(), path: "/etc".into() }), now);
    let effects = update(&mut state, Msg::Signal(ShellSignal::CommandEnd { exit: Some(0) }), now);
    assert_eq!(state.view, View::Panels, "it returns to the panels like a host command");
    assert!(
        effects.iter().any(|e| matches!(e, Effect::List { side: Side::Left, dir, .. } if dir == Path::new("/etc"))),
        "a cd in the container moves its panel: {effects:?}"
    );

    press(&mut state, now, key(KeyCode::Tab));
    assert_eq!(state.console, Console::Host, "a host panel brings back the host's command line");
    assert_eq!(state.shell.cwd, Path::new("/home"));
    assert!(state.docker.shell("c1").is_some(), "the container's shell keeps running");
    assert!(!state.shell.line_empty(), "the host's line kept what was typed there");

    press(&mut state, now, ctrl('u'));
    press(&mut state, now, key(KeyCode::Tab));
    assert_eq!(state.console, Console::Container("c1".into()));
    update(&mut state, Msg::DockerShellClosed { container: "c1".into() }, now);
    assert_eq!(state.console, Console::Host, "a shell that could not start leaves the host's");
    assert!(
        update(&mut state, Msg::Tick, now).iter().all(|e| !matches!(e, Effect::DockerShell(_))),
        "and is not retried"
    );
}

#[test]
fn disk_cleanup_goes_to_the_host_shell_while_the_command_line_follows_a_container() {
    use crate::Console;
    let now = Instant::now();
    let mut state = browsing_container(now);
    assert_eq!(state.console, Console::Container("c1".into()));
    let effects = crate::actions::perform(&mut state, Action::DockerDisk, now);
    let id = effects
        .iter()
        .find_map(|e| match e {
            Effect::Docker { id, request: crate::DockerRequest::DiskUse } => Some(*id),
            _ => None,
        })
        .expect("opening disk use measures it");
    let usage = borg_docker::DiskUse { networks: vec!["shop_backend".into()], ..Default::default() };
    let measured = crate::DockerData::DiskUse { usage: Box::new(usage), context: None, contexts: None };
    let effects = update(&mut state, Msg::Docker { id, result: Ok(measured) }, now);
    assert!(effects.contains(&Effect::UseConsole(Console::Host)), "the Docker view keeps the host's: {effects:?}");
    press(&mut state, now, key(KeyCode::End));
    let effects = press(&mut state, now, key(KeyCode::F(8)));
    assert_eq!(writes(&effects), "docker network prune");
    assert_eq!((&state.console, state.view), (&Console::Host, View::Shell));
}

#[test]
fn the_claude_log_is_saved_and_goes_from_memory_and_disk_when_it_is_turned_off() {
    use crate::AgentEvent;
    let (mut state, now) = setup_with(Vec::new());
    crate::actions::perform(&mut state, Action::AgentLog, now);
    assert!(state.notice.as_deref().is_some_and(|n| n.starts_with("Nothing remote has happened")));
    let read = "2026-11-09 14:00:00  read /srv/app";
    let effects = update(&mut state, Msg::Agent(AgentEvent::Note(read.into())), now);
    assert_eq!(effects, vec![Effect::SaveAgentNote(read.into())]);
    crate::actions::perform(&mut state, Action::AgentLog, now);
    assert_eq!(state.overlay, Some(Overlay::AgentLog { scroll: None }));
    state.overlay = None;

    assert_eq!(
        crate::actions::perform(&mut state, Action::ToggleAgentLog, now),
        vec![Effect::SaveConfig, Effect::ForgetAgentLog]
    );
    assert!(state.agent.log.is_empty(), "turning the log off forgets it");
    let effects = update(&mut state, Msg::Agent(AgentEvent::Note("2026-11-09 14:00:01  showed /srv".into())), now);
    assert!(effects.is_empty(), "nothing is saved while the log is off");
    assert!(state.agent.log.is_empty());
    crate::actions::perform(&mut state, Action::AgentLog, now);
    assert!(state.notice.as_deref().is_some_and(|n| n.starts_with("The remote log is off")));
}

#[test]
fn an_approval_scrolls_and_answers_and_a_host_command_is_typed_out_of_history() {
    use crate::{AgentEvent, Ask, AskKind, DiffLine};
    let (mut state, now) = setup_with(Vec::new());
    let body = (1..=200).map(|n| DiffLine::code(format!("echo {n}"))).collect();
    let ask = Ask { kind: AskKind::Run, summary: "echo 1".into(), place: "~/app".into(), origin: None, body };
    update(&mut state, Msg::Agent(AgentEvent::Ask { id: 7, ask }), now);
    let Some(approval) = state.approval.clone() else { panic!("{:?}", state.overlay) };
    assert!(!approval.accept, "No is chosen until the user picks Yes");
    press(&mut state, now, key(KeyCode::PageDown));
    press(&mut state, now, key(KeyCode::End));
    let Some(scrolled) = state.approval.clone() else { panic!() };
    assert!(scrolled.scroll > 100, "End shows the last lines: {}", scrolled.scroll);
    press(&mut state, now, key(KeyCode::Down));
    assert_eq!(state.approval, Some(scrolled.clone()), "it stops at the end");

    // Asked as the test starts, which counts as typing: Yes counts after the grace.
    let effects = press(&mut state, now + crate::agent::APPROVAL_GRACE, key(KeyCode::Char('y')));
    assert_eq!(effects, vec![Effect::AgentAnswer { id: 7, approved: true }]);
    assert_eq!(state.agent.running, Some((7, "echo 1".into())));
    let effects = update(&mut state, Msg::Agent(AgentEvent::Type { line: "make test".into() }), now);
    assert_eq!(effects, vec![Effect::Write(b" make test\r".to_vec())]);
    assert_eq!(state.view, View::Shell, "the console shows while it runs");
}

#[test]
fn approving_a_pairing_a_read_or_an_upload_leaves_nothing_running() {
    use crate::{AgentEvent, Ask, AskKind};
    let (mut state, now) = setup_with(Vec::new());
    for (id, kind) in [(1, AskKind::Pair), (2, AskKind::Read), (3, AskKind::Upload)] {
        let ask = Ask { kind, summary: "me@laptop".into(), place: String::new(), origin: None, body: Vec::new() };
        update(&mut state, Msg::Agent(AgentEvent::Ask { id, ask }), now);
        let effects = press(&mut state, now + crate::agent::APPROVAL_GRACE, key(KeyCode::Char('y')));
        assert_eq!(effects, vec![Effect::AgentAnswer { id, approved: true }]);
        assert_eq!(state.agent.running, None, "{kind:?} is answered at once, so the footer shows no work");
    }
}

fn pairing_ask() -> crate::Ask {
    crate::Ask {
        kind: crate::AskKind::Pair,
        summary: "me@laptop".into(),
        place: String::new(),
        origin: None,
        body: Vec::new(),
    }
}

#[test]
fn a_pairing_approval_shows_over_the_command_shown_plainly_and_the_connect_dialog_comes_back() {
    use crate::AgentEvent;
    use std::time::Duration;
    let (mut state, now) = setup_with(Vec::new());
    crate::actions::perform(&mut state, Action::ConnectClaude, now);
    update(&mut state, Msg::Agent(AgentEvent::Pairing(Ok("borg://relay.example.com/ab#cd".into()))), now);
    let shown = press(&mut state, now, key(KeyCode::F(3)));
    assert_eq!(shown, vec![Effect::ShowPlain("borg pair 'borg://relay.example.com/ab#cd'".into())]);

    let asked = now + Duration::from_secs(5);
    update(&mut state, Msg::Agent(AgentEvent::Ask { id: 7, ask: pairing_ask() }), asked);
    assert!(state.approval.is_some(), "the approval shows at once, over the Connect dialog");
    assert!(matches!(state.overlay, Some(Overlay::Connect(_))));
    let read = asked + crate::agent::APPROVAL_GRACE;
    assert_eq!(press(&mut state, read, key(KeyCode::Char('y'))), vec![Effect::AgentAnswer { id: 7, approved: true }]);
    assert!(state.approval.is_none());
    assert!(matches!(state.overlay, Some(Overlay::Connect(_))), "the Connect dialog comes back");

    let again = asked + Duration::from_secs(5);
    update(&mut state, Msg::Agent(AgentEvent::Ask { id: 8, ask: pairing_ask() }), again);
    assert!(update(&mut state, Msg::Pointer(crate::Pointer::Confirm(true)), again).is_empty(), "nor a click at once");
    let clicked = update(&mut state, Msg::Pointer(crate::Pointer::Confirm(true)), again + crate::agent::APPROVAL_GRACE);
    assert_eq!(clicked, vec![Effect::AgentAnswer { id: 8, approved: true }], "a click on Yes answers it too");
}

#[test]
fn an_approval_shows_over_any_dialog_and_takes_every_key_until_answered() {
    use crate::AgentEvent;
    use std::time::Duration;
    for opener in [KeyCode::F(9), KeyCode::F(1), KeyCode::F(7)] {
        let (mut state, now) = setup();
        press(&mut state, now, key(opener));
        type_text(&mut state, now, "ab");
        let open = state.overlay.clone();
        assert!(open.is_some(), "{opener:?} opens a dialog");

        let asked = now + Duration::from_secs(5);
        update(&mut state, Msg::Agent(AgentEvent::Ask { id: 1, ask: pairing_ask() }), asked);
        assert!(state.approval.is_some(), "the approval shows over {opener:?}");
        assert!(press(&mut state, asked, key(KeyCode::Char('x'))).is_empty());
        assert!(update(&mut state, Msg::Paste("pasted".into()), asked).is_empty());
        assert_eq!(state.overlay, open, "keys and pastes for the approval leave {opener:?} as it was");
        assert_eq!(press(&mut state, asked, key(KeyCode::Esc)), vec![Effect::AgentAnswer { id: 1, approved: false }]);
        assert_eq!(state.overlay, open, "{opener:?} comes back");
    }
}

#[test]
fn an_approval_ends_the_screensaver_and_the_intro() {
    use crate::AgentEvent;
    let mut settings = quiet();
    settings.preferences.screensaver = true;
    let (mut state, now) = setup_from(vec![dir("me")], settings);
    let due = now + state.settings.screensaver_after;
    update(&mut state, Msg::Tick, due);
    assert!(state.screensaver);
    update(&mut state, Msg::Agent(AgentEvent::Ask { id: 1, ask: pairing_ask() }), due);
    assert!(!state.screensaver, "the approval wakes the screen");
    assert!(state.approval.is_some());
    assert!(!state.can_screensave(), "no screensaver starts over it");
    let read = due + crate::agent::APPROVAL_GRACE;
    assert_eq!(press(&mut state, read, key(KeyCode::Char('y'))), vec![Effect::AgentAnswer { id: 1, approved: true }]);

    state.intro = Some(due);
    update(&mut state, Msg::Agent(AgentEvent::Ask { id: 2, ask: pairing_ask() }), due);
    assert_eq!(state.intro, None);
    assert!(state.approval.is_some());
}

#[test]
fn a_withdrawn_ask_closes_its_approval_with_a_notice_and_a_queued_one_never_shows() {
    use crate::{AgentEvent, Ask, AskKind, DiffLine};
    let (mut state, now) = setup_with(Vec::new());
    let run = Ask {
        kind: AskKind::Run,
        summary: "make".into(),
        place: "~".into(),
        origin: None,
        body: vec![DiffLine::code("make")],
    };
    update(&mut state, Msg::Agent(AgentEvent::Ask { id: 1, ask: pairing_ask() }), now);
    update(&mut state, Msg::Agent(AgentEvent::Ask { id: 2, ask: run }), now);
    update(&mut state, Msg::Agent(AgentEvent::Withdrawn { id: 2 }), now);
    assert_eq!(state.approval.as_ref().map(|approval| approval.id), Some(1));
    assert_eq!(state.notice, None, "the queued ask goes without a word");

    update(&mut state, Msg::Agent(AgentEvent::Withdrawn { id: 1 }), now);
    assert_eq!(state.approval, None, "nothing else waits");
    assert_eq!(state.notice, Some(borg_i18n::tr!(agent_withdrawn, summary = "me@laptop")));
}

#[test]
fn an_approved_capture_shows_in_the_footer_until_it_is_done() {
    use crate::{AgentEvent, Ask, AskKind};
    let (mut state, now) = setup_with(Vec::new());
    let summary = "capture on en0, 10 s";
    let ask =
        Ask { kind: AskKind::Capture, summary: summary.into(), place: String::new(), origin: None, body: Vec::new() };
    update(&mut state, Msg::Agent(AgentEvent::Ask { id: 4, ask }), now);
    let effects = press(&mut state, now + crate::agent::APPROVAL_GRACE, key(KeyCode::Char('y')));
    assert_eq!(effects, vec![Effect::AgentAnswer { id: 4, approved: true }]);
    assert_eq!(state.agent.running, Some((4, summary.into())));
    let notice = "Remote saved a capture of 12 packets to /work/capture.pcap".to_owned();
    update(&mut state, Msg::Agent(AgentEvent::Finished { id: 4, notice: notice.clone() }), now);
    assert_eq!((state.agent.running.clone(), state.notice.clone()), (None, Some(notice)));
}

#[test]
fn the_connect_dialog_offers_borg_cloud_and_copies_the_command_picked() {
    use crate::AgentEvent;
    let (mut state, now) = setup_with(Vec::new());
    state.agent.connect = crate::ConnectInfo {
        name: "borg-web1".into(),
        program: "/usr/bin/borg".into(),
        ssh: Some("me@10.0.0.5".into()),
        relay: "wss://relay.example.com".into(),
    };
    let effects = crate::actions::perform(&mut state, Action::ConnectClaude, now);
    assert_eq!(effects, vec![Effect::StartPairing], "opening it asks borg cloud for a pairing link");
    // Enter does nothing while the link is on its way.
    assert!(press(&mut state, now, key(KeyCode::Enter)).is_empty());
    update(&mut state, Msg::Agent(AgentEvent::Pairing(Ok("borg://relay.example.com/ab#cd".into()))), now);
    let effects = press(&mut state, now, key(KeyCode::Enter));
    assert_eq!(effects, vec![Effect::Copy("borg pair 'borg://relay.example.com/ab#cd'".into())]);

    // On borg cloud, keys edit the relay; Enter saves an address pasted there.
    crate::actions::perform(&mut state, Action::ConnectClaude, now);
    for _ in 0.."wss://relay.example.com".len() {
        press(&mut state, now, key(KeyCode::Backspace));
    }
    update(&mut state, Msg::Paste("relay.example.org".into()), now);
    let effects = press(&mut state, now, key(KeyCode::Enter));
    assert_eq!(effects, vec![Effect::SetRelay("relay.example.org".into())]);
    assert!(state.agent.pairing.is_none(), "the dialog waits for a link from the new relay");
    assert!(matches!(state.overlay, Some(Overlay::Connect(_))));
    press(&mut state, now, key(KeyCode::Esc));

    // The commands through SSH edit the target.
    crate::actions::perform(&mut state, Action::ConnectClaude, now);
    press(&mut state, now, key(KeyCode::Tab));
    for _ in 0.."me@10.0.0.5".len() {
        press(&mut state, now, key(KeyCode::Backspace));
    }
    for c in "prod".chars() {
        press(&mut state, now, key(KeyCode::Char(c)));
    }
    press(&mut state, now, key(KeyCode::Tab));
    let effects = press(&mut state, now, key(KeyCode::Enter));
    assert_eq!(
        effects,
        vec![Effect::Copy("claude mcp add borg-web1 -- ssh -o BatchMode=yes prod '/usr/bin/borg mcp'".into())]
    );
    assert!(state.overlay.is_none() && state.notice.as_deref().is_some_and(|n| n.starts_with("Copied")));

    // Paired computers can be forgotten from the dialog; borg cloud switched off says so.
    update(&mut state, Msg::Agent(AgentEvent::Paired { computers: vec!["me@laptop".into()] }), now);
    crate::actions::perform(&mut state, Action::ConnectClaude, now);
    assert_eq!(press(&mut state, now, key(KeyCode::F(8))), vec![Effect::ForgetComputers]);
    assert!(state.agent.computers.is_empty());
    press(&mut state, now, key(KeyCode::Esc));
    crate::actions::perform(&mut state, Action::ToggleAgentCloud, now);
    assert!(crate::actions::perform(&mut state, Action::ConnectClaude, now).is_empty());
    assert!(matches!(&state.agent.pairing, Some(Err(reason)) if reason.starts_with("Borg cloud is off")));
}

#[test]
fn an_agent_types_at_the_cursor_as_a_paste_and_never_runs_what_it_types() {
    use crate::{AgentEvent, typing_refusal};
    use borg_term::TermMode;
    let (mut state, now) = setup_with(Vec::new());
    assert_eq!(typing_refusal(&state, "ls -la"), None, "one line at a prompt types as keys");
    assert!(typing_refusal(&state, "cd /srv\nls").is_some(), "without paste support, a line break would run");
    state.shell.observed.mode = TermMode::BRACKETED_PASTE;
    assert_eq!(typing_refusal(&state, "cd /srv\nls"), None);
    let effects = update(&mut state, Msg::Agent(AgentEvent::Paste { text: "cd /srv\nls".into() }), now);
    assert_eq!(effects, vec![Effect::Write(b"\x1b[200~cd /srv\nls\x1b[201~".to_vec())]);
    assert_eq!(state.view, View::Shell, "several lines show in the console");
    assert!(!state.shell.line_empty(), "the command line holds the text until the user acts");

    state.shell.running = true;
    state.shell.observed.mode = TermMode::empty();
    assert!(typing_refusal(&state, "dd").is_some(), "a program without paste support would take text as keys");
}

#[test]
fn an_approval_over_the_console_takes_the_keys_but_not_a_yes_typed_as_it_appears() {
    use crate::{AgentEvent, Ask, AskKind, DiffLine, agent::APPROVAL_GRACE};
    use borg_term::TermMode;
    use std::time::Duration;
    let (mut state, now) = setup_with(Vec::new());
    state.view = View::Shell;
    state.shell.observed.mode = TermMode::ALT_SCREEN;
    let ask = |command: &str| Ask {
        kind: AskKind::Run,
        summary: command.into(),
        place: "~".into(),
        origin: None,
        body: vec![DiffLine::code(command)],
    };
    // The user is typing in a full-screen editor when Claude asks.
    press(&mut state, now, key(KeyCode::Char('i')));
    update(&mut state, Msg::Agent(AgentEvent::Ask { id: 1, ask: ask("make") }), now);
    assert!(state.approval.is_some());
    let typed_on = press(&mut state, now + Duration::from_millis(100), key(KeyCode::Char('y')));
    assert!(typed_on.is_empty(), "a y typed as the dialog appears neither approves nor reaches the editor");
    assert!(state.approval.is_some());
    let approved = press(&mut state, now + APPROVAL_GRACE, key(KeyCode::Char('y')));
    assert_eq!(approved, vec![Effect::AgentAnswer { id: 1, approved: true }]);

    // Asked while the user is idle, Yes waits out the grace too, and so does the ask behind it.
    let later = now + Duration::from_secs(5);
    update(&mut state, Msg::Agent(AgentEvent::Ask { id: 2, ask: ask("ls") }), later);
    update(&mut state, Msg::Agent(AgentEvent::Ask { id: 3, ask: ask("make clean") }), later);
    assert!(press(&mut state, later, key(KeyCode::Char('y'))).is_empty());
    let answered = later + APPROVAL_GRACE;
    assert_eq!(
        press(&mut state, answered, key(KeyCode::Char('y'))),
        vec![Effect::AgentAnswer { id: 2, approved: true }]
    );
    assert_eq!(state.approval.as_ref().map(|approval| approval.id), Some(3));
    assert!(press(&mut state, answered, key(KeyCode::Char('y'))).is_empty(), "a second y meant for the first ask");
    let next = answered + APPROVAL_GRACE;
    assert_eq!(press(&mut state, next, key(KeyCode::Char('y'))), vec![Effect::AgentAnswer { id: 3, approved: true }]);
}

#[test]
fn remote_picker_mounts_the_panel_that_opened_it() {
    let (mut state, now) = setup();
    state.active = Side::Right;
    let left = state.panel(Side::Left).dir().to_path_buf();
    update(&mut state, Msg::Key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::ALT)), now);
    update(
        &mut state,
        Msg::RemoteCatalog {
            choices: vec![crate::RemoteChoice {
                machine: "server".into(),
                session: Some("id".into()),
                name: "Work".into(),
                ..Default::default()
            }],
            complete: true,
        },
        now,
    );
    state.active = Side::Left;
    let effects = update(&mut state, Msg::Key(key(KeyCode::Enter)), now);
    assert_eq!(
        effects[0],
        Effect::MountRemote { side: Side::Right, machine: "server".into(), session: "id".into(), name: "Work".into() }
    );
    update(
        &mut state,
        Msg::MountRemote { side: Side::Right, machine: "server".into(), session: "id".into(), name: "Work".into() },
        now,
    );
    assert!(state.panel(Side::Right).remote_mount.is_some());
    assert!(state.panel(Side::Left).remote_mount.is_none());
    assert_eq!(state.panel(Side::Left).dir(), left);
}

#[test]
fn follow_routes_commands_and_ctrl_o_to_the_remote_and_restores_local_shell() {
    let (mut state, now) = setup();
    let local = state.shell.cwd.clone();
    let remote = crate::Console::Remote { machine: "server".into(), session: "id".into() };
    let effects = update(
        &mut state,
        Msg::MountRemote { side: Side::Left, machine: "server".into(), session: "id".into(), name: "Work".into() },
        now,
    );
    assert!(effects.contains(&Effect::UseConsole(remote.clone())));
    assert_eq!(state.console, remote);
    assert!(state.console_owns_panel());
    update(&mut state, Msg::Key(key(KeyCode::Char('x'))), now);
    let effects = update(&mut state, Msg::Key(key(KeyCode::Enter)), now);
    assert!(effects.contains(&Effect::Write(b"\r".to_vec())));
    assert_eq!(state.view, View::Shell);
    assert_eq!(state.console, remote);
    update(&mut state, Msg::Key(ctrl('o')), now);
    assert_eq!(state.view, View::Panels);
    update(&mut state, Msg::Key(key(KeyCode::Tab)), now);
    assert_eq!(state.console, crate::Console::Host);
    assert_eq!(state.shell.cwd, local);
    update(&mut state, Msg::Key(key(KeyCode::Tab)), now);
    assert_eq!(state.console, remote);
    update(&mut state, Msg::Key(ctrl('o')), now);
    assert_eq!(state.console, remote);
    assert_eq!(state.view, View::Shell);
    update(
        &mut state,
        Msg::RemoteConsoleClosed { machine: "server".into(), session: "id".into(), error: "disconnected".into() },
        now,
    );
    assert_eq!(state.view, View::Panels);
    assert_eq!(state.console, crate::Console::Host);
    assert!(!state.console_owns_panel());
    assert!(update(&mut state, Msg::Key(ctrl('o')), now).is_empty());
    assert_eq!(state.view, View::Panels);
    assert!(update(&mut state, Msg::Paste("echo wrong-machine\n".into()), now).is_empty());
    assert!(
        update(&mut state, Msg::Key(key(KeyCode::Enter)), now).iter().all(|effect| !matches!(effect, Effect::Write(_)))
    );
}

#[test]
fn failed_remote_console_keeps_its_error_while_typing_and_can_be_retried_from_alt_h() {
    let (mut state, now) = setup();
    let target =
        || Msg::MountRemote { side: Side::Left, machine: "server".into(), session: "id".into(), name: "Work".into() };
    update(&mut state, target(), now);
    let error = "Host needs a new Borg binary";
    update(
        &mut state,
        Msg::RemoteConsoleClosed { machine: "server".into(), session: "id".into(), error: error.into() },
        now,
    );
    assert!(state.settings.preferences.docker_console);
    for message in [Msg::Key(key(KeyCode::Char('a'))), Msg::Key(ctrl('o')), Msg::Paste("uname -a\n".into())] {
        assert!(update(&mut state, message, now).is_empty());
        assert_eq!(state.notice.as_deref(), Some(error));
    }
    let effects = update(&mut state, target(), now);
    assert!(
        effects
            .contains(&Effect::UseConsole(crate::Console::Remote { machine: "server".into(), session: "id".into() }))
    );
    assert!(state.failed_remote_console.is_none());
}

#[test]
fn raw_foreground_program_captures_keys_without_an_alternate_screen() {
    let (mut state, now) = setup();
    let captured = ShellView { foreground: Foreground::Program, raw_input: true, ..idle_prompt() };
    update(&mut state, Msg::Observe(captured), now);
    assert_eq!(state.effective_view(), View::Shell);
    assert_eq!(writes(&press(&mut state, now, ctrl('o'))), "\u{f}");
    assert_eq!(writes(&press(&mut state, now, key(KeyCode::F(1)))), "\u{1b}OP");
    assert!(state.overlay.is_none());
    update(&mut state, Msg::Observe(idle_prompt()), now);
    assert_eq!(state.effective_view(), View::Panels);
}

#[test]
fn remote_mode_updates_do_not_claim_that_pending_input_was_echoed() {
    let (mut state, now) = setup();
    update(
        &mut state,
        Msg::MountRemote { side: Side::Left, machine: "server".into(), session: "id".into(), name: "Work".into() },
        now,
    );
    update(&mut state, Msg::Key(key(KeyCode::Char('a'))), now);
    assert!(state.shell.unechoed);
    update(
        &mut state,
        Msg::RemoteConsoleOutput {
            echoed: false,
            machine: "server".into(),
            session: "id".into(),
            view: idle_prompt(),
            signals: Vec::new(),
        },
        now,
    );
    assert!(state.shell.unechoed);
    assert!(!state.shell.line_empty());
}

#[test]
fn remote_labels_rename_without_changing_connection_targets_and_preserve_the_picker() {
    let (mut state, now) = setup();
    update(&mut state, Msg::Key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::ALT)), now);
    let choices = vec![crate::RemoteChoice {
        identity: "public-key".into(),
        machine: "server-id".into(),
        session: Some("default".into()),
        name: "Default".into(),
        ..Default::default()
    }];
    update(&mut state, Msg::RemoteCatalog { choices: choices.clone(), complete: false }, now);
    update(&mut state, Msg::Key(key(KeyCode::F(2))), now);
    assert!(matches!(state.overlay, Some(Overlay::Prompt(ref p)) if p.title == "Rename host label"));
    update(&mut state, Msg::Key(ctrl('u')), now);
    update(&mut state, Msg::Paste("Production".into()), now);
    assert_eq!(
        update(&mut state, Msg::Key(key(KeyCode::Enter)), now),
        vec![Effect::SaveRemoteName { identity: "public-key".into(), session: None, name: "Production".into() }]
    );
    let mut names = crate::RemoteNames::default();
    names.set("public-key", None, "Production").unwrap();
    names.set("public-key", Some("default"), "Deploy").unwrap();
    update(&mut state, Msg::RemoteNames(Ok(names)), now);
    let Some(Overlay::Remotes(browser)) = &state.overlay else { panic!("picker") };
    assert_eq!(browser.choices[0].host_name(), "Production");
    assert_eq!(browser.choices[0].session_name(), "Deploy");
    assert_eq!(
        update(&mut state, Msg::Key(key(KeyCode::Enter)), now),
        vec![Effect::MountRemote {
            side: Side::Left,
            machine: "server-id".into(),
            session: "default".into(),
            name: "Default".into()
        }]
    );
    update(
        &mut state,
        Msg::MountRemote {
            side: Side::Left,
            machine: "server-id".into(),
            session: "default".into(),
            name: "Default".into(),
        },
        now,
    );
    assert_eq!(state.remote_host_name("server-id"), "Production");
    assert_eq!(state.remote_session_name("server-id", "default", "Default"), "Deploy");
    update(&mut state, Msg::Key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::ALT)), now);
    update(&mut state, Msg::RemoteCatalog { choices: choices.clone(), complete: false }, now);
    update(&mut state, Msg::Key(KeyEvent::new(KeyCode::F(2), KeyModifiers::SHIFT)), now);
    update(&mut state, Msg::Key(ctrl('u')), now);
    update(&mut state, Msg::RemoteCatalog { choices, complete: true }, now);
    update(&mut state, Msg::Key(key(KeyCode::Esc)), now);
    assert!(
        matches!(state.overlay, Some(Overlay::Remotes(ref browser)) if !browser.loading && browser.choices[0].session_name() == "Deploy")
    );
    update(&mut state, Msg::Key(KeyEvent::new(KeyCode::F(2), KeyModifiers::SHIFT)), now);
    update(&mut state, Msg::Key(ctrl('u')), now);
    assert_eq!(
        update(&mut state, Msg::Key(key(KeyCode::Enter)), now),
        vec![Effect::SaveRemoteName {
            identity: "public-key".into(),
            session: Some("default".into()),
            name: String::new()
        }]
    );
}

#[test]
fn remote_labels_are_scoped_to_the_paired_key_and_validate_user_input() {
    let mut names = crate::RemoteNames::default();
    names.set("key-one", None, "Prødüction").unwrap();
    names.set("key-one", Some("default"), "Deploy").unwrap();
    let mut first = crate::RemoteChoice {
        identity: "key-one".into(),
        machine: "hostname".into(),
        session: Some("default".into()),
        name: "Default".into(),
        ..Default::default()
    };
    names.apply(&mut first);
    assert_eq!(first.host_name(), "Prødüction");
    assert_eq!(first.session_name(), "Deploy");
    let mut second = first.clone();
    second.identity = "key-two".into();
    names.apply(&mut second);
    assert_eq!(second.host_name(), "hostname");
    assert_eq!(second.session_name(), "Default");
    assert!(names.set("key-one", None, "bad\nlabel").is_err());
    assert!(names.set("key-one", None, &"x".repeat(41)).is_err());
}

#[test]
fn a_catalog_refresh_cannot_redirect_an_open_rename_to_another_remote() {
    let (mut state, now) = setup();
    update(&mut state, Msg::Key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::ALT)), now);
    update(
        &mut state,
        Msg::RemoteCatalog {
            choices: vec![crate::RemoteChoice {
                identity: "first-key".into(),
                machine: "first".into(),
                session: Some("first-session".into()),
                name: "Work".into(),
                ..Default::default()
            }],
            complete: false,
        },
        now,
    );
    update(&mut state, Msg::Key(KeyEvent::new(KeyCode::F(2), KeyModifiers::SHIFT)), now);
    update(
        &mut state,
        Msg::RemoteCatalog {
            choices: vec![crate::RemoteChoice {
                identity: "second-key".into(),
                machine: "second".into(),
                session: Some("second-session".into()),
                name: "Default".into(),
                ..Default::default()
            }],
            complete: true,
        },
        now,
    );
    update(&mut state, Msg::Key(ctrl('u')), now);
    update(&mut state, Msg::Paste("Deploy".into()), now);
    assert_eq!(
        update(&mut state, Msg::Key(key(KeyCode::Enter)), now),
        vec![Effect::SaveRemoteName {
            identity: "first-key".into(),
            session: Some("first-session".into()),
            name: "Deploy".into()
        }]
    );
}

#[test]
fn panel_copy_captures_remote_endpoints_and_keeps_destination_relative_paths_remote() {
    for source_remote in [false, true] {
        for side in [Side::Left, Side::Right] {
            let (mut state, now) = setup();
            state.active = side;
            let remote_side = if source_remote { side } else { side.other() };
            crate::mount_remote(&mut state, remote_side, "server".into(), "remote-id".into(), "Work".into());
            if source_remote {
                let panel = state.panel(remote_side);
                let generation = panel.generation();
                update(&mut state, Msg::Batch { side: remote_side, generation, entries: vec![file("notes.txt")] }, now);
                update(&mut state, Msg::Listed { side: remote_side, generation, error: None }, now);
                state.panel_mut(remote_side).move_by(1);
            } else {
                state.panel_mut(side).move_by(1);
            }
            crate::actions::perform(&mut state, Action::Copy, now);
            let Some(Overlay::Prompt(prompt)) = state.overlay.as_mut() else { panic!("copy prompt") };
            assert!(prompt.label.contains(if source_remote { "client filesystem" } else { "host server:Work" }));
            let crate::PromptPurpose::RemoteCopy { from, to, .. } = &prompt.purpose else {
                panic!("remote copy purpose")
            };
            assert_eq!(from.is_some(), source_remote);
            assert_eq!(to.is_some(), !source_remote);
            prompt.input = crate::LineInput::new("nested");
            let expected = state.panel(side.other()).dir().join("nested");
            let effects = update(&mut state, Msg::Key(key(KeyCode::Enter)), now);
            assert!(effects.iter().any(|effect| matches!(effect, Effect::StartRemoteCopy { from, to, target, .. } if from.is_some() == source_remote && to.is_some() != source_remote && *target == expected)));
            assert_eq!(state.jobs.len(), 1);
        }
    }
}

#[test]
fn language_picker_saves_only_on_accept_and_menus_follow_state() {
    let (mut state, now) = setup();
    crate::actions::perform(&mut state, Action::PickLanguage, now);
    press(&mut state, now, key(KeyCode::Down));
    assert!(press(&mut state, now, key(KeyCode::Esc)).is_empty());
    assert_eq!(state.settings.preferences.language, borg_i18n::Locale::English);
    crate::actions::perform(&mut state, Action::PickLanguage, now);
    press(&mut state, now, key(KeyCode::End));
    let effects = press(&mut state, now, key(KeyCode::Enter));
    assert_eq!(effects, vec![Effect::SetLanguage(borg_i18n::Locale::Russian), Effect::SaveConfig]);
    assert_eq!(state.settings.preferences.language, borg_i18n::Locale::Russian);
    assert!(state.overlay.is_none());
    assert_eq!(menus(&state)[4].title, "Параметры");
    assert_eq!(borg_i18n::locale(), borg_i18n::Locale::English);
}

#[test]
fn forgetting_an_offline_host_confirms_locally_and_restores_its_panels_and_consoles() {
    let (mut state, now) = setup();
    let left = state.panel(Side::Left).dir().to_path_buf();
    let right = state.panel(Side::Right).dir().to_path_buf();
    crate::mount_remote(&mut state, Side::Left, "server".into(), "one".into(), "One".into());
    crate::mount_remote(&mut state, Side::Right, "server".into(), "two".into(), "Two".into());
    state.remote_consoles.insert(("server".into(), "two".into()), crate::ShellStatus::new(Default::default()));
    state.remote_consoles.insert(("other".into(), "default".into()), crate::ShellStatus::new(Default::default()));
    state.console = crate::Console::Remote { machine: "server".into(), session: "one".into() };
    state.parked_host = Some(crate::ShellStatus::new(left.clone()));
    crate::actions::perform(&mut state, Action::BrowseRemotes, now);
    update(
        &mut state,
        Msg::RemoteCatalog {
            choices: vec![
                crate::RemoteChoice { machine: "server".into(), name: "Unavailable".into(), ..Default::default() },
                crate::RemoteChoice {
                    machine: "other".into(),
                    session: Some("default".into()),
                    name: "Default".into(),
                    ..Default::default()
                },
            ],
            complete: true,
        },
        now,
    );
    update(&mut state, Msg::Key(key(KeyCode::F(8))), now);
    assert!(
        matches!(&state.overlay, Some(Overlay::Confirm(confirm)) if !confirm.accept && confirm.message.contains("server") && confirm.message.contains("files are kept"))
    );
    assert_eq!(update(&mut state, Msg::Key(key(KeyCode::Enter)), now), vec![Effect::OpenRemotes]);
    update(&mut state, Msg::Key(key(KeyCode::F(8))), now);
    assert_eq!(
        update(&mut state, Msg::Key(key(KeyCode::Char('y'))), now),
        vec![Effect::ForgetRemoteHost { machine: "server".into() }]
    );
    assert!(update(&mut state, Msg::Key(key(KeyCode::F(8))), now).is_empty());
    update(
        &mut state,
        Msg::RemoteHostForgotten { machine: "server".into(), result: Err("Cannot save pairing file".into()) },
        now,
    );
    assert!(state.panel(Side::Left).remote_mount.is_some());
    assert!(
        matches!(&state.overlay, Some(Overlay::Remotes(browser)) if browser.forgetting.is_none() && browser.choices.len()==2 && browser.error.as_deref()==Some("Cannot save pairing file"))
    );
    update(&mut state, Msg::Key(key(KeyCode::F(8))), now);
    update(&mut state, Msg::Key(key(KeyCode::Char('y'))), now);
    let effects = update(&mut state, Msg::RemoteHostForgotten { machine: "server".into(), result: Ok(()) }, now);
    assert!(effects.contains(&Effect::CloseRemoteConsole { machine: "server".into(), session: "one".into() }));
    assert!(effects.contains(&Effect::CloseRemoteConsole { machine: "server".into(), session: "two".into() }));
    assert!(!effects.iter().any(|effect| matches!(effect, Effect::DeleteRemoteSession { .. })));
    assert_eq!(state.panel(Side::Left).dir(), left);
    assert_eq!(state.panel(Side::Right).dir(), right);
    assert_eq!(state.console, crate::Console::Host);
    assert!(state.remote_consoles.contains_key(&("other".into(), "default".into())));
    assert!(
        matches!(&state.overlay, Some(Overlay::Remotes(browser)) if browser.forgetting.is_none() && browser.choices.len()==1 && browser.choices[0].machine=="other")
    );
}

#[test]
fn u_updates_the_chosen_host_after_asking_and_reports_how_it_came_back() {
    let (mut state, now) = setup();
    crate::actions::perform(&mut state, Action::BrowseRemotes, now);
    let host = |machine: &str, session: Option<&str>| crate::RemoteChoice {
        machine: machine.into(),
        session: session.map(Into::into),
        name: "Default".into(),
        ..Default::default()
    };
    update(
        &mut state,
        Msg::RemoteCatalog { choices: vec![host("server", Some("default")), host("gone", None)], complete: true },
        now,
    );
    update(&mut state, Msg::Key(key(KeyCode::Char('u'))), now);
    assert!(
        matches!(&state.overlay, Some(Overlay::Confirm(confirm)) if !confirm.accept && confirm.message.contains("server") && matches!(&confirm.purpose, crate::ConfirmPurpose::RemoteHostUpdate { machine, .. } if machine == "server"))
    );
    assert!(update(&mut state, Msg::Key(key(KeyCode::Enter)), now).is_empty(), "No is the default");
    assert!(matches!(&state.overlay, Some(Overlay::Remotes(browser)) if browser.updating.is_none()));
    update(&mut state, Msg::Key(key(KeyCode::Char('u'))), now);
    assert_eq!(
        update(&mut state, Msg::Key(key(KeyCode::Char('y'))), now),
        vec![Effect::UpdateRemoteHost { machine: "server".into() }]
    );
    assert!(matches!(&state.overlay, Some(Overlay::Remotes(browser)) if browser.updating.as_deref() == Some("server")));
    assert!(update(&mut state, Msg::Key(key(KeyCode::Char('u'))), now).is_empty(), "one update at a time");
    let effects = update(
        &mut state,
        Msg::RemoteHostUpdated { machine: "server".into(), result: Ok(crate::HostUpdate::Moved("0.12.0".into())) },
        now,
    );
    assert_eq!(effects, vec![Effect::OpenRemotes]);
    assert!(state.notice.as_deref().is_some_and(|notice| notice.contains("server") && notice.contains("0.12.0")));
    assert!(matches!(&state.overlay, Some(Overlay::Remotes(browser)) if browser.updating.is_none() && browser.loading));

    update(
        &mut state,
        Msg::RemoteCatalog { choices: vec![host("server", Some("default")), host("gone", None)], complete: true },
        now,
    );
    update(&mut state, Msg::Key(key(KeyCode::Char('u'))), now);
    update(&mut state, Msg::Key(key(KeyCode::Char('y'))), now);
    update(&mut state, Msg::RemoteHostUpdated { machine: "server".into(), result: Err("no network".into()) }, now);
    assert!(
        matches!(&state.overlay, Some(Overlay::Remotes(browser)) if browser.updating.is_none() && browser.error.as_deref() == Some("no network"))
    );

    update(&mut state, Msg::Key(key(KeyCode::Down)), now);
    update(&mut state, Msg::Key(key(KeyCode::Char('u'))), now);
    assert!(
        matches!(&state.overlay, Some(Overlay::Remotes(browser)) if browser.error.as_deref().is_some_and(|error| error.contains("gone"))),
        "a host with no session cannot be reached to update"
    );

    state.overlay = None;
    update(
        &mut state,
        Msg::RemoteHostUpdated { machine: "server".into(), result: Ok(crate::HostUpdate::Current("0.12.0".into())) },
        now,
    );
    assert!(
        state.notice.as_deref().is_some_and(|notice| notice.contains("already")),
        "the result still shows once the list is closed"
    );
}

#[test]
fn shift_f8_forgets_a_host_even_when_its_default_session_is_available() {
    let (mut state, now) = setup();
    crate::actions::perform(&mut state, Action::BrowseRemotes, now);
    update(
        &mut state,
        Msg::RemoteCatalog {
            choices: vec![crate::RemoteChoice {
                machine: "server".into(),
                session: Some("default".into()),
                name: "Default".into(),
                ..Default::default()
            }],
            complete: true,
        },
        now,
    );
    update(&mut state, Msg::Key(KeyEvent::new(KeyCode::F(8), KeyModifiers::SHIFT)), now);
    assert!(
        matches!(&state.overlay, Some(Overlay::Confirm(confirm)) if matches!(&confirm.purpose, crate::ConfirmPurpose::RemoteHostForget { machine, .. } if machine=="server"))
    );
    assert_eq!(
        update(&mut state, Msg::Key(key(KeyCode::Char('y'))), now),
        vec![Effect::ForgetRemoteHost { machine: "server".into() }]
    );
}

#[test]
fn unavailable_hosts_explain_why_files_and_mirror_cannot_open() {
    let (mut state, now) = setup();
    crate::actions::perform(&mut state, Action::BrowseRemotes, now);
    update(
        &mut state,
        Msg::RemoteCatalog {
            choices: vec![crate::RemoteChoice {
                machine: "offline-lab".into(),
                name: "Unavailable: connection timed out".into(),
                ..Default::default()
            }],
            complete: true,
        },
        now,
    );
    for code in [KeyCode::Char('m'), KeyCode::Char('M'), KeyCode::Enter] {
        assert!(update(&mut state, Msg::Key(key(code)), now).is_empty());
        assert!(
            matches!(&state.overlay, Some(Overlay::Remotes(browser)) if !browser.connecting && browser.error.as_ref().is_some_and(|error| error.contains("offline-lab") && error.contains("press R")))
        );
    }
    assert_eq!(update(&mut state, Msg::Key(key(KeyCode::Char('r'))), now), vec![Effect::OpenRemotes]);
    assert!(matches!(&state.overlay, Some(Overlay::Remotes(browser)) if browser.loading && browser.error.is_none()));
    update(&mut state, Msg::Key(key(KeyCode::Esc)), now);
    assert!(state.overlay.is_none());
}

#[test]
fn keys_held_with_ctrl_or_alt_work_in_a_cyrillic_layout() {
    let (mut state, now) = setup();
    press(&mut state, now, ctrl('щ'));
    assert_eq!(state.view, View::Shell);
    assert_eq!(writes(&press(&mut state, now, ctrl('с'))), "\u{3}");
    press(&mut state, now, ctrl('щ'));
    assert_eq!(state.view, View::Panels);
    press(&mut state, now, KeyEvent::new(KeyCode::Char('ь'), KeyModifiers::ALT));
    assert!(state.activity.side.is_some());
}

#[test]
fn keys_held_with_ctrl_work_in_other_non_latin_layouts() {
    for (o, c) in [('ο', 'ψ'), ('ם', 'ב'), ('خ', 'ؤ'), ('ო', 'ც'), ('ㅐ', 'ㅊ'), ('น', 'แ')] {
        let (mut state, now) = setup();
        press(&mut state, now, ctrl(o));
        assert_eq!(state.view, View::Shell, "{o}");
        assert_eq!(writes(&press(&mut state, now, ctrl(c))), "\u{3}", "{c}");
    }
}

#[test]
fn letters_typed_on_the_command_line_stay_as_typed() {
    let (mut state, now) = setup();
    assert_eq!(writes(&press(&mut state, now, key(KeyCode::Char('т')))), "т");
}

#[test]
fn a_question_answers_to_the_letters_its_buttons_show() {
    let (mut state, now) = setup_with(vec![file("a")]);
    state.settings.preferences.language = borg_i18n::Locale::Russian;
    press(&mut state, now, key(KeyCode::Down));
    press(&mut state, now, key(KeyCode::F(8)));
    assert!(press(&mut state, now, key(KeyCode::Char('н'))).is_empty());
    assert!(state.overlay.is_none());

    press(&mut state, now, key(KeyCode::F(8)));
    let effects = press(&mut state, now, key(KeyCode::Char('д')));
    assert_eq!(
        effects,
        vec![Effect::StartJob { id: 0, spec: Spec::Delete { sources: vec![PathBuf::from("/home/a")] } }]
    );
}

#[test]
fn f3_on_a_capture_file_opens_the_traffic_viewer() {
    let (mut state, now) = setup_with(vec![file("notes.txt"), file("trace.pcapng")]);
    while hovered(&state) != Some("trace.pcapng") {
        state.panel_mut(Side::Left).move_by(1);
    }
    let effects = press(&mut state, now, key(KeyCode::F(3)));
    assert_eq!(effects, [Effect::OpenCapture(PathBuf::from("/home/trace.pcapng"))]);
    state.settings.preferences.system_plugin = false;
    let effects = press(&mut state, now, key(KeyCode::F(3)));
    assert!(!effects.iter().any(|effect| matches!(effect, Effect::OpenCapture(_))), "without the plugin it is a file");
}

fn run_ask(command: &str) -> crate::Ask {
    crate::Ask {
        kind: crate::AskKind::Run,
        summary: command.into(),
        place: "in ~".into(),
        origin: None,
        body: vec![crate::DiffLine::code(command)],
    }
}

/// A host borg with one ask in its dialog and one behind it, and the board its viewers read.
fn host_with_two_asks() -> (AppState, Instant, crate::AskBoard) {
    use crate::AgentEvent;
    let (mut state, now) = setup_with(Vec::new());
    update(&mut state, Msg::Agent(AgentEvent::Ask { id: 1, ask: run_ask("make") }), now);
    update(&mut state, Msg::Agent(AgentEvent::Ask { id: 2, ask: run_ask("make test") }), now);
    let mut board = crate::AskBoard::default();
    assert!(board.refresh(&state, None));
    (state, now, board)
}

const LAPTOP: &str = "laptop [873dd93b]";
const DESKTOP: &str = "desktop [1f00aa21]";
const RUN: &str = "run-1";

#[test]
fn a_host_shows_its_viewers_every_waiting_ask_and_the_first_answer_wins() {
    use crate::{AgentEvent, End, HostAnswer, Refusal, check_answer};
    let (mut state, now, mut board) = host_with_two_asks();
    let feed = board.feed(RUN, LAPTOP);
    let waiting: Vec<_> = feed.waiting.iter().map(|waiting| (waiting.id, waiting.ask.summary.as_str())).collect();
    assert_eq!(waiting, [(1, "make"), (2, "make test")], "the dialog's ask first, then the one behind it");
    assert!(!board.refresh(&state, None), "nothing changed, so viewers wait on");

    let answer = HostAnswer { run: RUN.into(), id: 1, approved: true };
    assert_eq!(check_answer(&state, &answer, RUN, LAPTOP, None), Ok(()));
    let effects =
        update(&mut state, Msg::Agent(AgentEvent::AnsweredFrom { id: 1, approved: true, by: LAPTOP.into() }), now);
    assert_eq!(effects, vec![Effect::AgentAnswer { id: 1, approved: true }]);
    assert_eq!(state.notice, Some(borg_i18n::tr!(agent_answered_from, computer = "laptop", summary = "make")));
    assert_eq!(state.agent.running, Some((1, "make".into())), "the footer shows the command as for an answer here");
    assert_eq!(state.approval.as_ref().map(|approval| approval.id), Some(2), "the next ask shows");

    assert_eq!(
        check_answer(&state, &HostAnswer { approved: false, ..answer.clone() }, RUN, DESKTOP, None),
        Err(Refusal::Ended),
        "a second answer to the same ask, from anywhere, is refused"
    );
    assert!(board.refresh(&state, None));
    let feed = board.feed(RUN, DESKTOP);
    assert_eq!(feed.waiting.iter().map(|waiting| waiting.id).collect::<Vec<_>>(), [2]);
    assert_eq!(
        feed.ended.last().map(|ended| (ended.id, ended.end.clone())),
        Some((1, End::Answered { approved: true, by: Some(LAPTOP.into()) }))
    );

    let effects = press(&mut state, now + crate::agent::APPROVAL_GRACE, key(KeyCode::Char('n')));
    assert_eq!(effects, vec![Effect::AgentAnswer { id: 2, approved: false }], "the host's own screen answers too");
    assert_eq!(
        check_answer(&state, &HostAnswer { run: RUN.into(), id: 2, approved: true }, RUN, LAPTOP, None),
        Err(Refusal::Ended)
    );
    assert!(board.refresh(&state, None));
    assert_eq!(
        board.feed(RUN, LAPTOP).ended.last().map(|ended| ended.end.clone()),
        Some(End::Answered { approved: false, by: None })
    );
}

#[test]
fn a_host_refuses_answers_for_another_run_or_from_a_viewer_another_computer_outranks() {
    use crate::{HostAnswer, Refusal, check_answer};
    let (state, _, mut board) = host_with_two_asks();
    let answer = HostAnswer { run: "run-0".into(), id: 1, approved: true };
    assert_eq!(check_answer(&state, &answer, RUN, LAPTOP, None), Err(Refusal::OtherRun));
    let answer = HostAnswer { run: RUN.into(), id: 9, approved: true };
    assert_eq!(check_answer(&state, &answer, RUN, LAPTOP, None), Err(Refusal::Ended), "no ask has that number");

    let answer = HostAnswer { run: RUN.into(), id: 2, approved: true };
    assert_eq!(check_answer(&state, &answer, RUN, LAPTOP, Some(DESKTOP)), Err(Refusal::ViewOnly));
    assert_eq!(check_answer(&state, &answer, RUN, DESKTOP, Some(DESKTOP)), Ok(()), "the controlling computer answers");
    assert!(board.refresh(&state, Some(DESKTOP)));
    assert_eq!(board.feed(RUN, LAPTOP).view_only.as_deref(), Some(DESKTOP));
    assert_eq!(board.feed(RUN, DESKTOP).view_only, None);
}

#[test]
fn a_withdrawn_or_timed_out_ask_ends_for_the_viewers_too() {
    use crate::{AgentEvent, End, HostAnswer, Refusal, check_answer};
    let (mut state, now, mut board) = host_with_two_asks();
    update(&mut state, Msg::Agent(AgentEvent::Withdrawn { id: 2 }), now);
    update(&mut state, Msg::Agent(AgentEvent::Withdrawn { id: 1 }), now);
    assert_eq!(state.approval, None);
    assert!(board.refresh(&state, None));
    let feed = board.feed(RUN, LAPTOP);
    assert!(feed.waiting.is_empty());
    let ended: Vec<_> = feed.ended.iter().map(|ended| (ended.id, ended.end.clone())).collect();
    assert_eq!(ended, [(2, End::Withdrawn), (1, End::Withdrawn)]);
    let answer = HostAnswer { run: RUN.into(), id: 1, approved: true };
    assert_eq!(check_answer(&state, &answer, RUN, LAPTOP, None), Err(Refusal::Ended));
    let effects =
        update(&mut state, Msg::Agent(AgentEvent::AnsweredFrom { id: 1, approved: true, by: LAPTOP.into() }), now);
    assert!(effects.is_empty(), "an ask that ended is never carried out");
}

/// A viewer borg whose panel shows `web1`'s session, which waits for the asks in `waiting`.
fn viewer_feed(version: u64, waiting: &[(u64, &str)], ended: Vec<crate::Ended>) -> Msg {
    let feed = crate::AskFeed {
        run: RUN.into(),
        version,
        waiting: waiting.iter().map(|(id, command)| crate::Waiting { id: *id, ask: run_ask(command) }).collect(),
        ended,
        approve_all: Vec::new(),
        view_only: None,
    };
    Msg::HostAsks { machine: "web1".into(), session: "default".into(), event: crate::HostEvent::Feed(feed) }
}

#[test]
fn a_viewer_shows_a_host_ask_over_everything_and_sends_the_answer_back() {
    use crate::HostAnswer;
    let (mut state, now) = setup_with(Vec::new());
    state.screensaver = true;
    update(&mut state, viewer_feed(1, &[(7, "systemctl restart nginx")], Vec::new()), now);
    let approval = state.approval.clone().expect("the host's ask shows");
    assert!(!state.screensaver, "the ask wakes the screen");
    assert_eq!((approval.id, approval.accept), (7, false), "No stays selected");
    assert_eq!(approval.host.as_ref().map(|host| (host.server.as_str(), host.run.as_str())), Some(("web1", RUN)));

    let effects = press(&mut state, now + crate::agent::APPROVAL_GRACE, key(KeyCode::Char('y')));
    let answer = HostAnswer { run: RUN.into(), id: 7, approved: true };
    assert_eq!(
        effects,
        vec![Effect::AnswerHost { machine: "web1".into(), session: "default".into(), answer }],
        "the answer goes to the host, and nothing runs here"
    );
    assert_eq!(state.agent.running, None);
    assert_eq!(state.approval, None);
    update(&mut state, viewer_feed(1, &[(7, "systemctl restart nginx")], Vec::new()), now);
    assert_eq!(state.approval, None, "an ask answered here does not show again while the host takes it");
}

#[test]
fn a_viewer_closes_the_dialog_with_a_notice_when_the_host_answered_or_lost_the_ask() {
    use crate::{End, Ended, HostEvent};
    let (mut state, now) = setup_with(Vec::new());
    update(&mut state, viewer_feed(1, &[(7, "make"), (8, "make test")], Vec::new()), now);
    let answered = Ended { id: 7, end: End::Answered { approved: true, by: None } };
    update(&mut state, viewer_feed(2, &[(8, "make test")], vec![answered]), now);
    assert_eq!(state.notice, Some(borg_i18n::tr!(agent_host_answered, server = "web1", summary = "make")));
    assert_eq!(state.approval.as_ref().map(|approval| approval.id), Some(8), "the next one shows");

    let by_desktop = Ended { id: 8, end: End::Answered { approved: false, by: Some(DESKTOP.into()) } };
    update(&mut state, viewer_feed(3, &[(9, "ls")], vec![by_desktop]), now);
    assert_eq!(state.notice, Some(borg_i18n::tr!(agent_answered_from, computer = "desktop", summary = "make test")));

    update(&mut state, viewer_feed(4, &[(9, "ls")], Vec::new()), now);
    update(
        &mut state,
        Msg::HostAsks { machine: "web1".into(), session: "default".into(), event: HostEvent::Lost },
        now,
    );
    assert_eq!(state.approval, None);
    assert_eq!(state.notice, Some(borg_i18n::tr!(agent_host_lost, server = "web1")));
    update(&mut state, viewer_feed(5, &[(9, "ls")], Vec::new()), now);
    assert_eq!(state.approval.as_ref().map(|approval| approval.id), Some(9), "it comes back with the connection");

    update(&mut state, viewer_feed(6, &[], vec![Ended { id: 9, end: End::Withdrawn }]), now);
    assert_eq!(state.notice, Some(borg_i18n::tr!(agent_withdrawn, summary = "ls")));
    update(&mut state, viewer_feed(7, &[(10, "pwd")], Vec::new()), now);
    update(
        &mut state,
        Msg::HostAsks { machine: "web1".into(), session: "default".into(), event: HostEvent::Unwatched },
        now,
    );
    assert_eq!(state.approval, None, "a panel that stops showing the session takes its asks along");
    assert!(state.agent.hosts.is_empty());
}

#[test]
fn an_ask_answered_here_does_not_hide_the_one_with_its_number_after_the_host_restarts() {
    let (mut state, now) = setup_with(Vec::new());
    update(&mut state, viewer_feed(1, &[(1, "make")], Vec::new()), now);
    press(&mut state, now + crate::agent::APPROVAL_GRACE, key(KeyCode::Char('y')));
    update(
        &mut state,
        Msg::HostAsks { machine: "web1".into(), session: "default".into(), event: crate::HostEvent::Lost },
        now,
    );
    let Msg::HostAsks { machine, session, event: crate::HostEvent::Feed(mut feed) } =
        viewer_feed(1, &[(1, "make test")], Vec::new())
    else {
        unreachable!()
    };
    feed.run = "run-2".into();
    update(&mut state, Msg::HostAsks { machine, session, event: crate::HostEvent::Feed(feed) }, now);
    let shown = state.approval.as_ref().map(|approval| (approval.id, approval.ask.summary.as_str()));
    assert_eq!(shown, Some((1, "make test")), "the restarted host's first ask shows");
}

#[test]
fn a_view_only_viewer_shows_the_ask_takes_no_answer_and_esc_closes_it_here() {
    let (mut state, now) = setup_with(Vec::new());
    let Msg::HostAsks { machine, session, event: crate::HostEvent::Feed(mut feed) } =
        viewer_feed(1, &[(7, "make")], Vec::new())
    else {
        unreachable!()
    };
    feed.view_only = Some(DESKTOP.into());
    update(&mut state, Msg::HostAsks { machine, session, event: crate::HostEvent::Feed(feed) }, now);
    let later = now + crate::agent::APPROVAL_GRACE;
    for answer in [key(KeyCode::Char('y')), key(KeyCode::Enter), key(KeyCode::Char('n'))] {
        assert!(press(&mut state, later, answer).is_empty());
        assert!(state.approval.is_some(), "a view-only dialog takes no answer");
    }
    assert!(press(&mut state, later, key(KeyCode::Esc)).is_empty(), "Esc sends nothing to the host");
    assert_eq!(state.approval, None);
    update(&mut state, viewer_feed(2, &[(7, "make")], Vec::new()), now);
    assert_eq!(state.approval, None, "closed here, it stays closed while the host waits");
}

#[test]
fn ctrl_alt_a_turns_off_approve_all_in_every_host_session_that_has_it() {
    let (mut state, now) = setup_with(Vec::new());
    let Msg::HostAsks { machine, session, event: crate::HostEvent::Feed(mut feed) } = viewer_feed(1, &[], Vec::new())
    else {
        unreachable!()
    };
    feed.approve_all = vec![LAPTOP.into()];
    update(&mut state, Msg::HostAsks { machine, session, event: crate::HostEvent::Feed(feed) }, now);
    assert_eq!(
        crate::actions::perform(&mut state, crate::Action::ApprovalModeOff, now),
        vec![Effect::StopHostApproveAll { machine: "web1".into(), session: "default".into() }]
    );
    state.agent.auto_approve = vec![DESKTOP.into()];
    assert_eq!(crate::actions::perform(&mut state, crate::Action::ApprovalModeOff, now).len(), 2, "and here too");
}

#[test]
fn a_locked_computer_key_asks_for_its_password_and_hands_it_to_the_app() {
    let (mut state, now) = setup();
    update(&mut state, Msg::ComputerKeyLocked { wrong: false }, now);
    let Some(Overlay::Prompt(prompt)) = &state.overlay else { panic!("a password prompt") };
    assert!(prompt.purpose.masked());
    assert_eq!(prompt.label, borg_i18n::tr!(cloud_unlock_label));
    assert!(press(&mut state, now, key(KeyCode::Enter)).is_empty(), "an empty password waits");

    type_text(&mut state, now, "rain cobalt");
    let effects = press(&mut state, now, key(KeyCode::Enter));
    assert_eq!(effects, vec![Effect::UnlockComputerKey(crate::Password::new("rain cobalt"))]);
    assert!(state.overlay.is_none());
    assert_eq!(state.notice, Some(borg_i18n::tr!(cloud_unlock_working).into()));

    update(&mut state, Msg::ComputerKeyLocked { wrong: true }, now);
    let Some(Overlay::Prompt(prompt)) = &state.overlay else { panic!("the prompt asks again") };
    assert_eq!(prompt.label, borg_i18n::tr!(cloud_unlock_wrong));
    assert!(press(&mut state, now, key(KeyCode::Esc)).is_empty());
    assert!(state.overlay.is_none());

    crate::actions::perform(&mut state, Action::Mkdir, now);
    update(&mut state, Msg::ComputerKeyLocked { wrong: false }, now);
    assert!(
        matches!(&state.overlay, Some(Overlay::Prompt(prompt)) if !prompt.purpose.masked()),
        "an open dialog stays"
    );
    assert_eq!(state.notice, Some(borg_i18n::tr!(cloud_key_locked).into()));
}
