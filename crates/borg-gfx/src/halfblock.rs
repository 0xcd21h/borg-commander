use ratatui::{buffer::Buffer, layout::Rect, style::Color};

use crate::{CellSize, Picture, fit};

const HALF: CellSize = CellSize { width: 1, height: 2 };
const UPPER: char = '▀';
const LOWER: char = '▄';
const OPAQUE: u8 = 128;
const SAMPLES: u32 = 3;

fn sample(picture: &Picture, x: u32, y: u32, size: (u32, u32)) -> Option<Color> {
    let (w, h) = (picture.width(), picture.height());
    let (x0, x1) = (x * w / size.0, ((x + 1) * w / size.0).max(x * w / size.0 + 1));
    let (y0, y1) = (y * h / size.1, ((y + 1) * h / size.1).max(y * h / size.1 + 1));
    let step = |from: u32, to: u32| ((to - from) / SAMPLES).max(1);
    let points: Vec<[u8; 4]> = (y0..y1.min(h))
        .step_by(usize::try_from(step(y0, y1)).unwrap_or(1))
        .flat_map(|py| (x0..x1.min(w)).step_by(usize::try_from(step(x0, x1)).unwrap_or(1)).map(move |px| (px, py)))
        .map(|(px, py)| picture.pixel(px, py))
        .collect();
    let count = u32::try_from(points.len()).ok().filter(|n| *n > 0)?;
    let mean = |channel: usize| points.iter().map(|p| u32::from(p[channel])).sum::<u32>() / count;
    let channel = |index: usize| u8::try_from(mean(index)).unwrap_or(u8::MAX);
    (channel(3) >= OPAQUE).then(|| Color::Rgb(channel(0), channel(1), channel(2)))
}

pub fn draw_halfblocks(picture: &Picture, area: Rect, buf: &mut Buffer) {
    let fit = fit(picture.width(), picture.height(), area, HALF);
    let origin = (u32::from(fit.area.x) + u32::from(fit.offset.0), u32::from(fit.area.y) * 2 + u32::from(fit.offset.1));
    let at = |sx: u32, sy: u32| {
        let inside = sx >= origin.0 && sy >= origin.1 && sx < origin.0 + fit.size.0 && sy < origin.1 + fit.size.1;
        inside.then(|| sample(picture, sx - origin.0, sy - origin.1, fit.size)).flatten()
    };
    let area = fit.area.intersection(buf.area);
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            let (top, bottom) = (at(u32::from(x), u32::from(y) * 2), at(u32::from(x), u32::from(y) * 2 + 1));
            let cell = &mut buf[(x, y)];
            match (top, bottom) {
                (Some(top), Some(bottom)) => cell.set_char(UPPER).set_fg(top).set_bg(bottom),
                (Some(top), None) => cell.set_char(UPPER).set_fg(top),
                (None, Some(bottom)) => cell.set_char(LOWER).set_fg(bottom),
                (None, None) => cell,
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_pixels_per_cell_vertically() {
        let rgba = [[255, 0, 0, 255], [0, 0, 255, 255]].concat();
        let picture = Picture::from_rgba(1, 2, rgba).unwrap_or_else(|e| panic!("{e}"));
        let mut buf = Buffer::empty(Rect::new(0, 0, 3, 1));
        draw_halfblocks(&picture, buf.area, &mut buf);
        let cell = &buf[(1, 0)];
        assert_eq!(cell.symbol(), "▀");
        assert_eq!((cell.fg, cell.bg), (Color::Rgb(255, 0, 0), Color::Rgb(0, 0, 255)));
        assert_eq!(buf[(0, 0)].symbol(), " ");
    }
}
