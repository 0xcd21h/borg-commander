use ratatui::{buffer::Buffer, layout::Rect};

use super::{Gradient, braille::Canvas};
use crate::Rgb;

const VERTICES: [[f32; 3]; 8] = [
    [-1.0, -1.0, -1.0],
    [1.0, -1.0, -1.0],
    [1.0, 1.0, -1.0],
    [-1.0, 1.0, -1.0],
    [-1.0, -1.0, 1.0],
    [1.0, -1.0, 1.0],
    [1.0, 1.0, 1.0],
    [-1.0, 1.0, 1.0],
];
const FACES: [[usize; 4]; 6] = [[0, 1, 2, 3], [4, 5, 6, 7], [0, 1, 5, 4], [3, 2, 6, 7], [0, 3, 7, 4], [1, 2, 6, 5]];
const GRID_STEPS: [f32; 2] = [1.0 / 3.0, 2.0 / 3.0];
const SPIN: [f32; 3] = [0.71, 0.93, 0.29];
const CAMERA: f32 = 4.2;
const FILL: f32 = 0.24;
const PHASE: f32 = 0.9;
const NEAR_GLOW: f32 = 1.0;
const FAR_GLOW: f32 = 0.28;
const GRID_GLOW: f32 = 0.55;

type Point = (f32, f32, f32);

fn rotate([x, y, z]: [f32; 3], t: f32) -> [f32; 3] {
    let t = t + PHASE;
    let (sa, ca) = (t * SPIN[0]).sin_cos();
    let (sb, cb) = (t * SPIN[1]).sin_cos();
    let (sc, cc) = (t * SPIN[2]).sin_cos();
    let (y, z) = (y * ca - z * sa, y * sa + z * ca);
    let (x, z) = (x * cb + z * sb, -x * sb + z * cb);
    let (x, y) = (x * cc - y * sc, x * sc + y * cc);
    [x, y, z]
}

fn project(canvas: &Canvas, [x, y, z]: [f32; 3]) -> Point {
    let scale = canvas.width().min(canvas.height()) as f32 * FILL * CAMERA;
    let depth = CAMERA + z;
    let glow = FAR_GLOW + (NEAR_GLOW - FAR_GLOW) * ((1.0 - z) / 2.0).clamp(0.0, 1.0);
    (canvas.width() as f32 / 2.0 + x * scale / depth, canvas.height() as f32 / 2.0 + y * scale / depth, glow)
}

fn between(a: Point, b: Point, t: f32) -> Point {
    (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t, (a.2 + (b.2 - a.2) * t) * GRID_GLOW)
}

pub fn render_cube(cols: usize, rows: usize, t: f32) -> Canvas {
    let mut canvas = Canvas::new(cols, rows);
    let points: Vec<Point> = VERTICES.iter().map(|v| project(&canvas, rotate(*v, t))).collect();

    for [a, b, c, d] in FACES {
        let (pa, pb, pc, pd) = (points[a], points[b], points[c], points[d]);
        for step in GRID_STEPS {
            canvas.line(between(pa, pb, step), between(pd, pc, step));
            canvas.line(between(pa, pd, step), between(pb, pc, step));
        }
        canvas.line(pa, pb);
        canvas.line(pb, pc);
        canvas.line(pc, pd);
        canvas.line(pd, pa);
    }
    canvas
}

pub fn draw_cube(buf: &mut Buffer, area: Rect, t: f32, gradient: Gradient, background: Rgb) {
    let canvas = render_cube(usize::from(area.width), usize::from(area.height), t);
    let span = canvas.cols().saturating_sub(1).max(1) as f32;
    for row in 0..canvas.rows() {
        for col in 0..canvas.cols() {
            let Some((glyph, glow)) = canvas.cell(col, row) else { continue };
            let (Ok(x), Ok(y)) = (u16::try_from(col), u16::try_from(row)) else { continue };
            let color = background.mix(gradient.at(col as f32 / span), glow);
            buf[(area.x + x, area.y + y)].set_char(glyph).set_fg(color.into());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(t: f32) -> String {
        let canvas = render_cube(24, 10, t);
        (0..canvas.rows())
            .map(|row| (0..canvas.cols()).map(|col| canvas.cell(col, row).map_or(' ', |(c, _)| c)).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn cube_is_centered_and_changes_over_time() {
        let still = picture(0.0);
        let later = picture(1.0);
        assert_ne!(still, later);
        let middle = still.lines().nth(5).unwrap_or_default();
        assert!(middle.trim().chars().count() > 0);
        assert_eq!(picture(0.5), picture(0.5));
    }
}
