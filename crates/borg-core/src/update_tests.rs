use std::{
    path::{Path, PathBuf},
    time::Instant,
};

use borg_term::{Foreground, PromptState, ShellSignal, TermMode};
use borg_vfs::{Entry, EntryKind};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{
    Action, AppState, Effect, ListingMode, Msg, Overlay, PanelSetup, Settings, ShellView, Side, SortKey, Startup,
    ThemeChoice, View, menus, state::REVEAL_DELAY, update,
};

const COLS: u16 = 80;
const ROWS: u16 = 24;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

fn shift(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::SHIFT)
}

fn entry(name: &str, kind: EntryKind) -> Entry {
    Entry { name: name.into(), kind, size: 0, modified: None, executable: false, permissions: 0o644 }
}

fn dir(name: &str) -> Entry {
    entry(name, EntryKind::Dir)
}

fn file(name: &str) -> Entry {
    entry(name, EntryKind::File)
}

fn idle_prompt() -> ShellView {
    ShellView { prompt: PromptState::Empty, foreground: Foreground::Shell, mode: TermMode::empty() }
}

fn quiet() -> Settings {
    Settings { preferences: crate::Preferences { splash: false, ..crate::Preferences::DEFAULT }, ..Settings::default() }
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
            Effect::Write(bytes) => Some(bytes.clone()),
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
    assert_eq!(state.view, View::Panels);
}

