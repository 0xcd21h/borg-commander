use super::*;
use crate::{Msg, Settings, Startup, update};
use std::{
    collections::HashSet,
    path::Path,
    time::{Duration, Instant, UNIX_EPOCH},
};

const VERSION: &str = "1.2.0";
const OLD: Duration = Duration::from_secs(60 * 60);
const RUNNING: u32 = 100;
const ENDED: u32 = 200;

fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(30 * 24 * 60 * 60)
}

fn found(name: &str, age: Duration, kind: FoundKind) -> Found {
    Found { name: name.into(), kind, modified: now() - age, bytes: 10 }
}

fn file(name: &str, age: Duration) -> Found {
    found(name, age, FoundKind::File)
}

fn folder(name: &str, age: Duration, inside: Vec<Found>) -> Found {
    found(name, age, FoundKind::Folder(inside))
}

fn socket(name: &str, age: Duration, answers: bool) -> Found {
    found(name, age, FoundKind::Socket { answers })
}

fn runs(pid: u32) -> bool {
    pid == RUNNING
}

/// The watcher's own files are the user's record of what happened on the machine, so no rule
/// may take them away, however old they are.
#[test]
fn the_alert_log_and_the_baseline_are_never_left_over() {
    let root = Path::new("/state");
    let entries = [file(ALERT_LOG, STALE * 2), file(ALERT_LOG_ASIDE, STALE * 2), file(WATCH_BASELINE, STALE * 2)];
    assert!(runtime_leftovers(root, &entries, &judge(false)).is_empty());
    assert!(runtime_leftovers(root, &entries, &judge(true)).is_empty());
    let lists = CacheLists { sizes: None, indexes: &HashSet::new() };
    assert!(cache_leftovers(root, &entries, &lists, &judge(false)).is_empty());
}

#[test]
fn the_outbox_and_an_outbox_borg_could_not_read_are_never_left_over() {
    let root = Path::new("/state");
    let entries = [file(WATCH_OUTBOX, STALE * 2), file(WATCH_OUTBOX_ASIDE, STALE * 2)];
    assert!(runtime_leftovers(root, &entries, &judge(false)).is_empty());
    assert!(runtime_leftovers(root, &entries, &judge(true)).is_empty());
    let lists = CacheLists { sizes: None, indexes: &HashSet::new() };
    assert!(cache_leftovers(root, &entries, &lists, &judge(false)).is_empty());
}

fn judge(idle: bool) -> Judge<'static> {
    Judge { now: now(), version: VERSION, idle, runs: &runs }
}

fn names(root: &Path, leftovers: &[Leftover]) -> Vec<String> {
    leftovers.iter().map(|leftover| leftover.path.strip_prefix(root).unwrap().display().to_string()).collect()
}

#[test]
fn this_version_keeps_its_scripts_and_loses_folder_requests_of_ended_processes() {
    let root = Path::new("/run/borg");
    let entries = [
        file(TTL_FILE, OLD),
        file(&format!("TTL.{ENDED}"), OLD),
        file(&format!("TTL.{RUNNING}"), OLD),
        file("notes.txt", STALE * 2),
        folder(
            VERSION,
            Duration::ZERO,
            vec![
                file("integration.zsh", Duration::ZERO),
                file(&format!("cd-{RUNNING}"), OLD),
                file(&format!("cd-{ENDED}"), OLD),
                file(&format!("cd-{ENDED}.new"), OLD),
                file(&format!("cd-{ENDED}x"), OLD),
            ],
        ),
    ];
    let leftovers = runtime_leftovers(root, &entries, &judge(false));
    assert_eq!(
        names(root, &leftovers),
        [format!("TTL.{ENDED}"), format!("1.2.0/cd-{ENDED}"), format!("1.2.0/cd-{ENDED}.new")]
    );
}

#[test]
fn other_versions_go_once_stale_or_idle_unless_a_running_borg_asks_for_folders_there() {
    let root = Path::new("/run/borg");
    let stale = folder("1.1.0", STALE, vec![file("integration.zsh", STALE)]);
    let recent = folder("1.1.5", OLD, vec![file("integration.zsh", OLD), file(&format!("cd-{ENDED}"), OLD)]);
    let used = folder("1.0.0", STALE, vec![file(&format!("cd-{RUNNING}"), STALE)]);
    let entries = [stale, recent, used];
    assert_eq!(names(root, &runtime_leftovers(root, &entries, &judge(false))), ["1.1.0", "1.1.5/cd-200"]);
    assert_eq!(names(root, &runtime_leftovers(root, &entries, &judge(true))), ["1.1.0", "1.1.5"]);
}

