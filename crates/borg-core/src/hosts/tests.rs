//! The hosts screen: listing, answers, order, the cursor and the figures that worry the user.

use super::*;
use crate::{Action, Appearance, LoadAverage, Msg, Overlay, Pointer, Settings, Severity, Startup, ThemeChoice, update};
use borg_docker::Container;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

const GIB: u64 = 1024 * 1024 * 1024;

fn state() -> (AppState, Instant) {
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
    (state, now)
}

fn press(state: &mut AppState, code: KeyCode, modifiers: KeyModifiers) -> Vec<Effect> {
    update(state, Msg::Key(KeyEvent::new(code, modifiers)), state.epoch)
}

fn key(state: &mut AppState, code: KeyCode) -> Vec<Effect> {
    press(state, code, KeyModifiers::NONE)
}

fn host(name: &str) -> PairedHost {
    PairedHost {
        identity: format!("key-{name}"),
        machine: name.into(),
        relay: "wss://relay.example".into(),
        last_seen: None,
    }
}

fn session(name: &str, id: &str) -> RemoteChoice {
    RemoteChoice {
        identity: format!("key-{name}"),
        machine: name.into(),
        session: Some(id.into()),
        name: id.into(),
        ..Default::default()
    }
}

fn reply(name: &str, sessions: usize, figures: Option<HostFigures>) -> HostHeard {
    HostHeard::Answered(Box::new(HostReply {
        info: RemoteHostInfo { version: Some(env!("CARGO_PKG_VERSION").into()), ..Default::default() },
        sessions: (0..sessions).map(|index| session(name, &format!("s{index}"))).collect(),
        figures,
        seen: SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000),
    }))
}

fn answer(state: &mut AppState, name: &str, heard: HostHeard, now: Instant) {
    update(state, Msg::HostAnswered { identity: format!("key-{name}"), answer: heard }, now);
}

fn watching(effects: &[Effect]) -> Option<bool> {
    effects.iter().find_map(|effect| match effect {
        Effect::WatchHosts(on) => Some(*on),
        _ => None,
    })
}

/// The screen open with these hosts listed, each answering with one session.
fn opened(names: &[&str]) -> (AppState, Instant) {
    let (mut state, now) = state();
    let effects = update(&mut state, Msg::Pointer(Pointer::Action(Action::HostsOverview)), now);
    assert_eq!(watching(&effects), Some(true));
    let hosts = names.iter().map(|name| host(name)).collect();
    update(&mut state, Msg::HostsListed(Ok(hosts)), now);
    for name in names {
        answer(&mut state, name, reply(name, 1, Some(HostFigures::default())), now);
    }
    (state, now)
}

fn names(hosts: &Hosts) -> Vec<&str> {
    (0..hosts.rows.len()).filter_map(|row| hosts.card(row)).map(HostCard::name).collect()
}

#[test]
fn the_menu_opens_the_screen_and_asks_the_hosts_until_it_closes() {
    let (mut state, now) = opened(&["web-01"]);
    assert!(state.hosts.is_open() && state.monitor_open());
    assert!(state.hosts.expanded, "the cards take the whole width");
    let effects = key(&mut state, KeyCode::Esc);
    assert!(!state.hosts.is_open());
    assert_eq!(watching(&effects), Some(false));
    let effects = update(&mut state, Msg::Pointer(Pointer::Action(Action::HostsOverview)), now);
    assert_eq!(watching(&effects), Some(true));
    let effects = update(&mut state, Msg::Pointer(Pointer::Action(Action::HostsOverview)), now);
    assert_eq!(watching(&effects), Some(false), "the entry closes the open screen");
}

