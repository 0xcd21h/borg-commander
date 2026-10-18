use std::time::Instant;

use borg_core::{ListingMode, Panel, PanelMotion};
use borg_gfx::{Axis, Card, CellSize, Fill, Glow, Placement, Rgba, Shadow, Shape, Stroke, draw};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
};

use crate::{Rgb, look::Look, panel_view::geometry};

const PANEL_Z: i32 = -40;
const PANEL_HIGHLIGHT_Z: i32 = -38;
const OVERLAY_HIGHLIGHT_Z: i32 = -9;
const HIGHLIGHT_RADIUS_SHARE: f32 = 0.3;
const HALO_Z: i32 = -35;
const CURSOR_Z: i32 = -30;
const SHADOW_Z: i32 = -20;
const OVERLAY_Z: i32 = -10;
const OPAQUE: u8 = u8::MAX;
const RADIUS_SHARE: f32 = 0.42;
const PILL_RADIUS_SHARE: f32 = 0.5;
const BORDER_DIVISOR: u16 = 9;
const LIFT: f32 = 0.06;
const BREATH_STEPS: f32 = 6.0;
const HALO_STRENGTH: f32 = 0.9;
const SHADOW_STRENGTH: f32 = 0.6;
const SHADOW_DROP_SHARE: f32 = 0.35;

pub struct PanelCards<'a> {
    pub panel: &'a Panel,
    pub motion: &'a PanelMotion,
    pub area: Rect,
    pub active: bool,
    pub dim: f32,
    pub now: Instant,
}

fn rgba(rgb: Rgb, alpha: f32) -> Rgba {
    Rgba::new(rgb.r, rgb.g, rgb.b, (alpha.clamp(0.0, 1.0) * f32::from(OPAQUE)).round() as u8)
}

fn solid(rgb: Rgb) -> Rgba {
    rgba(rgb, 1.0)
}

fn pixels(cells: u16, cell: u16) -> u32 {
    u32::from(cells) * u32::from(cell)
}

fn split(px: u32, cell: u16) -> (u16, u16) {
    let cell = u32::from(cell.max(1));
    (u16::try_from(px / cell).unwrap_or(u16::MAX), u16::try_from(px % cell).unwrap_or(0))
}

fn place(shape: Shape, origin_px: (u32, u32), cell: CellSize, z: i32) -> Option<Placement> {
    let picture = draw(&shape).ok()?;
    let (x, offset_x) = split(origin_px.0, cell.width);
    let (y, offset_y) = split(origin_px.1, cell.height);
    Some(Placement::exact(picture, (x, y), (offset_x, offset_y), z))
}

fn border_width(cell: CellSize) -> u32 {
    u32::from((cell.width / BORDER_DIVISOR).max(1))
}

fn quantized(value: f32) -> f32 {
    (value * BREATH_STEPS).round() / BREATH_STEPS
}

fn background(look: &Look<'_>, dim: f32) -> [Rgb; 2] {
    let base = look.background().mix(Rgb::BLACK, dim);
    [base.mix(Rgb::WHITE, LIFT), base]
}

pub fn panel(cards: &PanelCards<'_>, look: &Look<'_>, cell: CellSize) -> Vec<Placement> {
    let styles = look.styles();
    let effects = look.theme.effects;
    let area = cards.area;
    let size = (pixels(area.width, cell.width), pixels(area.height, cell.height));
    let border = match (cards.active, effects.border_gradient) {
        (true, Some(pair)) => {
            let gradient = look.gradient(pair).brightened(quantized(look.breath()));
            [gradient.from, gradient.to]
        }
        (true, None) => [look.fg(styles.border_active); 2],
        (false, _) => [look.fg(styles.border).mix(look.background(), effects.inactive_dim); 2],
    };
    let glow = (cards.active && effects.glow > 0.0).then(|| Glow {
        color: rgba(look.accent().from, effects.glow * quantized(look.breath())),
        reach: u32::from(cell.width),
    });
    let card = Card {
        size,
        radius: (f32::from(cell.height) * RADIUS_SHARE) as u32,
        fill: Fill { colors: background(look, cards.dim).map(solid), axis: Axis::Vertical },
        border: Some(Stroke {
            colors: border.map(|rgb| solid(rgb.mix(Rgb::BLACK, cards.dim))),
            width: border_width(cell),
        }),
        glow,
    };
    let origin = (pixels(area.x, cell.width), pixels(area.y, cell.height));
    let mut placed: Vec<Placement> = place(Shape::Card(card), origin, cell, PANEL_Z).into_iter().collect();
    placed.extend(cursor(cards, look, cell));
    placed
}

fn cursor_fill(cards: &PanelCards<'_>, look: &Look<'_>) -> [Rgb; 2] {
    let styles = look.styles();
    let marked = cards.panel.hovered_entry().is_some_and(|entry| cards.panel.is_marked(entry));
    let style: Style = match (cards.active, marked) {
        (true, true) => styles.marked_cursor,
        (true, false) => styles.cursor,
        (false, _) => styles.cursor_inactive,
    };
    match look.theme.effects.cursor_gradient.filter(|_| cards.active && !marked) {
        Some(pair) => {
            let gradient = look.gradient(pair);
            [gradient.from, gradient.to]
        }
        None => [look.bg(style); 2],
    }
}

