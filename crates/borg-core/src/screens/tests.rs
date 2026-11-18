use super::*;
use crate::{
    Appearance, Msg, Settings, Startup, ThemeChoice,
    activity::{ActivityData, ProcessInfo, ProcessKey, SystemSample},
    update,
};
use borg_vfs::{Entry, EntryKind};

const WORKER: &str = "/opt/tools/bin/worker";
const TOOLS: &str = "/opt/tools/bin";

fn state() -> AppState {
    let (mut state, _) = AppState::new(
        Startup::at("/work".into()),
        120,
        32,
        Settings::default(),
        Appearance::new(vec![ThemeChoice { id: "midnight".into(), name: "Midnight".into() }], 0),
        std::time::Instant::now(),
    );
    state.settings.preferences.git_plugin = false;
    state
}

fn process(pid: u32, executable: Option<&str>) -> ProcessInfo {
    ProcessInfo {
        key: ProcessKey { pid, start: 1 },
        parent: None,
        name: format!("worker-{pid}"),
        executable: executable.map(PathBuf::from),
        cpu: Some(0),
        resident: Some(u64::from(pid)),
        virtual_bytes: Some(0),
        read: Some(0),
        written: Some(0),
        protection: None,
        nice: Some(0),
        stopped: false,
    }
}

fn press(state: &mut AppState, code: KeyCode, modifiers: KeyModifiers) -> Vec<Effect> {
    let now = state.epoch;
    update(state, Msg::Key(KeyEvent::new(code, modifiers)), now)
}

fn alt(state: &mut AppState, c: char) -> Vec<Effect> {
    press(state, KeyCode::Char(c), KeyModifiers::ALT)
}

/// The activity monitor open on the left panel's side, listing the processes.
fn monitor(processes: Vec<ProcessInfo>) -> AppState {
    let mut state = state();
    alt(&mut state, 'm');
    let id = state.activity.pending.as_ref().unwrap().0;
    let now = state.epoch;
    let sample = SystemSample { processes, ..Default::default() };
    update(&mut state, Msg::Activity { id, result: Ok(ActivityData::Sample(sample)) }, now);
    state
}

/// The folder the panel on `side` was asked to list, and the listing's generation.
fn listing(effects: &[Effect], side: Side) -> Option<(PathBuf, u64)> {
    effects.iter().find_map(|effect| match effect {
        Effect::List { side: listed, dir, generation } if *listed == side => Some((dir.clone(), *generation)),
        _ => None,
    })
}

fn file(name: &str) -> Entry {
    Entry {
        source: None,
        missing: false,
        usage: None,
        name: name.into(),
        kind: EntryKind::File,
        size: 0,
        modified: None,
        executable: true,
        permissions: 0o755,
    }
}

fn list(state: &mut AppState, side: Side, generation: u64, names: &[&str]) {
    let now = state.epoch;
    let entries = names.iter().map(|name| file(name)).collect();
    update(state, Msg::Batch { side, generation, entries }, now);
    update(state, Msg::Listed { side, generation, error: None }, now);
}

