//! The particle screensaver: the logo as dots that swirl out into a turning spiral galaxy and
//! swirl back into place. Every dot's path is a formula of time, so a moment always draws the
//! same picture.

use std::f32::consts::TAU;

use ratatui::{buffer::Buffer, layout::Rect};

use super::{Gradient, braille::Canvas, glitch::GLITCH_LOGO, noise::unit};
use crate::Rgb;

const BLOCK: char = '█';
const DOT_COLUMNS: usize = 2;
const DOT_ROWS: usize = 4;
const LOGO_COLUMNS: f32 = 34.0;
const LOGO_ROWS: f32 = 5.0;
const MAX_SCALE: usize = 3;
const FILL_WIDTH: f32 = 0.7;
const FILL_HEIGHT: f32 = 0.45;

const CYCLE: f32 = 16.0;
/// The logo holds together this long at the start of each cycle.
const HOLD: f32 = 4.0;
const BURST: f32 = 3.0;
const ADRIFT: f32 = 4.0;
const RETURN: f32 = 3.5;
/// Dots leave and come back in a wave from left to right, plus a little each of their own.
const WAVE: f32 = 0.9;
const JITTER: f32 = 0.4;
const _: () = assert!(HOLD + BURST + ADRIFT + RETURN + WAVE + JITTER <= CYCLE);

/// The galaxy's reach as a share of the screen's width and height, seen as a tilted disc.
const GALAXY: (f32, f32) = (0.46, 0.46);
const ARMS: u64 = 3;
/// How far an arm winds from the core to the rim, in radians.
const TWIST: f32 = 2.6;
const ARM_WIDTH: f32 = 0.8;
const CORE: f32 = 0.06;
/// Below 1, dots gather toward the core.
const FALLOFF: f32 = 0.75;
/// The galaxy turns this fast in radians a second, faster near the core.
const SPIN: f32 = 0.16;
const SPIN_CORE: f32 = 0.35;
const NEAR: f32 = 1.0;
const FAR: f32 = 0.5;
const SUBTITLE_GAP: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq)]
struct Dot {
    x: f32,
    y: f32,
    glow: f32,
    hue: f32,
}

fn scale(area: Rect) -> usize {
    let wide = f32::from(area.width) * FILL_WIDTH / LOGO_COLUMNS;
    let tall = f32::from(area.height) * FILL_HEIGHT / LOGO_ROWS;
    (wide.min(tall) as usize).clamp(1, MAX_SCALE)
}

fn ease_out(x: f32) -> f32 {
    1.0 - (1.0 - x).powi(3)
}

/// How far from home a dot is, from 0 (in place) to 1 (adrift), `local` seconds into its cycle.
pub fn scatter(local: f32) -> f32 {
    let away = HOLD + BURST;
    let back = away + ADRIFT;
    let home = back + RETURN;
    if !(HOLD..home).contains(&local) {
        0.0
    } else if local < away {
        ease_out((local - HOLD) / BURST)
    } else if local < back {
        1.0
    } else {
        1.0 - ((local - back) / RETURN).powi(3)
    }
}

/// The logo's top-left cell when it is `scale` times its size and centred in `cols` by `rows`.
fn corner(cols: usize, rows: usize, scale: usize) -> (usize, usize) {
    let size = |cells: f32| cells as usize * scale;
    (cols.saturating_sub(size(LOGO_COLUMNS)) / 2, rows.saturating_sub(size(LOGO_ROWS)) / 2)
}

/// Where each block of the logo puts its dots, in canvas dots. The logo starts on a whole cell,
/// so at rest every block fills one.
fn homes(cols: usize, rows: usize, scale: usize) -> Vec<(f32, f32)> {
    let (dots_x, dots_y) = (DOT_COLUMNS * scale, DOT_ROWS * scale);
    let (left, top) = corner(cols, rows, scale);
    let (left, top) = ((left * DOT_COLUMNS) as f32, (top * DOT_ROWS) as f32);
    let mut homes = Vec::new();
    for (row, line) in GLITCH_LOGO.iter().enumerate() {
        for (col, _) in line.chars().enumerate().filter(|(_, glyph)| *glyph == BLOCK) {
            for dy in 0..dots_y {
                for dx in 0..dots_x {
                    homes.push((left + (col * dots_x + dx) as f32, top + (row * dots_y + dy) as f32));
                }
            }
        }
    }
    homes
}