#[test]
fn hosts_answering_come_first_by_name_and_the_cursor_stays_on_its_host() {
    let (mut state, now) = opened(&["web-01", "Alpha", "db-01"]);
    assert_eq!(names(&state.hosts), ["Alpha", "db-01", "web-01"]);
    key(&mut state, KeyCode::Right);
    assert_eq!(state.hosts.selected().map(HostCard::name), Some("db-01"));
    answer(&mut state, "Alpha", HostHeard::Offline, now);
    assert_eq!(names(&state.hosts), ["db-01", "web-01", "Alpha"]);
    assert_eq!(state.hosts.selected().map(HostCard::name), Some("db-01"));
    assert_eq!(state.hosts.reachable(), 2, "the cards view lists offline hosts on one line");
    assert_eq!(state.hosts.offline().map(HostCard::name).collect::<Vec<_>>(), ["Alpha"]);
    key(&mut state, KeyCode::Char('v'));
    update(&mut state, Msg::Tick, now);
    assert_eq!(state.hosts.columns(), None);
    assert_eq!(state.hosts.reachable(), 3, "the rows view lists every host");
}

#[test]
fn hosts_no_longer_paired_go_and_new_ones_wait_for_an_answer() {
    let (mut state, now) = opened(&["web-01", "db-01"]);
    update(&mut state, Msg::HostsListed(Ok(vec![host("db-01"), host("ci")])), now);
    assert_eq!(names(&state.hosts), ["ci", "db-01"]);
    assert_eq!(state.hosts.card(0).map(|card| card.reach.clone()), Some(Reach::Waiting));
    assert_eq!(state.hosts.card(1).map(|card| card.reach.clone()), Some(Reach::Answering));
    assert_eq!(state.hosts.online(), 1);
}

#[test]
fn arrows_move_across_and_down_the_cards() {
    let names: Vec<String> = (0..7).map(|index| format!("host-{index}")).collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let (mut state, _) = opened(&names);
    assert_eq!(state.hosts.columns(), Some(3), "120 cells fit three cards");
    key(&mut state, KeyCode::Down);
    assert_eq!(state.hosts.cursor(), 3);
    key(&mut state, KeyCode::Right);
    assert_eq!(state.hosts.cursor(), 4);
    key(&mut state, KeyCode::Down);
    assert_eq!(state.hosts.cursor(), 6, "the last row stops at the last card");
    key(&mut state, KeyCode::Left);
    key(&mut state, KeyCode::Up);
    assert_eq!(state.hosts.cursor(), 2);
    key(&mut state, KeyCode::End);
    assert_eq!(state.hosts.cursor(), 6);
    key(&mut state, KeyCode::Home);
    assert_eq!(state.hosts.cursor(), 0);
}

#[test]
fn narrow_or_short_panes_show_rows() {
    assert_eq!(card_columns(80), 2);
    assert_eq!(card_columns(39), 1);
    assert!(fits_cards(40) && !fits_cards(39));
    assert!(fits_card_rows(CARDS_CHROME + CARD_HEIGHT) && !fits_card_rows(CARDS_CHROME + CARD_HEIGHT - 1));
}

#[test]
fn the_filter_keeps_hosts_by_name_version_or_system_and_esc_clears_it() {
    let (mut state, _) = opened(&["web-01", "web-02", "db-01"]);
    key(&mut state, KeyCode::Char('/'));
    assert!(state.hosts.typing());
    for c in "web !02".chars() {
        key(&mut state, KeyCode::Char(c));
    }
    assert_eq!(names(&state.hosts), ["web-01"]);
    key(&mut state, KeyCode::Esc);
    assert!(state.hosts.is_open() && !state.hosts.typing());
    assert_eq!(names(&state.hosts).len(), 3);
}

#[test]
fn a_quiet_host_keeps_its_figures_and_says_since_when() {
    let (mut state, now) = opened(&["web-01"]);
    let later = now + HOSTS_EVERY;
    answer(&mut state, "web-01", HostHeard::Silent, later);
    answer(&mut state, "web-01", HostHeard::Silent, later + HOSTS_EVERY);
    let card = state.hosts.card(0).unwrap();
    assert_eq!(card.reach, Reach::Silent { since: later });
    assert!(card.figures.is_some());
    assert_eq!(card.stale(now + STALE_AFTER - Duration::from_secs(1)), None);
    assert_eq!(card.stale(now + STALE_AFTER), Some(STALE_AFTER));
}

