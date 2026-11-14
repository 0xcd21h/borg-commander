//! The scan form: the network S scans, typed or picked from the ones this machine reaches.

use crossterm::event::{KeyCode, KeyEvent};

use super::ScanProblem;
use crate::{AppState, Effect, Overlay, input::LineInput};

/// The longest network the form takes, as long as `255.255.255.255/30`.
const NETWORK_LIMIT: usize = 18;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanForm {
    pub network: LineInput,
    /// The attached and routed networks ↑ and ↓ step through.
    pub choices: Vec<String>,
    pub problem: Option<ScanProblem>,
}

impl ScanForm {
    pub fn new(target: &str, choices: Vec<String>) -> Self {
        Self { network: LineInput::new(target.to_owned()), choices, problem: None }
    }

    /// The next choice after the typed one, or before it, wrapping round.
    fn step(&mut self, forward: bool) {
        let count = self.choices.len();
        if count == 0 {
            return;
        }
        let at = self.choices.iter().position(|choice| choice == self.network.text().trim());
        let next = match (at, forward) {
            (Some(at), true) => (at + 1) % count,
            (Some(at), false) => (at + count - 1) % count,
            (None, true) => 0,
            (None, false) => count - 1,
        };
        self.network = LineInput::new(self.choices[next].clone());
        self.problem = None;
    }
}

pub(crate) fn on_key(state: &mut AppState, mut form: ScanForm, key: &KeyEvent) -> Vec<Effect> {
    match crate::keyboard::latin(key).code {
        KeyCode::Esc => {
            state.overlay = None;
            return Vec::new();
        }
        KeyCode::Enter => return super::submit_scan(state, form),
        KeyCode::Down | KeyCode::Tab => form.step(true),
        KeyCode::Up | KeyCode::BackTab => form.step(false),
        _ => {
            let before = form.network.clone();
            if form.network.apply(key) == crate::input::Edit::Changed {
                if form.network.text().chars().count() > NETWORK_LIMIT {
                    form.network = before;
                }
                form.problem = None;
            }
        }
    }
    state.overlay = Some(Overlay::ScanNetwork(Box::new(form)));
    Vec::new()
}

/// Pastes into the network.
pub(crate) fn paste(form: &mut ScanForm, text: &str) {
    let room = NETWORK_LIMIT.saturating_sub(form.network.text().chars().count());
    let pasted: String = text.chars().filter(|c| !c.is_control()).take(room).collect();
    form.network = LineInput::new(format!("{}{pasted}", form.network.text()));
    form.problem = None;
}
