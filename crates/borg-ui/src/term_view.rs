use borg_term::{Cell as TermCell, Color as TermColor, Flags, NamedColor, Screen};
use ratatui::{
    buffer::{Buffer, Cell},
    layout::Rect,
    style::{Color, Modifier},
};

const MODIFIERS: [(Flags, Modifier); 7] = [
    (Flags::BOLD, Modifier::BOLD),
    (Flags::ITALIC, Modifier::ITALIC),
    (Flags::ALL_UNDERLINES, Modifier::UNDERLINED),
    (Flags::INVERSE, Modifier::REVERSED),
    (Flags::DIM, Modifier::DIM),
    (Flags::HIDDEN, Modifier::HIDDEN),
    (Flags::STRIKEOUT, Modifier::CROSSED_OUT),
];

const SPACERS: Flags = Flags::WIDE_CHAR_SPACER.union(Flags::LEADING_WIDE_CHAR_SPACER);
const BLANK: char = ' ';

pub fn draw_screen(screen: &Screen<'_>, area: Rect, buf: &mut Buffer) {
    let clipped = area.intersection(buf.area);
    for y in clipped.top()..clipped.bottom().min(area.y.saturating_add(screen.rows())) {
        for x in clipped.left()..clipped.right().min(area.x.saturating_add(screen.cols())) {
            convert(screen, screen.cell(x - area.x, y - area.y), &mut buf[(x, y)]);
        }
    }
}

pub fn draw_row(screen: &Screen<'_>, source: u16, target: u16, area: Rect, buf: &mut Buffer) {
    if source >= screen.rows() || !area.contains((area.x, target).into()) {
        return;
    }
    for col in 0..area.width.min(screen.cols()) {
        if let Some(out) = buf.cell_mut((area.x + col, target)) {
            convert(screen, screen.live_cell(col, source), out);
        }
    }
}

fn convert(screen: &Screen<'_>, cell: &TermCell, out: &mut Cell) {
    out.reset();
    if cell.flags.intersects(SPACERS) {
        return;
    }

    let mut symbol = String::from(if cell.c == '\0' { BLANK } else { cell.c });
    symbol.extend(cell.zerowidth().into_iter().flatten());
    out.set_symbol(&symbol);
    out.fg = color(screen, cell.fg);
    out.bg = color(screen, cell.bg);
    out.modifier = MODIFIERS
        .iter()
        .filter(|(flag, _)| cell.flags.intersects(*flag))
        .fold(Modifier::empty(), |acc, (_, modifier)| acc | *modifier);
}

fn color(screen: &Screen<'_>, color: TermColor) -> Color {
    let custom = |index: usize| screen.color(index).map(|rgb| Color::Rgb(rgb.r, rgb.g, rgb.b));
    match color {
        TermColor::Spec(rgb) => Color::Rgb(rgb.r, rgb.g, rgb.b),
        TermColor::Indexed(i) => custom(usize::from(i)).unwrap_or(Color::Indexed(i)),
        TermColor::Named(named) => custom(named as usize).unwrap_or_else(|| named_color(named)),
    }
}

fn named_color(named: NamedColor) -> Color {
    let index = match named {
        NamedColor::Black | NamedColor::DimBlack => 0,
        NamedColor::Red | NamedColor::DimRed => 1,
        NamedColor::Green | NamedColor::DimGreen => 2,
        NamedColor::Yellow | NamedColor::DimYellow => 3,
        NamedColor::Blue | NamedColor::DimBlue => 4,
        NamedColor::Magenta | NamedColor::DimMagenta => 5,
        NamedColor::Cyan | NamedColor::DimCyan => 6,
        NamedColor::White | NamedColor::DimWhite => 7,
        NamedColor::BrightBlack => 8,
        NamedColor::BrightRed => 9,
        NamedColor::BrightGreen => 10,
        NamedColor::BrightYellow => 11,
        NamedColor::BrightBlue => 12,
        NamedColor::BrightMagenta => 13,
        NamedColor::BrightCyan => 14,
        NamedColor::BrightWhite => 15,
        NamedColor::Foreground
        | NamedColor::Background
        | NamedColor::Cursor
        | NamedColor::BrightForeground
        | NamedColor::DimForeground => return Color::Reset,
    };
    Color::Indexed(index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use borg_term::{Emulator, GridSize};

    #[test]
    fn stale_terminal_dimensions_are_clipped_to_the_buffer() {
        let mut emulator = Emulator::new(GridSize { cols: 194, rows: 41 }, 0);
        emulator.feed(b"visible");
        let mut buffer = Buffer::empty(Rect::new(0, 0, 193, 40));
        draw_screen(&emulator.screen(), Rect::new(0, 0, 194, 41), &mut buffer);
        draw_row(&emulator.screen(), 38, 38, Rect::new(0, 0, 194, 41), &mut buffer);
        draw_row(&emulator.screen(), 40, 40, Rect::new(0, 0, 194, 41), &mut buffer);
        draw_row(&emulator.screen(), 41, 0, Rect::new(0, 0, 194, 41), &mut buffer);
        assert_eq!(buffer[(0, 0)].symbol(), "v");
        let mut empty = Buffer::empty(Rect::default());
        draw_screen(&emulator.screen(), Rect::new(0, 0, 194, 41), &mut empty);
    }
}
