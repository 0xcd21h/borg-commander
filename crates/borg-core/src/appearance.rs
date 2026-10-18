use std::time::{Duration, Instant};

use crate::{Easing, Fade};

const START: f32 = 0.0;
const END: f32 = 1.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeChoice {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Picker {
    pub selected: usize,
    pub original: usize,
}

#[derive(Debug, Clone)]
pub struct Appearance {
    choices: Vec<ThemeChoice>,
    current: usize,
    previous: Option<(usize, Fade)>,
}

impl Appearance {
    pub fn new(choices: Vec<ThemeChoice>, current: usize) -> Self {
        let current = current.min(choices.len().saturating_sub(1));
        Self { choices, current, previous: None }
    }

    pub fn choices(&self) -> &[ThemeChoice] {
        &self.choices
    }

    pub fn current(&self) -> usize {
        self.current
    }

    pub fn current_id(&self) -> Option<&str> {
        self.choices.get(self.current).map(|choice| choice.id.as_str())
    }

    pub fn transition(&self, now: Instant) -> Option<(usize, f32)> {
        self.previous.filter(|(_, fade)| fade.is_animating(now)).map(|(from, fade)| (from, fade.value(now)))
    }

    pub fn is_animating(&self, now: Instant) -> bool {
        self.transition(now).is_some()
    }

    pub fn select(&mut self, index: usize, now: Instant, duration: Duration) {
        if index == self.current || index >= self.choices.len() {
            return;
        }
        let fade = Fade::settled(START, now).toward(END, now, duration, Easing::OutCubic);
        self.previous = Some((self.current, fade));
        self.current = index;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choices(n: usize) -> Vec<ThemeChoice> {
        (0..n).map(|i| ThemeChoice { id: format!("t{i}"), name: format!("Theme {i}") }).collect()
    }

    #[test]
    fn selection_animates_from_previous_theme() {
        let now = Instant::now();
        let duration = Duration::from_millis(100);
        let mut appearance = Appearance::new(choices(3), 0);
        appearance.select(2, now, duration);

        assert_eq!(appearance.current_id(), Some("t2"));
        assert_eq!(appearance.transition(now + duration / 2).map(|(from, _)| from), Some(0));
        assert_eq!(appearance.transition(now + duration), None);
    }

    #[test]
    fn ignores_out_of_range_and_same_selection() {
        let now = Instant::now();
        let mut appearance = Appearance::new(choices(2), 5);
        assert_eq!(appearance.current(), 1);
        appearance.select(1, now, Duration::from_millis(100));
        appearance.select(9, now, Duration::from_millis(100));
        assert!(!appearance.is_animating(now));
    }
}
