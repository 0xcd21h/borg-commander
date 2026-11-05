use std::{
    path::{Path, PathBuf},
    time::Instant,
};

use borg_archive::Format;
use borg_drives::{DriveKind, Remember};
use borg_i18n::tr;
use borg_jobs::{Ask, Decision, Event, Outcome};
use borg_term::{Foreground, PromptState, ShellSignal, TermMode};
use borg_vfs::{Entry, EntryKind};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::*;
use crate::{
    Action, EditorFile, Elevation, FileStamp, Installer, JobCommand, LineInput, Msg, PackageManager, PanelSetup, Seek,
    Settings, ShellView, Startup, ThemeChoice, Tool, update,
};

const HOME: &str = "/home";
const OTHER: &str = "/tmp";
const ARCHIVE: &str = "/home/site.zip";
const PHOTO: &str = "/tmp/photo.jpg";
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
        let entries =
            if *side == Side::Left { vec![file("site.zip"), file("notes.txt")] } else { vec![file("photo.jpg")] };
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
    opened_as(Format::Zip, entries)
}

fn opened_as(format: Format, entries: &[Entry]) -> (AppState, Instant) {
    let (mut state, now) = setup();
    hover(&mut state, "site.zip");
    let effects = crate::actions::perform(&mut state, Action::Open, now);
    let (side, generation, dir) = listing(&effects).expect("the archive is listed");
    assert_eq!((side, dir), (Side::Left, Path::new(ARCHIVE)));
    update(&mut state, Msg::ArchiveOpened { side, generation, format, left_out: 0 }, now);
    list(&mut state, &effects, entries, now);
    (state, now)
}

fn mount(state: &AppState) -> Option<&ArchiveMount> {
    state.panel(Side::Left).archive.as_ref()
}

/// Answers the prompt on screen with `text`.
fn answer(state: &mut AppState, text: &str, now: Instant) -> Vec<Effect> {
    let Some(Overlay::Prompt(prompt)) = &mut state.overlay else { panic!("a prompt") };
    prompt.input = LineInput::new(text);
    press(state, KeyCode::Enter, KeyModifiers::NONE, now)
}

/// The change to the archive `effects` start, with the kind of job it runs in.
fn rewrite_in(state: &AppState, effects: &[Effect]) -> (RewriteRequest, JobKind) {
    let Some((id, request)) = effects.iter().find_map(|effect| match effect {
        Effect::StartRewrite { id, request } => Some((*id, request.clone())),
        _ => None,
    }) else {
        panic!("a change to the archive: {effects:?}")
    };
    let kind = state.jobs.iter().find(|job| job.id == id).map(|job| job.kind).expect("the job runs");
    (request, kind)
}

/// Whether the menu item that asks before long jobs again can be chosen.
fn ask_again(state: &AppState) -> Option<bool> {
    crate::menu::menus(state).iter().flat_map(|menu| &menu.entries).find_map(|entry| match entry {
        crate::MenuEntry::Item(item) if item.action == Action::AskAgain => Some(item.enabled),
        _ => None,
    })
}