fn between((low, high): (f32, f32), share: f32) -> f32 {
    low + (high - low) * share
}

/// A dot's angle around the centre `local` seconds into its cycle: from its place in the logo
/// out to its place in the galaxy, round with the galaxy, and back, always turning one way.
fn angle(local: f32, home: f32, galaxy: f32, spin: f32) -> f32 {
    let away = scatter(local);
    let out = HOLD + BURST;
    let back = out + ADRIFT;
    let turned = |at: f32| galaxy + spin * (at - HOLD);
    if !(HOLD..back + RETURN).contains(&local) {
        home
    } else if local < out {
        home + away * ((galaxy - home).rem_euclid(TAU) + spin * (local - HOLD))
    } else if local < back {
        turned(local)
    } else {
        let from = turned(back);
        from + (1.0 - away) * (home - from).rem_euclid(TAU) + away * spin * (local - back)
    }
}

fn dots(cols: usize, rows: usize, scale: usize, t: f32) -> Vec<Dot> {
    let (width, height) = ((cols * DOT_COLUMNS) as f32, (rows * DOT_ROWS) as f32);
    let (center_x, center_y) = (width / 2.0, height / 2.0);
    let (radius_x, radius_y) = (width * GALAXY.0, height * GALAXY.1);
    let logo_left = (corner(cols, rows, scale).0 * DOT_COLUMNS) as f32;
    let logo_width = LOGO_COLUMNS * (DOT_COLUMNS * scale) as f32;
    let cycle = t.max(0.0) % CYCLE;
    (0u64..)
        .zip(homes(cols, rows, scale))
        .map(|(id, (home_x, home_y))| {
            let random = |salt: u64| unit(id * 16 + salt);
            let across = ((home_x - logo_left) / logo_width).clamp(0.0, 1.0);
            let local = cycle - across * WAVE - random(0) * JITTER;
            let away = scatter(local);
            let (x, y) = ((home_x - center_x) / radius_x, (home_y - center_y) / radius_y);
            let home = (x.hypot(y), y.atan2(x));
            let reach = between((CORE, 1.0), random(1).powf(FALLOFF));
            let arm = (id % ARMS) as f32 * TAU / ARMS as f32;
            let galaxy = arm + reach * TWIST + (random(2) - 0.5) * ARM_WIDTH;
            let turn = angle(local, home.1, galaxy, SPIN / (reach + SPIN_CORE));
            let distance = between((home.0, reach), away);
            Dot {
                x: center_x + distance * turn.cos() * radius_x,
                y: center_y + distance * turn.sin() * radius_y,
                glow: between((NEAR, between((FAR, NEAR), (1.0 - reach).sqrt())), away),
                hue: between((across, reach), away),
            }
        })
        .collect()
}

fn subtitle(buf: &mut Buffer, area: Rect, scale: usize, text: &str, fade: f32, gradient: Gradient, background: Rgb) {
    let (_, top) = corner(usize::from(area.width), usize::from(area.height), scale);
    let below = top + LOGO_ROWS as usize * scale + usize::from(SUBTITLE_GAP);
    let y = area.y.saturating_add(u16::try_from(below).unwrap_or(u16::MAX));
    let width = u16::try_from(text.chars().count()).unwrap_or(u16::MAX);
    let x = area.x + area.width.saturating_sub(width) / 2;
    let span = f32::from(width.saturating_sub(1).max(1));
    for (i, glyph) in (0u16..).zip(text.chars()) {
        let position = (x + i, y);
        if area.contains(position.into()) {
            let color = background.mix(gradient.at(f32::from(i) / span), fade);
            buf[position].set_char(glyph).set_fg(color.into());
        }
    }
}

