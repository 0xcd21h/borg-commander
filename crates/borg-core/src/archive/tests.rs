use std::{
    path::{Path, PathBuf},
    time::Instant,
};

use borg_i18n::tr;
use borg_jobs::{Ask, Decision, Event, Outcome};
use borg_term::{Foreground, PromptState, ShellSignal, TermMode};
use borg_vfs::{Entry, EntryKind};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::*;
use crate::{
    Action, Elevation, Installer, JobCommand, Msg, PackageManager, PanelSetup, Seek, Settings, ShellView, Startup,
    ThemeChoice, Tool, update,
};

const HOME: &str = "/home";
const OTHER: &str = "/tmp";
const ARCHIVE: &str = "/home/site.zip";
const SECRET: &str = "s3cr3t-words";
const COLS: u16 = 100;
const ROWS: u16 = 30;

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

fn file(name: &str) -> Entry {
    entry(name, EntryKind::File)
}

fn folder(name: &str) -> Entry {
    entry(name, EntryKind::Dir)
}

/// Fills the listings `effects` asked for with `entries`.
fn list(state: &mut AppState, effects: &[Effect], entries: &[Entry], now: Instant) {
    for effect in effects {
        let Effect::List { side, generation, .. } = effect else { continue };
        let (side, generation) = (*side, *generation);
        update(state, Msg::Batch { side, generation, entries: entries.to_vec() }, now);
        update(state, Msg::Listed { side, generation, error: None }, now);
    }
}

fn listing(effects: &[Effect]) -> Option<(Side, u64, &Path)> {
    effects.iter().find_map(|effect| match effect {
        Effect::List { side, generation, dir } => Some((*side, *generation, dir.as_path())),
        _ => None,
    })
}

fn setup() -> (AppState, Instant) {
    let now = Instant::now();
    let themes = vec![ThemeChoice { id: "midnight".into(), name: "midnight".into() }];
    let startup = Startup {
        panels: [PanelSetup::at(PathBuf::from(HOME)), PanelSetup::at(PathBuf::from(OTHER))],
        active: Side::Left,
        split: crate::EVEN_SPLIT,
    };
    let settings = Settings {
        preferences: crate::Preferences { git_plugin: false, ..crate::Preferences::DEFAULT },
        ..Settings::default()
    };
    let (mut state, effects) = AppState::new(startup, COLS, ROWS, settings, crate::Appearance::new(themes, 0), now);
    for effect in &effects {
        let Effect::List { side, .. } = effect else { continue };
        let entries = if *side == Side::Left { vec![file("site.zip"), file("notes.txt")] } else { Vec::new() };
        list(&mut state, std::slice::from_ref(effect), &entries, now);
    }
    let view = ShellView {
        prompt: PromptState::Empty,
        command_rows: 1,
        foreground: Foreground::Shell,
        mode: TermMode::empty(),
        raw_input: false,
        line: borg_term::TypedLine::default(),
    };
    update(&mut state, Msg::Observe(view), now);
    update(&mut state, Msg::Signal(ShellSignal::PromptStart), now);
    state.settle();
    (state, now)
}

fn hover(state: &mut AppState, name: &str) {
    let side = state.active;
    assert!(state.panel_mut(side).seek(Seek::Here, |entry| entry.name == name), "{name} is listed");
}

fn press(state: &mut AppState, code: KeyCode, modifiers: KeyModifiers, now: Instant) -> Vec<Effect> {
    update(state, Msg::Key(KeyEvent::new(code, modifiers)), now)
}

/// Opens the archive in the left panel and lists `entries` at its top.
fn opened(entries: &[Entry]) -> (AppState, Instant) {
    let (mut state, now) = setup();
    hover(&mut state, "site.zip");
    let effects = crate::actions::perform(&mut state, Action::Open, now);
    let (side, generation, dir) = listing(&effects).expect("the archive is listed");
    assert_eq!((side, dir), (Side::Left, Path::new(ARCHIVE)));
    let format = borg_archive::Format::Zip;
    update(&mut state, Msg::ArchiveOpened { side, generation, format, left_out: 0 }, now);
    list(&mut state, &effects, entries, now);
    (state, now)
}

fn mount(state: &AppState) -> Option<&ArchiveMount> {
    state.panel(Side::Left).archive.as_ref()
}

