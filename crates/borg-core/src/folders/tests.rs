//! The hotlist, the recent folders, the picker above the command line and the folders dialog.

use super::*;

fn folders() -> Folders {
    Folders::new(
        vec!["/srv/www".into(), "/home/me/Projects".into()],
        vec!["/var/log".into(), "/home/me/Projects".into(), "/home/me/Downloads".into()],
        Some("/home/me".into()),
    )
}

#[test]
fn the_hotlist_comes_first_and_recent_folders_follow_without_repeats() {
    let choices = folders().choices("", None);
    let shown: Vec<_> = choices.iter().map(|c| (c.path.to_str().unwrap(), c.pinned)).collect();
    assert_eq!(
        shown,
        [("/srv/www", true), ("/home/me/Projects", true), ("/var/log", false), ("/home/me/Downloads", false)]
    );
}

#[test]
fn every_word_must_match_the_folder_as_shown_or_in_full() {
    let folders = folders();
    let paths = |query| folders.choices(query, None).into_iter().map(|c| c.path).collect::<Vec<_>>();
    assert_eq!(paths("~/pro"), [PathBuf::from("/home/me/Projects")]);
    assert_eq!(paths("home down"), [PathBuf::from("/home/me/Downloads")]);
    assert_eq!(paths("LOG"), [PathBuf::from("/var/log")]);
    assert!(paths("nowhere").is_empty());
    assert_eq!(folders.choices("", Some(Path::new("/srv/www"))).len(), 3, "the panel's own folder is left out");
}

#[test]
fn a_folder_named_like_the_last_word_comes_first() {
    let folders = Folders::new(
        vec!["/home/me/src/web".into()],
        vec!["/home/me/notes/src-old".into(), "/home/me/src".into()],
        Some("/home/me".into()),
    );
    let paths: Vec<_> = folders.choices("src", None).into_iter().map(|c| c.path).collect();
    assert_eq!(paths, [PathBuf::from("/home/me/src"), "/home/me/notes/src-old".into(), "/home/me/src/web".into()]);
}

#[test]
fn visits_move_to_the_front_and_stay_within_the_limit() {
    let mut folders = folders();
    assert!(folders.visit(Path::new("/home/me/Downloads")));
    assert!(!folders.visit(Path::new("/home/me/Downloads")));
    assert_eq!(folders.recent()[0], Path::new("/home/me/Downloads"));
    assert_eq!(folders.recent().len(), 3);
    for index in 0..RECENT_LIMIT + 5 {
        folders.visit(&PathBuf::from(format!("/tmp/{index}")));
    }
    assert_eq!(folders.recent().len(), RECENT_LIMIT);
    assert!(!folders.pin(Path::new("/srv/www")), "a folder is pinned once");
}

#[test]
fn a_panel_listed_again_is_no_new_visit() {
    let mut folders = Folders::default();
    assert!(folders.arrive(Side::Left, Path::new("/a")));
    assert!(folders.arrive(Side::Right, Path::new("/b")));
    assert!(!folders.arrive(Side::Left, Path::new("/a")), "reloading the left panel keeps /b the newest");
    assert_eq!(folders.recent(), [PathBuf::from("/b"), "/a".into()]);
    assert!(folders.arrive(Side::Left, Path::new("/c")));
}

#[test]
fn home_shows_as_a_tilde() {
    let folders = folders();
    assert_eq!(folders.display(Path::new("/home/me")), "~");
    assert_eq!(folders.display(Path::new("/home/me/Projects")), "~/Projects");
    assert_eq!(folders.display(Path::new("/home/meadow")), "/home/meadow");
}