#[test]
fn nothing_younger_than_the_settle_time_is_left_over() {
    let root = Path::new("/run/borg");
    let young = SETTLE - Duration::from_secs(1);
    let entries = [
        folder("1.1.0", young, vec![file("integration.zsh", young)]),
        file(&format!("TTL.{ENDED}"), young),
        folder(&format!("{CONSOLE_PREFIX}{ENDED}-abc"), young, Vec::new()),
        folder(VERSION, young, vec![file(&format!("cd-{ENDED}"), young)]),
    ];
    assert!(runtime_leftovers(root, &entries, &judge(true)).is_empty());
    let sockets = folder("borg-501-default", young, vec![socket("session-a-1.sock", young, false)]);
    assert!(socket_leftovers(Path::new("/tmp/borg-501-default"), &sockets, false, &judge(true)).is_empty());
}

#[test]
fn consoles_go_when_their_process_ends_and_old_ones_without_a_process_after_three_days() {
    let root = Path::new("/run/borg");
    let entries = [
        folder(&format!("{CONSOLE_PREFIX}{RUNNING}-abc"), STALE, Vec::new()),
        folder(&format!("{CONSOLE_PREFIX}{ENDED}-abc"), OLD, Vec::new()),
        folder(&format!("{CONSOLE_PREFIX}4xYz9"), OLD, Vec::new()),
        folder(&format!("{CONSOLE_PREFIX}Qw3rty"), STALE, Vec::new()),
    ];
    assert_eq!(names(root, &runtime_leftovers(root, &entries, &judge(false))), ["console-200-abc", "console-Qw3rty"]);
}

#[test]
fn a_capture_proxys_certificates_go_when_its_borg_ends() {
    let root = Path::new("/run/borg");
    let bundle = |pid: u32, rest: &str| file(&format!("{TRUST_PREFIX}{pid}{rest}"), OLD);
    let entries = [
        bundle(RUNNING, "-1.pem"),
        bundle(ENDED, "-1.pem"),
        bundle(ENDED, "-2.txt"),
        bundle(ENDED, ".pem"),
        file(&format!("{TRUST_PREFIX}{ENDED}-1.pem"), SETTLE - Duration::from_secs(1)),
    ];
    assert_eq!(names(root, &runtime_leftovers(root, &entries, &judge(false))), ["trust-200-1.pem"]);
}

#[test]
fn copies_out_of_containers_and_remote_previews_go_after_a_day() {
    let root = Path::new("/run/borg");
    let entries = [
        folder(
            DOCKER_SCRATCH,
            OLD,
            vec![folder("1", SCRATCH_AGE, vec![file("app.log", SCRATCH_AGE)]), folder("2", OLD, Vec::new())],
        ),
        folder(REMOTE_PREVIEWS, OLD, vec![file("view-a.txt", SCRATCH_AGE), file("view-b.txt", OLD)]),
    ];
    assert_eq!(
        names(root, &runtime_leftovers(root, &entries, &judge(false))),
        ["docker/1", "remote-preview/view-a.txt"]
    );
}

#[test]
fn a_runtime_folder_others_can_enter_goes_once_nothing_in_it_changed_for_three_days() {
    let path = Path::new("/tmp/borg-commander-501");
    let judge = judge(false);
    assert!(abandoned_leftover(path, &folder("borg-commander-501", OLD, Vec::new()), &judge).is_none());
    let leftover = abandoned_leftover(path, &folder("borg-commander-501", STALE, Vec::new()), &judge).unwrap();
    assert_eq!(leftover.path, path);
}

#[test]
fn sockets_nobody_answers_on_go_and_with_them_their_folder_unless_this_borg_uses_it() {
    let path = Path::new("/tmp/borg-501-default");
    let dead = folder(
        "borg-501-default",
        OLD,
        vec![socket("session-a-1.sock", OLD, false), socket("mirror-b.sock", OLD, false)],
    );
    assert_eq!(names(Path::new("/tmp"), &socket_leftovers(path, &dead, false, &judge(false))), ["borg-501-default"]);
    assert_eq!(
        names(path, &socket_leftovers(path, &dead, true, &judge(false))),
        ["session-a-1.sock", "mirror-b.sock"],
        "the folder this Borg uses stays",
    );
    let mixed = folder(
        "borg-501-default",
        OLD,
        vec![socket("session-a-1.sock", OLD, true), socket("session-b-2.sock", OLD, false), file("notes", OLD)],
    );
    assert_eq!(names(path, &socket_leftovers(path, &mixed, false, &judge(false))), ["session-b-2.sock"]);
}

