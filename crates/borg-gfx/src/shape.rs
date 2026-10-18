use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

use tiny_skia::{
    Color, FillRule, GradientStop, LinearGradient, Mask, Paint, Path, PathBuilder, Pixmap, PixmapPaint, Point, Shader,
    SpreadMode, Stroke as Line, Transform,
};

use crate::{GfxError, Picture};

const KAPPA: f32 = 0.552_284_8;
const BLUR_PASSES: usize = 3;
const CACHE_LIMIT: usize = 96;
const CHANNELS: usize = 4;
const HALF: f32 = 0.5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgba(pub [u8; 4]);

impl Rgba {
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self([r, g, b, a])
    }

    fn color(self) -> Color {
        let [r, g, b, a] = self.0;
        Color::from_rgba8(r, g, b, a)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Axis {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Fill {
    pub colors: [Rgba; 2],
    pub axis: Axis,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Stroke {
    pub colors: [Rgba; 2],
    pub width: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Glow {
    pub color: Rgba,
    pub reach: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Card {
    pub size: (u32, u32),
    pub radius: u32,
    pub fill: Fill,
    pub border: Option<Stroke>,
    pub glow: Option<Glow>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Shadow {
    pub size: (u32, u32),
    pub radius: u32,
    pub blur: u32,
    pub color: Rgba,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Shape {
    Card(Card),
    Shadow(Shadow),
}

impl Shape {
    pub fn size(&self) -> (u32, u32) {
        match self {
            Self::Card(card) => card.size,
            Self::Shadow(shadow) => (shadow.size.0 + shadow.blur * 2, shadow.size.1 + shadow.blur * 2),
        }
    }
}

fn rounded(x: f32, y: f32, w: f32, h: f32, radius: f32) -> Option<Path> {
    let r = radius.min(w * HALF).min(h * HALF).max(0.0);
    let k = r * KAPPA;
    let (right, bottom) = (x + w, y + h);
    let mut path = PathBuilder::new();
    path.move_to(x + r, y);
    path.line_to(right - r, y);
    path.cubic_to(right - r + k, y, right, y + r - k, right, y + r);
    path.line_to(right, bottom - r);
    path.cubic_to(right, bottom - r + k, right - r + k, bottom, right - r, bottom);
    path.line_to(x + r, bottom);
    path.cubic_to(x + r - k, bottom, x, bottom - r + k, x, bottom - r);
    path.line_to(x, y + r);
    path.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    path.close();
    path.finish()
}

fn gradient(colors: [Rgba; 2], axis: Axis, (w, h): (f32, f32)) -> Shader<'static> {
    let end = match axis {
        Axis::Horizontal => Point::from_xy(w, 0.0),
        Axis::Vertical => Point::from_xy(0.0, h),
    };
    let stops = vec![GradientStop::new(0.0, colors[0].color()), GradientStop::new(1.0, colors[1].color())];
    LinearGradient::new(Point::from_xy(0.0, 0.0), end, stops, SpreadMode::Pad, Transform::identity())
        .unwrap_or(Shader::SolidColor(colors[0].color()))
}

fn paint(shader: Shader<'static>) -> Paint<'static> {
    Paint { shader, anti_alias: true, ..Paint::default() }
}

fn blur(pixmap: &mut Pixmap, radius: u32) {
    let (w, h) = (pixmap.width() as usize, pixmap.height() as usize);
    let radius = radius as usize;
    if radius == 0 || w == 0 || h == 0 {
        return;
    }
    let data = pixmap.data_mut();
    let mut scratch = vec![0u8; data.len()];
    for _ in 0..BLUR_PASSES {
        pass(data, &mut scratch, w, h, radius, true);
        pass(&scratch, data, w, h, radius, false);
    }
}

fn pass(from: &[u8], to: &mut [u8], w: usize, h: usize, radius: usize, horizontal: bool) {
    let (lines, length) = if horizontal { (h, w) } else { (w, h) };
    let at = |line: usize, i: usize| if horizontal { (line * w + i) * CHANNELS } else { (i * w + line) * CHANNELS };
    let window = (radius * 2 + 1) as u32;
    for line in 0..lines {
        for channel in 0..CHANNELS {
            let sample = |i: isize| u32::from(from[at(line, i.clamp(0, length as isize - 1) as usize) + channel]);
            let mut sum: u32 = (-(radius as isize)..=radius as isize).map(sample).sum();
            for i in 0..length {
                to[at(line, i) + channel] = (sum / window) as u8;
                sum += sample(i as isize + radius as isize + 1);
                sum -= sample(i as isize - radius as isize);
            }
        }
    }
}

fn card(card: &Card) -> Option<Pixmap> {
    let (w, h) = card.size;
    let mut pixmap = Pixmap::new(w.max(1), h.max(1))?;
    let size = (w as f32, h as f32);
    let inset = card.border.map_or(0.0, |stroke| stroke.width as f32 * HALF);
    let body = rounded(inset, inset, size.0 - inset * 2.0, size.1 - inset * 2.0, card.radius as f32)?;
    pixmap.fill_path(
        &body,
        &paint(gradient(card.fill.colors, card.fill.axis, size)),
        FillRule::Winding,
        Transform::identity(),
        None,
    );

    if let Some(glow) = card.glow {
        let mut halo = Pixmap::new(w.max(1), h.max(1))?;
        let line = Line { width: glow.reach as f32 * 2.0, ..Line::default() };
        halo.stroke_path(&body, &paint(Shader::SolidColor(glow.color.color())), &line, Transform::identity(), None);
        blur(&mut halo, glow.reach);
        let mut clip = Mask::new(w.max(1), h.max(1))?;
        clip.fill_path(&body, FillRule::Winding, true, Transform::identity());
        pixmap.draw_pixmap(0, 0, halo.as_ref(), &PixmapPaint::default(), Transform::identity(), Some(&clip));
    }

    if let Some(stroke) = card.border {
        let line = Line { width: stroke.width as f32, ..Line::default() };
        pixmap.stroke_path(
            &body,
            &paint(gradient(stroke.colors, Axis::Horizontal, size)),
            &line,
            Transform::identity(),
            None,
        );
    }
    Some(pixmap)
}

fn shadow(shadow: &Shadow) -> Option<Pixmap> {
    let (w, h) = (shadow.size.0 + shadow.blur * 2, shadow.size.1 + shadow.blur * 2);
    let mut pixmap = Pixmap::new(w.max(1), h.max(1))?;
    let edge = shadow.blur as f32;
    let body = rounded(edge, edge, shadow.size.0 as f32, shadow.size.1 as f32, shadow.radius as f32)?;
    pixmap.fill_path(
        &body,
        &paint(Shader::SolidColor(shadow.color.color())),
        FillRule::Winding,
        Transform::identity(),
        None,
    );
    blur(&mut pixmap, shadow.blur / 2);
    Some(pixmap)
}

fn picture(pixmap: &Pixmap) -> Result<Picture, GfxError> {
    let rgba = pixmap
        .pixels()
        .iter()
        .flat_map(|pixel| {
            let color = pixel.demultiply();
            [color.red(), color.green(), color.blue(), color.alpha()]
        })
        .collect();
    Picture::from_rgba(pixmap.width(), pixmap.height(), rgba)
}

fn render(shape: &Shape) -> Result<Picture, GfxError> {
    let pixmap = match shape {
        Shape::Card(spec) => card(spec),
        Shape::Shadow(spec) => shadow(spec),
    };
    picture(&pixmap.ok_or(GfxError::TooLarge)?)
}

pub fn draw(shape: &Shape) -> Result<Picture, GfxError> {
    static CACHE: OnceLock<Mutex<HashMap<Shape, Picture>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(picture) = cache.lock().ok().and_then(|cache| cache.get(shape).cloned()) {
        return Ok(picture);
    }
    let picture = render(shape)?;
    if let Ok(mut cache) = cache.lock() {
        if cache.len() >= CACHE_LIMIT {
            cache.clear();
        }
        cache.insert(*shape, picture.clone());
    }
    Ok(picture)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Rgba = Rgba::new(255, 0, 0, 255);
    const BLUE: Rgba = Rgba::new(0, 0, 255, 255);

    fn sample() -> Card {
        Card {
            size: (40, 20),
            radius: 6,
            fill: Fill { colors: [RED, BLUE], axis: Axis::Horizontal },
            border: Some(Stroke { colors: [BLUE, BLUE], width: 1 }),
            glow: Some(Glow { color: Rgba::new(255, 255, 255, 200), reach: 3 }),
        }
    }

    #[test]
    fn cards_have_rounded_transparent_corners_and_a_gradient() {
        let picture = draw(&Shape::Card(sample())).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!((picture.width(), picture.height()), (40, 20));
        assert_eq!(picture.pixel(0, 0)[3], 0);
        let [left, ..] = picture.pixel(8, 10);
        let [right, ..] = picture.pixel(32, 10);
        assert!(left > right, "gradient should run from red to blue");
    }

    #[test]
    fn shadows_grow_by_their_blur_and_fade_out() {
        let spec = Shadow { size: (20, 10), radius: 4, blur: 6, color: Rgba::new(0, 0, 0, 180) };
        let picture = draw(&Shape::Shadow(spec)).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!((picture.width(), picture.height()), Shape::Shadow(spec).size());
        assert!(picture.pixel(16, 11)[3] > picture.pixel(1, 1)[3]);
    }

    #[test]
    fn identical_shapes_come_from_the_cache() {
        let a = draw(&Shape::Card(sample())).unwrap_or_else(|e| panic!("{e}"));
        let b = draw(&Shape::Card(sample())).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(a.id(), b.id());
    }
}
