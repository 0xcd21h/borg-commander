use ratatui::{buffer::Buffer, layout::Rect};

use super::{
    Gradient,
    noise::{hash, unit},
};
use crate::Rgb;

const GLYPHS: &[char] = &[
    'ｱ', 'ｲ', 'ｳ', 'ｴ', 'ｵ', 'ｶ', 'ｷ', 'ｸ', 'ｹ', 'ｺ', 'ｻ', 'ｼ', 'ｽ', 'ｾ', 'ｿ', '0', '1', '2', '3', '4', '5', '7', '8',
    '9', 'Z', ':', '=', '*', '+', '<', '>', '|',
];
const MIN_SPEED: f32 = 5.0;
const SPEED_RANGE: f32 = 14.0;
const MIN_LENGTH: f32 = 4.0;
const LENGTH_RANGE: f32 = 12.0;
const FLICKER_RATE: f32 = 9.0;
const HEAD: Rgb = Rgb::new(230, 255, 235);

pub fn draw_rain(buf: &mut Buffer, area: Rect, t: f32, gradient: Gradient, background: Rgb) {
    let height = f32::from(area.height);
    for column in 0..area.width {
        let seed = u64::from(column);
        let speed = MIN_SPEED + unit(seed * 5) * SPEED_RANGE;
        let length = MIN_LENGTH + unit(seed * 5 + 1) * LENGTH_RANGE;
        let cycle = height + length;
        let head = (t * speed + unit(seed * 5 + 2) * cycle) % cycle;
        let tick = (t * FLICKER_RATE) as u64;

        for row in 0..area.height {
            let behind = head - f32::from(row);
            if !(0.0..length).contains(&behind) {
                continue;
            }
            let fade = 1.0 - behind / length;
            let glyph = GLYPHS[(hash(seed << 32 | u64::from(row) << 8 | tick) as usize) % GLYPHS.len()];
            let color = if behind < 1.0 { HEAD } else { background.mix(gradient.at(fade), fade) };
            buf[(area.x + column, area.y + row)].set_char(glyph).set_fg(color.into());
        }
    }
}