#[test]
fn the_cache_loses_old_formats_unlisted_and_ended_scans_and_indexes_of_folders_no_longer_indexed() {
    let root = Path::new("/cache/borg");
    let listed: HashSet<String> = ["sizes-aa.sqlite".to_owned()].into();
    let indexed: HashSet<String> = ["11.idx".to_owned()].into();
    let entries = [
        file(SIZES_INDEX, OLD),
        file("sizes-aa.sqlite", OLD),
        file("sizes-aa.sqlite-journal", OLD),
        file("sizes-bb.sqlite", OLD),
        file("sizes-bb.sqlite-wal", OLD),
        file("sizes-cc.json", OLD),
        file(&format!("{SCAN_PREFIX}{ENDED}-3-dd.sqlite"), OLD),
        file(&format!("{SCAN_PREFIX}{RUNNING}-3-dd.sqlite"), OLD),
        file("sizes-index.part", OLD),
        file("pictures.db", OLD),
        folder(
            INDEX_DIR,
            OLD,
            vec![file("11.idx", OLD), file("22.idx", OLD), file("22.part", OLD), file("readme", OLD)],
        ),
    ];
    let lists = CacheLists { sizes: Some(&listed), indexes: &indexed };
    assert_eq!(
        names(root, &cache_leftovers(root, &entries, &lists, &judge(false))),
        [
            "sizes-bb.sqlite",
            "sizes-bb.sqlite-wal",
            "sizes-cc.json",
            ".scan-200-3-dd.sqlite",
            "sizes-index.part",
            "index/22.idx",
            "index/22.part",
        ],
    );
}

#[test]
fn an_unreadable_list_of_size_scans_keeps_every_size_scan() {
    let root = Path::new("/cache/borg");
    let entries = [file("sizes-aa.sqlite", OLD), file("sizes-cc.json", OLD)];
    let lists = CacheLists { sizes: None, indexes: &HashSet::new() };
    assert_eq!(names(root, &cache_leftovers(root, &entries, &lists, &judge(false))), ["sizes-cc.json"]);
}

fn state() -> AppState {
    let themes = vec![crate::ThemeChoice { id: "midnight".into(), name: "midnight".into() }];
    let appearance = crate::Appearance::new(themes, 0);
    let (state, _) =
        AppState::new(Startup::at("/work".into()), 100, 30, Settings::default(), appearance, Instant::now());
    state
}

fn key(state: &mut AppState, code: KeyCode) -> Vec<Effect> {
    update(state, Msg::Key(KeyEvent::from(code)), Instant::now())
}

fn report() -> StorageReport {
    let place =
        |place, path: Option<&str>, items| PlaceUsage { place, path: path.map(PathBuf::from), bytes: 100, items };
    StorageReport {
        places: vec![
            place(Place::Temporary, Some("/run/borg"), 3),
            place(Place::Sockets, Some("/tmp/borg-501-default"), 1),
            place(Place::Caches, Some("/cache/borg"), 4),
            place(Place::State, Some("/state/borg"), 9),
            place(Place::Settings, Some("/config/borg"), 1),
            place(Place::Captures, None, 1),
        ],
        captures: vec![SavedCapture { path: "/work/net/capture-1.pcap".into(), bytes: 2048, saved: now(), keys: None }],
        leftovers: vec![
            Leftover { path: "/run/borg/1.1.0".into(), bytes: 30 },
            Leftover { path: "/cache/borg/sizes-cc.json".into(), bytes: 70 },
        ],
        cleaned: None,
    }
}

fn opened(state: &mut AppState) -> Vec<Effect> {
    let effects = crate::actions::perform(state, crate::Action::Storage, Instant::now());
    update(state, Msg::Storage(report()), Instant::now());
    effects
}

fn listed(effects: &[Effect]) -> Option<&PathBuf> {
    effects.iter().find_map(|effect| match effect {
        Effect::List { dir, .. } => Some(dir),
        _ => None,
    })
}