#[test]
fn enter_opens_the_only_session_and_asks_which_of_several() {
    let (mut state, now) = opened(&["web-01", "db-01"]);
    answer(&mut state, "db-01", reply("db-01", 2, None), now);
    let effects = key(&mut state, KeyCode::Enter);
    assert!(
        matches!(state.overlay, Some(Overlay::Remotes(ref browser)) if browser.from_overview && browser.choices.len() == 2)
    );
    assert!(effects.is_empty());
    state.overlay = None;
    key(&mut state, KeyCode::Right);
    let effects = key(&mut state, KeyCode::Enter);
    assert!(effects.iter().any(|effect| matches!(effect, Effect::MountRemote { machine, .. } if machine == "web-01")));
    assert!(!state.hosts.is_open(), "the host's files take the panel");
}

#[test]
fn a_host_without_sessions_says_so() {
    let (mut state, now) = opened(&["web-01"]);
    answer(&mut state, "web-01", reply("web-01", 0, None), now);
    key(&mut state, KeyCode::Enter);
    assert!(state.notice.as_deref().is_some_and(|notice| notice.contains("web-01")));
    assert!(state.hosts.is_open());
}

#[test]
fn renaming_from_a_card_comes_back_to_the_cards() {
    let (mut state, _) = opened(&["web-01"]);
    key(&mut state, KeyCode::F(2));
    assert!(state.overlay.is_some());
    key(&mut state, KeyCode::Esc);
    assert!(state.overlay.is_none(), "Esc returns to the cards");
    assert!(state.hosts.is_open());
}

#[test]
fn o_in_the_remote_hosts_list_opens_the_overview() {
    let (mut state, _) = state();
    crate::remote::browse(&mut state);
    let effects = key(&mut state, KeyCode::Char('o'));
    assert!(state.overlay.is_none());
    assert!(state.hosts.is_open());
    assert_eq!(watching(&effects), Some(true));
}

fn figures() -> HostFigures {
    HostFigures {
        cores: 4,
        cpu: Some(3_100),
        load: Some(LoadAverage { one: 100, five: 100, fifteen: 100 }),
        uptime: Some(3_600),
        memory: Some(MemoryFigures { used: 8 * GIB, total: 16 * GIB, swap_used: 0, swap_total: 0 }),
        disk: Some(DiskFigures { mount: "/".into(), total: 100 * GIB, available: 60 * GIB }),
        watch: Some(WatchFigures { on: true, ..Default::default() }),
        containers: Some(ContainerFigures { running: 3, total: 3, troubled: 0 }),
    }
}

#[test]
fn a_calm_host_worries_nobody() {
    let calm = figures();
    assert_eq!(calm.load_share(), Some(2_500));
    assert_eq!(calm.worst(), None);
}

#[test]
fn load_worries_once_the_cores_cannot_keep_up() {
    let load = |one| HostFigures { load: Some(LoadAverage { one, five: one, fifteen: one }), ..figures() };
    assert_eq!(load(400).load_concern(), None, "four cores at a load of 4 keep up");
    assert_eq!(load(401).load_concern(), Some(Severity::Warning));
    assert_eq!(load(801).load_concern(), Some(Severity::Alert));
    assert_eq!(load(1_000).waiting(), Some(6));
    let unknown = HostFigures { cores: 0, ..load(1_000) };
    assert_eq!(unknown.load_concern(), None);
}

