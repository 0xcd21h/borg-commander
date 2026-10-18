use std::path::Path;

use ab_glyph::{Font, FontRef, PxScale, ScaleFont, point};
use anyhow::{Context, Result};
use borg_ui::Rgb;
use image::{Rgb as Pixel, RgbImage};
use ratatui::style::Modifier;
use unicode_width::UnicodeWidthStr;

use crate::Snapshot;

const FONT_REGULAR: &[u8] = include_bytes!("../assets/DejaVuSansMono.ttf");
const FONT_BOLD: &[u8] = include_bytes!("../assets/DejaVuSansMono-Bold.ttf");
const FONT_PX: f32 = 18.0;
const MARGIN: u32 = 14;
const UNDERLINE_OFFSET: f32 = 2.0;
const WIDTH_PROBE: char = 'M';

struct Grid {
    cell_w: u32,
    cell_h: u32,
    ascent: f32,
}

pub fn png(snapshot: &Snapshot, path: &Path) -> Result<()> {
    let regular = FontRef::try_from_slice(FONT_REGULAR).context("loading regular font")?;
    let bold = FontRef::try_from_slice(FONT_BOLD).context("loading bold font")?;
    let scale = PxScale::from(FONT_PX);
    let metrics = regular.as_scaled(scale);
    let grid = Grid {
        cell_w: metrics.h_advance(regular.glyph_id(WIDTH_PROBE)).ceil() as u32,
        cell_h: (metrics.ascent() - metrics.descent()).ceil() as u32,
        ascent: metrics.ascent(),
    };

    let palette = snapshot.palette();
    let width = u32::from(snapshot.cols()) * grid.cell_w + MARGIN * 2;
    let height = u32::from(snapshot.rows()) * grid.cell_h + MARGIN * 2;
    let mut image = RgbImage::from_pixel(width, height, pixel(palette.background));

    let buffer = snapshot.buffer();
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

            let origin_x = MARGIN + u32::from(x) * grid.cell_w;
            let origin_y = MARGIN + u32::from(y) * grid.cell_h;
            let span = u32::try_from(cell.symbol().width().max(1)).unwrap_or(1);
            fill(&mut image, origin_x, origin_y, grid.cell_w * span, grid.cell_h, style.bg);

            let font = if style.bold { &bold } else { &regular };
            let cell_rect = (origin_x, origin_y, grid.cell_w, grid.cell_h);
            if let Some(segments) = cell.symbol().chars().next().and_then(box_segments) {
                draw_box(&mut image, cell_rect, segments, style.fg);
                continue;
            }
            for ch in cell.symbol().chars().filter(|c| !c.is_whitespace()) {
                draw_glyph(&mut image, font, scale, ch, (origin_x, origin_y), &grid, style.fg);
            }
            if cell.modifier.contains(Modifier::UNDERLINED) {
                let line_y = origin_y + (grid.ascent + UNDERLINE_OFFSET) as u32;
                fill(&mut image, origin_x, line_y, grid.cell_w * span, 1, style.fg);
            }
        }
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    image.save(path).with_context(|| format!("writing {}", path.display()))
}

fn draw_glyph(
    image: &mut RgbImage,
    font: &FontRef<'_>,
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
