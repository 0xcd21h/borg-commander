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
    (0..area.height.min(screen.rows())).for_each(|row| draw_row(screen, row, area.y + row, area, buf));
}

pub fn draw_row(screen: &Screen<'_>, source: u16, target: u16, area: Rect, buf: &mut Buffer) {
    for col in 0..area.width.min(screen.cols()) {
        convert(screen, screen.cell(col, source), &mut buf[(area.x + col, target)]);
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
