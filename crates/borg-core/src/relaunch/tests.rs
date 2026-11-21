use std::{path::PathBuf, time::Instant};

use borg_jobs::Spec;

use crate::{AppState, Effect, Msg, PanelSetup, Settings, Side, Startup, ThemeChoice, update};

fn setup() -> (AppState, Instant) {
    let now = Instant::now();
    let themes = vec![ThemeChoice { id: "midnight".into(), name: "midnight".into() }];
    let startup = Startup {
        panels: [PanelSetup::at(PathBuf::from("/home")), PanelSetup::at(PathBuf::from("/tmp"))],
        active: Side::Left,
        split: crate::EVEN_SPLIT,
    };
    let (state, _) = AppState::new(startup, 80, 24, Settings::default(), crate::Appearance::new(themes, 0), now);
    (state, now)
}

fn relaunch(state: &mut AppState, now: Instant) -> Vec<Effect> {
    update(state, Msg::Relaunch { version: "9.9.9".into() }, now)
}

#[test]
fn an_idle_session_moves_to_the_newer_borg_at_once_and_only_once() {
    let (mut state, now) = setup();
    assert!(relaunch(&mut state, now).contains(&Effect::Relaunch));
    assert!(!update(&mut state, Msg::Tick, now).contains(&Effect::Relaunch));
}

#[test]
fn a_session_with_a_running_job_says_so_and_moves_once_the_job_is_done() {
    let (mut state, now) = setup();
    crate::jobs::start(&mut state, crate::JobKind::Delete, Spec::Delete { sources: vec!["/home/me".into()] }, now);
    assert!(!relaunch(&mut state, now).contains(&Effect::Relaunch));
    assert!(state.notice.as_deref().is_some_and(|notice| notice.contains("9.9.9")));
    assert!(!update(&mut state, Msg::Tick, now).contains(&Effect::Relaunch));
    state.jobs.clear();
    assert!(update(&mut state, Msg::Tick, now).contains(&Effect::Relaunch));
}

#[test]
fn a_recovery_key_on_screen_holds_the_move_until_it_is_closed() {
    let (mut state, now) = setup();
    let key = crate::Password::new("0a1b2c3d-4e5f6a7b");
    let cipher = PathBuf::from("/home/Papers");
    let vault =
        crate::VaultMount { cipher: cipher.clone(), root: "/run/user/1000/borg-commander/vaults/Papers".into() };
    update(&mut state, Msg::VaultCreated { side: Side::Left, cipher, key, opened: Ok(vault) }, now);
    assert!(!relaunch(&mut state, now).contains(&Effect::Relaunch));
    assert_eq!(state.notice.as_deref(), Some(borg_i18n::tr!(relaunch_waits_vault_key, version = "9.9.9").as_str()));
    for typed in "4e5f6a7b".chars() {
        update(&mut state, Msg::Key(crossterm::event::KeyCode::Char(typed).into()), now);
    }
    assert!(update(&mut state, Msg::Key(crossterm::event::KeyCode::Enter.into()), now).contains(&Effect::Relaunch));
}
