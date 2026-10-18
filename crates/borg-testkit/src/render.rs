use std::path::Path;

use ab_glyph::{Font, FontArc, GlyphId, PxScale, ScaleFont, point};
use anyhow::{Context, Result};
use borg_ui::Rgb;
use image::{Rgb as Pixel, RgbImage};
use ratatui::style::Modifier;
use unicode_width::UnicodeWidthStr;

use borg_gfx::{CellSize, Placement};

use crate::{Snapshot, snapshot::CellStyle};

const FONT_REGULAR: &[u8] = include_bytes!("../assets/DejaVuSansMono.ttf");
const FONT_BOLD: &[u8] = include_bytes!("../assets/DejaVuSansMono-Bold.ttf");
const FONT_SYMBOLS: &[u8] = include_bytes!("../assets/SymbolsNerdFontMono-Regular.ttf");
const FONT_PX: f32 = 18.0;
const ENV_FONT: &str = "BORG_SCREENSHOT_FONT";
const ENV_FONT_BOLD: &str = "BORG_SCREENSHOT_FONT_BOLD";
const SYMBOL_SPAN: f32 = 1.4;
const SYMBOL_HEIGHT_FILL: f32 = 0.72;
const MARGIN: u32 = 14;
const UNDERLINE_OFFSET: f32 = 2.0;
const WIDTH_PROBE: char = 'M';
const ALPHA_MAX: f32 = 255.0;
const MISSING_GLYPH: GlyphId = GlyphId(0);
const EIGHTHS: f32 = 8.0;
const LOWER_EIGHTHS: std::ops::RangeInclusive<char> = '\u{2581}'..='\u{2588}';
const LEFT_EIGHTHS: std::ops::RangeInclusive<char> = '\u{2589}'..='\u{258f}';
const BRAILLE: std::ops::RangeInclusive<char> = '\u{2800}'..='\u{28ff}';
const BRAILLE_DOTS: [(u32, u32); 8] = [(0, 0), (0, 1), (0, 2), (1, 0), (1, 1), (1, 2), (0, 3), (1, 3)];
const BRAILLE_COLUMNS: u32 = 2;
const BRAILLE_ROWS: u32 = 4;

struct Grid {
    cell_w: u32,
    cell_h: u32,
    ascent: f32,
}

pub fn png(snapshot: &Snapshot, path: &Path) -> Result<()> {
    let image = raster(snapshot)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    image.save(path).with_context(|| format!("writing {}", path.display()))
}

struct Fonts {
    regular: FontArc,
    bold: FontArc,
    symbols: FontArc,
}

fn embedded(bytes: &'static [u8]) -> Result<FontArc> {
    FontArc::try_from_slice(bytes).context("loading embedded font")
}

fn from_env(name: &str) -> Result<Option<FontArc>> {
    let Some(path) = std::env::var_os(name) else { return Ok(None) };
    let bytes = std::fs::read(&path).with_context(|| format!("reading font {}", path.display()))?;
    FontArc::try_from_vec(bytes).map(Some).with_context(|| format!("parsing font {}", path.display()))
}

fn fonts() -> Result<Fonts> {
    let custom = from_env(ENV_FONT)?;
    let bold = match (from_env(ENV_FONT_BOLD)?, &custom) {
        (Some(bold), _) => bold,
        (None, Some(regular)) => regular.clone(),
        (None, None) => embedded(FONT_BOLD)?,
    };
    let regular = custom.map_or_else(|| embedded(FONT_REGULAR), Ok)?;
    Ok(Fonts { regular, bold, symbols: embedded(FONT_SYMBOLS)? })
}

fn grid(font: &FontArc, scale: PxScale) -> Grid {
    let metrics = font.as_scaled(scale);
    Grid {
        cell_w: metrics.h_advance(font.glyph_id(WIDTH_PROBE)).ceil() as u32,
        cell_h: (metrics.ascent() - metrics.descent()).ceil() as u32,
        ascent: metrics.ascent(),
    }
}

