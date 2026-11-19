use std::time::Instant;

use borg_crypt::{age, sealed::Seal};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use zeroize::Zeroizing;

use super::*;
use crate::{Appearance, EditorFile, FileStamp, MenuCursor, Msg, Settings, Startup, ThemeChoice, update};

const PATH: &str = "/work/notes.md.age";
const NAME: &str = "notes.md.age";
const SECRET: &str = "hunter two";
const FAST_LOG_N: u8 = 1;

fn state() -> (AppState, Instant) {
    let now = Instant::now();
    let themes = vec![ThemeChoice { id: "midnight".into(), name: "Midnight".into() }];
    let (state, _) =
        AppState::new(Startup::at("/work".into()), 80, 12, Settings::default(), Appearance::new(themes, 0), now);
    (state, now)
}

fn sealed(then: Then) -> SealedFile {
    SealedFile { path: PATH.into(), format: Format::Age, then }
}

fn viewing(state: &mut AppState) -> SealedFile {
    state.opening = Some(PATH.into());
    sealed(Then::View)
}

fn press(state: &mut AppState, now: Instant, code: KeyCode) -> Vec<Effect> {
    update(state, Msg::Key(KeyEvent::new(code, KeyModifiers::NONE)), now)
}

fn type_text(state: &mut AppState, now: Instant, text: &str) {
    for typed in text.chars() {
        press(state, now, KeyCode::Char(typed));
    }
}

fn prompt_label(state: &AppState) -> &str {
    let Some(Overlay::Prompt(prompt)) = &state.overlay else { panic!("a password prompt") };
    assert!(prompt.purpose.masked());
    &prompt.label
}

fn seal() -> Seal {
    let work = age::WorkFactor::new(FAST_LOG_N).expect("a valid work factor");
    let fresh = age::Fresh {
        file_key: Zeroizing::new([1; age::FILE_KEY_LEN]),
        salt: [2; age::SALT_LEN],
        nonce: [3; age::NONCE_LEN],
    };
    let file = age::encrypt_text(b"notes", SECRET.as_bytes(), work, &fresh).expect("encrypt");
    borg_crypt::sealed::unseal(&file, SECRET, work).expect("open").1
}

fn edited(state: &mut AppState, now: Instant, text: &str) {
    let stamp = FileStamp { len: 1, modified: None, inode: 7 };
    let file = EditorFile { path: PATH.into(), text: text.into(), stamp };
    update(state, Msg::SealedEditorOpened { slot: EditorSlot::Screen, file, seal: seal() }, now);
}

#[test]
fn the_viewer_asks_for_the_password_and_asks_again_when_it_is_wrong() {
    let (mut state, now) = state();
    let file = viewing(&mut state);
    update(&mut state, Msg::Sealed(file.clone()), now);
    assert_eq!(prompt_label(&state), tr!(sealed_password_for, name = NAME, format = AGE_NAME));
    assert!(press(&mut state, now, KeyCode::Enter).is_empty(), "an empty password waits");

    type_text(&mut state, now, SECRET);
    let effects = press(&mut state, now, KeyCode::Enter);
    assert_eq!(effects, vec![Effect::Unseal { file: file.clone(), password: Password::new(SECRET) }]);
    assert!(!format!("{effects:?}").contains(SECRET), "the password never prints");
    assert!(state.overlay.is_none());
    assert_eq!(state.notice.as_deref(), Some(tr!(sealed_opening, name = NAME).as_str()));

    update(&mut state, Msg::UnsealFailed { file, error: UnsealError::Sealed(Error::WrongPassword) }, now);
    assert_eq!(prompt_label(&state), tr!(sealed_password_wrong, name = NAME));
    press(&mut state, now, KeyCode::Esc);
    assert!(state.overlay.is_none());
    assert_eq!(state.opening, None, "Esc stops opening the file");
}