#[test]
fn enter_opens_an_archive_as_a_folder_and_dot_dot_returns_to_it() {
    let (mut state, now) = opened(&[folder("docs"), file("readme.md")]);
    assert_eq!(state.panel(Side::Left).dir(), Path::new(ARCHIVE));
    assert_eq!(mount(&state).map(ArchiveMount::label), Some("zip"));
    assert_eq!(state.panel(Side::Left).shell_folder(), Path::new(HOME), "the shell stays beside the archive");
    assert!(state.panel(Side::Left).foreign());

    hover(&mut state, "docs");
    let effects = crate::actions::perform(&mut state, Action::Open, now);
    assert_eq!(listing(&effects).map(|(_, _, dir)| dir), Some(Path::new("/home/site.zip/docs")));
    list(&mut state, &effects, &[file("guide.md")], now);
    assert_eq!(mount(&state).and_then(|mount| mount.inside(state.panel(Side::Left).dir())), Some("docs".into()));

    let effects = crate::actions::perform(&mut state, Action::Leave, now);
    list(&mut state, &effects, &[folder("docs"), file("readme.md")], now);
    assert_eq!(state.panel(Side::Left).hovered_entry().map(|entry| entry.name.as_str()), Some("docs"));

    let effects = crate::actions::perform(&mut state, Action::Leave, now);
    assert_eq!(listing(&effects).map(|(_, _, dir)| dir), Some(Path::new(HOME)));
    assert!(mount(&state).is_none());
    list(&mut state, &effects, &[file("site.zip"), file("notes.txt")], now);
    assert_eq!(state.panel(Side::Left).hovered_entry().map(|entry| entry.name.as_str()), Some("site.zip"));
}

#[test]
fn ctrl_page_down_enters_an_archive_and_ctrl_page_up_leaves_it() {
    let (mut state, now) = setup();
    hover(&mut state, "site.zip");
    let effects = press(&mut state, KeyCode::PageDown, KeyModifiers::CONTROL, now);
    assert_eq!(listing(&effects).map(|(_, _, dir)| dir), Some(Path::new(ARCHIVE)));
    list(&mut state, &effects, &[file("readme.md")], now);
    let effects = press(&mut state, KeyCode::PageUp, KeyModifiers::CONTROL, now);
    assert_eq!(listing(&effects).map(|(_, _, dir)| dir), Some(Path::new(HOME)));
}

#[test]
fn an_archive_that_does_not_open_leaves_the_panel_where_it_was() {
    let (mut state, now) = setup();
    hover(&mut state, "site.zip");
    let effects = crate::actions::perform(&mut state, Action::Open, now);
    let (side, generation, _) = listing(&effects).expect("the archive is listed");
    update(&mut state, Msg::Listed { side, generation, error: Some("The archive is damaged.".into()) }, now);
    assert_eq!(state.panel(Side::Left).dir(), Path::new(HOME));
    assert!(mount(&state).is_none());
    assert_eq!(state.notice.as_deref(), Some("The archive is damaged."));
}

#[test]
fn an_encrypted_archive_asks_for_its_password_and_never_shows_it() {
    let (mut state, now) = setup();
    hover(&mut state, "site.zip");
    let effects = crate::actions::perform(&mut state, Action::Open, now);
    let (side, generation, _) = listing(&effects).expect("the archive is listed");
    update(&mut state, Msg::ArchiveLocked { side, generation, wrong: false }, now);
    let Some(Overlay::Prompt(prompt)) = &state.overlay else { panic!("a password prompt") };
    assert!(prompt.purpose.masked());
    assert_eq!(prompt.label, tr!(archive_password_for, name = "site.zip"));

    for typed in SECRET.chars() {
        press(&mut state, KeyCode::Char(typed), KeyModifiers::NONE, now);
    }
    let effects = press(&mut state, KeyCode::Enter, KeyModifiers::NONE, now);
    let (_, retry, dir) = listing(&effects).expect("the archive is listed again");
    assert_eq!(dir, Path::new(ARCHIVE));
    assert_eq!(mount(&state).and_then(|mount| mount.password.clone()), Some(Password::new(SECRET)));
    assert!(!format!("{:?}", state.panel(Side::Left).archive).contains(SECRET), "the password never prints");

    update(&mut state, Msg::ArchiveLocked { side, generation: retry, wrong: true }, now);
    let Some(Overlay::Prompt(prompt)) = &state.overlay else { panic!("the prompt asks again") };
    assert_eq!(prompt.label, tr!(archive_password_wrong, name = "site.zip"));
    assert!(press(&mut state, KeyCode::Enter, KeyModifiers::NONE, now).is_empty(), "an empty password waits");
    let effects = press(&mut state, KeyCode::Esc, KeyModifiers::NONE, now);
    assert_eq!(listing(&effects).map(|(_, _, dir)| dir), Some(Path::new(HOME)));
    assert!(mount(&state).is_none());
}

