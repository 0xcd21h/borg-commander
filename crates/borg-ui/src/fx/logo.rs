use ratatui::buffer::Buffer;

use super::Gradient;
use crate::Rgb;

pub const LOGO: [&str; 3] = ["█▀▀▄ █▀▀█ █▀▀█ █▀▀▀", "█▀▀▄ █  █ █▄▄▀ █ ▀█", "▀▀▀  ▀▀▀▀ ▀ ▀▀ ▀▀▀▀"];
const SHINE_PERIOD: f32 = 2.6;
const SHINE_WIDTH: f32 = 3.0;
const SHINE_STRENGTH: f32 = 0.6;

pub fn logo_width() -> u16 {
    LOGO.iter().map(|row| u16::try_from(row.chars().count()).unwrap_or(0)).max().unwrap_or(0)
}

pub fn draw_logo(buf: &mut Buffer, origin: (u16, u16), t: f32, gradient: Gradient) {
    let width = f32::from(logo_width().max(1));
    let shine = (t / SHINE_PERIOD).fract() * (width + SHINE_WIDTH * 2.0) - SHINE_WIDTH;
    for (dy, row) in (0u16..).zip(LOGO) {
        for (dx, glyph) in (0u16..).zip(row.chars()) {
            let position = (origin.0 + dx, origin.1 + dy);
            if glyph == ' ' || !buf.area.contains(position.into()) {
                continue;
            }
            let base = gradient.at(f32::from(dx) / width);
            let near = (1.0 - (f32::from(dx) - shine).abs() / SHINE_WIDTH).max(0.0);
            buf[position].set_char(glyph).set_fg(base.mix(Rgb::WHITE, near * SHINE_STRENGTH).into());
        }
    }
}
