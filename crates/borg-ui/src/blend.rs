use ratatui::{
    buffer::{Buffer, Cell},
    style::Modifier,
};

use crate::{Palette, Rgb};

const GLYPH_SWAP: f32 = 0.5;

struct Resolved<'a> {
    cell: &'a Cell,
    fg: Rgb,
    bg: Rgb,
}

fn resolve<'a>(cell: &'a Cell, palette: &Palette) -> Resolved<'a> {
    let fg = palette.foreground_of(cell.fg);
    let bg = palette.background_of(cell.bg);
    if cell.modifier.contains(Modifier::REVERSED) {
        Resolved { cell, fg: bg, bg: fg }
    } else {
        Resolved { cell, fg, bg }
    }
}

fn is_blank(cell: &Cell) -> bool {
    cell.symbol().trim().is_empty()
}

pub fn crossfade(top: &Buffer, bottom: &Buffer, opacity: f32, palette: &Palette, out: &mut Buffer) {
    let pairs = top.content.iter().zip(&bottom.content).zip(out.content.iter_mut());
    for ((upper, lower), target) in pairs {
        let upper = resolve(upper, palette);
        let lower = resolve(lower, palette);
        let bg = lower.bg.mix(upper.bg, opacity);
        let (glyph, strength) = match (is_blank(upper.cell), is_blank(lower.cell)) {
            (false, true) => (&upper, opacity),
            (true, false) => (&lower, 1.0 - opacity),
            _ if opacity >= GLYPH_SWAP => (&upper, (opacity - GLYPH_SWAP) / GLYPH_SWAP),
            _ => (&lower, (GLYPH_SWAP - opacity) / GLYPH_SWAP),
        };

        target.set_symbol(glyph.cell.symbol());
        target.fg = bg.mix(glyph.fg, strength).into();
        target.bg = bg.into();
        target.modifier = glyph.cell.modifier - Modifier::REVERSED;
    }
}

#[cfg(test)]
mod tests {
    use ratatui::{layout::Rect, style::Color};

    use super::*;

    fn single(symbol: &str, fg: Color, bg: Color) -> Buffer {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 1, 1));
        buffer[(0, 0)].set_symbol(symbol).set_fg(fg).set_bg(bg);
        buffer
    }

    #[test]
    fn shows_top_glyph_when_mostly_opaque() {
        let palette = Palette::default();
        let top = single("P", Color::Rgb(255, 255, 255), Color::Rgb(0, 0, 100));
        let bottom = single("T", Color::Rgb(0, 255, 0), Color::Rgb(0, 0, 0));
        let mut out = Buffer::empty(top.area);

        crossfade(&top, &bottom, 1.0, &palette, &mut out);
        assert_eq!(out[(0, 0)].symbol(), "P");
        assert_eq!(out[(0, 0)].bg, Color::Rgb(0, 0, 100));

        crossfade(&top, &bottom, 0.0, &palette, &mut out);
        assert_eq!(out[(0, 0)].symbol(), "T");
        assert_eq!(out[(0, 0)].fg, Color::Rgb(0, 255, 0));
    }

    #[test]
    fn lone_glyphs_fade_linearly_so_both_layers_show_mid_fade() {
        let palette = Palette::default();
        let white = Color::Rgb(255, 255, 255);
        let black = Color::Rgb(0, 0, 0);
        let mut out = Buffer::empty(Rect::new(0, 0, 1, 1));

        crossfade(&single(" ", white, black), &single("T", white, black), GLYPH_SWAP, &palette, &mut out);
        assert_eq!(out[(0, 0)].symbol(), "T");
        assert_eq!(out[(0, 0)].fg, Color::Rgb(128, 128, 128));

        crossfade(&single("P", white, black), &single(" ", white, black), GLYPH_SWAP, &palette, &mut out);
        assert_eq!(out[(0, 0)].symbol(), "P");
        assert_eq!(out[(0, 0)].fg, Color::Rgb(128, 128, 128));
    }

    #[test]
    fn glyphs_dissolve_into_background_mid_fade() {
        let palette = Palette::default();
        let top = single("P", Color::Rgb(255, 255, 255), Color::Rgb(0, 0, 0));
        let bottom = single("T", Color::Rgb(255, 255, 255), Color::Rgb(0, 0, 0));
        let mut out = Buffer::empty(top.area);

        crossfade(&top, &bottom, GLYPH_SWAP, &palette, &mut out);
        assert_eq!(out[(0, 0)].fg, Color::Rgb(0, 0, 0));
    }
}