#[test]
fn enter_on_typed_line_runs_command_and_reveals_shell() {
    let (mut state, now) = setup();
    state.shell.observed.prompt = PromptState::Typed;
    let effects = press(&mut state, now, key(KeyCode::Enter));
    assert_eq!(writes(&effects), "\r");
    assert_eq!(state.view, View::Shell);
    assert_eq!(state.pace(now), Some(crate::Pace::Smooth));
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
fn copy_prompts_for_destination_and_runs_in_shell() {
    let (mut state, now) = setup_with(vec![file("a"), file("b c")]);
    press(&mut state, now, key(KeyCode::Down));
    press(&mut state, now, key(KeyCode::Insert));
    press(&mut state, now, key(KeyCode::Insert));
    press(&mut state, now, key(KeyCode::F(5)));
    let Some(Overlay::Prompt(prompt)) = &state.overlay else { panic!("expected prompt") };
    assert_eq!(prompt.input.text(), "/tmp");
    assert_eq!(prompt.label, "Copy 2 items to");

    let effects = press(&mut state, now, key(KeyCode::Enter));
    assert_eq!(writes(&effects), " cp -R -i './a' './b c' '/tmp'\r");
    assert!(state.shell.running);
    assert_eq!(state.view, View::Panels);
}

#[test]
fn slow_operations_reveal_the_shell_and_return_afterwards() {
    let (mut state, now) = setup_with(vec![file("a")]);
    press(&mut state, now, key(KeyCode::Down));
    press(&mut state, now, key(KeyCode::F(5)));
    press(&mut state, now, key(KeyCode::Enter));
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
    press(&mut state, now, key(KeyCode::Down));
    press(&mut state, now, key(KeyCode::F(8)));
    press(&mut state, now, key(KeyCode::Enter));
    update(&mut state, Msg::Signal(ShellSignal::CommandEnd { exit: Some(1) }), now);
    assert_eq!(state.notice.as_deref(), Some("delete failed (exit 1) · Ctrl-O shows the output"));
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
    assert_eq!(writes(&effects), " rm -r -f './a'\r");
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

#[test]
fn operations_wait_for_an_idle_shell() {
    let (mut state, now) = setup_with(vec![file("a")]);
    state.shell.running = true;
    press(&mut state, now, key(KeyCode::Down));
    press(&mut state, now, key(KeyCode::F(8)));
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
    let options = menus(&state).iter().position(|m| m.title == "Options");
    assert!(matches!(state.overlay, Some(Overlay::Menu(cursor)) if Some(cursor.menu) == options.map(|o| o + 1)));
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
fn split_nudges_animate_and_clamp() {
    let (mut state, now) = setup();
    press(&mut state, now, KeyEvent::new(KeyCode::Right, KeyModifiers::ALT | KeyModifiers::SHIFT));
    assert!(state.split_target() > crate::EVEN_SPLIT);
    assert_eq!(state.pace(now), Some(crate::Pace::Smooth));

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

#[test]
fn enter_on_regular_file_opens_viewer() {
    let (mut state, now) = setup_with(vec![file("log.txt")]);
    press(&mut state, now, key(KeyCode::Down));
    let effects = press(&mut state, now, key(KeyCode::Enter));
    assert_eq!(effects, vec![Effect::View(PathBuf::from("/home/log.txt"))]);
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

#[test]
fn viewer_keys_scroll_wrap_and_close() {
    use std::sync::Arc;

    let (mut state, now) = setup();
    let content: Vec<u8> = (1..=100).flat_map(|i| format!("row {i}\n").into_bytes()).collect();
    let (width, height) = crate::viewer_size(state.cols, state.rows);
    let pager = borg_view::Pager::new(Arc::new(content), width, height).unwrap_or_else(|e| panic!("{e}"));
    update(&mut state, Msg::Opened(crate::Viewer { path: PathBuf::from("/home/log"), pager }), now);

    press(&mut state, now, key(KeyCode::End));
    let last = state.viewer.as_ref().and_then(|v| v.pager.rows().last().cloned());
    assert_eq!(last.as_deref(), Some("row 100"));

    press(&mut state, now, key(KeyCode::Char('w')));
    assert!(state.viewer.as_ref().is_some_and(|v| !v.pager.wrapped()));

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
fn splash_is_skipped_by_a_key_that_does_nothing_else() {
    let (mut state, now) = setup_from(vec![dir("me")], Settings::default());
    assert!(state.splash_active(now));
    let cursor = state.active_panel().cursor();
    assert!(press(&mut state, now, key(KeyCode::Down)).is_empty());
    assert!(!state.splash_active(now));
    assert_eq!(state.active_panel().cursor(), cursor);
    assert_eq!(state.pace(now), None);
}

#[test]
fn splash_ends_on_its_own() {
    let (mut state, now) = setup_from(vec![dir("me")], Settings::default());
    assert_eq!(state.wake_at(), Some(now + crate::SPLASH_DURATION));
    update(&mut state, Msg::Tick, now + crate::SPLASH_DURATION);
    assert!(state.splash_until.is_none());
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
fn delight_off_keeps_about_plain() {
    let mut settings = quiet();
    settings.preferences.delight = false;
    let (mut state, now) = setup_from(vec![dir("me")], settings);
    state.appearance = state.appearance.clone().with_secret(Some(secret()), Vec::new());
    press(&mut state, now, key(KeyCode::F(1)));
    assert!(chars(&mut state, now, crate::UNLOCK_WORD).is_empty());
    assert!(state.appearance.has_secret());
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
    assert!(press(&mut state, due, key(KeyCode::Down)).is_empty());
    assert!(!state.screensaver);
    assert_eq!(state.active_panel().cursor(), 0);
}

#[test]
fn entering_a_child_folder_slides_forward_and_back() {
    let (mut state, now) = setup();
    press(&mut state, now, key(KeyCode::Down));
    let effects = press(&mut state, now, key(KeyCode::Enter));
    let Some(Effect::List { side, generation, .. }) = effects.first().cloned() else { panic!("expected listing") };
    update(&mut state, Msg::Batch { side, generation, entries: vec![] }, now);
    let slide = state.motions[side.index()].slide(now);
    assert_eq!(slide.map(|(direction, _)| direction), Some(crate::SlideDirection::Forward));
    assert_eq!(state.pace(now), Some(crate::Pace::Smooth));
}

#[test]
fn closed_overlays_fade_out_and_new_ones_fade_in() {
    let mut settings = quiet();
    settings.motion = crate::Motion::LOCAL;
    let (mut state, now) = setup_from(vec![dir("me")], settings);
    press(&mut state, now, key(KeyCode::F(1)));
    let opened = now + crate::Motion::LOCAL.fade;
    press(&mut state, opened, key(KeyCode::Esc));
    let leaving = state.leaving.as_ref().map(|leaving| (&leaving.overlay, leaving.fade.value(opened)));
    assert!(matches!(leaving, Some((Overlay::About(_), shown)) if shown > 0.99));
    assert_eq!(state.pace(opened), Some(crate::Pace::Smooth));

    let gone = opened + crate::Motion::LOCAL.fade;
    press(&mut state, gone, key(KeyCode::F(9)));
    assert!(state.leaving.is_none());
    assert!(state.overlay_fade.is_animating(gone));
}
