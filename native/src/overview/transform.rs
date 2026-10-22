//! Where a world position lands on an overview image, the way DoD's client
//! draws it (`whamemer/dod13-client` `cl_dll/dod_map.cpp`,
//! `CHudDoDMap::DrawOverviewLayer` and `DrawOverviewEntities`).
//!
//! The image is cut into 128-pixel tiles, 8 across and 6 down for 1024x768.
//! Rows step world x *down* from `ORIGIN.x + 4096 / (1.33 * ZOOM)` across
//! `6144 / ZOOM`; columns step world y down from `ORIGIN.y + 4096 / ZOOM`
//! across `8192 * (4/3) / (1.33 * ZOOM)`. `ROTATED 1` first turns the world
//! a quarter turn about `ORIGIN`, as the blips are turned. Checked against
//! the stock overviews of dod_harrington, dod_anzio and the rotated
//! dod_cevo_russka_mtek: BSP floors drawn through their `.txt` sit on the
//! images.

use serde::{Deserialize, Serialize};

/// The client's own constant: 1.33, not 4/3.
const ASPECT: f32 = 1.33;

pub const WIDTH: u32 = 1024;
pub const HEIGHT: u32 = 768;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Transform {
    pub zoom: f32,
    pub origin: [f32; 3],
    pub rotated: bool,
    /// The layer's `HEIGHT`: which layer is used where. One layer, so any.
    pub height: f32,
}

impl Transform {
    /// `(x, y)` in image pixels, 1024x768.
    pub fn to_pixel(&self, x: f32, y: f32) -> [f32; 2] {
        let (ox, oy) = (self.origin[0], self.origin[1]);
        let (x, y) = if self.rotated {
            (ox + (y - oy), oy - (x - ox))
        } else {
            (x, y)
        };
        let z = self.zoom;
        let v = (ox + 4096.0 / (ASPECT * z) - x) / (6144.0 / z);
        let u = (oy + 4096.0 / z - y) / (8192.0 * 4.0 / 3.0 / (ASPECT * z));
        [u * WIDTH as f32, v * HEIGHT as f32]
    }

    /// The world `(x, y)` under an image pixel.
    pub fn to_world(&self, px: f32, py: f32) -> [f32; 2] {
        let (ox, oy) = (self.origin[0], self.origin[1]);
        let z = self.zoom;
        let (u, v) = (px / WIDTH as f32, py / HEIGHT as f32);
        let x = ox + 4096.0 / (ASPECT * z) - v * 6144.0 / z;
        let y = oy + 4096.0 / z - u * 8192.0 * 4.0 / 3.0 / (ASPECT * z);
        if self.rotated {
            [ox + oy - y, x - ox + oy]
        } else {
            [x, y]
        }
    }

    /// The `.txt` the game reads, naming `image` (`overviews/<map>.tga`).
    pub fn text(&self, map: &str, image: &str) -> String {
        format!(
            "// overview description file for {map}.bsp, made by DoD Studio\n\n\
             global \n{{\n\tZOOM\t{:.2}\n\tORIGIN\t{:.2}\t{:.2}\t{:.2}\n\tROTATED\t{}\n}}\n\n\
             layer \n{{\n\tIMAGE\t\"{image}\"\n\tHEIGHT\t{:.2}\n}}\n",
            self.zoom,
            self.origin[0],
            self.origin[1],
            self.origin[2],
            self.rotated as i32,
            self.height
        )
    }

    /// Reads a `.txt`, comments and all.
    pub fn parse(text: &str) -> Option<Transform> {
        let mut zoom = None;
        let mut origin = None;
        let mut rotated = false;
        let mut height = 0.0;
        for line in text.lines() {
            let line = line.split("//").next().unwrap_or("");
            let mut words = line.split_whitespace();
            match words.next().map(|w| w.to_ascii_uppercase()).as_deref() {
                Some("ZOOM") => zoom = words.next().and_then(|w| w.parse().ok()),
                Some("ORIGIN") => {
                    let v: Vec<f32> = words.filter_map(|w| w.parse().ok()).collect();
                    if v.len() >= 3 {
                        origin = Some([v[0], v[1], v[2]]);
                    }
                }
                Some("ROTATED") => {
                    rotated = words.next().and_then(|w| w.parse::<f32>().ok()) == Some(1.0)
                }
                Some("HEIGHT") => height = words.next().and_then(|w| w.parse().ok()).unwrap_or(0.0),
                _ => {}
            }
        }
        Some(Transform {
            zoom: zoom?,
            origin: origin?,
            rotated,
            height,
        })
    }