#[test]
fn alt_o_opens_the_program_s_folder_beside_the_list_which_keeps_its_place() {
    let mut state = monitor(vec![process(8, Some(WORKER)), process(7, Some("/usr/bin/other"))]);
    let list = state.activity.side.unwrap();
    let files = list.other();
    state.activity.expanded = true;
    let effects = alt(&mut state, 'o');
    assert_eq!(listing(&effects, files).map(|(dir, _)| dir), Some(PathBuf::from(TOOLS)));
    assert_eq!((state.lent, state.active), (Some(files), files), "the files take the keys");
    assert!(state.files_beside_focused());
    assert!(!state.activity.expanded, "a full-width list makes room");
    assert_eq!(state.activity.side, Some(list), "the monitor stays");
    let selected = state.activity.selected;

    press(&mut state, KeyCode::Tab, KeyModifiers::NONE);
    assert_eq!(state.active, list, "Tab gives the list the keys");
    press(&mut state, KeyCode::Down, KeyModifiers::NONE);
    assert_ne!(state.activity.selected, selected, "keys move the list again");
    let effects = alt(&mut state, 'o');
    assert_eq!(listing(&effects, files).map(|(dir, _)| dir), Some(PathBuf::from("/usr/bin")));
    assert_eq!(state.active, files, "Alt-O on another row moves the files");

    press(&mut state, KeyCode::Tab, KeyModifiers::NONE);
    press(&mut state, KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(
        (state.lent, state.active, state.activity.side),
        (None, list, Some(list)),
        "Esc brings the details back"
    );
}

#[test]
fn keys_go_to_the_files_while_they_have_them_and_the_screen_s_key_gives_the_pane_back() {
    let mut state = monitor(vec![process(7, Some(WORKER)), process(8, Some(WORKER))]);
    let list = state.activity.side.unwrap();
    alt(&mut state, 'o');
    let selected = state.activity.selected;
    press(&mut state, KeyCode::Down, KeyModifiers::NONE);
    assert_eq!(state.activity.selected, selected, "the list stays where it was");
    press(&mut state, KeyCode::Esc, KeyModifiers::NONE);
    assert!(state.lent.is_some(), "Esc in the files leaves them open");

    alt(&mut state, 'm');
    assert_eq!((state.lent, state.active, state.activity.side), (None, list, Some(list)));
    alt(&mut state, 'm');
    assert_eq!(state.activity.side, None, "the key again closes the monitor");
}

#[test]
fn the_pane_returns_when_the_screen_closes_fills_the_width_or_its_details_take_the_keys() {
    let mut state = monitor(vec![process(7, Some(WORKER))]);
    alt(&mut state, 'o');
    press(&mut state, KeyCode::Tab, KeyModifiers::NONE);
    alt(&mut state, 'w');
    assert_eq!(state.lent, None, "a maximized list takes the whole width");

    alt(&mut state, 'o');
    press(&mut state, KeyCode::Tab, KeyModifiers::NONE);
    press(&mut state, KeyCode::Char('f'), KeyModifiers::CONTROL);
    assert!(state.activity.search_form.is_some());
    assert_eq!(state.lent, None, "the memory search shows in the details");

    press(&mut state, KeyCode::Esc, KeyModifiers::NONE);
    alt(&mut state, 'o');
    alt(&mut state, 'n');
    assert!(state.network.is_open());
    assert_eq!(state.lent, None, "another screen opens in place of the monitor");
}

#[test]
fn a_program_deleted_while_it_runs_or_missing_from_its_folder_gets_a_notice() {
    let mut state = monitor(vec![process(7, Some("/tmp/.x/miner (deleted)"))]);
    let files = state.activity.side.unwrap().other();
    let effects = alt(&mut state, 'o');
    assert_eq!(listing(&effects, files).map(|(dir, _)| dir), Some(PathBuf::from("/tmp/.x")));
    assert_eq!(state.notice, Some(tr!(lent_program_deleted, file = "/tmp/.x/miner")));

    let mut state = monitor(vec![process(7, Some(WORKER))]);
    let (_, generation) = listing(&alt(&mut state, 'o'), files).unwrap();
    list(&mut state, files, generation, &["other"]);
    assert_eq!(state.notice, Some(tr!(lent_file_missing, file = WORKER)));

    let mut state = monitor(vec![process(7, Some(WORKER))]);
    let (_, generation) = listing(&alt(&mut state, 'o'), files).unwrap();
    list(&mut state, files, generation, &["worker"]);
    assert_eq!(state.notice, None);
    assert_eq!(state.panel(files).hovered_entry().map(|entry| entry.name.as_str()), Some("worker"));
}

#[test]
fn the_files_beside_a_screen_take_deletes_while_copies_wait_for_it_to_close() {
    let mut state = monitor(vec![process(7, Some(WORKER))]);
    let files = state.activity.side.unwrap().other();
    let (_, generation) = listing(&alt(&mut state, 'o'), files).unwrap();
    list(&mut state, files, generation, &["worker"]);

    press(&mut state, KeyCode::F(8), KeyModifiers::NONE);
    assert!(matches!(
        &state.overlay,
        Some(crate::Overlay::Confirm(confirm)) if matches!(confirm.purpose, crate::ConfirmPurpose::Delete { .. })
    ));
    assert_eq!(state.notice, None);
    press(&mut state, KeyCode::Esc, KeyModifiers::NONE);

    press(&mut state, KeyCode::F(5), KeyModifiers::NONE);
    assert_eq!(state.overlay, None, "the other panel lies under the monitor");
    assert_eq!(state.notice.as_deref(), Some(tr!(activity_close_for_files)));
}

#[test]
fn the_context_menu_beside_a_screen_names_the_file_and_in_the_list_the_row() {
    let mut state = monitor(vec![process(7, Some(WORKER))]);
    let files = state.activity.side.unwrap().other();
    let (_, generation) = listing(&alt(&mut state, 'o'), files).unwrap();
    list(&mut state, files, generation, &["worker"]);

    assert_eq!(context_menu_title(&mut state), "worker");
    let now = state.epoch;
    crate::context_menu::choose_labelled(&mut state, tr!(copy), now);
    assert_eq!(state.overlay, None);
    assert_eq!(state.notice.as_deref(), Some(tr!(activity_close_for_files)));

    press(&mut state, KeyCode::Tab, KeyModifiers::NONE);
    assert_eq!(context_menu_title(&mut state), "worker-7 7");
}

fn context_menu_title(state: &mut AppState) -> String {
    press(state, KeyCode::F(10), KeyModifiers::SHIFT);
    match &state.overlay {
        Some(crate::Overlay::ContextMenu(menu)) => menu.menu.title.clone(),
        other => panic!("no context menu: {other:?}"),
    }
}

#[test]
fn a_row_without_a_file_says_so_and_network_rows_point_to_the_monitor() {
    let mut unknown = monitor(vec![process(7, None)]);
    assert!(alt(&mut unknown, 'o').is_empty());
    assert_eq!((unknown.lent, unknown.notice.as_deref()), (None, Some(tr!(lent_no_file))));

    let mut network = state();
    alt(&mut network, 'n');
    assert!(alt(&mut network, 'o').is_empty());
    assert_eq!((network.lent, network.notice.as_deref()), (None, Some(tr!(lent_network_hint))));
}

#[test]
fn a_container_shows_its_compose_file_or_else_its_project_folder() {
    let container = |dir: Option<&str>, files: &[&str]| borg_docker::Container {
        id: "s1".into(),
        name: "shop-web-1".into(),
        image: "web:latest".into(),
        command: String::new(),
        created: String::new(),
        state: borg_docker::State::Running,
        status: String::new(),
        ports: String::new(),
        networks: String::new(),
        project: Some("shop".into()),
        service: Some("web".into()),
        project_dir: dir.map(str::to_owned),
        config_files: files.iter().map(|file| (*file).to_owned()).collect(),
    };
    let place_of = |container: borg_docker::Container| {
        let mut state = state();
        state.docker.containers = vec![container];
        state.docker.rows = vec![crate::DockerRow::Container(0)];
        place(&state, Screen::Docker)
    };
    let shop = "/srv/shop";
    assert_eq!(place_of(container(Some(shop), &["compose.yaml"])), Ok(Place::File("/srv/shop/compose.yaml".into())));
    assert_eq!(place_of(container(Some(shop), &["/etc/shop.yaml"])), Ok(Place::File("/etc/shop.yaml".into())));
    assert_eq!(place_of(container(Some(shop), &[])), Ok(Place::Folder(shop.into())));
    assert_eq!(place_of(container(None, &[])), Err(tr!(lent_no_compose_folder).to_owned()));
}

/// The open screen's filter as typed so far, and whether it still takes typing.
fn typed_filter(state: &AppState) -> (String, bool) {
    match open_screen(state).map(|(screen, _)| screen) {
        Some(Screen::Activity) => (state.activity.filter.clone(), state.activity.filtering),
        Some(Screen::Docker) => (state.docker.filter.clone(), state.docker.filtering),
        Some(Screen::Network) => (state.network.filter.clone(), state.network.filtering),
        Some(Screen::Startup) => (state.startup_items.filter.clone(), state.startup_items.filtering),
        Some(Screen::Alerts) => (state.alerts.filter.clone(), state.alerts.filtering),
        None => (String::new(), false),
    }
}

fn click(state: &mut AppState, pointer: crate::Pointer) -> Vec<Effect> {
    let now = state.epoch;
    update(state, Msg::Pointer(pointer), now)
}

fn type_filter(state: &mut AppState, text: &str) {
    for c in std::iter::once('/').chain(text.chars()) {
        press(state, KeyCode::Char(c), KeyModifiers::NONE);
    }
}

#[test]
fn a_click_on_a_screen_s_filter_while_it_is_typed_types_nothing_into_it() {
    use crate::Pointer;
    let on_filter = KeyCode::Char('/');
    let screens = [
        ('m', Pointer::ActivityControl { preview: false, code: on_filter }),
        ('c', Pointer::DockerControl { details: false, code: on_filter }),
        ('n', Pointer::NetworkControl { details: false, code: on_filter }),
        ('u', Pointer::StartupControl(on_filter)),
        ('l', Pointer::AlertsControl(on_filter)),
    ];
    for (key, pointer) in screens {
        let mut state = state();
        alt(&mut state, key);
        type_filter(&mut state, "wo");
        assert_eq!(typed_filter(&state), ("wo".into(), true), "Alt-{key} opens a screen with a filter");
        click(&mut state, pointer);
        assert_eq!(typed_filter(&state), ("wo".into(), true), "a click on the filter of Alt-{key} types nothing");
    }
}

#[test]
fn a_click_on_a_tab_or_a_column_title_while_a_filter_is_typed_acts_and_keeps_the_filter() {
    use crate::{ActivitySort, Pointer};
    let mut activity = monitor(vec![process(7, Some(WORKER)), process(8, Some(WORKER))]);
    type_filter(&mut activity, "wo");
    click(&mut activity, Pointer::ActivitySort(ActivitySort::Pid));
    assert_eq!(typed_filter(&activity), ("wo".into(), false), "the click keeps the filter as Enter does");
    assert_eq!(activity.activity.sort, ActivitySort::Pid);

    let mut docker = state();
    alt(&mut docker, 'c');
    type_filter(&mut docker, "wo");
    click(&mut docker, Pointer::DockerControl { details: false, code: KeyCode::Char('2') });
    assert_eq!(typed_filter(&docker), ("wo".into(), false), "a tab's click types nothing");
}

#[test]
fn a_double_click_on_a_process_while_its_filter_is_typed_opens_it() {
    use crate::Pointer;
    let mut state = monitor(vec![process(7, Some(WORKER)), process(8, Some(WORKER))]);
    type_filter(&mut state, "worker-8");
    click(&mut state, Pointer::ActivityRow { index: 0, open: true, preview: false });
    assert_eq!(typed_filter(&state), ("worker-8".into(), false));
    assert_eq!(state.activity.list, ActivityPage::Regions, "the process window opens");
}