fn cursor(cards: &PanelCards<'_>, look: &Look<'_>, cell: CellSize) -> Vec<Placement> {
    let panel = cards.panel;
    let (inner, content) = geometry(cards.area);
    let scroll = cards.motion.scroll(cards.now, panel.scroll()) as f32;
    let rows = panel.rows().max(1);
    let (column, row) = match panel.mode() {
        ListingMode::Full => (0, cards.motion.cursor(cards.now, panel.cursor()) - scroll),
        ListingMode::Brief => {
            let slot = panel.cursor().saturating_sub(scroll as usize);
            (slot / rows, (slot % rows) as f32)
        }
    };
    if row < 0.0 || row > (inner.height.saturating_sub(1)) as f32 {
        return Vec::new();
    }
    let columns = u16::try_from(panel.mode().columns()).unwrap_or(1).max(1);
    let width_cells = content.width / columns;
    let half = u32::from(cell.width) / 2;
    let left = pixels(content.x + width_cells * u16::try_from(column).unwrap_or(0), cell.width).saturating_sub(half);
    let top = pixels(inner.y, cell.height) + (row * f32::from(cell.height)).round() as u32;
    let size = (pixels(width_cells, cell.width) + half * 2, u32::from(cell.height));
    let fill = cursor_fill(cards, look);
    let pill = Card {
        size,
        radius: (f32::from(cell.height) * PILL_RADIUS_SHARE) as u32,
        fill: Fill { colors: fill.map(solid), axis: Axis::Horizontal },
        border: None,
        glow: None,
    };
    let mut placed: Vec<Placement> = Vec::new();
    let effects = look.theme.effects;
    if cards.active && effects.glow > 0.0 {
        let blur = u32::from(cell.height) / 2;
        let halo = Shadow {
            size,
            radius: pill.radius,
            blur,
            color: rgba(fill[0].mix(fill[1], 0.5), effects.glow * HALO_STRENGTH),
        };
        placed.extend(place(Shape::Shadow(halo), (left.saturating_sub(blur), top.saturating_sub(blur)), cell, HALO_Z));
    }
    placed.extend(place(Shape::Card(pill), (left, top), cell, CURSOR_Z));
    placed
}

pub fn overlay(rect: Rect, look: &Look<'_>, cell: CellSize) -> Vec<Placement> {
    let styles = look.styles();
    let size = (pixels(rect.width, cell.width), pixels(rect.height, cell.height));
    let origin = (pixels(rect.x, cell.width), pixels(rect.y, cell.height));
    let blur = u32::from(cell.height);
    let drop = (f32::from(cell.height) * SHADOW_DROP_SHARE) as u32;
    let radius = (f32::from(cell.height) * RADIUS_SHARE) as u32;
    let shadow = Shadow { size, radius, blur, color: rgba(Rgb::BLACK, SHADOW_STRENGTH) };
    let surface = look.bg(styles.overlay);
    let border = look.fg(styles.overlay_border);
    let card = Card {
        size,
        radius,
        fill: Fill { colors: [solid(surface.mix(Rgb::WHITE, LIFT)), solid(surface)], axis: Axis::Vertical },
        border: Some(Stroke { colors: [solid(border); 2], width: border_width(cell) }),
        glow: None,
    };
    let shadow_origin = ((origin.0 + drop).saturating_sub(blur), (origin.1 + drop).saturating_sub(blur));
    place(Shape::Shadow(shadow), shadow_origin, cell, SHADOW_Z)
        .into_iter()
        .chain(place(Shape::Card(card), origin, cell, OVERLAY_Z))
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    Panel,
    Overlay,
}

pub fn highlights(
    buf: &Buffer,
    rect: Rect,
    base: &[Color],
    layer: Layer,
    look: &Look<'_>,
    cell: CellSize,
) -> Vec<Placement> {
    let rect = rect.intersection(buf.area);
    let z = match layer {
        Layer::Panel => PANEL_HIGHLIGHT_Z,
        Layer::Overlay => OVERLAY_HIGHLIGHT_Z,
    };
    let radius = (f32::from(cell.height) * HIGHLIGHT_RADIUS_SHARE) as u32;
    let mut placed = Vec::new();
    for y in rect.top()..rect.bottom() {
        let mut x = rect.left();
        while x < rect.right() {
            let bg = buf[(x, y)].bg;
            let start = x;
            while x < rect.right() && buf[(x, y)].bg == bg {
                x += 1;
            }
            if bg == Color::Reset || base.contains(&bg) {
                continue;
            }
            let color = solid(look.palette.background_of(bg));
            let size = (pixels(x - start, cell.width), u32::from(cell.height));
            let card = Card {
                size,
                radius,
                fill: Fill { colors: [color; 2], axis: Axis::Vertical },
                border: None,
                glow: None,
            };
            placed.extend(place(Shape::Card(card), (pixels(start, cell.width), pixels(y, cell.height)), cell, z));
        }
    }
    placed
}

pub fn strip(rect: Rect, style: Style, look: &Look<'_>, cell: CellSize) -> Vec<Placement> {
    let size = (pixels(rect.width, cell.width), pixels(rect.height, cell.height));
    let card = Card {
        size,
        radius: 0,
        fill: Fill { colors: [solid(look.bg(style)); 2], axis: Axis::Vertical },
        border: None,
        glow: None,
    };
    place(Shape::Card(card), (pixels(rect.x, cell.width), pixels(rect.y, cell.height)), cell, OVERLAY_Z)
        .into_iter()
        .collect()
}
