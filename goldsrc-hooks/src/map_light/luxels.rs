//! A lightmapped surface's luxels: where each one sits in the world, and an
//! edit of their stored colour, for any per-map lighting change (the boxes
//! here; point lights, #1310, add their own edit).
//!
//! A world surface's baked light is `msurface_t.samples` (`com_model.h`): one
//! `color24` per luxel, `smax * tmax` of them for each light style the
//! surface has (`styles[]` up to the first 255), the styles' maps one after
//! the other. Luxels are 16 texels apart: luxel `(s, t)` sits at texture
//! coordinates `texturemins + 16 * (s, t)` (`R_AddDynamicLights` measures a
//! light's distance on that grid, pre-Anniversary `hw.dll+0x47110`). A point
//! on the surface's plane with those texture coordinates is its world
//! position: three linear equations (the two texture axes and the plane),
//! solved once per surface.

/// Texels between two luxels.
const LUXEL_SPACING: f32 = 16.0;
/// `MAXLIGHTMAPS`: a surface's style slots.
pub(crate) const MAX_STYLES: usize = 4;
/// A style slot past the surface's last style.
const NO_STYLE: u8 = 255;
/// Bytes per luxel per style (`color24`).
const BYTES_PER_LUXEL: usize = 3;
/// The engine refuses a lightmap bigger than this (`R_BuildLightMap`'s
/// "too large" check, 18 x 18 luxels, both builds).
pub(crate) const MAX_LUXELS: usize = 0x144;

/// What a surface's luxels need, copied out of the engine's `msurface_t`,
/// its `mtexinfo_t` and its `mplane_t`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Surface {
    pub texturemins: [i16; 2],
    pub extents: [i16; 2],
    pub styles: [u8; MAX_STYLES],
    /// The texture axes, `xyz` then offset (`mtexinfo_t.vecs`).
    pub vecs: [[f32; 4]; 2],
    /// The plane the surface lies on (`mplane_t.normal`, `.dist`).
    pub normal: [f32; 3],
    pub dist: f32,
    /// `SURF_PLANEBACK`: the surface faces away from its plane's normal
    /// (the map's two sides of a wall share one plane).
    pub back: bool,
}

impl Surface {
    /// Luxels across and down: `(extents >> 4) + 1`, as `R_BuildLightMap`
    /// works them out.
    pub(crate) fn size(&self) -> (usize, usize) {
        let side = |extent: i16| (extent.max(0) as usize >> 4) + 1;
        (side(self.extents[0]), side(self.extents[1]))
    }

    /// How many light styles the surface's samples hold.
    pub(crate) fn style_count(&self) -> usize {
        self.styles
            .iter()
            .position(|&style| style == NO_STYLE)
            .unwrap_or(MAX_STYLES)
    }

    /// The length of the surface's samples, in bytes, or `None` for a
    /// surface the engine would refuse to light.
    pub(crate) fn sample_len(&self) -> Option<usize> {
        let (smax, tmax) = self.size();
        (smax * tmax <= MAX_LUXELS).then(|| smax * tmax * self.style_count() * BYTES_PER_LUXEL)
    }

    /// The way the surface faces: its plane's normal, turned round for a
    /// surface on the plane's back.
    pub(crate) fn facing(&self) -> [f32; 3] {
        if self.back {
            self.normal.map(|n| -n)
        } else {
            self.normal
        }
    }

    /// The world positions of the surface's luxels, or `None` when its
    /// texture axes lie in its plane's normal direction (a broken texinfo:
    /// no point has one position).
    pub(crate) fn grid(&self) -> Option<LuxelGrid> {
        let a = [self.vecs[0][0], self.vecs[0][1], self.vecs[0][2]];
        let b = [self.vecs[1][0], self.vecs[1][1], self.vecs[1][2]];
        let c = self.normal;
        let (bc, ca, ab) = (cross(b, c), cross(c, a), cross(a, b));
        let det = dot(a, bc);
        if det.abs() < 1e-6 {
            return None;
        }
        // Cramer's rule: P = (rs * (b x c) + rt * (c x a) + rd * (a x b)) / det
        // for P.a = rs, P.b = rt, P.c = rd. Linear in (rs, rt), so the grid is
        // an origin plus a step per luxel along each axis.
        let solve = |rs: f32, rt: f32, rd: f32| -> [f32; 3] {
            std::array::from_fn(|i| (rs * bc[i] + rt * ca[i] + rd * ab[i]) / det)
        };
        let s0 = self.texturemins[0] as f32 - self.vecs[0][3];
        let t0 = self.texturemins[1] as f32 - self.vecs[1][3];
        let origin = solve(s0, t0, self.dist);
        let s_step = solve(LUXEL_SPACING, 0.0, 0.0);
        let t_step = solve(0.0, LUXEL_SPACING, 0.0);
        let (smax, tmax) = self.size();
        Some(LuxelGrid {
            origin,
            s_step,
            t_step,
            smax,
            tmax,
        })
    }
}