#[test]
fn memory_and_disk_worry_as_they_fill() {
    let memory = |used| HostFigures {
        memory: Some(MemoryFigures { used, total: 100 * GIB, swap_used: 0, swap_total: 0 }),
        ..figures()
    };
    assert_eq!(memory(89 * GIB).memory_concern(), None);
    assert_eq!(memory(90 * GIB).memory_concern(), Some(Severity::Warning));
    assert_eq!(memory(95 * GIB).memory_concern(), Some(Severity::Alert));
    let disk =
        |total, available| HostFigures { disk: Some(DiskFigures { mount: "/".into(), total, available }), ..figures() };
    assert_eq!(disk(100 * GIB, 11 * GIB).disk_concern(), None);
    assert_eq!(disk(100 * GIB, 10 * GIB).disk_concern(), Some(Severity::Warning));
    assert_eq!(disk(100 * GIB, 5 * GIB).disk_concern(), Some(Severity::Alert));
    assert_eq!(disk(2 * GIB, GIB / 2).disk_concern(), Some(Severity::Alert), "less than 1 GiB left is short");
}

#[test]
fn the_watcher_and_containers_worry_until_someone_looks() {
    let watch = |watch| HostFigures { watch: Some(watch), ..figures() };
    let on = WatchFigures { on: true, ..Default::default() };
    assert_eq!(watch(WatchFigures { unread_notices: 3, ..on }).watch_concern(), None);
    assert_eq!(watch(WatchFigures { unread_warnings: 1, ..on }).watch_concern(), Some(Severity::Warning));
    assert_eq!(watch(WatchFigures { unread_alerts: 1, ..on }).watch_concern(), Some(Severity::Alert));
    assert_eq!(watch(WatchFigures { frozen: 1, ..on }).worst(), Some(Severity::Alert));
    let troubled =
        HostFigures { containers: Some(ContainerFigures { running: 2, total: 3, troubled: 1 }), ..figures() };
    assert_eq!(troubled.worst(), Some(Severity::Warning));
}

#[test]
fn containers_count_those_running_and_those_in_trouble() {
    let container = |state: borg_docker::State, status: &str| Container {
        id: "c1".into(),
        name: "web".into(),
        image: "nginx".into(),
        command: String::new(),
        created: String::new(),
        state,
        status: status.into(),
        ports: String::new(),
        networks: String::new(),
        project: None,
        service: None,
        project_dir: None,
        config_files: Vec::new(),
    };
    let figures = ContainerFigures::of(&[
        container(borg_docker::State::Running, "Up 2 hours (healthy)"),
        container(borg_docker::State::Running, "Up 5 minutes (unhealthy)"),
        container(borg_docker::State::Restarting, "Restarting (1) 3 seconds ago"),
        container(borg_docker::State::Exited, "Exited (0) 3 days ago"),
    ]);
    assert_eq!(figures, ContainerFigures { running: 2, total: 4, troubled: 2 });
}

#[test]
fn the_fullest_filesystem_that_holds_files_counts() {
    assert!(holds_files("ext4", false, GIB));
    assert!(holds_files("apfs", false, GIB));
    assert!(!holds_files("tmpfs", false, GIB));
    assert!(!holds_files("overlay", false, GIB));
    assert!(!holds_files("fuse.sshfs", false, GIB));
    assert!(!holds_files("ext4", true, GIB), "a read-only filesystem fills no further");
    assert!(!holds_files("ext4", false, 0));
    let disk = |mount: &str, total, available| DiskFigures { mount: mount.into(), total, available };
    let fullest = fullest([disk("/", 100, 50), disk("/data", 1_000, 100), disk("/boot", 10, 1)]);
    assert_eq!(fullest.map(|disk| disk.mount), Some("/data".into()), "as full as /boot, and larger");
    assert_eq!(share(1, 3), 3_333);
    assert_eq!(share(5, 0), 0);
}

#[test]
fn versions_compare_by_their_numbers() {
    assert!(older_than("0.9.4", "0.12.0"));
    assert!(older_than("v0.11.9", "0.12.0"));
    assert!(!older_than("0.12.0", "0.12.0"));
    assert!(!older_than("0.12.0-rc1", "0.12.0"));
    assert!(!older_than("1.0.0", "0.12.0"));
}

