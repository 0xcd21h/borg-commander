//! The portrait of Sir Patrick Stewart that empty folders show on October 28: a small
//! grayscale picture drawn as braille dots, or as plain characters in the Classic theme.

use std::sync::OnceLock;

use ratatui::{buffer::Buffer, layout::Rect, style::Style};

const PICTURE: &[u8] = include_bytes!("../art/stewart.pgm");
/// From faint to dense, for the plain portrait.
const RAMP: [char; 10] = [' ', '.', ':', '-', '=', '+', '*', '#', '%', '@'];
/// Braille dot bits by row and column within a cell.
const DOTS: [[u8; 2]; 4] = [[0x01, 0x08], [0x02, 0x10], [0x04, 0x20], [0x40, 0x80]];
/// A terminal cell is about twice as tall as wide; a braille cell holds 2 × 4 dots.
const CELL_ASPECT: f32 = 2.0;

struct Gray {
    width: usize,
    height: usize,
    pixels: &'static [u8],
}

impl Gray {
    /// The brightness, from 0 to 1, at a point given as shares of the width and height.
    fn at(&self, x: f32, y: f32) -> f32 {
        let column = ((x * self.width as f32) as usize).min(self.width - 1);
        let row = ((y * self.height as f32) as usize).min(self.height - 1);
        f32::from(self.pixels[row * self.width + column]) / 255.0
    }
}

/// The binary PGM: `P5`, width, height and 255 as decimal words, then one byte per pixel.
fn picture() -> Option<&'static Gray> {
    static PICTURE_GRAY: OnceLock<Option<Gray>> = OnceLock::new();
    PICTURE_GRAY
        .get_or_init(|| {
            let mut words = PICTURE.split(u8::is_ascii_whitespace).filter(|word| !word.is_empty());
            let magic = words.next() == Some(b"P5".as_slice());
            let mut number = || std::str::from_utf8(words.next()?).ok()?.parse::<usize>().ok();
            let (width, height, depth) = (number()?, number()?, number()?);
            let start = PICTURE.len().checked_sub(width * height)?;
            (magic && depth == 255 && width > 0 && height > 0).then(|| Gray {
                width,
                height,
                pixels: &PICTURE[start..],
            })
        })
        .as_ref()
}

/// The largest portrait, in cells, that fits `room` with the picture's proportions.
pub fn size(room: (u16, u16)) -> (u16, u16) {
    let Some(gray) = picture() else { return (0, 0) };
    let ratio = gray.width as f32 / gray.height as f32 * CELL_ASPECT;
    let rows = f32::from(room.1).min(f32::from(room.0) / ratio);
    ((rows * ratio) as u16, rows as u16)
}

/// Draws the portrait filling `area`: braille dots by error diffusion, or plain characters.
pub fn draw(buf: &mut Buffer, area: Rect, style: Style, plain: bool) {
    let Some(gray) = picture() else { return };
    if area.is_empty() {
        return;
    }
    let (cols, rows) = (usize::from(area.width), usize::from(area.height));
    if plain {
        for (row, y) in (0..rows).zip(area.y..) {
            for (column, x) in (0..cols).zip(area.x..) {
                let level = gray.at((column as f32 + 0.5) / cols as f32, (row as f32 + 0.5) / rows as f32);
                let glyph = RAMP[((level * RAMP.len() as f32) as usize).min(RAMP.len() - 1)];
                buf[(x, y)].set_char(glyph).set_style(style);
            }
        }
        return;
    }
    let (width, height) = (cols * 2, rows * 4);
    let mut dots: Vec<f32> = (0..width * height)
        .map(|i| gray.at(((i % width) as f32 + 0.5) / width as f32, ((i / width) as f32 + 0.5) / height as f32))
        .collect();
    for i in 0..dots.len() {
        let lit = if dots[i] > 0.5 { 1.0 } else { 0.0 };
        let error = dots[i] - lit;
        dots[i] = lit;
        let (x, y) = (i % width, i / width);
        for (dx, dy, share) in [(1, 0, 7.0), (-1, 1, 3.0), (0, 1, 5.0), (1, 1, 1.0)] {
            let (nx, ny) = (x as isize + dx, y + dy);
            if (0..width as isize).contains(&nx) && ny < height {
                dots[ny * width + nx as usize] += error * share / 16.0;
            }
        }
    }
    for (row, y) in (0..rows).zip(area.y..) {
        for (column, x) in (0..cols).zip(area.x..) {
            let mut bits = 0;
            for (dy, line) in DOTS.iter().enumerate() {
                for (dx, bit) in line.iter().enumerate() {
                    if dots[(row * 4 + dy) * width + column * 2 + dx] > 0.5 {
                        bits |= bit;
                    }
                }
            }
            let glyph = char::from_u32(0x2800 + u32::from(bits)).unwrap_or(' ');
            buf[(x, y)].set_char(glyph).set_style(style);
        }
    }
}