#[test]
fn a_failure_says_why_and_stops_opening() {
    let (mut state, now) = state();
    let file = viewing(&mut state);
    update(&mut state, Msg::UnsealFailed { file, error: UnsealError::Sealed(Error::Damaged) }, now);
    assert_eq!(state.notice.as_deref(), Some(tr!(sealed_damaged, name = NAME).as_str()));
    assert_eq!(state.opening, None);
    assert!(state.overlay.is_none());
}

#[test]
fn every_failure_has_its_own_words() {
    let errors = [
        UnsealError::Sealed(Error::WrongPassword),
        UnsealError::Sealed(Error::Damaged),
        UnsealError::Sealed(Error::Unsupported),
        UnsealError::Sealed(Error::NotForPassword),
        UnsealError::Sealed(Error::TooLong),
        UnsealError::Sealed(Error::NeedsMemory),
        UnsealError::NotText,
        UnsealError::Failed("disk on fire".into()),
    ];
    let texts: Vec<String> = errors.iter().map(|error| describe(error, PATH.as_ref())).collect();
    assert!(texts.iter().all(|text| text.contains(NAME)), "{texts:?}");
    let distinct: std::collections::BTreeSet<&String> = texts.iter().collect();
    assert_eq!(distinct.len(), texts.len());
}

#[test]
fn a_password_request_for_a_file_no_longer_opening_is_dropped() {
    let (mut state, now) = state();
    state.opening = Some("/work/other.txt".into());
    update(&mut state, Msg::Sealed(sealed(Then::View)), now);
    assert!(state.overlay.is_none());
    assert_eq!(state.opening, Some("/work/other.txt".into()));
}

#[test]
fn an_open_dialog_is_not_replaced_by_the_prompt() {
    let (mut state, now) = state();
    let file = viewing(&mut state);
    state.overlay = Some(Overlay::Menu(MenuCursor { menu: 0, item: None }));
    update(&mut state, Msg::Sealed(file), now);
    assert!(matches!(state.overlay, Some(Overlay::Menu(_))));
    assert_eq!(state.notice.as_deref(), Some(tr!(sealed_needs_password, name = NAME).as_str()));
    assert_eq!(state.opening, None);
}

#[test]
fn the_editor_saves_through_the_seal_it_opened_with() {
    let (mut state, now) = state();
    edited(&mut state, now, "notes\n");
    type_text(&mut state, now, "new ");
    let effects = update(&mut state, Msg::Key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)), now);
    let [Effect::SaveFile { bytes, seal: Some(used), .. }] = &effects[..] else { panic!("{effects:?}") };
    assert_eq!(bytes, b"new notes\n");
    assert_eq!(used.format(), Format::Age);
    let stamp = FileStamp { len: 2, modified: None, inode: 7 };
    update(&mut state, Msg::EditorSaved(EditorSlot::Screen, Ok(stamp)), now);
    let note = state.editor.as_ref().and_then(|editor| editor.note.clone());
    assert_eq!(note, Some(tr!(editor_saved_encrypted, format = AGE_NAME)));
}

#[test]
fn encrypted_text_over_the_limit_is_not_saved() {
    let (mut state, now) = state();
    edited(&mut state, now, &"a".repeat(TEXT_LIMIT));
    type_text(&mut state, now, "b");
    let effects = update(&mut state, Msg::Key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)), now);
    assert!(effects.is_empty(), "{effects:?}");
    let note = state.editor.as_ref().and_then(|editor| editor.note.clone());
    assert_eq!(note, Some(tr!(editor_sealed_too_long, kib = TEXT_LIMIT / BYTES_PER_KIB)));
    assert!(state.editor.as_ref().is_some_and(|editor| editor.buffer.modified()));
}

#[test]
fn an_edit_prompt_left_with_esc_opens_nothing() {
    let (mut state, now) = state();
    update(&mut state, Msg::Sealed(sealed(Then::Edit(EditorSlot::Screen))), now);
    type_text(&mut state, now, SECRET);
    assert!(press(&mut state, now, KeyCode::Esc).is_empty());
    assert!(state.overlay.is_none() && state.editor.is_none());
}