fn change_of(change: Change) -> RewriteRequest {
    RewriteRequest { archive: ARCHIVE.into(), password: None, change, originals: Vec::new() }
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
fn dot_dot_leaves_a_folder_of_an_archive_that_went_away() {
    let gone = "No such file or directory (os error 2)";
    let (mut state, now) = opened(&[folder("docs")]);
    hover(&mut state, "docs");
    let effects = crate::actions::perform(&mut state, Action::Open, now);
    list(&mut state, &effects, &[file("guide.md")], now);

    let effects = crate::actions::perform(&mut state, Action::Leave, now);
    let (side, generation, dir) = listing(&effects).expect("the archive's top is listed");
    assert_eq!(dir, Path::new(ARCHIVE));
    let effects = update(&mut state, Msg::Listed { side, generation, error: Some(gone.into()) }, now);
    assert_eq!(listing(&effects).map(|(_, _, dir)| dir), Some(Path::new(HOME)), "the panel leaves the archive");
    assert!(mount(&state).is_none());
    assert_eq!(state.notice.as_deref(), Some(gone));
    list(&mut state, &effects, &[file("notes.txt")], now);
    assert_eq!(state.panel(Side::Left).dir(), Path::new(HOME));
}

#[test]
fn a_folder_an_archive_no_longer_holds_keeps_the_archive_open() {
    let missing = "This archive holds no folder docs.";
    let (mut state, now) = opened(&[folder("docs")]);
    hover(&mut state, "docs");
    let effects = crate::actions::perform(&mut state, Action::Open, now);
    list(&mut state, &effects, &[file("guide.md")], now);

    let effect = state.reload(Side::Left);
    let (side, generation, _) = listing(std::slice::from_ref(&effect)).expect("the folder is listed again");
    let effects = update(&mut state, Msg::Listed { side, generation, error: Some(missing.into()) }, now);
    assert!(listing(&effects).is_none());
    assert_eq!(state.panel(Side::Left).dir(), Path::new("/home/site.zip/docs"));
    assert!(mount(&state).is_some());

    let effects = crate::actions::perform(&mut state, Action::Leave, now);
    list(&mut state, &effects, &[folder("other")], now);
    assert_eq!(state.panel(Side::Left).dir(), Path::new(ARCHIVE));
    assert!(mount(&state).is_some());
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
    assert_eq!(state.pace(now), None, "an empty field keeps still");

    for typed in SECRET.chars() {
        press(&mut state, KeyCode::Char(typed), KeyModifiers::NONE, now);
    }
    let Some(Overlay::Prompt(prompt)) = &state.overlay else { panic!("the prompt stays") };
    assert_eq!(prompt.typed, Some(now));
    assert_eq!(state.pace(now), Some(crate::Pace::Ambient), "the hidden characters glitch");
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
fn delete_and_new_folders_change_the_archive() {
    let (mut state, now) = opened(&[folder("docs"), file("readme.md")]);
    hover(&mut state, "readme.md");
    crate::actions::perform(&mut state, Action::Delete, now);
    assert!(matches!(state.overlay, Some(Overlay::Confirm(_))));
    let effects = press(&mut state, KeyCode::Enter, KeyModifiers::NONE, now);
    let removal = Change::Remove { paths: vec!["readme.md".into()] };
    assert_eq!(rewrite_in(&state, &effects), (change_of(removal), JobKind::Delete));

    crate::actions::perform(&mut state, Action::Mkdir, now);
    let effects = answer(&mut state, "drafts", now);
    let folder = Change::Folder { path: "drafts".into() };
    assert_eq!(rewrite_in(&state, &effects), (change_of(folder), JobKind::ArchiveFolder));
}

#[test]
fn f6_in_an_archive_renames_inside_it_or_moves_entries_out() {
    let (mut state, now) = opened(&[folder("docs"), file("readme.md")]);
    hover(&mut state, "readme.md");
    crate::actions::perform(&mut state, Action::Move, now);
    let effects = answer(&mut state, "docs", now);
    let moved = Change::Move { paths: vec!["readme.md".into()], to: "docs".into() };
    assert_eq!(rewrite_in(&state, &effects), (change_of(moved), JobKind::Move));

    crate::actions::perform(&mut state, Action::Move, now);
    let Some(Overlay::Prompt(prompt)) = &state.overlay else { panic!("the move prompt") };
    assert_eq!(prompt.input.text(), OTHER);
    let effects = press(&mut state, KeyCode::Enter, KeyModifiers::NONE, now);
    let Some(Effect::StartUnpack { request, .. }) =
        effects.iter().find(|effect| matches!(effect, Effect::StartUnpack { .. }))
    else {
        panic!("an unpack job: {effects:?}")
    };
    assert_eq!(request.paths, ["readme.md"]);
    assert_eq!(request.target, UnpackTarget::Folder(OTHER.into()));
    assert!(request.remove, "the entries leave the archive");
}

#[test]
fn f5_and_f6_from_a_folder_put_files_into_the_archive() {
    let (mut state, now) = opened(&[folder("docs")]);
    state.active = Side::Right;
    hover(&mut state, "photo.jpg");
    crate::actions::perform(&mut state, Action::Copy, now);
    let Some(Overlay::Prompt(prompt)) = &state.overlay else { panic!("the copy prompt") };
    assert_eq!(prompt.input.text(), ARCHIVE);
    let effects = press(&mut state, KeyCode::Enter, KeyModifiers::NONE, now);
    let added = || Change::Add { sources: vec![PHOTO.into()], into: String::new() };
    assert_eq!(rewrite_in(&state, &effects), (change_of(added()), JobKind::Pack));

    crate::actions::perform(&mut state, Action::Move, now);
    let effects = answer(&mut state, &format!("{ARCHIVE}/docs"), now);
    let into_docs = Change::Add { sources: vec![PHOTO.into()], into: "docs".into() };
    let moved = RewriteRequest { originals: vec![PHOTO.into()], ..change_of(into_docs) };
    assert_eq!(rewrite_in(&state, &effects), (moved, JobKind::Move));
}

#[test]
fn archives_borg_does_not_write_stay_read_only() {
    let (mut state, now) = opened_as(Format::Rar, &[file("readme.md")]);
    hover(&mut state, "readme.md");
    let read_only = tr!(archive_read_only, format = Format::Rar.label());
    for action in [Action::Delete, Action::Move, Action::Mkdir, Action::Edit] {
        state.notice = None;
        assert!(crate::actions::perform(&mut state, action, now).is_empty(), "{action:?}");
        assert_eq!(state.notice.as_deref(), Some(read_only.as_str()), "{action:?}");
    }
    state.active = Side::Right;
    state.notice = None;
    assert!(crate::actions::perform(&mut state, Action::Copy, now).is_empty(), "nothing copies into a RAR archive");
    assert_eq!(state.notice.as_deref(), Some(read_only.as_str()));
}

#[test]
fn what_an_archive_cannot_do_says_so() {
    let (mut state, now) = opened(&[file("readme.md")]);
    hover(&mut state, "readme.md");
    for action in [Action::QuickView, Action::Compress] {
        state.notice = None;
        assert!(crate::actions::perform(&mut state, action, now).is_empty(), "{action:?}");
        assert_eq!(state.notice.as_deref(), Some(tr!(archive_unsupported)), "{action:?}");
    }
}

#[test]
fn f4_edits_a_file_of_the_archive_and_each_save_writes_it_back() {
    let (mut state, now) = opened(&[file("readme.md")]);
    hover(&mut state, "readme.md");
    let shown = PathBuf::from("/home/site.zip/readme.md");
    let slot = EditorSlot::Screen;
    let effects = crate::actions::perform(&mut state, Action::Edit, now);
    let Some(Effect::StartUnpack { request, .. }) =
        effects.iter().find(|effect| matches!(effect, Effect::StartUnpack { .. }))
    else {
        panic!("an unpack job: {effects:?}")
    };
    assert_eq!(request.target, UnpackTarget::Editor { shown: shown.clone(), slot });

    let scratch = PathBuf::from("/run/borg/archive-view/edit-1/readme.md");
    let text = String::from("hello");
    let stamp = FileStamp { len: text.len() as u64, modified: None, inode: 1 };
    let packed =
        PackedFile { archive: ARCHIVE.into(), password: None, entry: "readme.md".into(), shown: shown.clone() };
    let file = EditorFile { path: scratch.clone(), text, stamp };
    update(&mut state, Msg::ArchiveEditorOpened { slot, packed, result: Ok(file) }, now);
    assert!(state.opening.is_none());
    assert_eq!(state.editor.as_ref().map(crate::Editor::shown_path), Some(shown.as_path()));

    press(&mut state, KeyCode::Char('!'), KeyModifiers::NONE, now);
    let effects = press(&mut state, KeyCode::F(2), KeyModifiers::NONE, now);
    assert!(matches!(&effects[..], [Effect::SaveFile { path, .. }] if *path == scratch), "{effects:?}");
    let effects = update(&mut state, Msg::EditorSaved(slot, Ok(stamp)), now);
    let replaced = Change::Replace { path: "readme.md".into(), source: scratch };
    assert_eq!(rewrite_in(&state, &effects), (change_of(replaced), JobKind::Save));
    let note = state.editor.as_ref().and_then(|editor| editor.note.clone());
    assert_eq!(note, Some(tr!(editor_saved_into_archive, name = "site.zip")));
}

#[test]
fn a_long_job_asks_and_an_always_answer_can_be_undone_from_the_menu() {
    let (mut state, now) = opened(&[file("readme.md")]);
    hover(&mut state, "readme.md");
    crate::actions::perform(&mut state, Action::Delete, now);
    let effects = press(&mut state, KeyCode::Enter, KeyModifiers::NONE, now);
    let Some(id) = effects.iter().find_map(|effect| match effect {
        Effect::StartRewrite { id, .. } => Some(*id),
        _ => None,
    }) else {
        panic!("a change to the archive")
    };
    let ask = Ask::Slow {
        path: ARCHIVE.into(),
        file: true,
        bytes: 1 << 33,
        estimate: std::time::Duration::from_secs(600),
        kind: DriveKind::HardDrive,
    };
    update(&mut state, Msg::Job { id, event: Event::Asking(ask) }, now);
    assert!(matches!(state.overlay, Some(Overlay::Question(_))));
    let effects = press(&mut state, KeyCode::Char('f'), KeyModifiers::NONE, now);
    let always = Decision::Always(Remember::File);
    assert_eq!(effects, vec![Effect::JobControl { id, command: JobCommand::Answer(always) }]);

    assert_eq!(ask_again(&state), Some(false), "nothing to undo yet");
    update(&mut state, Msg::KeptAnswers(1), now);
    assert_eq!(ask_again(&state), Some(true));
    assert_eq!(crate::actions::perform(&mut state, Action::AskAgain, now), vec![Effect::AskAgain]);
    assert_eq!(state.kept_answers, 0);
    assert_eq!(state.notice.as_deref(), Some(tr!(asking_again)));
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
        remove: false,
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
        remove: false,
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
