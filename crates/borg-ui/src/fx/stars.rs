use std::f32::consts::TAU;

use ratatui::{buffer::Buffer, layout::Rect};

use super::noise::unit;
use crate::Rgb;

const CELLS_PER_STAR: u32 = 14;
const DRIFT: f32 = 0.22;
const CELL_ASPECT: f32 = 2.0;
const GLYPHS: [char; 4] = ['·', '∙', '•', '✦'];
const STAR: Rgb = Rgb::new(235, 240, 255);
const BORN: f32 = 0.12;

pub fn draw_stars(buf: &mut Buffer, area: Rect, t: f32, background: Rgb) {
    let count = u32::from(area.width) * u32::from(area.height) / CELLS_PER_STAR;
    let (cx, cy) = (f32::from(area.width) / 2.0, f32::from(area.height) / 2.0);
    let reach = cx.max(cy * CELL_ASPECT);

    for star in 0..u64::from(count) {
        let angle = unit(star * 3) * TAU;
        let speed = 0.3 + unit(star * 3 + 1) * 0.7;
        let progress = (t * DRIFT * speed + unit(star * 3 + 2)).fract();
        if progress < BORN {
            continue;
        }
        let radius = progress * progress * reach;
        let x = cx + angle.cos() * radius;
        let y = cy + angle.sin() * radius / CELL_ASPECT;
        if x < 0.0 || y < 0.0 || x >= f32::from(area.width) || y >= f32::from(area.height) {
            continue;
        }
        let glyph = GLYPHS[((progress * GLYPHS.len() as f32) as usize).min(GLYPHS.len() - 1)];
        let (col, row) = (area.x + x as u16, area.y + y as u16);
        buf[(col, row)].set_char(glyph).set_fg(background.mix(STAR, progress.sqrt()).into());
    }
}
