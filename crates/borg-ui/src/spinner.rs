//! The spinner for background work. It appears only once the work has run for
//! [`borg_core::SPINNER_DELAY`], fades in, turns a braille glyph through the theme accent,
//! and sweeps a soft light across its label.

use std::{f32::consts::TAU, time::Instant};

use borg_core::{Busy, Task};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::Widget,
};
use unicode_width::UnicodeWidthStr;

use crate::look::Look;

const FRAMES: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
const FRAMES_PER_SECOND: f32 = 12.0;
const FADE_IN: f32 = 0.3;
const HUE_PERIOD: f32 = 2.4;
const SWEEP_PERIOD: f32 = 2.0;
const SWEEP_REACH: f32 = 4.0;
const SWEEP_GLOW: f32 = 0.75;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spinner {
    shown: Instant,
    now: Instant,
}

impl Spinner {
    /// The task's spinner, once the task has run long enough to show one.
    pub fn of(busy: &Busy, task: Task, now: Instant) -> Option<Self> {
        busy.shown_since(task, now).map(|shown| Self { shown, now })
    }

    fn elapsed(self) -> f32 {
        self.now.saturating_duration_since(self.shown).as_secs_f32()
    }

    /// The glyph followed by the label, drawn over `base`.
    pub fn spans(self, label: &str, look: &Look<'_>, base: Style) -> Vec<Span<'static>> {
        let t = self.elapsed();
        let presence = (t / FADE_IN).clamp(0.0, 1.0);
        let background = look.bg(base);
        let text = look.fg(base);
        let accent = look.accent();
        let glyph = FRAMES[(t * FRAMES_PER_SECOND) as usize % FRAMES.len()];
        let hue = 0.5 - 0.5 * (t * TAU / HUE_PERIOD).cos();
        let mut spans = vec![
            Span::styled(glyph.to_string(), base.fg(background.mix(accent.at(hue), presence).into())),
            Span::styled(" ", base),
        ];
        let chars: Vec<char> = label.chars().collect();
        let travel = chars.len() as f32 + SWEEP_REACH * 2.0;
        let light = (t / SWEEP_PERIOD).fract() * travel - SWEEP_REACH;
        spans.extend(chars.into_iter().enumerate().map(|(i, c)| {
            let glow = (1.0 - (i as f32 - light).abs() / SWEEP_REACH).clamp(0.0, 1.0) * SWEEP_GLOW;
            let color = background.mix(text.mix(accent.from, glow), presence);
            Span::styled(c.to_string(), base.fg(color.into()))
        }));
        spans
    }

    pub fn line(self, label: &str, look: &Look<'_>, base: Style) -> Line<'static> {
        Line::from(self.spans(label, look, base))
    }

    /// Draws the spinner and label centred in `area`.
    pub fn draw_centered(self, label: &str, look: &Look<'_>, base: Style, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let width = (label.width() as u16 + 2).min(area.width);
        let row = Rect { x: area.x + (area.width - width) / 2, y: area.y + area.height / 2, width, height: 1 };
        self.line(label, look, base).render(row, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use borg_core::{SPINNER_DELAY, Side};
    use std::time::Duration;

    #[test]
    fn frames_cycle_and_follow_the_elapsed_time() {
        let shown = Instant::now();
        let at = |seconds: f32| Spinner { shown, now: shown + Duration::from_secs_f32(seconds) };
        let glyph = |spinner: Spinner| FRAMES[(spinner.elapsed() * FRAMES_PER_SECOND) as usize % FRAMES.len()];
        let frames: std::collections::HashSet<_> = (0..10).map(|i| glyph(at(i as f32 / FRAMES_PER_SECOND))).collect();
        assert_eq!(frames.len(), FRAMES.len());
        assert_eq!(glyph(at(0.0)), glyph(at(0.0)));
        assert!(SPINNER_DELAY >= Duration::from_millis(500));
        assert!(Spinner::of(&Busy::default(), Task::Listing(Side::Left), shown).is_none());
    }
}
