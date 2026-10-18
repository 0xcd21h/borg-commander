use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent};

use crate::{AppState, Effect, Overlay};

pub const UNLOCK_WORD: &str = "borg";
pub const RAIN_DURATION: Duration = Duration::from_secs(8);
const MEMORY: usize = 10;
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
const UNLOCKED_NOTICE: &str = "Cube theme unlocked · Options › Theme…";
const FUTILE_NOTICE: &str = "Resistance is futile.";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct About {
    pub recent: Vec<KeyCode>,
}

pub(crate) fn on_about_key(state: &mut AppState, mut about: About, key: &KeyEvent, now: Instant) -> Vec<Effect> {
    if matches!(key.code, KeyCode::Esc | KeyCode::Enter | KeyCode::F(1 | 10)) {
        state.overlay = None;
        return Vec::new();
    }

    about.recent.push(key.code);
    let overflow = about.recent.len().saturating_sub(MEMORY);
    about.recent.drain(..overflow);
    let spelled = typed_word(&about.recent);
    let konami = about.recent.ends_with(&KONAMI);
    state.overlay = Some(Overlay::About(about));

    if !state.settings.preferences.delight {
        return Vec::new();
    }
    if konami {
        state.rain_until = Some(now + RAIN_DURATION);
        return Vec::new();
    }
    if spelled.ends_with(UNLOCK_WORD) {
        return match state.appearance.unlock() {
            Some(_) => {
                state.notice = Some(UNLOCKED_NOTICE.to_owned());
                vec![Effect::SaveConfig]
            }
            None => {
                state.notice = Some(FUTILE_NOTICE.to_owned());
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