#[test]
fn only_a_plain_cd_counts() {
    assert_eq!(cd_argument("cd "), Some(""));
    assert_eq!(cd_argument("cd  ~/Pro"), Some("~/Pro"));
    assert_eq!(cd_argument("cd '~/My Files"), Some("~/My Files"));
    assert_eq!(cd_argument("cd"), None);
    assert_eq!(cd_argument("cdx foo"), None);
    assert_eq!(cd_argument("cd foo && make"), None);
    assert_eq!(cd_argument("cd $(git rev-parse --show-toplevel)"), None);
    assert!(plain_cd("cd"));
    assert!(plain_cd("cd -"));
    assert!(plain_cd("cd $HOME/src "));
    assert!(!plain_cd("cd src; ls"));
    assert!(!plain_cd("cd src\nls"));
    assert!(!plain_cd("echo cd"));
}

mod flows {
    use super::*;
    use crate::{Appearance, Msg, PanelSetup, Settings, ShellView, Startup, ThemeChoice, state::REVEAL_DELAY, update};
    use borg_term::{Foreground, PromptState, ShellSignal};
    use std::time::Instant;

    const HOME: &str = "/home/me";
    const PROJECTS: &str = "/home/me/Projects";

    fn setup() -> (AppState, Instant) {
        let now = Instant::now();
        let mut settings = Settings::default();
        settings.preferences.git_plugin = false;
        let startup = Startup {
            panels: [PanelSetup::at(HOME.into()), PanelSetup::at("/tmp".into())],
            active: Side::Left,
            split: crate::EVEN_SPLIT,
        };
        let theme = ThemeChoice { id: "midnight".into(), name: "Midnight".into() };
        let (mut state, effects) = AppState::new(startup, 120, 32, settings, Appearance::new(vec![theme], 0), now);
        state.folders = Folders::new(vec![PROJECTS.into()], vec!["/var/log".into()], Some(HOME.into()));
        for effect in effects {
            list(&mut state, effect, now);
        }
        update(&mut state, Msg::Signal(ShellSignal::PromptStart), now);
        echo(&mut state, "", now);
        (state, now)
    }

    fn list(state: &mut AppState, effect: Effect, now: Instant) -> Vec<Effect> {
        let Effect::List { side, generation, .. } = effect else { return Vec::new() };
        update(state, Msg::Batch { side, generation, entries: Vec::new() }, now);
        update(state, Msg::Listed { side, generation, error: None }, now)
    }

    fn echo(state: &mut AppState, line: &str, now: Instant) -> Vec<Effect> {
        let prompt = if line.is_empty() { PromptState::Empty } else { PromptState::Typed };
        let line = TypedLine { text: line.into(), cursor: line.len() };
        let view = ShellView { prompt, foreground: Foreground::Shell, line, ..ShellView::default() };
        update(state, Msg::ShellOutput(view), now)
    }

    fn type_line(state: &mut AppState, line: &str, now: Instant) -> Vec<Effect> {
        for c in line.chars() {
            update(state, Msg::Key(KeyEvent::from(KeyCode::Char(c))), now);
        }
        echo(state, line, now)
    }

    fn press(state: &mut AppState, code: KeyCode, now: Instant) -> Vec<Effect> {
        update(state, Msg::Key(KeyEvent::from(code)), now)
    }

    fn listed_dir(effects: &[Effect]) -> Option<&Path> {
        effects.iter().find_map(|effect| match effect {
            Effect::List { side: Side::Left, dir, .. } => Some(dir.as_path()),
            _ => None,
        })
    }

    #[test]
    fn a_typed_cd_keeps_the_panels_and_the_panel_follows_the_shell() {
        let (mut state, now) = setup();
        type_line(&mut state, "cd /srv", now);
        press(&mut state, KeyCode::Enter, now);
        assert_eq!(state.view, View::Panels, "the console does not flash up for a cd");
        let effects =
            update(&mut state, Msg::Signal(ShellSignal::Cwd { host: String::new(), path: "/srv".into() }), now);
        assert_eq!(listed_dir(&effects), Some(Path::new("/srv")));
        update(&mut state, Msg::Signal(ShellSignal::CommandEnd { exit: Some(0) }), now);
        assert_eq!((state.view, state.notice.as_deref()), (View::Panels, None));

        type_line(&mut state, "cd /nowhere", now);
        press(&mut state, KeyCode::Enter, now);
        update(&mut state, Msg::Signal(ShellSignal::CommandEnd { exit: Some(1) }), now);
        assert_eq!(state.notice.as_deref(), Some("cd failed (exit 1) · Ctrl-O shows the output"));
    }

