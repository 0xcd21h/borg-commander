use ratatui::layout::Rect;

use crate::CellSize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Fit {
    pub area: Rect,
    pub offset: (u16, u16),
    pub size: (u32, u32),
}

fn axis(start: u16, cells: u16, cell: u16, pixels: u32) -> (u16, u16, u16) {
    let room = u32::from(cells) * u32::from(cell);
    let before = room.saturating_sub(pixels) / 2;
    let cell = u32::from(cell.max(1));
    let first = start + u16::try_from(before / cell).unwrap_or(0);
    let offset = u16::try_from(before % cell).unwrap_or(0);
    let span = (u32::from(offset) + pixels).div_ceil(cell);
    (first, offset, u16::try_from(span).unwrap_or(cells).min(cells))
}

pub fn fit(width: u32, height: u32, area: Rect, cell: CellSize) -> Fit {
    let room_w = f64::from(area.width) * f64::from(cell.width);
    let room_h = f64::from(area.height) * f64::from(cell.height);
    let scale = (room_w / f64::from(width.max(1))).min(room_h / f64::from(height.max(1))).min(1.0);
    let size =
        (((f64::from(width) * scale).round() as u32).max(1), ((f64::from(height) * scale).round() as u32).max(1));
    let (x, offset_x, cols) = axis(area.x, area.width, cell.width, size.0);
    let (y, offset_y, rows) = axis(area.y, area.height, cell.height, size.1);
    Fit { area: Rect { x, y, width: cols, height: rows }, offset: (offset_x, offset_y), size }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CELL: CellSize = CellSize { width: 10, height: 20 };

    #[test]
    fn large_pictures_shrink_to_fit_and_center() {
        let fit = fit(2000, 1000, Rect::new(0, 0, 80, 20), CELL);
        assert_eq!(fit.size, (800, 400));
        assert_eq!(fit.area, Rect::new(0, 0, 80, 20));

        let tall = super::fit(100, 400, Rect::new(5, 2, 80, 20), CELL);
        assert_eq!(tall.size, (100, 400));
        assert_eq!((tall.area.x, tall.offset.0, tall.area.width), (5 + 35, 0, 10));
    }

    #[test]
    fn small_pictures_keep_their_size_and_sub_cell_offset() {
        let fit = fit(15, 15, Rect::new(0, 0, 4, 2), CELL);
        assert_eq!(fit.size, (15, 15));
        assert_eq!((fit.area.x, fit.offset.0, fit.area.width), (1, 2, 2));
        assert_eq!((fit.area.y, fit.offset.1, fit.area.height), (0, 12, 2));
    }
}
