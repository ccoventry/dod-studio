// patch/decal_strip/geometry.rs
// Decal wire layouts, plane fitting and tiling: pure functions over
// coordinates, with no demo or map access of their own.

use dem::types::{EngineMessage, NetMessage, SvcTempEntity, TempEntity};

/// TE_WORLDDECAL. 7 bytes: 3 × WRITE_COORD + 1 × WRITE_BYTE texture index.
/// Chosen as the flush burst's carrier message because it takes no entity index
/// and — unlike TE_GUNSHOTDECAL — plays no ricochet sound. Note this is the
/// message type, independent of which texture index it is asked to draw.
const TE_WORLDDECAL: u8 = 116;

fn decode_coord(b: &[u8]) -> f32 {
    i16::from_le_bytes([b[0], b[1]]) as f32 / 8.0
}

fn encode_coord(v: f32) -> [u8; 2] {
    ((v * 8.0).round() as i16).to_le_bytes()
}

/// Texture index carried by a decal temp entity, by wire layout.
/// TE_WORLDDECAL/HIGH: coord(6) + index. TE_GUNSHOTDECAL/TE_DECALHIGH:
/// coord(6) + entity(2) + index. TE_DECAL: coord(6) + index + entity(2).
pub(in crate::patch) fn decal_texture_index(entity_type: u8, payload: &[u8]) -> Option<u8> {
    match entity_type {
        116 | 117 if payload.len() >= 7 => Some(payload[6]),
        104 if payload.len() >= 7 => Some(payload[6]),
        109 | 118 if payload.len() >= 9 => Some(payload[8]),
        _ => None,
    }
}

