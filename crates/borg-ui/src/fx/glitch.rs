//! The glitching BORG logo, shared by the startup intro, the About window, and the glitch
//! screensaver: rows tear sideways and smear, glyphs corrupt, colours split and flare, and
//! a scan beam passes, all scaled by a `heat` from 0 (clean) to 1 (wrecked).

use ratatui::{buffer::Buffer, layout::Rect};

use super::{
    Gradient,
    noise::{hash, unit},
};
use crate::Rgb;

pub const GLITCH_LOGO: [&str; 5] = [
    "██████   ██████  ██████   ██████",
    "██   ██ ██    ██ ██   ██ ██",
    "██████  ██    ██ ██████  ██   ███",
    "██   ██ ██    ██ ██   ██ ██    ██",
    "██████   ██████  ██   ██  ██████",
];
pub const GLITCH_WIDTH: u16 = 34;
pub const GLITCH_HEIGHT: u16 = 5;

/// Corruption glyphs, all one cell wide and in the bundled Borg Mono font: shades,
/// quadrants, box lines, shapes, and Greek, Cyrillic and math letters.
const NOISE: [char; 40] = [
    '▓', '▒', '░', '▚', '▞', '▙', '▟', '▛', '▜', '▀', '▄', '▌', '▐', '■', '▪', '◢', '◣', '◤', '◥', '╳', '╱', '╲', '┼',
    '╪', '╫', '¦', '≡', '∆', '¤', 'Ξ', 'Ψ', 'Ж', 'λ', 'Ø', '§', '∑', '∂', '√', '#', '%',
];
/// Trails left behind a torn row, from faint to dense.
const SMEAR: [char; 4] = ['░', '░', '▒', '▓'];
/// Colours for corrupted rows: hot magenta, acid green, electric blue, toxic orange,
/// ultraviolet, and the two halves of a colour split.
const VIVID: [Rgb; 7] = [
    Rgb::new(255, 0, 170),
    Rgb::new(190, 255, 0),
    Rgb::new(0, 140, 255),
    Rgb::new(255, 120, 0),
    Rgb::new(150, 60, 255),
    Rgb::new(255, 64, 110),
    Rgb::new(64, 220, 255),
];
const SPLIT_RED: Rgb = Rgb::new(255, 40, 90);
const SPLIT_CYAN: Rgb = Rgb::new(40, 230, 255);
const BEAM: Rgb = Rgb::WHITE;
const FRAMES_PER_SECOND: f32 = 24.0;
const MAX_TEAR: f32 = 14.0;
const BEAM_PERIOD: f32 = 1.4;

fn seed(frame: u64, row: u64, column: u64, salt: u64) -> u64 {
    hash(frame.wrapping_mul(0x9e37) ^ row.wrapping_mul(0x85eb) ^ column.wrapping_mul(0xc2b2) ^ salt)
}

fn put(buf: &mut Buffer, clip: Rect, x: i32, y: u16, glyph: char, fg: Rgb, bg: Option<Rgb>) {
    let Ok(x) = u16::try_from(x) else { return };
    if !clip.contains((x, y).into()) || !buf.area.contains((x, y).into()) {
        return;
    }
    let cell = &mut buf[(x, y)];
    cell.set_char(glyph).set_fg(fg.into());
    if let Some(bg) = bg {
        cell.set_bg(bg.into());
    }
}

