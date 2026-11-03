//! A password field's characters as glitch noise: each one a corrupted glyph that keeps changing
//! at its own pace, in the theme's accent colors, with a scan beam sweeping across them and
//! flares as keys are typed. The picture depends on how many characters there are and on the
//! time, never on the characters themselves.

use ratatui::{buffer::Buffer, layout::Rect};

use super::{
    Gradient, burst_heat,
    glitch::{BEAM, NOISE, VIVID, frame_at, seed},
    noise::{hash, unit},
};
use crate::Rgb;

/// Glyph changes per second, for the steadiest character and the most restless one.
const CHURN: (f32, f32) = (2.5, 12.0);
/// How long a keystroke keeps the field hot, in seconds.
const FLARE: f32 = 0.5;
const FLARE_CURVE: f32 = 1.5;
const BURST_PERIOD: f32 = 2.8;
const CALM: f32 = 0.1;
/// Seconds between scan beam passes, the characters it crosses each second, and how many it
/// lights at once.
const BEAM_PERIOD: f32 = 2.4;
const BEAM_SPEED: f32 = 36.0;
const BEAM_WIDTH: f32 = 2.5;
const VIVID_ODDS: f32 = 0.4;
const INVERT_ODDS: f32 = 0.18;
/// How much of its color a character shows when the field is calm.
const CALM_VISIBILITY: f32 = 0.7;

/// The characters of a password field to draw: `count` of them, from `first` on when the field
/// scrolled, `t` seconds after Borg started and `typed` seconds after a key last changed them.
#[derive(Debug, Clone, Copy)]
pub struct Cipher {
    pub first: usize,
    pub count: usize,
    pub t: f32,
    pub typed: Option<f32>,
}

impl Cipher {
    fn flare(&self) -> f32 {
        self.typed.map_or(0.0, |since| (1.0 - since / FLARE).clamp(0.0, 1.0).powf(FLARE_CURVE))
    }

    fn heat(&self) -> f32 {
        burst_heat(self.t, BURST_PERIOD, CALM).max(self.flare())
    }

    /// Where the scan beam is, in characters from the first one.
    fn beam(&self) -> f32 {
        (self.t / BEAM_PERIOD).fract() * BEAM_PERIOD * BEAM_SPEED - BEAM_WIDTH
    }

    fn glyph(&self, index: u64) -> char {
        let pace = CHURN.0 + unit(seed(0, index, 0, 60)) * (CHURN.1 - CHURN.0);
        let step = (self.t.max(0.0) * pace + unit(seed(0, index, 0, 61))) as u64;
        NOISE[(hash(seed(step, index, 0, 62)) % NOISE.len() as u64) as usize]
    }
}

/// Draws the characters along the top row of `area`, one per cell, over `background`.
pub fn draw_cipher(buf: &mut Buffer, area: Rect, cipher: Cipher, gradient: Gradient, background: Rgb) {
    let area = area.intersection(buf.area);
    if area.is_empty() {
        return;
    }
    let (heat, flare, beam) = (cipher.heat(), cipher.flare(), cipher.beam());
    let frame = frame_at(cipher.t);
    let last = cipher.count.saturating_sub(1);
    let span = last.max(1) as f32;
    let y = area.y;
    for (x, index) in (area.left()..area.right()).zip(cipher.first..cipher.count) {
        let i = index as u64;
        let odds = |salt, odds: f32| unit(seed(frame, i, 0, salt)) < odds * heat;
        let base = gradient.at(index as f32 / span);
        let mut fg = if odds(63, VIVID_ODDS) {
            VIVID[(hash(seed(frame, i, 0, 64)) % VIVID.len() as u64) as usize]
        } else {
            background.mix(base, CALM_VISIBILITY + (1.0 - CALM_VISIBILITY) * heat)
        };
        let lit = (1.0 - (index as f32 - beam).abs() / BEAM_WIDTH).max(0.0);
        let newest = if index == last { flare } else { 0.0 };
        fg = fg.mix(BEAM, lit.max(newest));
        let cell = &mut buf[(x, y)];
        cell.set_char(cipher.glyph(i));
        if odds(65, INVERT_ODDS) {
            cell.set_fg(background.into()).set_bg(fg.into());
        } else {
            cell.set_fg(fg.into()).set_bg(background.into());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GRADIENT: Gradient = Gradient { from: Rgb::new(0, 200, 120), to: Rgb::new(80, 120, 255) };

    fn drawn(cipher: Cipher, width: u16) -> Buffer {
        let mut buf = Buffer::empty(Rect::new(0, 0, width, 1));
        draw_cipher(&mut buf, Rect::new(0, 0, width, 1), cipher, GRADIENT, Rgb::BLACK);
        buf
    }

    fn glyphs(buf: &Buffer) -> String {
        buf.content.iter().map(|cell| cell.symbol()).collect()
    }

    #[test]
    fn each_character_shows_as_noise_that_keeps_changing() {
        let at = |t| Cipher { first: 0, count: 7, t, typed: None };
        let buf = drawn(at(3.1), 10);
        let shown = glyphs(&buf);
        assert!(shown.chars().take(7).all(|glyph| NOISE.contains(&glyph)), "{shown:?}");
        assert_eq!(&shown[shown.len() - 3..], "   ", "nothing past the last character");
        assert_eq!(drawn(at(3.1), 10), buf, "the same moment, the same picture");
        let later = (1..20).map(|step| glyphs(&drawn(at(3.1 + step as f32 * 0.08), 10)));
        assert!(later.filter(|frame| *frame != shown).count() > 15, "the noise moves");
    }

    #[test]
    fn a_scrolled_field_draws_what_fits_and_the_newest_character_flares() {
        let cipher = Cipher { first: 4, count: 12, t: 1.0, typed: Some(0.0) };
        let buf = drawn(cipher, 8);
        assert!(glyphs(&buf).chars().all(|glyph| NOISE.contains(&glyph)));
        let newest = buf.content.last().map(|cell| (cell.fg, cell.bg)).unwrap();
        assert!([newest.0, newest.1].contains(&BEAM.into()), "the newest character is lit as it is typed");
        let settled = drawn(Cipher { typed: Some(FLARE), ..cipher }, 8);
        let newest = settled.content.last().map(|cell| (cell.fg, cell.bg)).unwrap();
        assert!(![newest.0, newest.1].contains(&BEAM.into()));
    }
}