/// Where a surface's luxels are in the world (see [`Surface::grid`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LuxelGrid {
    origin: [f32; 3],
    s_step: [f32; 3],
    t_step: [f32; 3],
    pub smax: usize,
    pub tmax: usize,
}

impl LuxelGrid {
    /// Luxel `(s, t)`'s world position.
    pub(crate) fn position(&self, s: usize, t: usize) -> [f32; 3] {
        std::array::from_fn(|i| {
            self.origin[i] + s as f32 * self.s_step[i] + t as f32 * self.t_step[i]
        })
    }

    /// The smallest box holding every luxel: the grid is flat, so its four
    /// corners bound it.
    pub(crate) fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        let (s, t) = (self.smax - 1, self.tmax - 1);
        let corners = [
            self.position(0, 0),
            self.position(s, 0),
            self.position(0, t),
            self.position(s, t),
        ];
        let mut min = corners[0];
        let mut max = corners[0];
        for corner in &corners[1..] {
            for i in 0..3 {
                min[i] = min[i].min(corner[i]);
                max[i] = max[i].max(corner[i]);
            }
        }
        (min, max)
    }
}

/// Runs `edit` on every luxel of a surface's `samples`, with its world
/// position and style slot, once per style; `edit` returns whether it
/// changed the colour. Returns how many luxels changed in at least one
/// style. `samples` shorter than the grid needs changes nothing.
pub(crate) fn edit_luxels(
    grid: &LuxelGrid,
    styles: usize,
    samples: &mut [u8],
    mut edit: impl FnMut([f32; 3], usize, &mut [u8; 3]) -> bool,
) -> usize {
    let per_style = grid.smax * grid.tmax;
    if samples.len() < per_style * styles * BYTES_PER_LUXEL {
        return 0;
    }
    let mut changed = 0;
    for t in 0..grid.tmax {
        for s in 0..grid.smax {
            let position = grid.position(s, t);
            let luxel = t * grid.smax + s;
            let mut any = false;
            for style in 0..styles {
                let at = (style * per_style + luxel) * BYTES_PER_LUXEL;
                let mut rgb = [samples[at], samples[at + 1], samples[at + 2]];
                if edit(position, style, &mut rgb) {
                    samples[at..at + BYTES_PER_LUXEL].copy_from_slice(&rgb);
                    any = true;
                }
            }
            changed += usize::from(any);
        }
    }
    changed
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A floor at z = -288 textured on world x/y, as most floors are: 64 x 32
    /// world units from (1600, 100), so 5 x 3 luxels.
    pub(crate) fn floor() -> Surface {
        Surface {
            texturemins: [1600, 100],
            extents: [64, 32],
            styles: [0, NO_STYLE, NO_STYLE, NO_STYLE],
            vecs: [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0]],
            normal: [0.0, 0.0, 1.0],
            dist: -288.0,
            back: false,
        }
    }

    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        (0..3).all(|i| (a[i] - b[i]).abs() < 1e-3)
    }

    #[test]
    fn the_size_is_extents_over_16_plus_one() {
        assert_eq!(floor().size(), (5, 3));
    }

    #[test]
    fn samples_hold_every_style_map() {
        let mut surface = floor();
        assert_eq!(surface.sample_len(), Some(5 * 3 * 3));
        surface.styles = [0, 37, NO_STYLE, NO_STYLE];
        assert_eq!(surface.sample_len(), Some(5 * 3 * 3 * 2));
        surface.styles = [0, 1, 2, 3];
        assert_eq!(surface.style_count(), 4);
    }

    #[test]
    fn a_lightmap_the_engine_would_refuse_has_no_samples() {
        let mut surface = floor();
        surface.extents = [16 * 18, 16 * 18];
        assert_eq!(surface.sample_len(), None);
    }

    #[test]
    fn a_floors_luxels_sit_on_its_plane_16_units_apart() {
        let grid = floor().grid().expect("a floor has a grid");
        assert!(close(grid.position(0, 0), [1600.0, 100.0, -288.0]));
        assert!(close(grid.position(4, 2), [1664.0, 132.0, -288.0]));
    }

    #[test]
    fn a_wall_with_offset_and_scaled_texture_axes_lands_on_its_luxels() {
        // A wall facing -x at x = 1584: texture s along y at half a texel
        // per unit, with an offset; t down z.
        let wall = Surface {
            texturemins: [32, -128],
            extents: [32, 48],
            styles: [0, NO_STYLE, NO_STYLE, NO_STYLE],
            vecs: [[0.0, 0.5, 0.0, 7.0], [0.0, 0.0, -1.0, 3.0]],
            normal: [-1.0, 0.0, 0.0],
            dist: -1584.0,
            back: false,
        };
        let grid = wall.grid().expect("a wall has a grid");
        for (s, t) in [(0, 0), (2, 3), (1, 2)] {
            let p = grid.position(s, t);
            let texel_s = p[1] * 0.5 + 7.0;
            let texel_t = -p[2] + 3.0;
            assert!((p[0] - 1584.0).abs() < 1e-3, "{p:?}");
            assert!((texel_s - (32.0 + 16.0 * s as f32)).abs() < 1e-3, "{p:?}");
            assert!((texel_t - (-128.0 + 16.0 * t as f32)).abs() < 1e-3, "{p:?}");
        }
    }

    #[test]
    fn a_surface_on_its_planes_back_faces_the_other_way() {
        let mut ceiling = floor();
        assert_eq!(ceiling.facing(), [0.0, 0.0, 1.0]);
        ceiling.back = true;
        assert_eq!(ceiling.facing(), [0.0, 0.0, -1.0]);
        // The same plane, so the same luxel positions.
        assert_eq!(ceiling.grid(), floor().grid());
    }

    #[test]
    fn texture_axes_along_the_normal_have_no_grid() {
        let mut broken = floor();
        broken.vecs = [[0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 1.0, 0.0]];
        assert_eq!(broken.grid(), None);
    }

    #[test]
    fn the_bounds_are_the_grids_corners() {
        let grid = floor().grid().expect("grid");
        let (min, max) = grid.bounds();
        assert!(close(min, [1600.0, 100.0, -288.0]));
        assert!(close(max, [1664.0, 132.0, -288.0]));
    }

    #[test]
    fn an_edit_sees_every_luxel_in_every_style_and_counts_luxels_changed() {
        let mut surface = floor();
        surface.styles = [0, 37, NO_STYLE, NO_STYLE];
        let grid = surface.grid().expect("grid");
        let mut samples = vec![10u8; surface.sample_len().expect("len")];
        let mut seen = 0;
        let changed = edit_luxels(&grid, 2, &mut samples, |p, style, rgb| {
            seen += 1;
            // Only the first column, only style 1.
            if p[0] < 1601.0 && style == 1 {
                rgb[0] = 99;
                return true;
            }
            false
        });
        assert_eq!(seen, 5 * 3 * 2);
        assert_eq!(changed, 3);
        // Style 1's map starts after style 0's 15 luxels; the first column
        // is luxels 0, 5 and 10.
        for luxel in [0, 5, 10] {
            assert_eq!(samples[(15 + luxel) * 3], 99);
        }
        assert_eq!(samples.iter().filter(|&&b| b == 99).count(), 3);
    }

    #[test]
    fn samples_too_short_for_the_grid_change_nothing() {
        let grid = floor().grid().expect("grid");
        let mut samples = vec![10u8; 4];
        assert_eq!(edit_luxels(&grid, 1, &mut samples, |_, _, _| true), 0);
    }
}