#[test]
fn archives_open_read_only() {
    let (mut state, now) = opened(&[file("readme.md")]);
    hover(&mut state, "readme.md");
    for action in [Action::Delete, Action::Move, Action::Mkdir, Action::Edit, Action::QuickView, Action::Compress] {
        state.notice = None;
        assert!(crate::actions::perform(&mut state, action, now).is_empty(), "{action:?}");
        assert_eq!(state.notice.as_deref(), Some(tr!(archive_read_only)), "{action:?}");
    }
    state.active = Side::Right;
    state.notice = None;
    assert!(crate::actions::perform(&mut state, Action::Copy, now).is_empty(), "nothing copies into an archive");
    assert_eq!(state.notice.as_deref(), Some(tr!(archive_read_only)));
}

#[test]
fn an_archive_inside_an_archive_opens_once_copied_out() {
    let (mut state, now) = opened(&[file("inner.tar.gz")]);
    hover(&mut state, "inner.tar.gz");
    assert!(crate::actions::perform(&mut state, Action::Open, now).is_empty());
    assert_eq!(state.notice.as_deref(), Some(tr!(archive_nested)));
}

#[test]
fn view_unpacks_the_file_for_the_viewer_and_esc_stops_it() {
    let (mut state, now) = opened(&[folder("docs")]);
    hover(&mut state, "docs");
    let effects = crate::actions::perform(&mut state, Action::Open, now);
    list(&mut state, &effects, &[file("guide.md")], now);
    hover(&mut state, "guide.md");

    let shown = PathBuf::from("/home/site.zip/docs/guide.md");
    let effects = crate::actions::perform(&mut state, Action::View, now);
    let [Effect::StartUnpack { id, request }] = &effects[..] else { panic!("an unpack job: {effects:?}") };
    let expected = UnpackRequest {
        archive: ARCHIVE.into(),
        password: None,
        paths: vec!["docs/guide.md".into()],
        base: "docs".into(),
        target: UnpackTarget::Viewer { shown: shown.clone() },
    };
    assert_eq!(request, &expected);
    assert_eq!(state.opening.as_ref(), Some(&shown));
    let id = *id;

    let effects = press(&mut state, KeyCode::Esc, KeyModifiers::NONE, now);
    assert_eq!(effects, vec![Effect::JobControl { id, command: JobCommand::Cancel }]);
    assert!(state.opening.is_none());
    state.notice = None;
    let cancelled = Outcome::Cancelled { done: Default::default() };
    update(&mut state, Msg::Job { id, event: Event::Finished(cancelled) }, now);
    assert!(state.jobs.is_empty());
    assert!(state.notice.is_none(), "nobody waits for it any more");

    let effects = crate::actions::perform(&mut state, Action::View, now);
    let [Effect::StartUnpack { id, .. }] = &effects[..] else { panic!("an unpack job") };
    update(&mut state, Msg::Job { id: *id, event: Event::Finished(Outcome::Failed("broken".into())) }, now);
    assert!(state.opening.is_none());
    assert!(state.notice.as_deref().unwrap_or_default().contains("broken"));
}

#[test]
fn copy_unpacks_the_selection_into_the_other_panel_and_asks_about_bombs() {
    let (mut state, now) = opened(&[folder("docs")]);
    hover(&mut state, "docs");
    let effects = crate::actions::perform(&mut state, Action::Open, now);
    list(&mut state, &effects, &[file("guide.md"), file("faq.md")], now);
    hover(&mut state, "guide.md");

    crate::actions::perform(&mut state, Action::Copy, now);
    let Some(Overlay::Prompt(prompt)) = &state.overlay else { panic!("the copy prompt") };
    assert_eq!(prompt.input.text(), OTHER);
    let effects = press(&mut state, KeyCode::Enter, KeyModifiers::NONE, now);
    let Some(Effect::StartUnpack { id, request }) =
        effects.iter().find(|effect| matches!(effect, Effect::StartUnpack { .. }))
    else {
        panic!("an unpack job: {effects:?}")
    };
    assert_eq!(request.paths, ["docs/guide.md"]);
    assert_eq!(request.base, "docs");
    assert_eq!(request.target, UnpackTarget::Folder(OTHER.into()));
    let id = *id;
    assert_eq!(state.jobs.iter().find(|job| job.id == id).map(|job| job.shown_on), Some(Side::Right));

    let ask = Ask::Expand { archive: ARCHIVE.into(), packed: 1 << 20, unpacked: 1 << 34, files: 3 };
    update(&mut state, Msg::Job { id, event: Event::Asking(ask) }, now);
    assert!(matches!(state.overlay, Some(Overlay::Question(_))));
    let effects = press(&mut state, KeyCode::Char('u'), KeyModifiers::NONE, now);
    assert_eq!(effects, vec![Effect::JobControl { id, command: JobCommand::Answer(Decision::Proceed) }]);
}