pub fn cell_size() -> Result<CellSize> {
    let grid = grid(&fonts()?.regular, PxScale::from(FONT_PX));
    Ok(CellSize { width: u16::try_from(grid.cell_w)?, height: u16::try_from(grid.cell_h)? })
}

pub fn raster(snapshot: &Snapshot) -> Result<RgbImage> {
    let Fonts { regular, bold, symbols } = fonts()?;
    let scale = PxScale::from(FONT_PX);
    let grid = grid(&regular, scale);

    let palette = snapshot.palette();
    let width = u32::from(snapshot.cols()) * grid.cell_w + MARGIN * 2;
    let height = u32::from(snapshot.rows()) * grid.cell_h + MARGIN * 2;
    let mut image = RgbImage::from_pixel(width, height, pixel(palette.background));
    let cells = cells(snapshot, &grid);

    for cell in &cells {
        fill(&mut image, cell.rect.0, cell.rect.1, cell.rect.2, cell.rect.3, cell.style.bg);
    }
    let (under, over): (Vec<&Placement>, Vec<&Placement>) =
        snapshot.placements().iter().partition(|placement| placement.z < 0);
    under.into_iter().for_each(|placement| blit(&mut image, placement, &grid));
    for cell in &cells {
        let font = if cell.style.bold { &bold } else { &regular };
        draw_cell(&mut image, font, &symbols, scale, cell, &grid);
    }
    over.into_iter().for_each(|placement| blit(&mut image, placement, &grid));
    Ok(image)
}

struct Painted {
    symbol: String,
    style: CellStyle,
    underlined: bool,
    rect: (u32, u32, u32, u32),
}

fn cells(snapshot: &Snapshot, grid: &Grid) -> Vec<Painted> {
    let buffer = snapshot.buffer();
    let mut painted = Vec::new();
    for y in 0..snapshot.rows() {
        let mut covered = 0;
        for x in 0..snapshot.cols() {
            if covered > 0 {
                covered -= 1;
                continue;
            }
            let cell = &buffer[(x, y)];
            covered = cell.symbol().width().saturating_sub(1);
            let mut style = snapshot.style_at(x, y);
            if snapshot.cursor() == Some((x, y)) {
                std::mem::swap(&mut style.fg, &mut style.bg);
            }
            let span = u32::try_from(cell.symbol().width().max(1)).unwrap_or(1);
            let origin = (MARGIN + u32::from(x) * grid.cell_w, MARGIN + u32::from(y) * grid.cell_h);
            painted.push(Painted {
                symbol: cell.symbol().to_owned(),
                style,
                underlined: cell.modifier.contains(Modifier::UNDERLINED),
                rect: (origin.0, origin.1, grid.cell_w * span, grid.cell_h),
            });
        }
    }
    painted
}

fn draw_cell(image: &mut RgbImage, text: &FontArc, symbols: &FontArc, scale: PxScale, cell: &Painted, grid: &Grid) {
    let (x, y, width, _) = cell.rect;
    let cell_rect = (x, y, grid.cell_w, grid.cell_h);
    let first = cell.symbol.chars().next();
    if let Some(segments) = first.and_then(box_segments) {
        draw_box(image, cell_rect, segments, cell.style.fg);
        return;
    }
    if let Some(dots) = first.filter(|c| BRAILLE.contains(c)) {
        draw_braille(image, cell_rect, u32::from(dots) - u32::from(*BRAILLE.start()), cell.style.fg);
        return;
    }
    if let Some(block) = first.and_then(block_fraction) {
        draw_block(image, cell_rect, block, cell.style.fg);
        return;
    }
    for ch in cell.symbol.chars().filter(|c| !c.is_whitespace()) {
        if text.glyph_id(ch) == MISSING_GLYPH && symbols.glyph_id(ch) != MISSING_GLYPH {
            draw_symbol(image, symbols, scale, ch, cell_rect, cell.style.fg);
        } else {
            draw_glyph(image, text, scale, ch, (x, y), grid, cell.style.fg);
        }
    }
    if cell.underlined {
        let line_y = y + (grid.ascent + UNDERLINE_OFFSET) as u32;
        fill(image, x, line_y, width, 1, cell.style.fg);
    }
}

