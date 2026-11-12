use super::*;
use std::os::unix::ffi::OsStringExt;
use std::time::{Duration, UNIX_EPOCH};

#[test]
fn game2048_round_trips_and_rejects_corrupt_or_oversized_saves() {
    let temp = tempfile::tempdir().unwrap();
    let dir = StateDir::at(temp.path().join("state"));
    assert!(dir.load_game2048().unwrap().is_none());
    let mut game = borg_core::Game2048::new(42);
    game.slide(borg_core::GameDirection::Left);
    dir.save_game2048(&game).unwrap();
    assert_eq!(dir.load_game2048().unwrap().unwrap().saved(), game.saved());
    let file = dir.path().join(GAME2048_FILE);
    fs::write(&file, b"{invalid").unwrap();
    assert!(dir.load_game2048().is_err());
    fs::write(&file, vec![b' '; MAX_GAME_BYTES as usize + 1]).unwrap();
    assert!(dir.load_game2048().is_err());
    let mut saved = game.saved();
    saved.cells[0] = u8::MAX;
    fs::write(&file, serde_json::to_vec(&saved).unwrap()).unwrap();
    assert!(dir.load_game2048().is_err());
}

#[test]
fn recent_folders_round_trip_privately_and_none_removes_the_file() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let dir = StateDir::at(temp.path().join("state"));
    assert!(dir.load_recent_folders().unwrap().is_empty());
    let odd = PathBuf::from(std::ffi::OsString::from_vec(b"/tmp/\xff name".to_vec()));
    dir.save_recent_folders(&[odd.clone(), "/srv".into()]).unwrap();
    assert_eq!(dir.load_recent_folders().unwrap(), [odd, "/srv".into()]);
    let file = dir.path().join(RECENT_FOLDERS_FILE);
    assert_eq!(fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
    dir.save_recent_folders(&[]).unwrap();
    assert!(!file.exists());
}

#[test]
fn the_remote_log_is_private_to_its_session_and_keeps_its_newest_entries() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let state = StateDir::at(temp.path().join("state"));
    let dir = state.for_session("work");
    assert!(dir.load_agent_log().unwrap().is_empty());
    let ran = "2026-11-09 14:00:00  ran in ~/srv\n$ make\n[exit 0]";
    dir.save_agent_note(ran).unwrap();
    let file = state.workspace("work").join(AGENT_LOG_FILE);
    let mode = |file: &Path| fs::metadata(file).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&file), 0o600);
    fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
    dir.save_agent_note("2026-11-09 14:00:01  read /srv/app").unwrap();
    assert_eq!(mode(&file), 0o600, "a log others could read is made private again");
    assert_eq!(dir.load_agent_log().unwrap(), [ran, "2026-11-09 14:00:01  read /srv/app"]);
    assert!(state.load_agent_log().unwrap().is_empty(), "each session keeps its own log");

    let quarter = "x".repeat(MAX_AGENT_LOG_BYTES as usize / 4);
    for n in 0..5 {
        dir.save_agent_note(&format!("{n} {quarter}")).unwrap();
        assert!(fs::metadata(&file).unwrap().len() <= MAX_AGENT_LOG_BYTES);
    }
    assert_eq!(mode(&file), 0o600, "trimming keeps the log private");
    let kept: Vec<_> = dir.load_agent_log().unwrap().iter().map(|entry| entry[..1].to_owned()).collect();
    assert_eq!(kept, ["3", "4"]);

    dir.forget_agent_log().unwrap();
    assert!(!file.exists());
    dir.forget_agent_log().unwrap();
}

#[test]
fn a_remote_log_past_its_limit_loads_its_newest_whole_entries() {
    let temp = tempfile::tempdir().unwrap();
    let dir = StateDir::at(temp.path().to_path_buf());
    let oldest = agent_log::line(&"x".repeat(MAX_AGENT_LOG_BYTES as usize));
    fs::write(temp.path().join(AGENT_LOG_FILE), oldest + &agent_log::line("newest")).unwrap();
    assert_eq!(dir.load_agent_log().unwrap(), ["newest"]);
}

#[test]
fn the_update_check_keeps_when_it_ran_and_the_skipped_version() {
    let dir = tempfile::tempdir().unwrap();
    let state = StateDir::at(dir.path().join("state"));
    assert_eq!(state.update_check(), UpdateCheck::default(), "no file yet");
    let checked = Some(UNIX_EPOCH + Duration::from_secs(1_800_000_000));
    state.save_update_check(&UpdateCheck { checked, skipped: None }).unwrap();
    crate::update::skip(&state, "0.11.0").unwrap();
    let skipped = Some("0.11.0".parse().unwrap());
    assert_eq!(state.update_check(), UpdateCheck { checked, skipped });
    let text = fs::read_to_string(state.path().join(UPDATE_CHECK_FILE)).unwrap();
    assert_eq!(text, "1800000000\n0.11.0\n");
}

fn saved(left: &Path, right: &Path) -> Session {
    let panel = |dir: &Path, focus: &str| PanelState {
        dir: dir.to_path_buf(),
        focus: Some(focus.to_owned()),
        mode: ModeName::Brief,
        sort: SortName::Size,
        reverse: true,
    };
    Session { active: SideName::Right, split: 0.6, left: panel(left, "a"), right: panel(right, "b") }
}

#[test]
fn active_panel_starts_in_cwd_and_other_panel_is_restored() {
    let other = std::env::temp_dir();
    let cwd = PathBuf::from("/somewhere/else");
    let startup = saved(&other, Path::new("/old")).startup(&cwd);

    assert_eq!(startup.active, Side::Right);
    assert_eq!(startup.panels[1].dir, cwd);
    assert_eq!(startup.panels[1].focus, None);
    assert_eq!(startup.panels[0].dir, other);
    assert_eq!(startup.panels[0].focus.as_deref(), Some("a"));
    assert_eq!(startup.panels[0].mode, ListingMode::Brief);
    assert_eq!(startup.panels[0].sort, Sort { key: SortKey::Size, reverse: true });
    assert!((startup.split - 0.6).abs() < f32::EPSILON);
}

#[test]
fn missing_directories_fall_back_to_cwd() {
    let cwd = PathBuf::from("/cwd");
    let startup = saved(Path::new("/does/not/exist"), Path::new("/cwd")).startup(&cwd);
    assert_eq!(startup.panels[0].dir, cwd);
    assert_eq!(startup.panels[0].focus, None);
}

#[test]
fn session_round_trips_through_toml() {
    let session = saved(Path::new("/a"), Path::new("/b"));
    let text = toml::to_string(&session).unwrap_or_default();
    assert_eq!(toml::from_str::<Session>(&text).ok(), Some(session));
}

#[test]
fn saved_collections_survive_missing_sources_and_non_utf8_names() {
    let temp = tempfile::tempdir().unwrap();
    let dir = StateDir::at(temp.path().into());
    let raw = PathBuf::from(std::ffi::OsString::from_vec(b"/work/raw-\xff".to_vec()));
    let entries = vec![borg_vfs::Entry::unavailable(raw.clone()), borg_vfs::Entry::unavailable("/missing/file".into())];
    dir.save_collections(&[borg_core::Collection { name: "Bundle".into(), entries }]).unwrap();
    let loaded = dir.load_collections().unwrap();
    assert_eq!(loaded[0].name, "Bundle");
    assert_eq!(loaded[0].entries[0].identity(), raw);
    assert!(loaded[0].entries[1].missing);
}