#[test]
fn figures_left_out_by_a_host_read_as_unknown() {
    let figures: HostFigures = serde_json::from_str(r#"{"cores": 2, "future": true}"#).unwrap();
    assert_eq!(figures, HostFigures { cores: 2, ..Default::default() });
    assert_eq!(figures.worst(), None);
}

#[test]
fn an_agent_reads_the_hosts_as_the_page_lists_them() {
    let now = Instant::now();
    let seen = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    let mut hosts = Hosts::default();
    let paired =
        vec![host("web-02"), host("web-01"), host("old-nas"), PairedHost { last_seen: Some(seen), ..host("db") }];
    hosts.list(paired.clone());
    let busy = HostFigures {
        disk: Some(DiskFigures { mount: "/srv".into(), total: 100 * GIB, available: 8 * GIB }),
        containers: Some(ContainerFigures { running: 2, total: 3, troubled: 1 }),
        ..figures()
    };
    hosts.answer("key-web-01", reply("web-01", 2, Some(busy)), now);
    hosts.answer("key-web-02", HostHeard::Silent, now);
    let older = HostReply {
        info: RemoteHostInfo { version: Some("0.1.0".into()), ..Default::default() },
        sessions: Vec::new(),
        figures: None,
        seen,
    };
    hosts.answer("key-old-nas", HostHeard::Answered(Box::new(older)), now);
    hosts.answer("key-db", HostHeard::Offline, now);

    let reports = host_reports(&hosts, now + Duration::from_secs(3));
    let order: Vec<&str> = reports.iter().map(|report| report.name.as_str()).collect();
    assert_eq!(order, ["old-nas", "web-01", "web-02", "db"], "the page's order: offline hosts last");

    let web = &reports[1];
    assert_eq!(web.target, "remote:web-01");
    assert_eq!(web.reach, ReachReport::Answering);
    assert_eq!(web.level, Some(ConcernLevel::Warning));
    let warning = |figure| ConcernReport { figure, level: ConcernLevel::Warning };
    assert_eq!(web.concerns, vec![warning(Figure::Disk), warning(Figure::Containers)]);
    let figures = web.figures.as_ref().expect("figures");
    assert_eq!(figures.cpu_percent, Some(31.0));
    assert_eq!(figures.load.as_ref().and_then(|load| load.percent_of_cores), Some(25.0));
    assert_eq!(figures.disk.as_ref().map(|disk| disk.percent_used), Some(92.0));
    let targets: Vec<&str> = web.sessions.iter().map(|session| session.target.as_str()).collect();
    assert_eq!(targets, ["remote:web-01/s0", "remote:web-01/s1"], "each session is a target too");

    assert_eq!(reports[2].reach, ReachReport::Silent);
    assert_eq!(reports[2].silent_seconds, Some(3));
    assert!(reports[0].older_borg && reports[0].figures.is_none(), "an older Borg sends no figures");
    assert_eq!(reports[3].reach, ReachReport::Offline);
    assert_eq!(reports[3].last_answered.as_deref(), Some("2023-11-14T22:13:20Z"));

    let json = serde_json::to_value(web).unwrap();
    assert_eq!(json["figures"]["memory"]["percent"], 50.0);
    assert_eq!(json["concerns"][0], serde_json::json!({ "figure": "disk", "level": "warning" }));
    assert_eq!(json["reach"], "answering");
    assert!(json.get("error").is_none() && json.get("silent_seconds").is_none(), "{json}");

    hosts.filter = "web !02".into();
    hosts.list(paired);
    let kept: Vec<String> = host_reports(&hosts, now).into_iter().map(|report| report.name).collect();
    assert_eq!(kept, ["web-01"], "the page's filter narrows them");
}
