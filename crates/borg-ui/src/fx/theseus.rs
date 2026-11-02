//! The Theseus screensaver: the logo's planks are replaced one at a time. Each old plank drops
//! to the floor below, where the old planks come together as a second logo. The picture tears
//! in from noise, glitches in bursts and with every swap, flares when the last original plank
//! goes, and tears apart as it fades.

use ratatui::{buffer::Buffer, layout::Rect};

use super::{
    Gradient,
    glitch::{
        GLITCH_HEIGHT, GLITCH_LOGO, GLITCH_WIDTH, Row, beam_row, burst_heat, draw_static, frame_at, glitch_row,
        settling_heat,
    },
    noise::unit,
};
use crate::Rgb;

const BLOCK: char = '█';
/// A new plank has a seam on its right, so the rebuilt logo shows its joints.
const NEW_PLANK: char = '▉';
const UPPER_HALF: char = '▀';
const LOWER_HALF: char = '▄';
const FLOOR_GAP: u16 = 3;
/// Rows from a plank in the first logo down to its place in the second.
const DROP: u16 = GLITCH_HEIGHT + FLOOR_GAP;
const HEIGHT: u16 = DROP + GLITCH_HEIGHT;
const HALVES: f32 = 2.0;

const FADE_IN: f32 = 1.5;
/// The first logo stands untouched this long before the first plank goes.
const HOLD: f32 = 4.0;
/// Every plank is swapped at its own moment within this many seconds.
const REPLACE: f32 = 40.0;
/// A plank's place stays empty this long before the new one goes in.
const REFIT: f32 = 0.6;
const FALL: f32 = 1.4;
/// Both logos stand finished this long before the picture fades and starts again.
const REST: f32 = 9.0;
const FADE_OUT: f32 = 3.0;
const CYCLE: f32 = HOLD + REPLACE + FALL + REST + FADE_OUT;
const SHINE: f32 = 0.8;
const SHINE_STRENGTH: f32 = 0.7;
const LANDING: f32 = 0.35;
const LANDING_STRENGTH: f32 = 0.45;
const SALT: u64 = 0x7e5e;

const BURST_PERIOD: f32 = 6.0;
const BURST_STRENGTH: f32 = 0.55;
/// A row twitches for this long when one of its planks is taken out or lands.
const JOLT: f32 = 0.3;
const SWAP_JOLT: f32 = 0.45;
const LANDING_JOLT: f32 = 0.25;
/// The whole picture flares for this long once the last original plank is gone.
const LAST_FLARE: f32 = 1.4;
const LEAVING_CURVE: i32 = 2;
/// Keeps the second logo's rows from tearing in step with the first's.
const FLOOR_ROWS: u64 = 8;

fn planks() -> Vec<(u16, u16)> {
    (0u16..)
        .zip(GLITCH_LOGO)
        .flat_map(|(row, line)| {
            (0u16..).zip(line.chars()).filter(|(_, glyph)| *glyph == BLOCK).map(move |(col, _)| (col, row))
        })
        .collect()
}

/// When, in seconds into the cycle, the plank with this index is taken out.
fn taken_at(index: usize) -> f32 {
    HOLD + unit(index as u64 ^ SALT) * REPLACE
}

fn put(buf: &mut Buffer, area: Rect, (x, y): (u16, u16), glyph: char, color: Rgb) {
    if area.contains((x, y).into()) {
        buf[(x, y)].set_char(glyph).set_fg(color.into());
    }
}

/// Draws a falling plank `half_rows` half rows below the top of `x`'s column, across two
/// cells when it is between rows.
fn falling(buf: &mut Buffer, area: Rect, x: u16, top: u16, half_rows: u16, color: Rgb) {
    let y = top + half_rows / 2;
    if half_rows.is_multiple_of(2) {
        put(buf, area, (x, y), BLOCK, color);
    } else {
        put(buf, area, (x, y), LOWER_HALF, color);
        put(buf, area, (x, y + 1), UPPER_HALF, color);
    }
}

fn jolt(since: f32, strength: f32) -> f32 {
    if (0.0..JOLT).contains(&since) { strength * (1.0 - since / JOLT).powi(2) } else { 0.0 }
}