/// Draws the logo with its top-left at `origin`, clipped to `clip`. `t` is seconds and
/// picks the frame, so the same time always draws the same picture.
pub fn draw_glitch(
    buf: &mut Buffer,
    origin: (u16, u16),
    clip: Rect,
    heat: f32,
    t: f32,
    gradient: Gradient,
    background: Rgb,
) {
    let heat = heat.clamp(0.0, 1.0);
    let frame = (t.max(0.0) * FRAMES_PER_SECOND) as u64;
    let width = f32::from(GLITCH_WIDTH - 1);
    let beam = if heat > 0.05 { Some(((t / BEAM_PERIOD).fract() * f32::from(GLITCH_HEIGHT + 3)) as u16) } else { None };
    for (row, line) in (0u16..).zip(GLITCH_LOGO) {
        let r = u64::from(row);
        let y = origin.1 + row;
        let torn = unit(seed(frame, r, 0, 1)) < 0.65 * heat;
        let tear = if torn { ((unit(seed(frame, r, 0, 2)) * 2.0 - 1.0) * MAX_TEAR * heat).round() as i32 } else { 0 };
        let tint = (tear != 0).then(|| VIVID[(hash(seed(frame, r, 0, 3)) % VIVID.len() as u64) as usize]);
        let inverted = unit(seed(frame, r, 0, 4)) < 0.12 * heat;
        let split = heat > 0.25 && unit(seed(frame, r, 0, 5)) < 0.55 * heat;
        let lit = beam == Some(row);
        let cells: Vec<char> = line.chars().collect();
        let x0 = i32::from(origin.0) + tear;

        if split {
            for (c, &glyph) in (0i32..).zip(&cells) {
                if glyph != ' ' {
                    put(buf, clip, x0 + c - 2, y, '▓', background.mix(SPLIT_RED, 0.7), None);
                    put(buf, clip, x0 + c + 2, y, '▓', background.mix(SPLIT_CYAN, 0.7), None);
                }
            }
        }
        if tear != 0 {
            let reach = tear.abs();
            let anchor = if tear > 0 { i32::from(origin.0) } else { x0 + i32::from(GLITCH_WIDTH) };
            let color = tint.unwrap_or(gradient.from);
            for k in 0..reach {
                let density = SMEAR[(k * SMEAR.len() as i32 / reach.max(1)) as usize];
                let (x, glyph) = if tear > 0 { (anchor + k, density) } else { (anchor + reach - 1 - k, density) };
                put(buf, clip, x, y, glyph, background.mix(color, 0.35 + 0.5 * k as f32 / reach as f32), None);
            }
        }
        for (c, &glyph) in (0u64..).zip(&cells) {
            let corrupt = |salt, odds: f32| unit(seed(frame, r, c, salt)) < odds;
            let shown = if glyph != ' ' {
                if corrupt(6, 0.4 * heat) {
                    NOISE[(hash(seed(frame, r, c, 7)) % NOISE.len() as u64) as usize]
                } else {
                    glyph
                }
            } else if corrupt(8, 0.05 * heat) {
                NOISE[(hash(seed(frame, r, c, 9)) % NOISE.len() as u64) as usize]
            } else {
                continue;
            };
            let base = gradient.at(c as f32 / width);
            let fg = if lit {
                BEAM
            } else if corrupt(10, 0.2 * heat) {
                VIVID[(hash(seed(frame, r, c, 11)) % VIVID.len() as u64) as usize]
            } else {
                tint.map_or(base, |tint| base.mix(tint, 0.75))
            };
            let x = x0 + c as i32;
            if inverted {
                put(buf, clip, x, y, shown, background, Some(fg));
            } else {
                put(buf, clip, x, y, shown, fg, None);
            }
        }
    }
}