#[test]
fn files_copy_out_of_an_archive_only_into_a_folder_on_this_machine() {
    let (mut state, now) = opened(&[file("readme.md")]);
    hover(&mut state, "readme.md");
    state.panel_mut(Side::Right).mount = Some(crate::Mount { id: "c1".into(), name: "web".into(), host: OTHER.into() });
    assert!(crate::actions::perform(&mut state, Action::Copy, now).is_empty());
    assert_eq!(state.notice.as_deref(), Some(tr!(archive_copy_destination)));
}

#[test]
fn extract_unpacks_the_whole_archive_here_or_into_the_other_panel() {
    let (mut state, now) = setup();
    hover(&mut state, "site.zip");
    let everything = |target: &str| UnpackRequest {
        archive: ARCHIVE.into(),
        password: None,
        paths: vec![String::new()],
        base: String::new(),
        target: UnpackTarget::Folder(target.into()),
    };
    for (action, target, side) in
        [(Action::ExtractHere, HOME, Side::Left), (Action::ExtractToOther, OTHER, Side::Right)]
    {
        let effects = crate::actions::perform(&mut state, action, now);
        let Some(Effect::StartUnpack { id, request }) = effects.last() else { panic!("an unpack job: {effects:?}") };
        assert_eq!(request, &everything(target));
        assert_eq!(state.jobs.iter().find(|job| job.id == *id).map(|job| job.shown_on), Some(side));
    }
    hover(&mut state, "notes.txt");
    assert!(crate::actions::perform(&mut state, Action::ExtractHere, now).is_empty(), "only archives unpack");

    hover(&mut state, "site.zip");
    state.panel_mut(Side::Right).open_results(crate::ResultSource::Collection("picked".into()), Vec::new(), 1);
    assert!(crate::actions::perform(&mut state, Action::ExtractToOther, now).is_empty());
    assert_eq!(state.notice.as_deref(), Some(tr!(archive_copy_destination)));
}

#[test]
fn a_rar_archive_without_a_reader_offers_to_install_bsdtar() {
    let (mut state, now) = setup();
    hover(&mut state, "site.zip");
    let effects = crate::actions::perform(&mut state, Action::Open, now);
    let (side, generation, _) = listing(&effects).expect("the archive is listed");
    let installer = Installer { manager: PackageManager::Apt, elevation: Some(Elevation::Sudo) };
    update(&mut state, Msg::ArchiveNeedsTool { side, generation, installer: Some(installer) }, now);
    assert!(matches!(state.overlay, Some(Overlay::Install(offer)) if offer.tool == Tool::Bsdtar));
    assert_eq!(installer.command(Tool::Bsdtar), "sudo apt-get install libarchive-tools");
    assert!(mount(&state).is_none());
    assert_eq!(state.panel(Side::Left).dir(), Path::new(HOME));
}

#[test]
fn the_first_read_shows_its_progress_and_esc_leaves() {
    let (mut state, now) = setup();
    hover(&mut state, "site.zip");
    let effects = crate::actions::perform(&mut state, Action::Open, now);
    let (side, generation, _) = listing(&effects).expect("the archive is listed");
    update(&mut state, Msg::ArchiveReading { side, generation, read: 10, total: 100 }, now);
    assert_eq!(mount(&state).and_then(|mount| mount.reading), Some((10, 100)));
    state.notice = None;
    let effects = press(&mut state, KeyCode::Esc, KeyModifiers::NONE, now);
    assert_eq!(listing(&effects).map(|(_, _, dir)| dir), Some(Path::new(HOME)));
    assert!(mount(&state).is_none());
}
