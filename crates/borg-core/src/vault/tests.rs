use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::*;
use crate::{
    Action, Appearance, Elevation, Installer, MenuCursor, Msg, PackageManager, Settings, Startup, ThemeChoice, update,
};

const HOME: &str = "/work";
const CIPHER: &str = "/work/Papers";
const ROOT: &str = "/run/user/1000/borg-commander/vaults/Papers";
const NAME: &str = "Papers";
const SECRET: &str = "hunter two";

fn state() -> (AppState, Instant) {
    let now = Instant::now();
    let themes = vec![ThemeChoice { id: "midnight".into(), name: "Midnight".into() }];
    let (state, _) =
        AppState::new(Startup::at(HOME.into()), 80, 12, Settings::default(), Appearance::new(themes, 0), now);
    (state, now)
}

fn vault() -> VaultMount {
    VaultMount { cipher: CIPHER.into(), root: ROOT.into() }
}

fn press(state: &mut AppState, now: Instant, code: KeyCode) -> Vec<Effect> {
    update(state, Msg::Key(KeyEvent::new(code, KeyModifiers::NONE)), now)
}

fn type_text(state: &mut AppState, now: Instant, text: &str) -> Vec<Effect> {
    text.chars().for_each(|typed| {
        press(state, now, KeyCode::Char(typed));
    });
    press(state, now, KeyCode::Enter)
}

fn shown_prompt(state: &AppState) -> &Prompt {
    let Some(Overlay::Prompt(prompt)) = &state.overlay else { panic!("a prompt") };
    prompt
}

fn listed(state: &mut AppState, now: Instant, effects: &[Effect]) -> PathBuf {
    let Some(Effect::List { side, generation, dir }) = effects.first() else { panic!("a listing") };
    update(state, Msg::Listed { side: *side, generation: *generation, error: None }, now);
    dir.clone()
}

fn found(state: &mut AppState, now: Instant, open: Option<PathBuf>) -> Vec<Effect> {
    let Effect::List { generation, .. } = state.navigate(Side::Left, CIPHER.into(), None) else { panic!("a listing") };
    update(state, Msg::VaultFound { side: Side::Left, generation, cipher: CIPHER.into(), open }, now)
}

fn inside(state: &mut AppState, now: Instant) {
    let effects = update(state, Msg::VaultOpened { side: Side::Left, vault: vault() }, now);
    listed(state, now, &effects);
}

#[test]
fn a_locked_vault_asks_for_its_password_in_a_hidden_field_and_the_panel_stays() {
    let (mut state, now) = state();
    let effects = found(&mut state, now, None);
    assert!(!effects.iter().any(|effect| matches!(effect, Effect::List { .. } | Effect::OpenVault { .. })));
    let prompt = shown_prompt(&state);
    assert!(prompt.purpose.masked());
    assert!(prompt.label.contains(NAME));
    assert_eq!(state.panel(Side::Left).destination(), Path::new(HOME));
}

#[test]
fn typing_the_password_opens_the_vault_and_wipes_the_field() {
    let (mut state, now) = state();
    found(&mut state, now, None);
    let effects = type_text(&mut state, now, SECRET);
    let [Effect::OpenVault { side: Side::Left, cipher, password }] = effects.as_slice() else { panic!("{effects:?}") };
    assert_eq!(cipher, Path::new(CIPHER));
    assert_eq!(password.as_str(), SECRET);
    assert!(state.overlay.is_none());
}

#[test]
fn esc_leaves_the_vault_closed_and_an_empty_enter_waits() {
    let (mut state, now) = state();
    found(&mut state, now, None);
    assert!(press(&mut state, now, KeyCode::Enter).is_empty());
    assert!(state.overlay.is_some());
    assert!(press(&mut state, now, KeyCode::Esc).is_empty());
    assert!(state.overlay.is_none());
}

#[test]
fn a_wrong_password_asks_again_and_says_so() {
    let (mut state, now) = state();
    update(
        &mut state,
        Msg::VaultFailed { side: Side::Left, cipher: CIPHER.into(), error: VaultError::WrongPassword },
        now,
    );
    assert_eq!(shown_prompt(&state).label, tr!(vault_password_wrong, name = NAME));
}

