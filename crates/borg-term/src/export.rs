use std::fmt::Write as _;

use alacritty_terminal::{
    grid::Dimensions,
    index::{Column, Line},
    term::{
        Term,
        cell::{Cell, Flags},
    },
    vte::ansi::{Color, NamedColor},
};

const RESET: &str = "\x1b[0m";
const NEWLINE: &str = "\r\n";
const BRIGHT_OFFSET: u8 = 8;
const FLAG_CODES: [(Flags, u8); 7] = [
    (Flags::BOLD, 1),
    (Flags::DIM, 2),
    (Flags::ITALIC, 3),
    (Flags::UNDERLINE, 4),
    (Flags::INVERSE, 7),
    (Flags::HIDDEN, 8),
    (Flags::STRIKEOUT, 9),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Pen {
    fg: Color,
    bg: Color,
    flags: Flags,
}

impl Pen {
    fn of(cell: &Cell) -> Self {
        Self {
            fg: cell.fg,
            bg: cell.bg,
            flags: cell.flags & FLAG_CODES.iter().fold(Flags::empty(), |a, (f, _)| a | *f),
        }
    }

    fn plain() -> Self {
        Self {
            fg: Color::Named(NamedColor::Foreground),
            bg: Color::Named(NamedColor::Background),
            flags: Flags::empty(),
        }
    }

    fn sgr(self) -> String {
        let mut codes = vec![String::from("0")];
        codes
            .extend(FLAG_CODES.iter().filter(|(flag, _)| self.flags.contains(*flag)).map(|(_, code)| code.to_string()));
        codes.extend(color_code(self.fg, 30, 90, 38));
        codes.extend(color_code(self.bg, 40, 100, 48));
        format!("\x1b[{}m", codes.join(";"))
    }
}

fn color_code(color: Color, base: u8, bright: u8, extended: u8) -> Option<String> {
    match color {
        Color::Named(named) => {
            let index = named as usize;
            let index = u8::try_from(index).ok().filter(|i| *i < BRIGHT_OFFSET * 2)?;
            Some(if index < BRIGHT_OFFSET { base + index } else { bright + index - BRIGHT_OFFSET }.to_string())
        }
        Color::Indexed(index) => Some(format!("{extended};5;{index}")),
        Color::Spec(rgb) => Some(format!("{extended};2;{};{};{}", rgb.r, rgb.g, rgb.b)),
    }
}

pub fn export<T>(term: &Term<T>, max_lines: usize) -> Vec<u8> {
    let grid = term.grid();
    let history = i32::try_from(grid.history_size()).unwrap_or(i32::MAX);
    let screen = i32::try_from(grid.screen_lines()).unwrap_or(i32::MAX);
    let columns = grid.columns();

    let lines: Vec<String> = (-history..screen)
        .map(|line| {
            let row = &grid[Line(line)];
            let cells: Vec<&Cell> = (0..columns).map(|col| &row[Column(col)]).collect();
            let used = cells.iter().rposition(|cell| !blank(cell)).map_or(0, |i| i + 1);
            render(&cells[..used])
        })
        .collect();

    let last = lines.iter().rposition(|line| !line.is_empty()).map_or(0, |i| i + 1);
    let first = last.saturating_sub(max_lines);
    let mut out = lines[first..last].join(NEWLINE);
    if !out.is_empty() {
        out.push_str(RESET);
    }
    out.into_bytes()
}

fn blank(cell: &Cell) -> bool {
    (cell.c == ' ' || cell.c == '\0') && Pen::of(cell) == Pen::plain()
}

fn render(cells: &[&Cell]) -> String {
    let mut out = String::new();
    let mut pen = Pen::plain();
    for cell in cells {
        if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
            continue;
        }
        let next = Pen::of(cell);
        if next != pen {
            out.push_str(&next.sgr());
            pen = next;
        }
        out.push(if cell.c == '\0' { ' ' } else { cell.c });
        cell.zerowidth().into_iter().flatten().for_each(|c| out.push(*c));
    }
    if pen != Pen::plain() {
        let _ = write!(out, "{RESET}");
    }
    out
}