    /// The largest zoom (two decimals, as written) that fits `points` inside
    /// the image with `margin` pixels to spare, in whichever orientation
    /// fits it larger, centred.
    pub fn fit(points: &[[f32; 2]], margin: f32) -> Option<Transform> {
        let (mut x0, mut x1, mut y0, mut y1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
        for p in points {
            x0 = x0.min(p[0]);
            x1 = x1.max(p[0]);
            y0 = y0.min(p[1]);
            y1 = y1.max(p[1]);
        }
        if points.is_empty() {
            return None;
        }
        let centre = [(x0 + x1) / 2.0, (y0 + y1) / 2.0];
        let mut best: Option<Transform> = None;
        for rotated in [false, true] {
            for step in (20..=400).rev() {
                let zoom = step as f32 / 100.0;
                let mut t = Transform {
                    zoom,
                    origin: [centre[0], centre[1], 0.0],
                    rotated,
                    height: 0.0,
                };
                // Centre it: the pixel position is affine in the origin.
                let base = t.to_pixel(centre[0], centre[1]);
                t.origin[0] += 1.0;
                let dx = t.to_pixel(centre[0], centre[1]);
                t.origin[0] -= 1.0;
                t.origin[1] += 1.0;
                let dy = t.to_pixel(centre[0], centre[1]);
                t.origin[1] -= 1.0;
                let j = [
                    [dx[0] - base[0], dy[0] - base[0]],
                    [dx[1] - base[1], dy[1] - base[1]],
                ];
                let det = j[0][0] * j[1][1] - j[0][1] * j[1][0];
                if det.abs() < 1e-9 {
                    continue;
                }
                let want = [WIDTH as f32 / 2.0 - base[0], HEIGHT as f32 / 2.0 - base[1]];
                let sx = (want[0] * j[1][1] - want[1] * j[0][1]) / det;
                let sy = (j[0][0] * want[1] - j[1][0] * want[0]) / det;
                t.origin[0] = ((centre[0] + sx) * 100.0).round() / 100.0;
                t.origin[1] = ((centre[1] + sy) * 100.0).round() / 100.0;
                let fits = [[x0, y0], [x0, y1], [x1, y0], [x1, y1]].iter().all(|c| {
                    let p = t.to_pixel(c[0], c[1]);
                    p[0] >= margin
                        && p[0] <= WIDTH as f32 - margin
                        && p[1] >= margin
                        && p[1] <= HEIGHT as f32 - margin
                });
                if fits {
                    if best.is_none_or(|b| zoom > b.zoom) {
                        best = Some(t);
                    }
                    break;
                }
            }
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// dod_harrington's stock `.txt`, whose image the BSP was checked against.
    fn harrington() -> Transform {
        Transform::parse(
            "// overview description file for dod_harrington\n\nglobal \n{\n\tZOOM\t\t1.45\n\
             \tORIGIN\t\t20.0\t-752.00\t-40.00\n\tROTATED\t\t0\n}\n\nlayer \n{\n\
             \tIMAGE\t\"overviews/dod_harrington.bmp\"\n\tHEIGHT\t-481.00\n}\n",
        )
        .unwrap()
    }

    #[test]
    fn reads_the_stock_text() {
        let t = harrington();
        assert_eq!(t.zoom, 1.45);
        assert_eq!(t.origin, [20.0, -752.0, -40.0]);
        assert!(!t.rotated);
        assert_eq!(t.height, -481.0);
    }

    #[test]
    fn world_x_runs_up_the_image_and_y_runs_left() {
        let t = harrington();
        let a = t.to_pixel(0.0, 0.0);
        let up = t.to_pixel(100.0, 0.0);
        let left = t.to_pixel(0.0, 100.0);
        assert!(up[1] < a[1] && (up[0] - a[0]).abs() < 1e-3);
        assert!(left[0] < a[0] && (left[1] - a[1]).abs() < 1e-3);
    }

    #[test]
    fn to_world_undoes_to_pixel_both_ways_round() {
        for rotated in [false, true] {
            let t = Transform {
                rotated,
                ..harrington()
            };
            for (x, y) in [(0.0, 0.0), (-1432.0, -1520.0), (1056.0, 608.0)] {
                let p = t.to_pixel(x, y);
                let w = t.to_world(p[0], p[1]);
                assert!(
                    (w[0] - x).abs() < 0.05 && (w[1] - y).abs() < 0.05,
                    "{rotated} {w:?}"
                );
            }
        }
    }

    #[test]
    fn rotated_turns_the_world_about_the_origin() {
        let t = Transform {
            rotated: true,
            ..harrington()
        };
        let a = t.to_pixel(20.0, -752.0);
        let east = t.to_pixel(120.0, -752.0);
        // Unrotated, +x goes up; rotated, it goes right.
        assert!(east[0] > a[0] && (east[1] - a[1]).abs() < 1e-3);
    }

    #[test]
    fn the_text_round_trips() {
        let t = harrington();
        let again = Transform::parse(&t.text("dod_harrington", "overviews/dod_harrington.tga"));
        assert_eq!(again, Some(t));
    }

    #[test]
    fn fit_keeps_everything_inside_the_margin() {
        let points = [[-2000.0, -3000.0], [2000.0, 1500.0], [0.0, 0.0]];
        let t = Transform::fit(&points, 24.0).unwrap();
        for p in points {
            let px = t.to_pixel(p[0], p[1]);
            assert!(px[0] >= 24.0 && px[0] <= 1000.0 && px[1] >= 24.0 && px[1] <= 744.0);
        }
        // Taller in y than x: unrotated puts y across, the wide way.
        assert!(!t.rotated);
    }
}