fn blit(image: &mut RgbImage, placement: &Placement, grid: &Grid) {
    let picture = &placement.picture;
    let cell = CellSize {
        width: u16::try_from(grid.cell_w).unwrap_or(u16::MAX),
        height: u16::try_from(grid.cell_h).unwrap_or(u16::MAX),
    };
    let fit = placement.fit(cell);
    let left = MARGIN + u32::from(fit.area.x) * grid.cell_w + u32::from(fit.offset.0);
    let top = MARGIN + u32::from(fit.area.y) * grid.cell_h + u32::from(fit.offset.1);
    let (width, height) = fit.size;
    for ty in 0..height {
        for tx in 0..width {
            let [r, g, b, a] = picture.pixel(tx * picture.width() / width, ty * picture.height() / height);
            let (px, py) = (left + tx, top + ty);
            if px < image.width() && py < image.height() {
                let under = image.get_pixel(px, py);
                let blended = Rgb::new(under[0], under[1], under[2]).mix(Rgb::new(r, g, b), f32::from(a) / ALPHA_MAX);
                image.put_pixel(px, py, pixel(blended));
            }
        }
    }
}

fn draw_symbol(image: &mut RgbImage, font: &FontArc, scale: PxScale, ch: char, cell: (u32, u32, u32, u32), color: Rgb) {
    let (x, y, width, height) = cell;
    let Some(probe) = font.outline_glyph(font.glyph_id(ch).with_scale_and_position(scale, point(0.0, 0.0))) else {
        return;
    };
    let bounds = probe.px_bounds();
    let (glyph_w, glyph_h) = (bounds.width().max(1.0), bounds.height().max(1.0));
    let span = width as f32 * SYMBOL_SPAN;
    let shrink = (span / glyph_w).min(height as f32 * SYMBOL_HEIGHT_FILL / glyph_h);
    let scaled = PxScale { x: scale.x * shrink, y: scale.y * shrink };
    let Some(outline) = font.outline_glyph(font.glyph_id(ch).with_scale_and_position(scaled, point(0.0, 0.0))) else {
        return;
    };
    let fitted = outline.px_bounds();
    let left = x as f32 + (span - fitted.width()) / 2.0;
    let top = y as f32 + (height as f32 - fitted.height()) / 2.0;
    outline.draw(|gx, gy, coverage| {
        let (px, py) = ((left + gx as f32) as u32, (top + gy as f32) as u32);
        if px < image.width() && py < image.height() {
            let under = image.get_pixel(px, py);
            let blended = Rgb::new(under[0], under[1], under[2]).mix(color, coverage);
            image.put_pixel(px, py, pixel(blended));
        }
    });
}

fn draw_glyph(
    image: &mut RgbImage,
    font: &FontArc,
    scale: PxScale,
    ch: char,
    (origin_x, origin_y): (u32, u32),
    grid: &Grid,
    color: Rgb,
) {
    let glyph = font.glyph_id(ch).with_scale_and_position(scale, point(origin_x as f32, origin_y as f32 + grid.ascent));
    let Some(outline) = font.outline_glyph(glyph) else { return };
    let bounds = outline.px_bounds();
    outline.draw(|gx, gy, coverage| {
        let px = bounds.min.x as i64 + i64::from(gx);
        let py = bounds.min.y as i64 + i64::from(gy);
        let (Ok(px), Ok(py)) = (u32::try_from(px), u32::try_from(py)) else { return };
        if px < image.width() && py < image.height() {
            let under = image.get_pixel(px, py);
            let blended = Rgb::new(under[0], under[1], under[2]).mix(color, coverage);
            image.put_pixel(px, py, pixel(blended));
        }
    });
}

