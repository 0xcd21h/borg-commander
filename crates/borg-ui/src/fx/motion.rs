use ratatui::{buffer::Buffer, layout::Rect};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unfold {
    Down,
    Up,
    FromCenter,
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

        let mut buf = Buffer::with_lines(["aaaa", "bbbb", "cccc", "dddd"]);
        let shown = reveal(&mut buf, &before, area, 0.5, Unfold::Up);
        assert_eq!(buf, Buffer::with_lines(["....", "....", "cccc", "dddd"]));
        assert_eq!(shown, Rect::new(0, 2, 4, 2));
    }
}