    #[test]
    fn a_cd_that_takes_a_while_shows_the_shell() {
        let (mut state, now) = setup();
        type_line(&mut state, "cd /mnt/slow", now);
        press(&mut state, KeyCode::Enter, now);
        update(&mut state, Msg::Tick, now + REVEAL_DELAY);
        assert_eq!(state.view, View::Shell);
    }

    #[test]
    fn other_commands_and_lines_not_echoed_yet_show_the_shell() {
        let (mut state, now) = setup();
        type_line(&mut state, "cd src && make", now);
        press(&mut state, KeyCode::Enter, now);
        assert_eq!(state.view, View::Shell);

        let (mut state, now) = setup();
        type_line(&mut state, "cd src", now);
        update(&mut state, Msg::Key(KeyEvent::from(KeyCode::Char(';'))), now);
        press(&mut state, KeyCode::Enter, now);
        assert_eq!(state.view, View::Shell, "the shell has not shown what was typed last");
    }

    #[test]
    fn the_picker_shows_matches_and_up_then_enter_opens_one() {
        let (mut state, now) = setup();
        type_line(&mut state, "cd", now);
        assert!(cd_choices(&state).is_none(), "the picker waits for the space after cd");
        type_line(&mut state, "cd pro", now);
        let choices = cd_choices(&state).unwrap();
        assert_eq!(choices.choices, [FolderChoice { path: PROJECTS.into(), pinned: true }]);

        assert!(press(&mut state, KeyCode::Up, now).is_empty());
        assert_eq!(state.cd_picker.selected, Some(0));
        assert_eq!(state.active_panel().cursor(), 0, "the panel's cursor stays");
        let effects = press(&mut state, KeyCode::Enter, now);
        assert_eq!(effects[0], Effect::Write(CLEAR_LINE.to_vec()));
        assert_eq!(listed_dir(&effects), Some(Path::new(PROJECTS)));
        assert!(cd_choices(&state).is_none());
        assert!(state.shell.observed.line.text.is_empty(), "the picker hides at once");
    }

    #[test]
    fn down_from_the_bottom_row_and_esc_give_the_keys_back() {
        let (mut state, now) = setup();
        type_line(&mut state, "cd ", now);
        assert_eq!(cd_choices(&state).map(|picker| picker.choices.len()), Some(3), "all but the panel's own folder");
        press(&mut state, KeyCode::Up, now);
        press(&mut state, KeyCode::Up, now);
        assert_eq!(state.cd_picker.selected, Some(1));
        press(&mut state, KeyCode::Down, now);
        press(&mut state, KeyCode::Down, now);
        assert_eq!(state.cd_picker.selected, None);
        let effects = press(&mut state, KeyCode::Enter, now);
        assert_eq!(state.operation.as_ref().map(|op| op.label), Some(CD), "Enter runs the typed cd");
        assert!(effects.iter().any(|effect| matches!(effect, Effect::Write(bytes) if bytes == b"\r")));

        let (mut state, now) = setup();
        type_line(&mut state, "cd v", now);
        press(&mut state, KeyCode::Esc, now);
        assert!(cd_choices(&state).is_none());
        type_line(&mut state, "cd va", now);
        assert!(cd_choices(&state).is_none(), "Esc hides it for this cd");
        type_line(&mut state, "", now);
        type_line(&mut state, "cd va", now);
        assert!(cd_choices(&state).is_some(), "the next cd shows it again");
    }

