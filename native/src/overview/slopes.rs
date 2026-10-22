//! Slopes too steep to stand on that a player still gets onto (#371): he
//! jumps or drops onto one and slides off, as on dod_harrington's grass
//! banks. The overview outlines them instead of filling them.
//!
//! A slope here is an upward face steeper than a floor (normal z 0.3 to
//! 0.7). It is touched when, at some point of it, a crouching player fits
//! just above it and a reached floor beside it is no more than a jump below.

use super::level::{Face, Level};
use super::reach::{self, CROUCH_HALF, GRID, JUMP, Reach};
use super::scene;
use super::transform::Transform;

/// Steeper than a floor, flatter than a wall.
const STEEPEST: f32 = 0.3;

/// Whether `face` is a slope a player slides on: upward, too steep to
/// stand on, and of something solid (the same as a floor, bar the slope).
pub fn is_slope(level: &Level, face: &Face) -> bool {
    face.normal_z > STEEPEST && face.normal_z <= 0.7 && reach::is_floor_kind(level, face)
}

/// The outline of every touched slope, as segments in image pixels: each
/// face's edges that no other touched slope face shares.
pub fn slope_edges(level: &Level, reach: &Reach, t: &Transform) -> Vec<[[f32; 2]; 2]> {
    let touched: Vec<&Vec<[f32; 3]>> = level
        .faces
        .iter()
        .filter(|f| f.points.len() >= 3 && is_slope(level, f) && touched(level, reach, &f.points))
        .map(|f| &f.points)
        .collect();
    scene::outline(&touched)
        .into_iter()
        .map(|[a, b]| [t.to_pixel(a[0], a[1]), t.to_pixel(b[0], b[1])])
        .collect()
}

/// Every cell centre inside the face's footprint (its centre for a face
/// smaller than a cell), with the face's height there.
fn samples(points: &[[f32; 3]], n: [f32; 3], d: f32) -> Vec<[f32; 3]> {
    let height = |x: f32, y: f32| (d - n[0] * x - n[1] * y) / n[2];
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for p in points {
        x0 = x0.min(p[0]);
        y0 = y0.min(p[1]);
        x1 = x1.max(p[0]);
        y1 = y1.max(p[1]);
    }
    let inside = |x: f32, y: f32| {
        let (mut pos, mut neg) = (true, true);
        for k in 0..points.len() {
            let (a, b) = (points[k], points[(k + 1) % points.len()]);
            let cross = (b[0] - a[0]) * (y - a[1]) - (b[1] - a[1]) * (x - a[0]);
            pos &= cross >= -1e-4;
            neg &= cross <= 1e-4;
        }
        pos || neg
    };
    let mut out = Vec::new();
    let mut x = (x0 / GRID).floor() * GRID + GRID / 2.0;
    while x <= x1 {
        let mut y = (y0 / GRID).floor() * GRID + GRID / 2.0;
        while y <= y1 {
            if inside(x, y) {
                out.push([x, y, height(x, y)]);
            }
            y += GRID;
        }
        x += GRID;
    }
    if out.is_empty() {
        let c = points.len() as f32;
        let (cx, cy) = points
            .iter()
            .fold((0.0, 0.0), |a, p| (a.0 + p[0] / c, a.1 + p[1] / c));
        out.push([cx, cy, height(cx, cy)]);
    }
    out
}

fn touched(level: &Level, reach: &Reach, points: &[[f32; 3]]) -> bool {
    let (n, d) = reach::plane_of(points);
    if n[2].abs() < 1e-3 {
        return false;
    }
    // How far the crouch box's lower corners sink into the slope.
    let rise = 16.0 * (n[0].abs() + n[1].abs()) / n[2].abs();
    samples(points, n, d).into_iter().any(|[x, y, z]| {
        let Some(cell) = reach.cell_at(x, y) else {
            return false;
        };
        let (i, j) = (cell / reach.ny, cell % reach.ny);
        let beside = [(i, j), (i + 1, j), (i - 1, j), (i, j + 1), (i, j - 1)]
            .into_iter()
            .filter(|&(a, b)| (0..reach.nx).contains(&a) && (0..reach.ny).contains(&b))
            .any(|(a, b)| {
                reach
                    .nodes_in(a * reach.ny + b)
                    .any(|k| reach.reached[k] && z <= reach.z[k] + JUMP)
            });
        beside && reach::fits(level, [x, y, z + rise + 2.0 + CROUCH_HALF])
    })
}