#[test]
fn a_vault_asking_while_another_dialog_is_open_says_how_to_open_it() {
    let (mut state, now) = state();
    state.overlay = Some(Overlay::Menu(MenuCursor { menu: 0, item: None }));
    found(&mut state, now, None);
    assert!(matches!(state.overlay, Some(Overlay::Menu(_))));
    assert_eq!(state.notice.as_deref(), Some(tr!(vault_needs_password, name = NAME).as_str()));
}

#[test]
fn an_open_vault_shows_its_files_from_its_top_and_dot_dot_returns_to_its_folder() {
    let (mut state, now) = state();
    let effects = found(&mut state, now, Some(ROOT.into()));
    assert_eq!(listed(&mut state, now, &effects), Path::new(ROOT));
    assert!(state.overlay.is_none());
    assert!(at_root(&state, Side::Left));
    assert_eq!(vault().inside(&Path::new(ROOT).join("2024/tax.pdf")), Path::new("/2024/tax.pdf"));
    let back = leave(&mut state, Side::Left).expect("a listing");
    let Effect::List { dir, .. } = back else { panic!("a listing") };
    assert_eq!(dir, Path::new(HOME));
    assert!(state.panel(Side::Left).vault.is_none());
}

#[test]
fn leaving_the_vault_another_way_forgets_it_in_that_panel() {
    let (mut state, now) = state();
    inside(&mut state, now);
    state.navigate(Side::Left, "/tmp".into(), None);
    assert!(state.panel(Side::Left).vault.is_none());
}

#[test]
fn closing_takes_every_panel_in_the_vault_back_to_its_folder() {
    let (mut state, now) = state();
    inside(&mut state, now);
    state.navigate(Side::Right, Path::new(ROOT).join("2024"), None);
    state.active = Side::Left;
    let close = crate::actions::perform(&mut state, Action::CloseVault, now);
    assert_eq!(close, vec![Effect::CloseVault { root: ROOT.into() }]);
    let effects = update(&mut state, Msg::VaultClosed { root: ROOT.into(), error: None }, now);
    let dirs: Vec<(Side, PathBuf)> = effects
        .into_iter()
        .filter_map(|effect| match effect {
            Effect::List { side, dir, .. } => Some((side, dir)),
            _ => None,
        })
        .collect();
    assert_eq!(dirs, vec![(Side::Left, HOME.into()), (Side::Right, "/run/user/1000/borg-commander/vaults".into())]);
    assert_eq!(state.notice.as_deref(), Some(tr!(vault_closed, name = NAME).as_str()));
}

#[test]
fn a_vault_in_use_stays_open_and_says_why() {
    let (mut state, now) = state();
    inside(&mut state, now);
    let effects = update(&mut state, Msg::VaultClosed { root: ROOT.into(), error: Some(VaultError::Busy) }, now);
    assert!(effects.is_empty());
    assert!(state.panel(Side::Left).vault.is_some());
    assert_eq!(state.notice.as_deref(), Some(tr!(vault_busy, name = NAME).as_str()));
}

#[test]
fn closing_outside_a_vault_says_so() {
    let (mut state, now) = state();
    assert!(crate::actions::perform(&mut state, Action::CloseVault, now).is_empty());
    assert_eq!(state.notice.as_deref(), Some(tr!(vault_none)));
}

#[test]
fn creating_asks_for_a_name_and_the_password_twice_until_they_match() {
    let (mut state, now) = state();
    crate::actions::perform(&mut state, Action::CreateVault, now);
    assert!(!shown_prompt(&state).purpose.masked());
    type_text(&mut state, now, NAME);
    assert!(shown_prompt(&state).purpose.masked());
    type_text(&mut state, now, SECRET);
    assert_eq!(shown_prompt(&state).label, tr!(vault_repeat_label));
    type_text(&mut state, now, "hunter three");
    assert_eq!(shown_prompt(&state).label, tr!(vault_passwords_differ, name = NAME));
    type_text(&mut state, now, SECRET);
    let effects = type_text(&mut state, now, SECRET);
    let [Effect::CreateVault { side: Side::Left, cipher, password }] = effects.as_slice() else {
        panic!("{effects:?}")
    };
    assert_eq!(cipher, Path::new(CIPHER));
    assert_eq!(password.as_str(), SECRET);
}