    #[test]
    fn ctrl_backslash_opens_the_dialog_where_insert_pins_the_panels_folder() {
        let (mut state, now) = setup();
        let ctrl_backslash = KeyEvent::new(KeyCode::Char('4'), KeyModifiers::CONTROL);
        update(&mut state, Msg::Key(ctrl_backslash), now);
        let Some(Overlay::Folders(dialog)) = &state.overlay else { panic!("the folders dialog") };
        let rows = dialog_choices(&state, dialog.filter.text());
        assert_eq!(rows[dialog.selected].path, Path::new(HOME), "it opens on the panel's folder");

        let effects = press(&mut state, KeyCode::Insert, now);
        assert_eq!(effects, [Effect::SetHotlist(vec![PROJECTS.into(), HOME.into()])]);
        assert!(press(&mut state, KeyCode::Insert, now).is_empty(), "a pinned folder stays pinned");

        type_line(&mut state, "log", now);
        let Some(Overlay::Folders(dialog)) = &state.overlay else { panic!("still open") };
        assert_eq!(dialog.filter.text(), "log");
        let effects = press(&mut state, KeyCode::Enter, now);
        assert!(state.overlay.is_none());
        assert_eq!(listed_dir(&effects), Some(Path::new("/var/log")));
    }

    #[test]
    fn f8_takes_folders_off_the_lists_and_esc_clears_the_filter_first() {
        let (mut state, now) = setup();
        crate::actions::perform(&mut state, crate::Action::Folders, now);
        press(&mut state, KeyCode::Home, now);
        assert_eq!(press(&mut state, KeyCode::F(8), now), [Effect::SetHotlist(Vec::new())]);
        press(&mut state, KeyCode::End, now);
        assert_eq!(press(&mut state, KeyCode::Delete, now), [Effect::SaveFolders]);
        assert_eq!(state.folders.recent(), [PathBuf::from("/tmp"), HOME.into()], "/var/log is forgotten");

        type_line(&mut state, "x", now);
        press(&mut state, KeyCode::Esc, now);
        assert!(matches!(&state.overlay, Some(Overlay::Folders(dialog)) if dialog.filter.text().is_empty()));
        press(&mut state, KeyCode::Esc, now);
        assert!(state.overlay.is_none());
    }

    #[test]
    fn opening_folders_remembers_them_while_history_is_on() {
        let (mut state, now) = setup();
        assert_eq!(state.folders.recent()[..2], [PathBuf::from("/tmp"), HOME.into()], "both panels arrived");
        let effect = state.navigate(Side::Left, "/opt".into(), None);
        assert_eq!(list(&mut state, effect, now), [Effect::SaveFolders]);
        assert_eq!(state.folders.recent()[0], Path::new("/opt"));
        let effect = state.reload(Side::Left);
        assert!(list(&mut state, effect, now).is_empty(), "a reload is no visit");

        state.settings.folder_history = false;
        let effect = state.navigate(Side::Left, "/etc".into(), None);
        assert!(list(&mut state, effect, now).is_empty());
        assert_eq!(state.folders.recent()[0], Path::new("/opt"));
    }

    fn read(state: &mut AppState, dir: &str, names: &[&str], now: Instant) -> Vec<Effect> {
        let names = names.iter().map(|name| (*name).to_owned()).collect();
        update(state, Msg::Subfolders { dir: dir.into(), names }, now)
    }

    fn shown(state: &AppState) -> Vec<PathBuf> {
        cd_choices(state)
            .map(|picker| picker.choices.into_iter().map(|choice| choice.path).collect())
            .unwrap_or_default()
    }