fn entry(name: &str) -> borg_vfs::Entry {
    borg_vfs::Entry {
        source: None,
        missing: false,
        usage: None,
        name: name.into(),
        kind: borg_vfs::EntryKind::File,
        size: 0,
        modified: None,
        executable: false,
        permissions: 0o644,
    }
}

fn arrive(state: &mut AppState, effects: Vec<Effect>, names: &[&str]) {
    let Some(Effect::List { side, generation, .. }) = effects.into_iter().find(|e| matches!(e, Effect::List { .. }))
    else {
        panic!("a folder opens")
    };
    let entries = names.iter().map(|name| entry(name)).collect();
    update(state, Msg::Batch { side, generation, entries }, Instant::now());
    update(state, Msg::Listed { side, generation, error: None }, Instant::now());
}

#[test]
fn the_window_measures_when_it_opens_and_enter_opens_a_place_or_the_folder_of_a_capture() {
    let mut state = state();
    let effects = opened(&mut state);
    assert!(effects.iter().any(|effect| matches!(effect, Effect::ScanStorage)));
    let Some(Overlay::Storage(window)) = &state.overlay else { panic!("the window is open") };
    assert_eq!(window.rows().len(), 7, "six places and one capture");

    key(&mut state, KeyCode::Down);
    key(&mut state, KeyCode::Down);
    let effects = key(&mut state, KeyCode::Enter);
    assert_eq!(listed(&effects), Some(&PathBuf::from("/cache/borg")));
    assert!(state.overlay.is_none());

    opened(&mut state);
    key(&mut state, KeyCode::End);
    let Some(Overlay::Storage(window)) = &state.overlay else { panic!("the window is open") };
    assert!(matches!(window.chosen(), Some(StorageRow::Capture(_))));
    let effects = key(&mut state, KeyCode::Enter);
    assert_eq!(listed(&effects), Some(&PathBuf::from("/work/net")));
    arrive(&mut state, effects, &["a.txt", "capture-1.pcap", "z.txt"]);
    assert_eq!(
        state.active_panel().hovered_entry().map(|entry| entry.name.as_str()),
        Some("capture-1.pcap"),
        "the cursor lands on the capture",
    );
}

#[test]
fn enter_on_the_captures_place_moves_to_the_first_capture() {
    let mut state = state();
    opened(&mut state);
    for _ in 0..5 {
        key(&mut state, KeyCode::Down);
    }
    let effects = key(&mut state, KeyCode::Enter);
    assert!(listed(&effects).is_none());
    let Some(Overlay::Storage(window)) = &state.overlay else { panic!("the window stays open") };
    assert!(matches!(window.chosen(), Some(StorageRow::Capture(_))));
}

#[test]
fn f8_asks_before_removing_the_leftovers_and_yes_removes_them_and_measures_again() {
    let mut state = state();
    opened(&mut state);
    key(&mut state, KeyCode::F(8));
    let Some(Overlay::Confirm(confirm)) = &state.overlay else { panic!("F8 asks first") };
    assert!(matches!(confirm.purpose, ConfirmPurpose::CleanStorage { .. }));

    key(&mut state, KeyCode::Esc);
    assert!(matches!(state.overlay, Some(Overlay::Storage(_))), "No goes back to the window");

    key(&mut state, KeyCode::F(8));
    let effects = key(&mut state, KeyCode::Enter);
    let removed: Vec<&PathBuf> = effects
        .iter()
        .find_map(|effect| match effect {
            Effect::CleanStorage(paths) => Some(paths.iter().collect()),
            _ => None,
        })
        .expect("Yes removes the leftovers");
    assert_eq!(removed, [Path::new("/run/borg/1.1.0"), Path::new("/cache/borg/sizes-cc.json")]);
    let Some(Overlay::Storage(window)) = &state.overlay else { panic!("the window waits for the new report") };
    assert!(window.report.is_none());
}

#[test]
fn f8_with_nothing_left_over_says_so() {
    let mut state = state();
    crate::actions::perform(&mut state, crate::Action::Storage, Instant::now());
    update(&mut state, Msg::Storage(StorageReport { leftovers: Vec::new(), ..report() }), Instant::now());
    key(&mut state, KeyCode::F(8));
    assert!(matches!(state.overlay, Some(Overlay::Storage(_))));
    assert_eq!(state.notice.as_deref(), Some(tr!(storage_no_leftovers)));
}
