use ratatui::{
    buffer::{Buffer, Cell},
    layout::Rect,
};

use crate::{Palette, Rgb};

const SHADOW_STRENGTH: f32 = 0.35;
const SHADOW_WIDTH: u16 = 2;
const BOX_DRAWING: std::ops::RangeInclusive<char> = '\u{2500}'..='\u{257f}';

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gradient {
    pub from: Rgb,
    pub to: Rgb,
}

impl Gradient {
    pub fn at(self, t: f32) -> Rgb {
        self.from.mix(self.to, t)
    }

    pub fn brightened(self, factor: f32) -> Self {
        let scale = |c: Rgb| if factor >= 1.0 { c.mix(Rgb::WHITE, factor - 1.0) } else { Rgb::BLACK.mix(c, factor) };
        Self { from: scale(self.from), to: scale(self.to) }
    }

    pub fn dimmed(self, background: Rgb, visibility: f32) -> Self {
        Self { from: background.mix(self.from, visibility), to: background.mix(self.to, visibility) }
    }

    pub fn reversed(self) -> Self {
        Self { from: self.to, to: self.from }
    }
}

fn resolved(cell: &Cell, palette: &Palette) -> (Rgb, Rgb) {
    (palette.foreground_of(cell.fg), palette.background_of(cell.bg))
}

fn cells(area: Rect, rect: Rect) -> impl Iterator<Item = (u16, u16)> {
    let rect = rect.intersection(area);
    (rect.top()..rect.bottom()).flat_map(move |y| (rect.left()..rect.right()).map(move |x| (x, y)))
}

fn is_blank(cell: &Cell) -> bool {
    cell.symbol().trim().is_empty()
}

pub fn fade_to_background(buf: &mut Buffer, rect: Rect, amount: f32, palette: &Palette) {
    for (x, y) in cells(buf.area, rect) {
        let cell = &mut buf[(x, y)];
        let (fg, bg) = resolved(cell, palette);
        cell.fg = fg.mix(bg, amount).into();
    }
}

pub fn darken(buf: &mut Buffer, rect: Rect, amount: f32, palette: &Palette) {
    for (x, y) in cells(buf.area, rect) {
        let cell = &mut buf[(x, y)];
        let (fg, bg) = resolved(cell, palette);
        cell.fg = fg.mix(Rgb::BLACK, amount).into();
        cell.bg = bg.mix(Rgb::BLACK, amount).into();
    }
}

pub fn shadow(buf: &mut Buffer, rect: Rect, palette: &Palette) {
    let right = Rect { x: rect.right(), y: rect.y + 1, width: SHADOW_WIDTH, height: rect.height };
    let bottom = Rect { x: rect.x + SHADOW_WIDTH, y: rect.bottom(), width: rect.width, height: 1 };
    darken(buf, right, SHADOW_STRENGTH, palette);
    darken(buf, bottom, SHADOW_STRENGTH, palette);
}

pub fn gradient_text(buf: &mut Buffer, rect: Rect, gradient: Gradient) {
    let span = f32::from(rect.width.saturating_sub(1).max(1));
    for (x, y) in cells(buf.area, rect) {
        let cell = &mut buf[(x, y)];
        if !is_blank(cell) {
            cell.fg = gradient.at(f32::from(x - rect.x) / span).into();
        }
    }
}

pub fn gradient_fill(buf: &mut Buffer, rect: Rect, gradient: Gradient) {
    let span = f32::from(rect.width.saturating_sub(1).max(1));
    for (x, y) in cells(buf.area, rect) {
        buf[(x, y)].bg = gradient.at(f32::from(x - rect.x) / span).into();
    }
}

pub fn gradient_border(buf: &mut Buffer, rect: Rect, gradient: Gradient) {
    let perimeter = perimeter(rect);
    let total = perimeter.len().saturating_sub(1).max(1) as f32;
    for (index, (x, y)) in perimeter.into_iter().enumerate() {
        if !buf.area.contains((x, y).into()) {
            continue;
        }
        let cell = &mut buf[(x, y)];
        if !cell.symbol().chars().all(|c| BOX_DRAWING.contains(&c)) {
            continue;
        }
        cell.fg = gradient.at(index as f32 / total).into();
    }
}

