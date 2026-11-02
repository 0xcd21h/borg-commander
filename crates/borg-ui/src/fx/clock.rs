//! The clock screensaver: the time in the logo's block letters, drifting over the glitch
//! screen. Each minute the old digits tear apart and the new ones settle, and small bursts
//! keep it moving in between.

use jiff::Zoned;
use ratatui::{buffer::Buffer, layout::Rect};

use super::{
    Gradient,
    glitch::{Blocks, burst_heat, draw_glitch, draw_static, drifting, settling_heat, subtitle},
};
use crate::Rgb;

const DIGITS: [[&str; 5]; 10] = [
    [" ██████ ", "██    ██", "██    ██", "██    ██", " ██████ "],
    ["   ██   ", " ████   ", "   ██   ", "   ██   ", " ██████ "],
    [" ██████ ", "      ██", " ██████ ", "██      ", "████████"],
    ["███████ ", "      ██", "  █████ ", "      ██", "███████ "],
    ["██    ██", "██    ██", "████████", "      ██", "      ██"],
    ["████████", "██      ", "███████ ", "      ██", "███████ "],
    [" ██████ ", "██      ", "███████ ", "██    ██", " ██████ "],
    ["████████", "      ██", "     ██ ", "    ██  ", "    ██  "],
    [" ██████ ", "██    ██", " ██████ ", "██    ██", " ██████ "],
    [" ██████ ", "██    ██", " ███████", "      ██", " ██████ "],
];
const COLON: [&str; 5] = ["  ", "██", "  ", "██", "  "];
const DARK_COLON: [&str; 5] = ["  "; 5];
const GAP: &str = " ";
const DATE_FORMAT: &str = "%Y-%m-%d";
const SECONDS_PER_MINUTE: f32 = 60.0;
const NANOS_PER_SECOND: f32 = 1e9;
/// The old digits tear apart over the minute's last moments.
const TEAR_DOWN: f32 = 0.6;
/// The new digits settle over the minute's first moments.
const SETTLE: f32 = 0.9;
const TEAR_CURVE: f32 = 2.0;
const BURST_PERIOD: f32 = 7.0;
const BURST_CALM: f32 = 0.03;
const BURST_STRENGTH: f32 = 0.35;
const COLON_LIT: f32 = 0.5;

fn seconds(clock: &Zoned) -> f32 {
    f32::from(clock.second()) + clock.subsec_nanosecond() as f32 / NANOS_PER_SECOND
}

/// The time as rows of block letters, `HH:MM`, with the colon lit in the first half of each
/// second.
pub fn clock_rows(hour: i8, minute: i8, second: f32) -> Vec<String> {
    let digit = |value: i8| DIGITS[usize::try_from(value.rem_euclid(10)).unwrap_or(0)];
    let colon = if second.fract() < COLON_LIT { COLON } else { DARK_COLON };
    let glyphs = [digit(hour / 10), digit(hour % 10), colon, digit(minute / 10), digit(minute % 10)];
    (0..COLON.len()).map(|row| glyphs.iter().map(|glyph| glyph[row]).collect::<Vec<_>>().join(GAP)).collect()
}

/// How torn the digits are `second` seconds into the minute: wrecked as the minute turns,
/// clean in between.
pub fn minute_heat(second: f32) -> f32 {
    let left = SECONDS_PER_MINUTE - second;
    if second < SETTLE {
        settling_heat(second, SETTLE)
    } else {
        (1.0 - left / TEAR_DOWN).clamp(0.0, 1.0).powf(TEAR_CURVE)
    }
}

pub fn draw_clock_screen(buf: &mut Buffer, area: Rect, t: f32, clock: &Zoned, gradient: Gradient, background: Rgb) {
    if area.is_empty() {
        return;
    }
    let second = seconds(clock);
    let heat = minute_heat(second).max(BURST_STRENGTH * burst_heat(t, BURST_PERIOD, BURST_CALM));
    draw_static(buf, area, heat, t, background);
    let rows = clock_rows(clock.hour(), clock.minute(), second);
    let rows: Vec<&str> = rows.iter().map(String::as_str).collect();
    let size = Blocks::fitted(&rows, (0, 0));
    let blocks = Blocks { origin: drifting(area, (size.width, size.height()), t), ..size };
    draw_glitch(buf, blocks, area, heat, t, gradient, background);
    subtitle(buf, area, blocks, &clock.strftime(DATE_FORMAT).to_string(), heat, t, gradient);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(time: &str) -> Zoned {
        format!("2026-01-15T{time}+00:00[UTC]").parse().unwrap()
    }

    fn text(buf: &Buffer) -> String {
        let area = buf.area;
        (area.top()..area.bottom())
            .map(|y| (area.left()..area.right()).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn every_digit_has_the_same_size() {
        let width = DIGITS[0][0].chars().count();
        assert!(DIGITS.iter().flatten().all(|row| row.chars().count() == width));
        assert!(COLON.iter().chain(&DARK_COLON).all(|row| row.chars().count() == COLON[0].chars().count()));
    }

    #[test]
    fn rows_spell_the_time_and_the_colon_blinks() {
        let lit = clock_rows(9, 41, 12.2);
        assert_eq!(lit.len(), COLON.len());
        assert_eq!(lit[4], [DIGITS[0][4], DIGITS[9][4], COLON[4], DIGITS[4][4], DIGITS[1][4]].join(GAP));
        assert_eq!(lit[1], [DIGITS[0][1], DIGITS[9][1], COLON[1], DIGITS[4][1], DIGITS[1][1]].join(GAP));
        let dark = clock_rows(9, 41, 12.7);
        assert_ne!(lit[1], dark[1]);
        assert_eq!(lit[0], dark[0]);
    }

    #[test]
    fn the_digits_tear_only_as_the_minute_turns() {
        assert!(minute_heat(0.0) > 0.99);
        assert!(minute_heat(59.9) > 0.5);
        assert!(minute_heat(59.9) > minute_heat(59.6));
        assert_eq!(minute_heat(30.0), 0.0);
        assert_eq!(minute_heat(SETTLE), 0.0);
        assert!((0..600).all(|tenth| (0.0..=1.0).contains(&minute_heat(tenth as f32 / 10.0))));
    }

    fn blocks(text: &str) -> usize {
        text.chars().filter(|&glyph| glyph == '█').count()
    }

    #[test]
    fn a_calm_frame_shows_the_time_and_a_turning_minute_tears_it() {
        let area = Rect::new(0, 0, 60, 12);
        let gradient = Gradient { from: Rgb::new(0, 255, 0), to: Rgb::new(0, 0, 255) };
        let draw = |clock: &Zoned| {
            let mut buf = Buffer::empty(area);
            draw_clock_screen(&mut buf, area, 0.0, clock, gradient, Rgb::BLACK);
            buf
        };
        let whole: usize = clock_rows(12, 34, 20.1).iter().map(|row| blocks(row)).sum();
        let calm = text(&draw(&at("12:34:20.1")));
        assert!(blocks(&calm) * 10 >= whole * 9);
        assert!(calm.contains("2026-01-15"));
        let turning = text(&draw(&at("12:35:00.05")));
        assert!(blocks(&turning) * 10 < whole * 8);
        assert_eq!(draw(&at("12:34:20.1")), draw(&at("12:34:20.1")));
    }
}