    #[test]
    fn a_cd_to_a_typed_folder_lists_its_subfolders_before_the_recent_ones() {
        let (mut state, now) = setup();
        assert_eq!(type_line(&mut state, "cd /Volumes/", now), [Effect::ReadSubfolders("/Volumes".into())]);
        assert!(echo(&mut state, "cd /Volumes/", now).is_empty(), "a folder is asked for once");
        assert!(read(&mut state, "/Volumes", &["Macintosh HD", "Backup"], now).is_empty());
        assert_eq!(shown(&state), [PathBuf::from("/Volumes/Backup"), "/Volumes/Macintosh HD".into()]);
        assert!(!cd_choices(&state).unwrap().completes, "in a cd, picking opens the folder");

        type_line(&mut state, "cd /Volumes/b", now);
        assert_eq!(shown(&state), [PathBuf::from("/Volumes/Backup")], "typing narrows them without reading again");
        press(&mut state, KeyCode::Up, now);
        let effects = press(&mut state, KeyCode::Enter, now);
        assert_eq!(listed_dir(&effects), Some(Path::new("/Volumes/Backup")));
    }

    #[test]
    fn a_folder_typed_without_its_slash_lists_what_is_inside() {
        let (mut state, now) = setup();
        assert_eq!(type_line(&mut state, "cd /Volumes", now), [Effect::ReadSubfolders("/".into())]);
        assert_eq!(read(&mut state, "/", &["Volumes", "usr"], now), [Effect::ReadSubfolders("/Volumes".into())]);
        read(&mut state, "/Volumes", &["Data"], now);
        assert_eq!(shown(&state), [PathBuf::from("/Volumes/Data")]);

        type_line(&mut state, "cd ~/Pro", now);
        assert!(read(&mut state, "/Volumes", &["Late"], now).is_empty(), "an answer nobody waits for is dropped");
        read(&mut state, HOME, &["Projects", "Pictures"], now);
        assert_eq!(shown(&state), [PathBuf::from(PROJECTS)], "the hotlist does not repeat it");
        assert!(cd_choices(&state).unwrap().choices[0].pinned);
    }

    #[test]
    fn a_path_in_any_command_offers_its_subfolders_and_picking_types_one() {
        let (mut state, now) = setup();
        assert!(type_line(&mut state, "git commit -m fix", now).is_empty(), "no path, no picker");
        assert!(cd_choices(&state).is_none());

        assert_eq!(type_line(&mut state, "cp notes.txt /Volumes/Ma", now), [Effect::ReadSubfolders("/Volumes".into())]);
        read(&mut state, "/Volumes", &["Macintosh HD", "Data"], now);
        let picker = cd_choices(&state).unwrap();
        assert_eq!(picker.choices.len(), 1);
        assert!(picker.completes, "in another command, picking types the folder");

        press(&mut state, KeyCode::Up, now);
        let mut typed = vec![0x7f; 2];
        typed.extend(b"Macintosh\\ HD/");
        assert_eq!(press(&mut state, KeyCode::Enter, now), [Effect::Write(typed)]);
        assert_eq!(state.cd_picker.selected, None);
        assert_eq!(state.active_panel().dir(), Path::new(HOME), "the panel stays");
        assert_eq!(
            echo(&mut state, "cp notes.txt /Volumes/Macintosh\\ HD/", now),
            [Effect::ReadSubfolders("/Volumes/Macintosh HD".into())]
        );
    }

    #[test]
    fn esc_hides_the_picker_until_the_next_path() {
        let (mut state, now) = setup();
        type_line(&mut state, "ls /Volumes/", now);
        read(&mut state, "/Volumes", &["Data"], now);
        press(&mut state, KeyCode::Esc, now);
        assert!(cd_choices(&state).is_none());
        type_line(&mut state, "ls /Volumes/D", now);
        assert!(cd_choices(&state).is_none(), "still hidden for this path");
        type_line(&mut state, "ls /Volumes/D ", now);
        type_line(&mut state, "ls /Volumes/D /Volumes/", now);
        read(&mut state, "/Volumes", &["Data"], now);
        assert_eq!(shown(&state), [PathBuf::from("/Volumes/Data")]);
    }
}