pub fn erase_border(buf: &mut Buffer, rect: Rect) {
    for (x, y) in perimeter(rect) {
        if buf.area.contains((x, y).into()) && buf[(x, y)].symbol().chars().all(|c| BOX_DRAWING.contains(&c)) {
            buf[(x, y)].set_symbol(" ");
        }
    }
}

pub fn glow(buf: &mut Buffer, rect: Rect, color: Rgb, strength: f32, palette: &Palette) {
    for (x, y) in cells(buf.area, rect) {
        let cell = &mut buf[(x, y)];
        let (fg, bg) = resolved(cell, palette);
        cell.bg = bg.mix(color, strength).into();
        cell.fg = fg.into();
    }
}

fn perimeter(rect: Rect) -> Vec<(u16, u16)> {
    if rect.width == 0 || rect.height == 0 {
        return Vec::new();
    }
    let (left, right, top, bottom) = (rect.left(), rect.right() - 1, rect.top(), rect.bottom() - 1);
    let top_edge = (left..=right).map(|x| (x, top));
    let right_edge = (top + 1..=bottom).map(|y| (right, y));
    let bottom_edge = (left..right).rev().map(|x| (x, bottom));
    let left_edge = (top + 1..bottom).rev().map(|y| (left, y));
    top_edge.chain(right_edge).chain(bottom_edge).chain(left_edge).collect()
}

#[cfg(test)]
mod tests {
    use ratatui::style::Color;

    use super::*;

    const RED: Rgb = Rgb::new(255, 0, 0);
    const BLUE: Rgb = Rgb::new(0, 0, 255);

    fn filled(width: u16, height: u16, symbol: &str) -> Buffer {
        let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
        for y in 0..height {
            for x in 0..width {
                buf[(x, y)].set_symbol(symbol).set_fg(Color::Rgb(200, 200, 200)).set_bg(Color::Rgb(100, 100, 100));
            }
        }
        buf
    }

    #[test]
    fn gradient_text_spans_from_first_to_last_glyph() {
        let mut buf = filled(5, 1, "x");
        let area = buf.area;
        gradient_text(&mut buf, area, Gradient { from: RED, to: BLUE });
        assert_eq!(buf[(0, 0)].fg, Color::Rgb(255, 0, 0));
        assert_eq!(buf[(4, 0)].fg, Color::Rgb(0, 0, 255));
    }

    #[test]
    fn border_gradient_walks_the_perimeter() {
        let mut buf = filled(4, 3, "─");
        let area = buf.area;
        gradient_border(&mut buf, area, Gradient { from: RED, to: BLUE });
        assert_eq!(buf[(0, 0)].fg, Color::Rgb(255, 0, 0));
        assert_eq!(buf[(0, 1)].fg, Color::Rgb(0, 0, 255));
        assert_eq!(perimeter(Rect::new(0, 0, 4, 3)).len(), 10);

        let mut titled = filled(4, 1, "T");
        let area = titled.area;
        gradient_border(&mut titled, area, Gradient { from: RED, to: BLUE });
        assert_eq!(titled[(0, 0)].fg, Color::Rgb(200, 200, 200));
    }

    #[test]
    fn shadow_darkens_only_outside_right_and_bottom() {
        let palette = Palette::default();
        let mut buf = filled(10, 6, " ");
        shadow(&mut buf, Rect::new(1, 1, 4, 3), &palette);
        assert_eq!(buf[(1, 1)].bg, Color::Rgb(100, 100, 100));
        assert_ne!(buf[(5, 2)].bg, Color::Rgb(100, 100, 100));
        assert_ne!(buf[(4, 4)].bg, Color::Rgb(100, 100, 100));
    }

    #[test]
    fn fading_moves_text_toward_background() {
        let palette = Palette::default();
        let mut buf = filled(1, 1, "x");
        let area = buf.area;
        fade_to_background(&mut buf, area, 0.5, &palette);
        assert_eq!(buf[(0, 0)].fg, Color::Rgb(150, 150, 150));
    }
}
