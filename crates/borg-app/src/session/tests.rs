use super::*;
use std::os::unix::ffi::OsStringExt;

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
fn update_checks_are_due_once_a_day() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let state = StateDir::at(dir.path().join("state"));
    let day = Duration::from_secs(86_400);
    let now = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
    assert!(state.update_check_due(now, day));
    state.record_update_check(now).unwrap_or_else(|e| panic!("{e}"));
    assert!(!state.update_check_due(now + day / 2, day));
    assert!(state.update_check_due(now + day, day));
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