pub fn draw_particles(buf: &mut Buffer, area: Rect, t: f32, gradient: Gradient, background: Rgb, title: &str) {
    if area.is_empty() {
        return;
    }
    let (cols, rows) = (usize::from(area.width), usize::from(area.height));
    let mut canvas = Canvas::new(cols, rows);
    let mut hues = vec![(0.0f32, 0.0f32); cols * rows];
    let scale = scale(area);
    for dot in dots(cols, rows, scale, t) {
        let (x, y) = (dot.x.round(), dot.y.round());
        if x < 0.0 || y < 0.0 {
            continue;
        }
        let (col, row) = (x as usize / DOT_COLUMNS, y as usize / DOT_ROWS);
        if col >= cols || row >= rows {
            continue;
        }
        canvas.plot(x, y, dot.glow);
        let brightest = &mut hues[row * cols + col];
        if dot.glow >= brightest.0 {
            *brightest = (dot.glow, dot.hue);
        }
    }
    for row in 0..rows {
        for col in 0..cols {
            let Some((glyph, glow)) = canvas.cell(col, row) else { continue };
            let (Ok(x), Ok(y)) = (u16::try_from(col), u16::try_from(row)) else { continue };
            let color = background.mix(gradient.at(hues[row * cols + col].1), glow);
            buf[(area.x + x, area.y + y)].set_char(glyph).set_fg(color.into());
        }
    }
    let fade = 1.0 - scatter(t.max(0.0) % CYCLE - WAVE / 2.0);
    if fade > 0.0 {
        subtitle(buf, area, scale, title, fade, gradient, background);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: char = '⣿';

    fn picture(t: f32) -> Buffer {
        let area = Rect::new(0, 0, 80, 24);
        let gradient = Gradient { from: Rgb::new(0, 255, 0), to: Rgb::new(0, 0, 255) };
        let mut buf = Buffer::empty(area);
        draw_particles(&mut buf, area, t, gradient, Rgb::BLACK, "c o m m a n d e r");
        buf
    }

    fn full_cells(buf: &Buffer) -> usize {
        buf.content().iter().filter(|cell| cell.symbol().starts_with(FULL)).count()
    }

    fn blocks() -> usize {
        GLITCH_LOGO.iter().flat_map(|row| row.chars()).filter(|&glyph| glyph == BLOCK).count()
    }

    #[test]
    fn dots_stay_home_while_the_logo_holds_and_scatter_after() {
        assert_eq!(scatter(0.0), 0.0);
        assert_eq!(scatter(HOLD - 0.1), 0.0);
        assert_eq!(scatter(HOLD + BURST + 1.0), 1.0);
        assert_eq!(scatter(HOLD + BURST + ADRIFT + RETURN), 0.0);
        assert!((0..160).all(|tenth| (0.0..=1.0).contains(&scatter(tenth as f32 / 10.0))));
    }

    #[test]
    fn the_logo_is_whole_at_rest_and_comes_apart_adrift() {
        let rest = picture(1.0);
        assert_eq!(full_cells(&rest), blocks());
        let text: String = rest.content().iter().map(|cell| cell.symbol()).collect();
        assert!(text.contains("c o m m a n d e r"));
        let adrift = picture(HOLD + BURST + 2.0);
        assert!(full_cells(&adrift) < blocks() / 4);
        assert_eq!(picture(9.3), picture(9.3));
        assert_eq!(full_cells(&picture(CYCLE + 1.0)), blocks());
    }

    fn gap(a: f32, b: f32) -> f32 {
        let turn = (a - b).rem_euclid(TAU);
        turn.min(TAU - turn)
    }

    #[test]
    fn a_dot_turns_one_way_and_lands_where_it_started() {
        let (home, galaxy, spin) = (1.0, 4.0, 0.3);
        let steps: Vec<f32> = (0..=160).map(|tenth| angle(tenth as f32 / 10.0, home, galaxy, spin)).collect();
        assert!(steps.windows(2).all(|pair| (pair[1] - pair[0]).rem_euclid(TAU) < 1.0));
        let last = HOLD + BURST + ADRIFT + RETURN;
        assert!(gap(angle(last - 1e-3, home, galaxy, spin), home) < 0.01);
        assert!(
            gap(angle(HOLD + BURST + 1e-3, home, galaxy, spin), angle(HOLD + BURST - 1e-3, home, galaxy, spin)) < 0.01
        );
        assert!(gap(angle(HOLD + BURST + 1.0, home, galaxy, spin), galaxy + spin * (BURST + 1.0)) < 1e-3);
    }
}
