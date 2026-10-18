use alacritty_terminal::grid::Dimensions;
use portable_pty::PtySize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridSize {
    pub cols: u16,
    pub rows: u16,
}

impl Dimensions for GridSize {
    fn total_lines(&self) -> usize {
        self.screen_lines()
    }

    fn screen_lines(&self) -> usize {
        usize::from(self.rows)
    }

    fn columns(&self) -> usize {
        usize::from(self.cols)
    }
}

impl From<GridSize> for PtySize {
    fn from(size: GridSize) -> Self {
        Self { rows: size.rows, cols: size.cols, pixel_width: 0, pixel_height: 0 }
    }
}
