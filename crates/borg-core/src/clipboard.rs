use std::collections::VecDeque;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{AppState, Effect};

/// Copies kept, newest first.
const HISTORY: usize = 20;
/// Bytes kept across all copies; the newest is kept whatever its size.
const BYTES: usize = 4 * 1024 * 1024;

/// What was copied in Borg. Pasting from it works wherever Borg runs, also where the terminal
/// cannot set or read the computer's clipboard, as in many browser terminals.
#[derive(Debug, Clone, Default)]
pub struct Clipboard {
    entries: VecDeque<String>,
}

impl Clipboard {
    pub fn latest(&self) -> Option<&str> {
        self.entries.front().map(String::as_str)
    }

    pub fn entries(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(String::as_str)
    }

    fn push(&mut self, text: String) {
        self.entries.retain(|entry| *entry != text);
        self.entries.push_front(text);
        let mut total = 0;
        let kept = self
            .entries
            .iter()
            .take(HISTORY)
            .take_while(|entry| {
                total += entry.len();
                total <= BYTES
            })
            .count();
        self.entries.truncate(kept.max(1));
    }
}

/// Keeps `text` in Borg's clipboard and asks the terminal to put it on the computer's.
pub(crate) fn copy(state: &mut AppState, text: String) -> Vec<Effect> {
    if text.is_empty() {
        return Vec::new();
    }
    state.clipboard.push(text.clone());
    vec![Effect::Copy(text)]
}

/// Ctrl-V, or Shift-Insert where the terminal leaves it to the program, in Borg's own fields.
pub(crate) fn is_paste(key: &KeyEvent) -> bool {
    matches!(
        (key.code, key.modifiers),
        (KeyCode::Char('v'), KeyModifiers::CONTROL) | (KeyCode::Insert, KeyModifiers::SHIFT)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_move_to_the_front_once_and_history_stays_bounded() {
        let mut clipboard = Clipboard::default();
        for n in 0..HISTORY + 5 {
            clipboard.push(n.to_string());
        }
        clipboard.push("3".into());
        assert_eq!(clipboard.latest(), Some("3"));
        assert_eq!(clipboard.entries().count(), HISTORY);
        assert_eq!(clipboard.entries().filter(|entry| *entry == "3").count(), 1);
    }

    #[test]
    fn large_copies_push_older_ones_out_but_the_newest_stays() {
        let mut clipboard = Clipboard::default();
        clipboard.push("small".into());
        clipboard.push("x".repeat(BYTES + 1));
        assert_eq!(clipboard.entries().count(), 1);
        assert_eq!(clipboard.latest().map(str::len), Some(BYTES + 1));
    }
}
