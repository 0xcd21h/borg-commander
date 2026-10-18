use ratatui::{
    buffer::{Buffer, Cell},
    layout::Rect,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unfold {
    Down,
    Up,
    FromCenter,
}

pub fn shift(buf: &mut Buffer, rect: Rect, dx: i32, fill: &Cell) {
    let rect = rect.intersection(buf.area);
    if dx == 0 || rect.is_empty() {
        return;
    }
    let width = i32::from(rect.width);
    for y in rect.top()..rect.bottom() {
        let row: Vec<Cell> = (rect.left()..rect.right()).map(|x| buf[(x, y)].clone()).collect();
        for (offset, x) in (rect.left()..rect.right()).enumerate() {
            let source = i32::try_from(offset).unwrap_or(i32::MAX) - dx;
            let cell = usize::try_from(source).ok().filter(|_| source < width).and_then(|i| row.get(i));
            buf[(x, y)] = cell.unwrap_or(fill).clone();
        }
    }
}

pub fn reveal(buf: &mut Buffer, before: &Buffer, rect: Rect, progress: f32, unfold: Unfold) -> Rect {
    let rect = rect.intersection(buf.area);
    let shown = (f32::from(rect.height) * progress.clamp(0.0, 1.0)).ceil() as u16;
    let hidden = rect.height.saturating_sub(shown);
    let (top, bottom) = match unfold {
        Unfold::Down => (rect.top() + shown, rect.bottom()),
        Unfold::Up => (rect.top(), rect.top() + hidden),
        Unfold::FromCenter => (rect.top(), rect.top() + hidden / 2),
    };
    let tail = match unfold {
        Unfold::Down | Unfold::Up => rect.bottom()..rect.bottom(),
        Unfold::FromCenter => rect.bottom() - (hidden - hidden / 2)..rect.bottom(),
    };
    for y in (top..bottom).chain(tail.clone()) {
        for x in rect.left()..rect.right() {
            buf[(x, y)] = before[(x, y)].clone();
        }
    }
    let first = match unfold {
        Unfold::Down => rect.top(),
        Unfold::Up => rect.top() + hidden,
        Unfold::FromCenter => rect.top() + hidden / 2,
    };
    Rect { y: first, height: shown, ..rect }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(text: &str) -> Buffer {
        Buffer::with_lines([text])
    }

    fn text(buf: &Buffer) -> String {
        (0..buf.area.width).map(|x| buf[(x, 0)].symbol().to_owned()).collect()
    }

    #[test]
    fn shifting_moves_content_and_fills_the_gap() {
        let mut buf = line("abcde");
        let area = buf.area;
        let mut fill = Cell::default();
        fill.set_symbol(".");
        shift(&mut buf, area, 2, &fill);
        assert_eq!(text(&buf), "..abc");
        shift(&mut buf, area, -3, &fill);
        assert_eq!(text(&buf), "bc...");
    }

    #[test]
    fn reveal_restores_rows_not_yet_unfolded() {
        let before = Buffer::with_lines(["....", "....", "....", "...."]);
        let mut buf = Buffer::with_lines(["aaaa", "bbbb", "cccc", "dddd"]);
        let area = buf.area;
        let shown = reveal(&mut buf, &before, area, 0.5, Unfold::Down);
        assert_eq!(buf, Buffer::with_lines(["aaaa", "bbbb", "....", "...."]));
        assert_eq!(shown, Rect::new(0, 0, 4, 2));

        let mut buf = Buffer::with_lines(["aaaa", "bbbb", "cccc", "dddd"]);
        let shown = reveal(&mut buf, &before, area, 0.5, Unfold::FromCenter);
        assert_eq!(buf, Buffer::with_lines(["....", "bbbb", "cccc", "...."]));
        assert_eq!(shown, Rect::new(0, 1, 4, 2));
    }
}