/// Brief interference over an empty folder, without the logo: a few torn noise bars and a
/// colour split. `progress` runs from 0 to 1 through the flash, which fades as it ends.
pub fn draw_interference(buf: &mut Buffer, area: Rect, t: f32, progress: f32, background: Rgb) {
    if area.is_empty() {
        return;
    }
    let heat = (1.0 - progress.clamp(0.0, 1.0)).powf(1.5);
    let frame = (t.max(0.0) * FRAMES_PER_SECOND) as u64;
    let pick = |salt, items: u64| hash(seed(frame, salt, 0, 30)) % items.max(1);
    for bar in 0..3 {
        if unit(seed(frame, bar, 0, 31)) > 0.35 + 0.4 * heat {
            continue;
        }
        let y = area.y + pick(32 + bar, u64::from(area.height)) as u16;
        let start = area.x + pick(36 + bar, u64::from(area.width)) as u16;
        let length = 2 + (unit(seed(frame, bar, 0, 40)) * f32::from(area.width) * 0.5 * heat) as u16;
        let color = VIVID[pick(44 + bar, VIVID.len() as u64) as usize];
        for x in start..start.saturating_add(length).min(area.right()) {
            let glyph = NOISE[(hash(seed(frame, bar, u64::from(x), 48)) % NOISE.len() as u64) as usize];
            buf[(x, y)].set_char(glyph).set_fg(background.mix(color, 0.25 + 0.5 * heat).into());
        }
    }
    if heat > 0.4 {
        let y = area.y + pick(50, u64::from(area.height)) as u16;
        let x = area.x + pick(51, u64::from(area.width)) as u16;
        let reach = 2 + (6.0 * heat) as u16;
        for (shift, color) in [(0, SPLIT_RED), (2, SPLIT_CYAN)] {
            for k in 0..reach {
                let x = x.saturating_add(k + shift);
                if x < area.right() {
                    let glyph = SMEAR[usize::from(k) * SMEAR.len() / usize::from(reach)];
                    buf[(x, y)].set_char(glyph).set_fg(background.mix(color, 0.5 * heat).into());
                }
            }
        }
    }
}

/// Heat for periodic bursts: mostly calm, flaring up every few seconds for under a
/// second, with the timing varying from burst to burst.
pub fn burst_heat(t: f32, period: f32, calm: f32) -> f32 {
    let index = (t / period).floor();
    let start = index * period + unit(index as u64) * period * 0.5;
    let since = t - start;
    let length = 0.45 + unit(index as u64 + 7) * 0.4;
    if (0.0..length).contains(&since) { calm.max((1.0 - since / length).powf(1.5)) } else { calm }
}

const SCANLINE: f32 = 0.18;
const SAVER_PERIOD: f32 = 3.2;
const SAVER_CALM: f32 = 0.03;
const DRIFT: (f32, f32) = (0.07, 0.05);
const BAR_ODDS: f32 = 0.45;
const FLASH: f32 = 0.14;
const SUBTITLE_GAP: u16 = 1;

fn subtitle(buf: &mut Buffer, clip: Rect, origin: (u16, u16), text: &str, heat: f32, t: f32, gradient: Gradient) {
    let frame = (t.max(0.0) * FRAMES_PER_SECOND) as u64;
    let width = text.chars().count() as u16;
    let x = origin.0 + GLITCH_WIDTH.saturating_sub(width) / 2;
    let y = origin.1 + GLITCH_HEIGHT + SUBTITLE_GAP;
    for (i, glyph) in (0u64..).zip(text.chars()) {
        let shown = if glyph != ' ' && unit(seed(frame, 99, i, 12)) < 0.3 * heat {
            NOISE[(hash(seed(frame, 99, i, 13)) % NOISE.len() as u64) as usize]
        } else {
            glyph
        };
        put(buf, clip, i32::from(x) + i as i32, y, shown, gradient.at(i as f32 / f32::from(width.max(2) - 1)), None);
    }
}

