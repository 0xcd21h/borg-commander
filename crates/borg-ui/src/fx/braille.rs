const BRAILLE_BASE: u32 = 0x2800;
const DOT_COLUMNS: usize = 2;
const DOT_ROWS: usize = 4;
const DOT_BITS: [[u8; DOT_ROWS]; DOT_COLUMNS] = [[0x01, 0x02, 0x04, 0x40], [0x08, 0x10, 0x20, 0x80]];

#[derive(Debug, Clone)]
pub struct Canvas {
    cols: usize,
    rows: usize,
    dots: Vec<f32>,
}

impl Canvas {
    pub fn new(cols: usize, rows: usize) -> Self {
        Self { cols, rows, dots: vec![0.0; cols * DOT_COLUMNS * rows * DOT_ROWS] }
    }

    pub fn width(&self) -> usize {
        self.cols * DOT_COLUMNS
    }

    pub fn height(&self) -> usize {
        self.rows * DOT_ROWS
    }

    pub fn plot(&mut self, x: f32, y: f32, intensity: f32) {
        let (Ok(x), Ok(y)) = (usize::try_from(x.round() as i64), usize::try_from(y.round() as i64)) else { return };
        let width = self.width();
        if x < width && y < self.height() {
            let dot = &mut self.dots[y * width + x];
            *dot = dot.max(intensity);
        }
    }

    pub fn line(&mut self, from: (f32, f32, f32), to: (f32, f32, f32)) {
        let steps = (to.0 - from.0).abs().max((to.1 - from.1).abs()).ceil().max(1.0);
        let count = steps as usize;
        (0..=count).for_each(|i| {
            let t = i as f32 / steps;
            self.plot(from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t, from.2 + (to.2 - from.2) * t);
        });
    }

    pub fn cell(&self, col: usize, row: usize) -> Option<(char, f32)> {
        let mut bits = 0u32;
        let mut brightest = 0.0f32;
        for (dx, column) in DOT_BITS.iter().enumerate() {
            for (dy, bit) in column.iter().enumerate() {
                let value = self.dots[(row * DOT_ROWS + dy) * self.width() + col * DOT_COLUMNS + dx];
                if value > 0.0 {
                    bits |= u32::from(*bit);
                    brightest = brightest.max(value);
                }
            }
        }
        (bits != 0).then(|| (char::from_u32(BRAILLE_BASE + bits).unwrap_or(' '), brightest))
    }

    pub fn cols(&self) -> usize {
        self.cols
    }

    pub fn rows(&self) -> usize {
        self.rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_dots_to_braille_bits() {
        let mut canvas = Canvas::new(1, 1);
        canvas.plot(0.0, 0.0, 1.0);
        canvas.plot(1.0, 3.0, 0.5);
        assert_eq!(canvas.cell(0, 0), Some(('\u{2881}', 1.0)));
    }

    #[test]
    fn lines_cover_every_step_and_ignore_outside_points() {
        let mut canvas = Canvas::new(4, 1);
        canvas.line((0.0, 0.0, 1.0), (7.0, 0.0, 1.0));
        canvas.plot(-3.0, 99.0, 1.0);
        assert!((0..4).all(|col| canvas.cell(col, 0).is_some()));
    }
}