/// Whether a decal was stamped onto world geometry, and therefore whether its
/// coordinate stays true outside the demo that produced it.
///
/// This only matters for `decal_atlas`. Within one demo a mark on a door is a
/// serviceable flush position, because the door is wherever the demo last left
/// it. In a store that outlives the demo it is a coordinate that will one day
/// point at the air a door used to occupy, and a flush position that misses
/// allocates no ring slot.
///
/// Layouts are the ones documented on `decal_texture_index`. Anything not
/// listed stays out of the atlas: it still serves this demo, it simply never
/// becomes a durable claim about the map.
pub(super) fn is_world_decal(entity_type: u8, payload: &[u8]) -> bool {
    let entity_at = |i: usize| -> Option<u16> {
        payload
            .get(i..i + 2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
    };
    match entity_type {
        // TE_WORLDDECAL / HIGH carry no entity field at all: world by
        // construction, which is also why the flush emits this type.
        116 | 117 => true,
        // coord(6) + entity(2) + index(1)
        109 | 118 => entity_at(6) == Some(0),
        // coord(6) + index(1) + entity(2)
        104 => entity_at(7) == Some(0),
        // TE_BSPDECAL: coord(6) + 16-bit texture index(2) + entity(2)
        13 => entity_at(8) == Some(0),
        _ => false,
    }
}

pub(super) fn decal_position(payload: &[u8]) -> Option<[f32; 3]> {
    if payload.len() < 6 {
        return None;
    }
    Some([
        decode_coord(&payload[0..2]),
        decode_coord(&payload[2..4]),
        decode_coord(&payload[4..6]),
    ])
}

pub(in crate::patch) fn distance(a: &[f32; 3], b: &[f32; 3]) -> f32 {
    let (dx, dy, dz) = (a[0] - b[0], a[1] - b[1], a[2] - b[2]);
    (dx * dx + dy * dy + dz * dz).sqrt()
}

// ── Plane geometry ───────────────────────────────────────────────────────────
// Shared with `decal_probe`, which measures on the same fitted planes this
// tiles across. Kept here because the dependency runs probe -> strip.

/// Groups values into runs no wider than `tolerance`, returning each run's mean
/// and its members' indices.
///
/// A sweep rather than bucket-rounding: rounding puts two values a hair apart
/// into different buckets whenever they straddle a boundary, which would split
/// one surface into two undersized patches and lose it to a minimum-size check.
pub(in crate::patch) fn cluster(values: &[f32], tolerance: f32) -> Vec<(f32, Vec<usize>)> {
    let mut order: Vec<usize> = (0..values.len()).collect();
    order.sort_by(|&a, &b| {
        values[a]
            .partial_cmp(&values[b])
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut out: Vec<(f32, Vec<usize>)> = Vec::new();
    let mut current: Vec<usize> = Vec::new();
    let mut anchor = f32::NAN;

    for idx in order {
        let v = values[idx];
        if current.is_empty() {
            anchor = v;
            current.push(idx);
        } else if (v - anchor).abs() <= tolerance {
            current.push(idx);
        } else {
            let mean = current.iter().map(|&i| values[i]).sum::<f32>() / current.len() as f32;
            out.push((mean, std::mem::take(&mut current)));
            anchor = v;
            current.push(idx);
        }
    }
    if !current.is_empty() {
        let mean = current.iter().map(|&i| values[i]).sum::<f32>() / current.len() as f32;
        out.push((mean, current));
    }
    out
}

/// The two axes that lie in a plane whose normal runs along `axis`.
pub(in crate::patch) fn tangent_axes(axis: usize) -> (usize, usize) {
    match axis {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    }
}

pub(in crate::patch) fn extent(members: &[[f32; 3]], ax: usize) -> f32 {
    let mut lo = f32::INFINITY;
    let mut hi = f32::NEG_INFINITY;
    for m in members {
        lo = lo.min(m[ax]);
        hi = hi.max(m[ax]);
    }
    if lo.is_finite() { hi - lo } else { 0.0 }
}

/// Splits a coplanar set into spatially connected patches.
///
/// Coplanar is not contiguous. Every floor in a map that happens to sit at the
/// same height lands in one Z cluster — the first run of this picked exactly
/// that: a "plane" whose decals spanned 2173 x 5273 units across the whole map.
/// A grid centred anywhere in it would have had columns hanging in mid-air over
/// a different room. Linking members that sit within `radius` of each other is
/// what makes "there is surface between these two decals" a defensible claim.
pub(in crate::patch) fn connected_patches(members: &[[f32; 3]], radius: f32) -> Vec<Vec<[f32; 3]>> {
    let n = members.len();
    let mut seen = vec![false; n];
    let mut out = Vec::new();

    for start in 0..n {
        if seen[start] {
            continue;
        }
        seen[start] = true;
        let mut stack = vec![start];
        let mut patch = Vec::new();
        while let Some(i) = stack.pop() {
            patch.push(members[i]);
            for j in 0..n {
                if !seen[j] && distance(&members[i], &members[j]) <= radius {
                    seen[j] = true;
                    stack.push(j);
                }
            }
        }
        out.push(patch);
    }
    out
}

// ── Tiling ───────────────────────────────────────────────────────────────────

/// Coplanarity tolerance when grouping decals into a candidate surface. Matches
/// the probe's: a decal sits on the plane, not near it, so this only has to
/// absorb coordinate quantisation.
const PLANE_TOLERANCE: f32 = 2.0;

/// How close two decals must be to count as evidence of the same continuous
/// patch of surface. See `connected_patches` for why coplanar alone is not
/// enough.
const PATCH_LINK_RADIUS: f32 = 160.0;

/// Decals needed before a patch is believed to be a real surface rather than a
/// coincidence of two stray marks.
const MIN_PATCH_DECALS: usize = 4;

/// Spacing between tiled positions.
///
/// `m_Size` for the small bullet hole was measured at ~4 units, and that is the
/// decal's own radius — two of them overlap only if their centres come within
/// ~8 units. A 16-unit pitch is twice that, so no tile can be recycled as an
/// overlap of its neighbour, which is the failure that stops the ring advancing.
const TILE_PITCH: f32 = 16.0;

/// Cap on how far a tiled grid may spread from its patch centre along either
/// in-plane axis.
///
/// Movement along a plane is unconstrained as far as the engine is concerned —
/// synthesised positions 224 units apart all created decals, two of them with
/// no real decal within 30 units. The cap is not an engine limit but a
/// confidence one: the further a tile sits from the decals proving the surface,
/// the more it is inference. ~200 units keeps a grid inside the room its
/// evidence came from.
const TILE_MAX_EXTENT: f32 = 200.0;

/// How close a tile must come to a real decal on its own patch to be kept.
///
/// A tile that lands past the end of a wall hits nothing, and a position that
/// creates no decal allocates no pool slot — so the sweep silently comes up
/// short rather than failing. Dilating the proven decals by this much is the
/// compromise between that risk and the position count the ring needs.
/// How far from a coordinate to look for the surface the engine will draw on.
///
/// Deliberately generous. This is not a claim about where `R_DecalShoot` will
/// place a decal — it is the radius within which a face has to be CLEARED as
/// not-in-shot before the candidate is trusted. Erring long costs candidates;
/// erring short is how a decal lands on a face nobody tested. The fine sweep
/// measured the engine accepting positions ~3 units off a face, so this is
/// that with a little margin.
///
/// It does double duty. A candidate with no face inside it is not a decal spot
/// at all — tiling lays a grid across a fitted plane, and a plane runs on past
/// the brush that proved it, so some tiles land in open air where no decal can
/// appear. Those are dropped here rather than counted into a sweep that then
/// turns fewer ring slots than it reports.
pub(super) const DECAL_PROJECTION_REACH: f32 = 4.0;

/// How far off the face the drawn point is placed for testing. The decal is
/// visible from this side, and a trace ending here is not stopped by the face
/// it sits on — comfortably clear of the tree's own 0.03125 plane epsilon.
pub(super) const DECAL_SURFACE_LIFT: f32 = 1.0;

const TILE_REACH: f32 = 64.0;

/// Ceiling on tiles generated per patch.
///
/// `TILE_MAX_EXTENT` and `TILE_PITCH` already bound a grid at 13x13, so at the
/// current values this cannot trigger. It exists so that widening the extent or
/// tightening the pitch cannot quietly turn one densely-shot wall into tens of
/// thousands of candidates for the camera filters to score.
const MAX_TILES_PER_PATCH: usize = 512;

/// Positions tiled across the planes the demo's own decals prove exist.
///
/// The flush needs one distinct position per few ring slots, and harvesting
/// them one-per-real-decal never yielded enough — a 256-slot ring wants 68
/// positions and a busy demo offered 30. Tiling is what closes that: the engine
/// does not care how far a decal sits from another along a surface, only that
/// there IS surface, so one patch of proven wall can carry a whole grid.
///
/// Returned in no particular order; the caller ranks them by camera clearance
/// and enforces spacing across the whole pool.
pub(super) fn tile_positions(harvested: &[[f32; 3]]) -> Vec<[f32; 3]> {
    let mut out = Vec::new();

    for axis in 0..3 {
        let (t1, t2) = tangent_axes(axis);
        let values: Vec<f32> = harvested.iter().map(|p| p[axis]).collect();

        for (value, idxs) in cluster(&values, PLANE_TOLERANCE) {
            if idxs.len() < MIN_PATCH_DECALS {
                continue;
            }
            let coplanar: Vec<[f32; 3]> = idxs.iter().map(|&i| harvested[i]).collect();

            for patch in connected_patches(&coplanar, PATCH_LINK_RADIUS) {
                if patch.len() < MIN_PATCH_DECALS {
                    continue;
                }
                tile_patch(&patch, axis, value, t1, t2, &mut out);
            }
        }
    }

    out
}

/// Lays a grid over one patch, keeping only the tiles its decals vouch for.
fn tile_patch(
    patch: &[[f32; 3]],
    axis: usize,
    value: f32,
    t1: usize,
    t2: usize,
    out: &mut Vec<[f32; 3]>,
) {
    // Centred on the patch's own centre of mass rather than its bounding box,
    // so the extent cap is spent where the evidence actually is. A wall with
    // one stray mark 400 units down its length would otherwise drag the grid
    // half way to nothing.
    let centre = |ax: usize| patch.iter().map(|m| m[ax]).sum::<f32>() / patch.len() as f32;
    let half = TILE_MAX_EXTENT / 2.0;

    let span = |ax: usize| -> (f32, f32) {
        let c = centre(ax);
        let lo = patch.iter().fold(f32::INFINITY, |a, m| a.min(m[ax]));
        let hi = patch.iter().fold(f32::NEG_INFINITY, |a, m| a.max(m[ax]));
        (lo.max(c - half), hi.min(c + half))
    };

    let (lo1, hi1) = span(t1);
    let (lo2, hi2) = span(t2);

    let steps = |lo: f32, hi: f32| -> usize { ((hi - lo) / TILE_PITCH).floor() as usize + 1 };
    let (n1, n2) = (steps(lo1, hi1), steps(lo2, hi2));

    let mut placed = 0usize;
    for i in 0..n1 {
        for j in 0..n2 {
            if placed >= MAX_TILES_PER_PATCH {
                return;
            }
            let mut p = [0.0f32; 3];
            // Straight onto the fitted plane. The fine sweep put that plane
            // ~0.5 units proud of the true BSP one, well inside the ~3 units of
            // slack `R_DecalShoot`'s walk allows, so no offset is applied —
            // guessing at one is how a whole sweep lands in mid-air.
            p[axis] = value;
            p[t1] = lo1 + i as f32 * TILE_PITCH;
            p[t2] = lo2 + j as f32 * TILE_PITCH;

            if patch.iter().any(|m| distance(m, &p) <= TILE_REACH) {
                out.push(p);
                placed += 1;
            }
        }
    }
}

pub(in crate::patch) fn build_world_decal(pos: &[f32; 3], texture_index: u8) -> NetMessage {
    let mut payload = Vec::with_capacity(7);
    payload.extend_from_slice(&encode_coord(pos[0]));
    payload.extend_from_slice(&encode_coord(pos[1]));
    payload.extend_from_slice(&encode_coord(pos[2]));
    payload.push(texture_index);

    NetMessage::EngineMessage(Box::new(EngineMessage::SvcTempEntity(SvcTempEntity {
        entity_type: TE_WORLDDECAL,
        entity: TempEntity::TeWorldDecal(payload),
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::patch::decal_strip::MIN_POSITION_SPACING;

    /// A patch of decals on an upright wall at x = `plane`.
    fn wall_patch(plane: f32, ys: &[f32], zs: &[f32]) -> Vec<[f32; 3]> {
        let mut out = Vec::new();
        for &y in ys {
            for &z in zs {
                out.push([plane, y, z]);
            }
        }
        out
    }

    #[test]
    fn the_spacing_filter_cannot_decimate_a_tiled_grid() {
        // Tiles are laid at TILE_PITCH and then every candidate has to clear
        // MIN_POSITION_SPACING against the pool. If the spacing ever exceeded
        // the pitch, neighbouring tiles would reject each other and tiling
        // would quietly stop multiplying positions at all.
        assert!(
            MIN_POSITION_SPACING < TILE_PITCH,
            "spacing {} must stay under the tile pitch {}",
            MIN_POSITION_SPACING,
            TILE_PITCH
        );
        // And the pitch must clear the engine's overlap distance, which is
        // twice the measured ~4-unit decal radius. Inside that, the engine
        // recycles instead of allocating and the ring stops turning.
        assert!(TILE_PITCH > 8.0, "tiles would overlap and be recycled");
    }

    #[test]
    fn tiling_multiplies_positions_across_a_proven_plane() {
        let members = wall_patch(100.0, &[0.0, 20.0, 40.0, 60.0], &[0.0, 20.0]);
        let tiles = tile_positions(&members);

        assert!(
            tiles.len() > members.len(),
            "tiling produced {} positions from {} decals — the whole point is a grid per patch",
            tiles.len(),
            members.len()
        );
        for t in &tiles {
            assert!(
                (t[0] - 100.0).abs() < 0.001,
                "tile {:?} left the fitted plane — it would miss the wall entirely",
                t
            );
        }
    }

    #[test]
    fn tiles_stay_within_reach_of_a_real_decal() {
        // A tile past the end of a wall hits nothing, and a position that
        // creates no decal allocates no ring slot — so the sweep comes up short
        // silently rather than failing.
        let members = wall_patch(100.0, &[0.0, 20.0, 40.0, 60.0], &[0.0, 20.0]);
        let tiles = tile_positions(&members);

        for t in &tiles {
            let nearest = members
                .iter()
                .map(|m| distance(m, t))
                .fold(f32::INFINITY, f32::min);
            assert!(
                nearest <= TILE_REACH,
                "tile {:?} sits {:.1} units from any proven decal",
                t,
                nearest
            );
        }
    }

    #[test]
    fn coplanar_but_distant_groups_are_not_bridged() {
        // Coplanar is not contiguous: two stretches of wall at the same x with
        // a doorway between them must not get tiles hung across the gap.
        let mut members = wall_patch(100.0, &[0.0, 20.0, 40.0, 60.0], &[0.0, 20.0]);
        members.extend(wall_patch(
            100.0,
            &[900.0, 920.0, 940.0, 960.0],
            &[0.0, 20.0],
        ));

        let tiles = tile_positions(&members);
        assert!(!tiles.is_empty());

        for t in &tiles {
            let in_gap = t[1] > 60.0 + TILE_REACH && t[1] < 900.0 - TILE_REACH;
            assert!(!in_gap, "tile {:?} hangs in the gap between two patches", t);
        }
    }

    #[test]
    fn a_long_wall_is_capped_at_the_tiling_extent() {
        // The cap is a confidence limit, not an engine one: the further a tile
        // sits from the decals proving the surface, the more it is inference.
        let ys: Vec<f32> = (0..13).map(|i| i as f32 * 50.0).collect();
        let members = wall_patch(100.0, &ys, &[0.0]);

        let tiles = tile_positions(&members);
        assert!(!tiles.is_empty());

        let lo = tiles.iter().fold(f32::INFINITY, |a, t| a.min(t[1]));
        let hi = tiles.iter().fold(f32::NEG_INFINITY, |a, t| a.max(t[1]));
        assert!(
            hi - lo <= TILE_MAX_EXTENT + TILE_PITCH,
            "tiles spanned {:.0} units across a 600-unit wall; the cap is {}",
            hi - lo,
            TILE_MAX_EXTENT
        );
    }

    #[test]
    fn a_couple_of_stray_marks_are_not_treated_as_a_surface() {
        // Two decals prove two points, not a plane worth tiling.
        let members = vec![[100.0, 0.0, 0.0], [100.0, 20.0, 0.0]];
        assert!(tile_positions(&members).is_empty());
    }
}