/// The glitch screensaver: a drifting logo on a scanlined screen, calm most of the time and
/// tearing apart in bursts, with noise bars across the screen and brief colour flashes.
pub fn draw_glitch_screen(buf: &mut Buffer, area: Rect, t: f32, gradient: Gradient, background: Rgb, title: &str) {
    if area.is_empty() {
        return;
    }
    let heat = burst_heat(t, SAVER_PERIOD, SAVER_CALM);
    let frame = (t.max(0.0) * FRAMES_PER_SECOND) as u64;
    let flash = heat > 0.8 && frame.is_multiple_of(3);
    let tone = |y: u16| {
        let base = if (y - area.y) % 2 == 1 { background.mix(Rgb::BLACK, SCANLINE) } else { background };
        if flash { base.mix(VIVID[(frame % VIVID.len() as u64) as usize], FLASH) } else { base }
    };
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            buf[(x, y)].set_char(' ').set_bg(tone(y).into());
        }
    }
    if heat > 0.3 {
        for bar in 0..(1 + (heat * 4.0) as u64) {
            if unit(seed(frame, bar, 0, 20)) > BAR_ODDS + heat * 0.4 {
                continue;
            }
            let y = area.y + (hash(seed(frame, bar, 0, 21)) % u64::from(area.height)) as u16;
            let start = (hash(seed(frame, bar, 0, 22)) % u64::from(area.width)) as u16;
            let length = (unit(seed(frame, bar, 0, 23)) * f32::from(area.width) * 0.6) as u16;
            let color = VIVID[(hash(seed(frame, bar, 0, 24)) % VIVID.len() as u64) as usize];
            for x in (area.x + start)..(area.x + start).saturating_add(length).min(area.right()) {
                let glyph = NOISE[(hash(seed(frame, bar, u64::from(x), 25)) % NOISE.len() as u64) as usize];
                buf[(x, y)].set_char(glyph).set_fg(tone(y).mix(color, 0.3 + 0.5 * heat).into());
            }
        }
    }
    let room = (area.width.saturating_sub(GLITCH_WIDTH), area.height.saturating_sub(GLITCH_HEIGHT + SUBTITLE_GAP + 1));
    let drift = |share: f32, speed: f32, phase: f32| share * (0.5 + 0.45 * (t * speed + phase).sin());
    let origin = (
        area.x + drift(f32::from(room.0), DRIFT.0, 0.0) as u16,
        area.y + drift(f32::from(room.1), DRIFT.1, 1.3) as u16,
    );
    draw_glitch(buf, origin, area, heat, t, gradient, background);
    subtitle(buf, area, origin, title, heat, t, gradient);
}

/// The startup intro: the logo tears in from noise and settles within `settle` seconds.
pub fn draw_glitch_intro(
    buf: &mut Buffer,
    area: Rect,
    elapsed: f32,
    settle: f32,
    gradient: Gradient,
    background: Rgb,
    title: &str,
) {
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            buf[(x, y)].reset();
            buf[(x, y)].set_bg(background.into());
        }
    }
    let heat = (1.0 - elapsed / settle.max(f32::EPSILON)).clamp(0.0, 1.0).powf(1.6);
    let origin = (
        area.x + area.width.saturating_sub(GLITCH_WIDTH) / 2,
        area.y + area.height.saturating_sub(GLITCH_HEIGHT + SUBTITLE_GAP + 1) / 2,
    );
    draw_glitch(buf, origin, area, heat, elapsed, gradient, background);
    subtitle(buf, area, origin, title, heat, elapsed, gradient);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_match_the_declared_size_and_bursts_rise_and_settle() {
        assert_eq!(GLITCH_LOGO.len(), usize::from(GLITCH_HEIGHT));
        assert!(GLITCH_LOGO.iter().all(|row| row.chars().count() <= usize::from(GLITCH_WIDTH)));
        assert!(NOISE.iter().chain(&SMEAR).all(|&c| unicode_width::UnicodeWidthChar::width(c) == Some(1)));
        let heats: Vec<f32> = (0..400).map(|i| burst_heat(i as f32 * 0.05, 3.0, 0.02)).collect();
        assert!(heats.iter().any(|&h| h > 0.8) && heats.iter().all(|&h| (0.02..=1.0).contains(&h)));
    }

    #[test]
    fn a_frame_is_reproducible_and_a_cold_logo_is_clean() {
        let area = Rect::new(0, 0, 60, 8);
        let gradient = Gradient { from: Rgb::new(0, 255, 0), to: Rgb::new(0, 0, 255) };
        let draw = |heat, t| {
            let mut buf = Buffer::empty(area);
            draw_glitch(&mut buf, (10, 1), area, heat, t, gradient, Rgb::BLACK);
            buf
        };
        assert_eq!(draw(0.9, 1.25), draw(0.9, 1.25));
        let clean = draw(0.0, 1.25);
        let text: String = (0..60).map(|x| clean[(x, 3)].symbol().to_owned()).collect();
        assert!(text.contains("██████  ██    ██ ██████  ██   ███"));
    }
}