/// How glitched the whole picture is `moment` seconds into the cycle, when the last original
/// plank goes at `last`: it tears in from noise, bursts now and then, flares as the last plank
/// goes, and tears apart as it fades.
fn picture_heat(moment: f32, last: f32) -> f32 {
    let arriving = settling_heat(moment, FADE_IN);
    let leaving = ((moment - (CYCLE - FADE_OUT)) / FADE_OUT).clamp(0.0, 1.0).powi(LEAVING_CURVE);
    let since_last = moment - last;
    let flare = if since_last >= 0.0 { settling_heat(since_last, LAST_FLARE) } else { 0.0 };
    let burst = BURST_STRENGTH * burst_heat(moment, BURST_PERIOD, 0.0);
    arriving.max(leaving).max(flare).max(burst)
}

fn shine(color: Rgb, age: f32, length: f32, strength: f32) -> Rgb {
    let left = (1.0 - age / length).clamp(0.0, 1.0);
    color.mix(Rgb::WHITE, left * left * strength)
}

/// Draws the screensaver `elapsed` seconds after it started.
pub fn draw_theseus(buf: &mut Buffer, area: Rect, elapsed: f32, gradient: Gradient, background: Rgb) {
    if area.is_empty() {
        return;
    }
    let moment = elapsed.max(0.0) % CYCLE;
    let shown = (moment / FADE_IN).min((CYCLE - moment) / FADE_OUT).clamp(0.0, 1.0);
    let ink = |color: Rgb| background.mix(color, shown);
    let left = area.x + area.width.saturating_sub(GLITCH_WIDTH) / 2;
    let top = area.y + area.height.saturating_sub(HEIGHT) / 2;
    let span = f32::from(GLITCH_WIDTH - 1);
    let planks: Vec<((u16, u16), f32)> =
        planks().into_iter().enumerate().map(|(index, plank)| (plank, moment - taken_at(index))).collect();
    let last = (0..planks.len()).map(taken_at).fold(HOLD, f32::max);
    let old = |col: u16| gradient.at(f32::from(col) / span);
    let new = |col: u16| gradient.at(1.0 - f32::from(col) / span);
    let heat = picture_heat(moment, last);
    draw_static(buf, area, heat, elapsed, background);

    for &((col, row), since) in &planks {
        if (0.0..FALL).contains(&since) {
            let drop = (since / FALL).powi(2) * f32::from(DROP) * HALVES;
            let half_rows = row * 2 + drop.round() as u16;
            falling(buf, area, left + col, top, half_rows, ink(old(col)));
        }
    }

    let blank: Vec<(char, Rgb)> = (0..GLITCH_WIDTH).map(|col| (' ', ink(old(col)))).collect();
    let mut first = vec![blank.clone(); usize::from(GLITCH_HEIGHT)];
    let mut second = vec![blank; usize::from(GLITCH_HEIGHT)];
    let mut twitch = [[0.0f32; 2]; GLITCH_HEIGHT as usize];
    for &((col, row), since) in &planks {
        let (c, r) = (usize::from(col), usize::from(row));
        if since < 0.0 {
            first[r][c] = (BLOCK, ink(old(col)));
        } else if since >= REFIT {
            first[r][c] = (NEW_PLANK, ink(shine(new(col), since - REFIT, SHINE, SHINE_STRENGTH)));
        }
        if since >= FALL {
            second[r][c] = (BLOCK, ink(shine(old(col), since - FALL, LANDING, LANDING_STRENGTH)));
        }
        twitch[r][0] = twitch[r][0].max(jolt(since, SWAP_JOLT));
        twitch[r][1] = twitch[r][1].max(jolt(since - FALL, LANDING_JOLT));
    }

    let frame = frame_at(elapsed);
    let beam = beam_row(elapsed, heat, GLITCH_HEIGHT);
    for (row, ((first, second), twitch)) in (0u16..).zip(first.iter().zip(&second).zip(twitch)) {
        let lit = beam == Some(row);
        let id = u64::from(row);
        let upper = Row { cells: first, width: GLITCH_WIDTH, origin: (left, top + row), id, lit };
        glitch_row(buf, area, upper, heat.max(twitch[0]), frame, background);
        let lower =
            Row { cells: second, width: GLITCH_WIDTH, origin: (left, top + DROP + row), id: id + FLOOR_ROWS, lit };
        glitch_row(buf, area, lower, heat.max(twitch[1]), frame, background);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: Rect = Rect::new(0, 0, 60, 20);

    fn picture(elapsed: f32) -> Vec<String> {
        let gradient = Gradient { from: Rgb::new(0, 255, 0), to: Rgb::new(0, 0, 255) };
        let mut buf = Buffer::empty(AREA);
        draw_theseus(&mut buf, AREA, elapsed, gradient, Rgb::BLACK);
        (AREA.top()..AREA.bottom())
            .map(|y| (AREA.left()..AREA.right()).map(|x| buf[(x, y)].symbol()).collect())
            .collect()
    }

    fn count(rows: &[String], glyph: char) -> usize {
        rows.iter().flat_map(|row| row.chars()).filter(|&c| c == glyph).count()
    }

    #[test]
    fn every_plank_goes_once_within_the_window() {
        let total = planks().len();
        assert_eq!(total, GLITCH_LOGO.iter().flat_map(|row| row.chars()).filter(|&c| c == BLOCK).count());
        assert!((0..total).map(taken_at).all(|at| (HOLD..HOLD + REPLACE).contains(&at)));
    }

    fn last() -> f32 {
        (0..planks().len()).map(taken_at).fold(HOLD, f32::max)
    }

    /// The first moment from `from` on with no glitch over the whole picture and no plank in
    /// motion, so every cell shows as it is.
    fn calm_from(from: f32) -> f32 {
        let still = |moment: f32| {
            picture_heat(moment, last()) == 0.0
                && (0..planks().len()).all(|index| !(0.0..FALL + JOLT).contains(&(moment - taken_at(index))))
        };
        (0..).map(|tenth| from + tenth as f32 / 10.0).find(|&moment| still(moment)).unwrap_or(from)
    }

    #[test]
    fn the_first_logo_starts_original_and_ends_with_a_second() {
        let total = planks().len();
        let start = picture(calm_from(FADE_IN));
        assert_eq!(count(&start, BLOCK), total);
        assert_eq!(count(&start, NEW_PLANK), 0);

        let finished = calm_from(last() + FALL + JOLT);
        assert!(finished < CYCLE - FADE_OUT);
        let end = picture(finished);
        assert_eq!(count(&end, NEW_PLANK), total);
        assert_eq!(count(&end, BLOCK), total);
        let top = usize::from(AREA.height.saturating_sub(HEIGHT) / 2);
        let first: Vec<String> =
            end[top..top + usize::from(GLITCH_HEIGHT)].iter().map(|row| row.replace(NEW_PLANK, "█")).collect();
        let floor = top + usize::from(DROP);
        assert_eq!(first, end[floor..floor + usize::from(GLITCH_HEIGHT)]);
        assert_eq!(picture(23.4), picture(23.4));
        assert_eq!(picture(CYCLE + calm_from(FADE_IN)), start);
    }

    #[test]
    fn the_picture_glitches_in_out_and_as_the_last_plank_goes() {
        let noise = |rows: &[String]| {
            rows.iter().flat_map(|row| row.chars()).filter(|c| !matches!(c, ' ' | '█' | '▉' | '▀' | '▄')).count()
        };
        assert!(picture_heat(0.0, last()) > 0.99);
        assert!(picture_heat(CYCLE - 0.01, last()) > 0.99);
        assert!(picture_heat(last() + 0.05, last()) > 0.9);
        assert!(noise(&picture(last() + 0.05)) > 10);
        assert_eq!(noise(&picture(calm_from(FADE_IN))), 0);
    }

    #[test]
    fn a_falling_plank_moves_by_half_rows() {
        let column = Rect::new(0, 0, 1, 4);
        let mut buf = Buffer::empty(column);
        falling(&mut buf, column, 0, 0, 3, Rgb::WHITE);
        let column: String = (0..4).map(|y| buf[(0, y)].symbol().to_owned()).collect();
        assert_eq!(column, format!(" {LOWER_HALF}{UPPER_HALF} "));
    }
}