#[derive(Debug, Clone, Copy)]
struct Segments {
    left: bool,
    right: bool,
    up: bool,
    down: bool,
}

fn box_segments(ch: char) -> Option<Segments> {
    let (left, right, up, down) = match ch {
        '─' => (true, true, false, false),
        '│' => (false, false, true, true),
        '┌' => (false, true, false, true),
        '┐' => (true, false, false, true),
        '└' => (false, true, true, false),
        '┘' => (true, false, true, false),
        '├' => (false, true, true, true),
        '┤' => (true, false, true, true),
        '┬' => (true, true, false, true),
        '┴' => (true, true, true, false),
        '┼' => (true, true, true, true),
        _ => return None,
    };
    Some(Segments { left, right, up, down })
}

fn draw_box(image: &mut RgbImage, (x, y, w, h): (u32, u32, u32, u32), segments: Segments, color: Rgb) {
    let (cx, cy) = (x + w / 2, y + h / 2);
    let spans = [
        (segments.left, (x, cy, cx - x + 1, 1)),
        (segments.right, (cx, cy, x + w - cx, 1)),
        (segments.up, (cx, y, 1, cy - y + 1)),
        (segments.down, (cx, cy, 1, y + h - cy)),
    ];
    for (_, (sx, sy, sw, sh)) in spans.into_iter().filter(|(on, _)| *on) {
        fill(image, sx, sy, sw, sh, color);
    }
}

fn draw_braille(image: &mut RgbImage, (x, y, w, h): (u32, u32, u32, u32), bits: u32, color: Rgb) {
    let (step_x, step_y) = (w / BRAILLE_COLUMNS, h / BRAILLE_ROWS);
    let size = step_x.min(step_y).div_ceil(2).max(1);
    for (bit, (col, row)) in BRAILLE_DOTS.into_iter().enumerate() {
        if bits & (1 << bit) != 0 {
            let cx = x + col * step_x + (step_x - size) / 2;
            let cy = y + row * step_y + (step_y - size) / 2;
            fill(image, cx, cy, size, size, color);
        }
    }
}

fn block_fraction(ch: char) -> Option<[f32; 4]> {
    let eighth = |offset: u32| offset as f32 / EIGHTHS;
    match ch {
        '▀' => Some([0.0, 0.0, 1.0, 0.5]),
        '▐' => Some([0.5, 0.0, 1.0, 1.0]),
        c if LOWER_EIGHTHS.contains(&c) => Some([0.0, 1.0 - eighth(u32::from(c) - 0x2580), 1.0, 1.0]),
        c if LEFT_EIGHTHS.contains(&c) => Some([0.0, 0.0, eighth(0x2590 - u32::from(c)), 1.0]),
        _ => None,
    }
}

fn draw_block(image: &mut RgbImage, (x, y, w, h): (u32, u32, u32, u32), [x0, y0, x1, y1]: [f32; 4], color: Rgb) {
    let at = |origin: u32, size: u32, fraction: f32| origin + (size as f32 * fraction).round() as u32;
    let (left, top, right, bottom) = (at(x, w, x0), at(y, h, y0), at(x, w, x1), at(y, h, y1));
    fill(image, left, top, right - left, bottom - top, color);
}

fn fill(image: &mut RgbImage, x: u32, y: u32, w: u32, h: u32, color: Rgb) {
    let (max_x, max_y) = ((x + w).min(image.width()), (y + h).min(image.height()));
    for py in y..max_y {
        for px in x..max_x {
            image.put_pixel(px, py, pixel(color));
        }
    }
}

fn pixel(color: Rgb) -> Pixel<u8> {
    Pixel([color.r, color.g, color.b])
}
