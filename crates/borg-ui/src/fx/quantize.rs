use ratatui::{buffer::Buffer, style::Color};

use crate::{Palette, Rgb};

const CUBE_START: u8 = 16;
const CUBE_SIDE: u8 = 6;
const GREY_START: u8 = 232;

fn distance(a: Rgb, b: Rgb) -> u32 {
    let d = |x: u8, y: u8| u32::from(x.abs_diff(y)).pow(2);
    d(a.r, b.r) + d(a.g, b.g) + d(a.b, b.b)
}

fn nearest(rgb: Rgb, palette: &Palette) -> u8 {
    let level = |channel: u8| -> u8 {
        (0..CUBE_SIDE)
            .min_by_key(|step| palette.indexed(CUBE_START + step * CUBE_SIDE * CUBE_SIDE).r.abs_diff(channel))
            .unwrap_or(0)
    };
    let cube = CUBE_START + level(rgb.r) * CUBE_SIDE * CUBE_SIDE + level(rgb.g) * CUBE_SIDE + level(rgb.b);
    let grey = (GREY_START..=u8::MAX).min_by_key(|i| distance(palette.indexed(*i), rgb)).unwrap_or(GREY_START);
    if distance(palette.indexed(cube), rgb) <= distance(palette.indexed(grey), rgb) { cube } else { grey }
}

fn downsample(color: Color, palette: &Palette) -> Color {
    match color {
        Color::Rgb(r, g, b) => Color::Indexed(nearest(Rgb::new(r, g, b), palette)),
        other => other,
    }
}

pub fn quantize(buf: &mut Buffer, palette: &Palette) {
    for cell in &mut buf.content {
        cell.fg = downsample(cell.fg, palette);
        cell.bg = downsample(cell.bg, palette);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_truecolor_to_the_closest_xterm_index() {
        let palette = Palette::default();
        assert_eq!(nearest(Rgb::new(255, 0, 0), &palette), 196);
        assert_eq!(nearest(Rgb::new(250, 250, 250), &palette), 231);
        assert_eq!(nearest(Rgb::new(120, 120, 122), &palette), 243);
        assert_eq!(downsample(Color::Cyan, &palette), Color::Cyan);
    }
}
