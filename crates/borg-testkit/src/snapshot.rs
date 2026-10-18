use std::path::Path;

use anyhow::Result;
use borg_gfx::Placement;
use borg_term::Screen;
use borg_ui::{Palette, Rgb, draw_screen};
use ratatui::{buffer::Buffer, layout::Rect, style::Modifier};
use unicode_width::UnicodeWidthStr;

use crate::render;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellStyle {
    pub fg: Rgb,
    pub bg: Rgb,
    pub bold: bool,
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    buffer: Buffer,
    palette: Palette,
    cursor: Option<(u16, u16)>,
    placements: Vec<Placement>,
}

impl Snapshot {
    pub(crate) fn capture(screen: &Screen<'_>, palette: &Palette) -> Self {
        let area = Rect::new(0, 0, screen.cols(), screen.rows());
        let mut buffer = Buffer::empty(area);
        draw_screen(screen, area, &mut buffer);
        let cursor = screen.cursor_visible().then(|| screen.cursor());
        Self { buffer, palette: palette.clone(), cursor, placements: Vec::new() }
    }

    pub fn from_buffer(buffer: Buffer, palette: &Palette, cursor: Option<(u16, u16)>) -> Self {
        Self { buffer, palette: palette.clone(), cursor, placements: Vec::new() }
    }

    pub fn with_placements(self, placements: Vec<Placement>) -> Self {
        Self { placements, ..self }
    }

    pub(crate) fn placements(&self) -> &[Placement] {
        &self.placements
    }

    pub fn raster(&self) -> Result<image::RgbImage> {
        render::raster(self)
    }

    pub fn cols(&self) -> u16 {
        self.buffer.area.width
    }

    pub fn rows(&self) -> u16 {
        self.buffer.area.height
    }

    pub fn cursor(&self) -> Option<(u16, u16)> {
        self.cursor
    }

    pub fn lines(&self) -> Vec<String> {
        (0..self.rows())
            .map(|y| self.columns(y).into_iter().map(|(_, s)| s).collect::<String>().trim_end().to_owned())
            .collect()
    }

    pub fn text(&self) -> String {
        self.lines().join("\n")
    }

    pub fn contains(&self, needle: &str) -> bool {
        self.find(needle).is_some()
    }

    pub fn find(&self, needle: &str) -> Option<(u16, u16)> {
        (0..self.rows()).find_map(|y| {
            let columns = self.columns(y);
            let line: String = columns.iter().map(|(_, s)| s.as_str()).collect();
            let byte = line.find(needle)?;
            let mut offset = 0;
            columns.iter().find_map(|(x, symbol)| {
                let hit = (offset == byte).then_some((*x, y));
                offset += symbol.len();
                hit
            })
        })
    }

    pub fn style_at(&self, x: u16, y: u16) -> CellStyle {
        let cell = &self.buffer[(x, y)];
        let fg = self.palette.foreground_of(cell.fg);
        let bg = self.palette.background_of(cell.bg);
        let (fg, bg) = if cell.modifier.contains(Modifier::REVERSED) { (bg, fg) } else { (fg, bg) };
        CellStyle { fg, bg, bold: cell.modifier.contains(Modifier::BOLD) }
    }

    pub fn style_of(&self, needle: &str) -> Option<CellStyle> {
        self.find(needle).map(|(x, y)| self.style_at(x, y))
    }

    pub fn save_png(&self, path: &Path) -> Result<()> {
        render::png(self, path)
    }

    pub(crate) fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    pub(crate) fn palette(&self) -> &Palette {
        &self.palette
    }

    fn columns(&self, y: u16) -> Vec<(u16, String)> {
        let mut skip = 0;
        (0..self.cols())
            .filter_map(|x| {
                if skip > 0 {
                    skip -= 1;
                    return None;
                }
                let symbol = self.buffer[(x, y)].symbol().to_owned();
                skip = symbol.width().saturating_sub(1);
                Some((x, symbol))
            })
            .collect()
    }
}
