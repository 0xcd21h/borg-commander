use ratatui::style::Color;

const CUBE_START: u8 = 16;
const GREY_START: u8 = 232;
const CUBE_SIDE: u8 = 6;
const CUBE_FIRST_STEP: u8 = 55;
const CUBE_STEP: u8 = 40;
const GREY_BASE: u8 = 8;
const GREY_STEP: u8 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const BLACK: Self = Self::new(0, 0, 0);
    pub const WHITE: Self = Self::new(255, 255, 255);

    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Relative luminance as defined by WCAG.
    pub fn luminance(self) -> f32 {
        let linear = |channel: u8| {
            let c = f32::from(channel) / 255.0;
            if c <= 0.039_28 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * linear(self.r) + 0.7152 * linear(self.g) + 0.0722 * linear(self.b)
    }

    /// WCAG contrast ratio, from 1 (none) to 21 (black on white).
    pub fn contrast(self, other: Self) -> f32 {
        let (a, b) = (self.luminance(), other.luminance());
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    pub fn mix(self, other: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        let channel = |a: u8, b: u8| {
            let value = f32::from(a) + (f32::from(b) - f32::from(a)) * t;
            value.round().clamp(0.0, f32::from(u8::MAX)) as u8
        };
        Self::new(channel(self.r, other.r), channel(self.g, other.g), channel(self.b, other.b))
    }
}

impl From<Rgb> for Color {
    fn from(rgb: Rgb) -> Self {
        Color::Rgb(rgb.r, rgb.g, rgb.b)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Palette {
    pub ansi: [Rgb; 16],
    pub foreground: Rgb,
    pub background: Rgb,
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            ansi: [
                Rgb::new(0x00, 0x00, 0x00),
                Rgb::new(0xcd, 0x31, 0x31),
                Rgb::new(0x0d, 0xbc, 0x79),
                Rgb::new(0xe5, 0xe5, 0x10),
                Rgb::new(0x24, 0x72, 0xc8),
                Rgb::new(0xbc, 0x3f, 0xbc),
                Rgb::new(0x11, 0xa8, 0xcd),
                Rgb::new(0xe5, 0xe5, 0xe5),
                Rgb::new(0x66, 0x66, 0x66),
                Rgb::new(0xf1, 0x4c, 0x4c),
                Rgb::new(0x23, 0xd1, 0x8b),
                Rgb::new(0xf5, 0xf5, 0x43),
                Rgb::new(0x3b, 0x8e, 0xea),
                Rgb::new(0xd6, 0x70, 0xd6),
                Rgb::new(0x29, 0xb8, 0xdb),
                Rgb::new(0xff, 0xff, 0xff),
            ],
            foreground: Rgb::new(0xcc, 0xcc, 0xcc),
            background: Rgb::new(0x1e, 0x1e, 0x1e),
        }
    }
}

impl Palette {
    pub fn foreground_of(&self, color: Color) -> Rgb {
        self.resolve(color).unwrap_or(self.foreground)
    }

    pub fn background_of(&self, color: Color) -> Rgb {
        self.resolve(color).unwrap_or(self.background)
    }

    pub fn indexed(&self, index: u8) -> Rgb {
        match index {
            i if i < CUBE_START => self.ansi[usize::from(i)],
            i if i < GREY_START => {
                let offset = i - CUBE_START;
                let level = |v: u8| if v == 0 { 0 } else { CUBE_FIRST_STEP + v * CUBE_STEP };
                Rgb::new(
                    level(offset / (CUBE_SIDE * CUBE_SIDE)),
                    level(offset / CUBE_SIDE % CUBE_SIDE),
                    level(offset % CUBE_SIDE),
                )
            }
            i => {
                let grey = GREY_BASE + (i - GREY_START) * GREY_STEP;
                Rgb::new(grey, grey, grey)
            }
        }
    }

    fn resolve(&self, color: Color) -> Option<Rgb> {
        let ansi = |i: u8| Some(self.ansi[usize::from(i)]);
        match color {
            Color::Reset => None,
            Color::Black => ansi(0),
            Color::Red => ansi(1),
            Color::Green => ansi(2),
            Color::Yellow => ansi(3),
            Color::Blue => ansi(4),
            Color::Magenta => ansi(5),
            Color::Cyan => ansi(6),
            Color::Gray => ansi(7),
            Color::DarkGray => ansi(8),
            Color::LightRed => ansi(9),
            Color::LightGreen => ansi(10),
            Color::LightYellow => ansi(11),
            Color::LightBlue => ansi(12),
            Color::LightMagenta => ansi(13),
            Color::LightCyan => ansi(14),
            Color::White => ansi(15),
            Color::Indexed(i) => Some(self.indexed(i)),
            Color::Rgb(r, g, b) => Some(Rgb::new(r, g, b)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_xterm_cube_and_greys() {
        let palette = Palette::default();
        assert_eq!(palette.indexed(16), Rgb::new(0, 0, 0));
        assert_eq!(palette.indexed(196), Rgb::new(0xff, 0, 0));
        assert_eq!(palette.indexed(231), Rgb::new(0xff, 0xff, 0xff));
        assert_eq!(palette.indexed(232), Rgb::new(8, 8, 8));
        assert_eq!(palette.indexed(255), Rgb::new(238, 238, 238));
    }

    #[test]
    fn mixes_linearly() {
        let mid = Rgb::new(0, 100, 200).mix(Rgb::new(100, 200, 0), 0.5);
        assert_eq!(mid, Rgb::new(50, 150, 100));
    }
}
