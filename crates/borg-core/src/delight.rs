use borg_i18n::tr;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent};

use crate::{AppState, Effect, Overlay};

pub const UNLOCK_WORD: &str = "borg";
pub const RAIN_DURATION: Duration = Duration::from_secs(8);
const MEMORY: usize = 10;
const GAME_WORD: &str = "2048";
/// Opens the Connect dialog from About; no secret word has it.
pub const CONNECT_KEY: char = 'c';
const KONAMI: [KeyCode; 10] = [
    KeyCode::Up,
    KeyCode::Up,
    KeyCode::Down,
    KeyCode::Down,
    KeyCode::Left,
    KeyCode::Right,
    KeyCode::Left,
    KeyCode::Right,
    KeyCode::Char('b'),
    KeyCode::Char('a'),
];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct About {
    pub recent: Vec<KeyCode>,
}

pub(crate) fn on_about_key(state: &mut AppState, mut about: About, key: &KeyEvent, now: Instant) -> Vec<Effect> {
    if matches!(key.code, KeyCode::Esc | KeyCode::Enter | KeyCode::F(1 | 10)) {
        state.overlay = None;
        return Vec::new();
    }

    if key.code == KeyCode::Char(CONNECT_KEY) {
        return crate::agent::open_connect(state);
    }
    about.recent.push(key.code);
    let overflow = about.recent.len().saturating_sub(MEMORY);
    about.recent.drain(..overflow);
    let spelled = typed_word(&about.recent);
    let konami = about.recent.ends_with(&KONAMI);
    if spelled.ends_with(GAME_WORD) {
        return crate::game2048::open(state, now);
    }
    state.overlay = Some(Overlay::About(about));

    if konami {
        state.rain_until = Some(now + RAIN_DURATION);
        return Vec::new();
    }
    if spelled.ends_with(UNLOCK_WORD) {
        return match state.appearance.unlock() {
            Some(_) => {
                state.notice = Some(tr!(cube_theme_unlocked_options_theme).to_owned());
                vec![Effect::SaveConfig]
            }
            None => {
                state.notice = Some(tr!(resistance_is_futile).to_owned());
                Vec::new()
            }
        };
    }
    Vec::new()
}

fn typed_word(keys: &[KeyCode]) -> String {
    keys.iter()
        .filter_map(|code| match code {
            KeyCode::Char(c) => Some(c.to_ascii_lowercase()),
            _ => None,
        })
        .collect()
}