#[test]
fn a_vault_name_is_one_folder_name() {
    for name in ["a/b", "..", "."] {
        let (mut state, now) = state();
        crate::actions::perform(&mut state, Action::CreateVault, now);
        type_text(&mut state, now, name);
        assert!(state.overlay.is_none(), "{name}");
        assert_eq!(state.notice.as_deref(), Some(tr!(vault_bad_name)), "{name}");
    }
}

#[test]
fn the_recovery_key_shows_once_beside_the_new_vault() {
    let (mut state, now) = state();
    let key = Password::new("00010203-04050607");
    let effects = update(&mut state, Msg::VaultCreated { side: Side::Left, vault: vault(), key: key.clone() }, now);
    assert_eq!(listed(&mut state, now, &effects), Path::new(ROOT));
    let Some(Overlay::VaultKey(shown)) = &state.overlay else { panic!("the recovery key") };
    assert_eq!((shown.name.as_str(), &shown.key), (NAME, &key));
    press(&mut state, now, KeyCode::Enter);
    assert!(state.overlay.is_none());
}

#[test]
fn a_missing_gocryptfs_is_offered_for_install_when_borg_knows_how() {
    let (mut state, now) = state();
    let installer = Installer { manager: PackageManager::Apt, elevation: Some(Elevation::Sudo) };
    let error = VaultError::NoTool(Some(installer));
    update(&mut state, Msg::VaultFailed { side: Side::Left, cipher: CIPHER.into(), error }, now);
    assert!(matches!(state.overlay, Some(Overlay::Install(offer)) if offer.tool == Tool::Gocryptfs));
    let (mut state, now) = self::state();
    let error = VaultError::NoTool(None);
    update(&mut state, Msg::VaultFailed { side: Side::Left, cipher: CIPHER.into(), error }, now);
    assert_eq!(state.notice.as_deref(), Some(tr!(vault_no_gocryptfs)));
}

#[test]
fn a_vault_found_by_a_listing_inside_it_marks_the_panel() {
    let (mut state, now) = state();
    let Effect::List { generation, .. } = state.navigate(Side::Left, Path::new(ROOT).join("2024"), None) else {
        panic!("a listing")
    };
    update(&mut state, Msg::VaultShown { side: Side::Left, generation, vault: vault() }, now);
    assert_eq!(state.panel(Side::Left).vault, Some(vault()));
}

#[test]
fn every_vault_password_field_hides_what_is_typed() {
    let purposes = [
        PromptPurpose::VaultPassword { side: Side::Left, cipher: CIPHER.into() },
        PromptPurpose::VaultNewPassword { side: Side::Left, cipher: CIPHER.into() },
        PromptPurpose::VaultRepeat { side: Side::Left, cipher: CIPHER.into(), first: Password::new(SECRET) },
    ];
    assert!(purposes.iter().all(PromptPurpose::masked));
    assert!(!PromptPurpose::VaultName { side: Side::Left }.masked());
}

#[test]
fn the_recovery_key_reads_in_two_lines_of_four_groups() {
    let key = "0a1b2c3d-4e5f6a7b-8c9d0e1f-2a3b4c5d-6e7f8a9b-0c1d2e3f-4a5b6c7d-8e9f0a1b";
    let shown = VaultKey { name: NAME.into(), key: Password::new(key) };
    assert_eq!(shown.lines(), ["0a1b2c3d-4e5f6a7b-8c9d0e1f-2a3b4c5d-", "6e7f8a9b-0c1d2e3f-4a5b6c7d-8e9f0a1b"]);
    assert_eq!(shown.lines().concat(), key);
}

#[test]
fn an_old_gocryptfs_says_creating_needs_a_newer_one() {
    let (mut state, now) = state();
    let error = VaultError::TooOld("2.3".into());
    update(&mut state, Msg::VaultFailed { side: Side::Left, cipher: CIPHER.into(), error }, now);
    assert_eq!(state.notice.as_deref(), Some(tr!(vault_gocryptfs_too_old, version = "2.3").as_str()));
}
