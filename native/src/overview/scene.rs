//! What the overview page draws: the reachable floors as polygons in image
//! pixels, each in an area with a default colour; water; capture zones;
//! flags and spawns. The page renders and edits it; nothing here draws.
//!
//! Areas: each grid cell's top reachable floor is outdoor if a line straight
//! up from it reaches sky, indoor otherwise. Connected cells of the same
//! kind (and, outdoors, the same height band) at similar heights make one
//! area. A face belongs to the area most of its cells landed in.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use super::level::{CONTENTS_EMPTY, CONTENTS_SKY, CONTENTS_SOLID, Level};
use super::reach::{self, Reach};
use super::transform::Transform;

/// The colours of the user's own hand-edited dod_harrington overview, which
/// is the style these follow (sampled from its pixels).
pub const BACKGROUND: [u8; 3] = [0, 255, 0];
pub const VOID: [u8; 3] = [16, 17, 14];
pub const OUTDOOR: [[u8; 3]; 3] = [[94, 94, 85], [145, 145, 130], [201, 192, 169]];
pub const INDOOR: [[u8; 3]; 4] = [
    [146, 155, 247],
    [125, 29, 55],
    [138, 222, 117],
    [213, 227, 79],
];
pub const SMALL: [u8; 3] = [186, 157, 128];
pub const STAIRS: [u8; 3] = [255, 255, 255];
pub const WATER: [u8; 3] = [64, 208, 213];

/// The palette the page offers, in this order.
pub const PALETTE: [[u8; 3]; 12] = [
    OUTDOOR[0], OUTDOOR[1], OUTDOOR[2], INDOOR[0], INDOOR[1], INDOOR[2], INDOOR[3], SMALL, STAIRS,
    WATER, VOID, BACKGROUND,
];

/// Indoor areas smaller than this many cells take [`SMALL`].
const SMALL_CELLS: usize = 30;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Area {
    pub id: u32,
    pub indoor: bool,
    pub colour: [u8; 3],
    /// A world point inside the area, for keeping edits across a rebuild.
    pub anchor: [f32; 2],
    pub cells: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneFace {
    /// Image pixels, 1024x768.
    pub points: Vec<[f32; 2]>,
    /// Draw order: lowest first.
    pub z: f32,
    pub area: u32,
    /// A stair tread: drawn white by default.
    pub stairs: bool,
    /// The face's index in the BSP, so an edit to one face survives a rebuild.
    pub face: u32,
}

/// A water surface, seen from above.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Water {
    /// Image pixels, 1024x768.
    pub points: Vec<[f32; 2]>,
    /// Its height: floors above it (a bridge) are drawn over it.
    pub z: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Marker {
    pub name: String,
    /// Image pixels.
    pub at: [f32; 2],
    /// World position, for keeping edits.
    pub world: [f32; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scene {
    pub map: String,
    pub transform: Transform,
    pub width: u32,
    pub height: u32,
    pub faces: Vec<SceneFace>,
    pub areas: Vec<Area>,
    pub water: Vec<Water>,
    /// Each capture area's footprint as polygons; the page draws the rim of
    /// their union.
    pub cap_zones: Vec<Vec<Vec<[f32; 2]>>>,
    /// The outline of every drawn floor that can be broken (a breakable
    /// floor or cover), as line segments in image pixels: drawn dotted.
    #[serde(default)]
    pub breakable_edges: Vec<[[f32; 2]; 2]>,
    /// The outline of slopes too steep to stand on that a player still
    /// gets onto and slides off (`slopes.rs`): drawn as an outline only.
    #[serde(default)]
    pub slope_edges: Vec<[[f32; 2]; 2]>,
    /// The map file's checksum, as the engine computes it: edits made on
    /// another version of the map are told apart by it.
    #[serde(default)]
    pub checksum: u32,
    pub flags: Vec<Marker>,
    pub allies: Vec<Marker>,
    pub axis: Vec<Marker>,
    pub palette: Vec<[u8; 3]>,
    pub background: [u8; 3],
    pub void: [u8; 3],
}

/// Is a line up from `p` open to the sky?
fn outdoor(level: &Level, p: [f32; 3]) -> bool {
    let mut q = p;
    for _ in 0..160 {
        match level.point_contents(q) {
            CONTENTS_SKY => return true,
            CONTENTS_SOLID => return false,
            _ => q[2] += 32.0,
        }
    }
    false
}

fn percentile(sorted: &[f32], q: f32) -> f32 {
    if sorted.is_empty() {
        return 0.0;
    }
    let at = (q * (sorted.len() - 1) as f32).round() as usize;
    sorted[at.min(sorted.len() - 1)]
}

/// A narrow face with a reachable floor a step above on one side and a step
/// below on the other.
fn is_tread(points: &[[f32; 3]], reach: &Reach) -> bool {
    if points.len() < 3 {
        return false;
    }
    let n = points.len() as f32;
    let (cx, cy, cz) = points.iter().fold((0.0, 0.0, 0.0), |a, p| {
        (a.0 + p[0] / n, a.1 + p[1] / n, a.2 + p[2] / n)
    });
    // Extents along the polygon's principal axes.
    let (mut sxx, mut sxy, mut syy) = (0.0f32, 0.0f32, 0.0f32);
    for p in points {
        let (dx, dy) = (p[0] - cx, p[1] - cy);
        sxx += dx * dx;
        sxy += dx * dy;
        syy += dy * dy;
    }
    let angle = 0.5 * (2.0 * sxy).atan2(sxx - syy);
    let (c, s) = (angle.cos(), angle.sin());
    let (mut a0, mut a1, mut b0, mut b1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
    for p in points {
        let (dx, dy) = (p[0] - cx, p[1] - cy);
        let a = dx * c + dy * s;
        let b = -dx * s + dy * c;
        a0 = a0.min(a);
        a1 = a1.max(a);
        b0 = b0.min(b);
        b1 = b1.max(b);
    }
    let (short, long) = ((a1 - a0).min(b1 - b0), (a1 - a0).max(b1 - b0));
    if short > 20.0 || long < 24.0 {
        return false;
    }
    let Some(cell) = reach.cell_at(cx, cy) else {
        return false;
    };
    let (i, j) = (cell / reach.ny, cell % reach.ny);
    let (mut up, mut down) = (false, false);
    for di in -3..=3 {
        for dj in -3..=3 {
            let (ii, jj) = (i + di, j + dj);
            if !(0..reach.nx).contains(&ii) || !(0..reach.ny).contains(&jj) {
                continue;
            }
            for k in reach.nodes_in(ii * reach.ny + jj) {
                if !reach.reached[k] {
                    continue;
                }
                let dz = reach.z[k] - cz;
                up |= dz > 4.0 && dz <= 24.0;
                down |= (-24.0..-4.0).contains(&dz);
            }
        }
    }
    up && down
}

/// `point_name` as the game shows it: looked up in the game's own
/// localisation (`#POINT_*` keys) and without a leading "the".
fn flag_name(key: &str, strings: &HashMap<String, String>) -> String {
    let raw = strings
        .get(&key.trim_start_matches('#').to_ascii_lowercase())
        .cloned()
        .unwrap_or_else(|| key.to_string());
    let raw = raw.trim();
    let raw = raw
        .strip_prefix("the ")
        .or_else(|| raw.strip_prefix("The "))
        .unwrap_or(raw);
    let mut chars = raw.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
}

/// Reads `dod/resource/dod_english.txt` (UTF-16) into lower-cased keys.
pub fn read_strings(path: &std::path::Path) -> HashMap<String, String> {
    let Ok(bytes) = std::fs::read(path) else {
        return HashMap::new();
    };
    let text = if bytes.starts_with(&[0xff, 0xfe]) {
        let units: Vec<u16> = bytes[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(&bytes).into_owned()
    };
    let mut out = HashMap::new();
    for line in text.lines() {
        let quoted: Vec<&str> = line.split('"').collect();
        if quoted.len() >= 5 {
            out.insert(quoted[1].to_ascii_lowercase(), quoted[3].to_string());
        }
    }
    out
}

/// Each capture area's footprint, in pixels. A brush with faces gives its
/// faces; newer compilers keep none for trigger brushes, so those are
/// sampled from the brush's own hull instead.
fn cap_zones(level: &Level, t: &Transform) -> Vec<Vec<Vec<[f32; 2]>>> {
    let mut zones = Vec::new();
    for entity in level
        .entities
        .iter()
        .filter(|e| e.classname() == "dod_capture_area")
    {
        let Some(model) = entity.brush_submodel().map(|m| m as usize) else {
            continue;
        };
        let Some(m) = level.models.get(model) else {
            continue;
        };
        let mut polygons: Vec<Vec<[f32; 2]>> = level
            .faces
            .iter()
            .filter(|f| f.model == model && f.points.len() >= 3)
            .map(|f| f.points.iter().map(|p| t.to_pixel(p[0], p[1])).collect())
            .collect();
        if polygons.is_empty() {
            let step = 4.0;
            let mut x = m.mins[0] + 2.0;
            while x < m.maxs[0] {
                let mut y = m.mins[1] + 2.0;
                while y < m.maxs[1] {
                    let inside = [0.25, 0.5, 0.75].iter().any(|t| {
                        let z = m.mins[2] + t * (m.maxs[2] - m.mins[2]);
                        level.model_point_contents(model, [x, y, z]) != CONTENTS_EMPTY
                    });
                    if inside {
                        let o = m.offset;
                        let h = step / 2.0;
                        polygons.push(
                            [[-h, -h], [h, -h], [h, h], [-h, h]]
                                .iter()
                                .map(|d| t.to_pixel(x + o[0] + d[0], y + o[1] + d[1]))
                                .collect(),
                        );
                    }
                    y += step;
                }
                x += step;
            }
        }
        if !polygons.is_empty() {
            zones.push(polygons);
        }
    }
    zones
}

/// Builds the scene for `level`. `strings` names the flags.
pub fn build(level: &Level, strings: &HashMap<String, String>) -> Result<Scene, String> {
    build_until(level, strings, &|_| false)
}

/// [`build`], telling `progress` how far along it is and giving up with
/// [`reach::CANCELLED`] when it answers true (see [`reach::build_until`]).
pub fn build_until(
    level: &Level,
    strings: &HashMap<String, String>,
    progress: &dyn Fn(f32) -> bool,
) -> Result<Scene, String> {
    let reach = reach::build_until(level, progress)?;
    if progress(0.95) {
        return Err(reach::CANCELLED.to_string());
    }

    // The top reachable floor of every cell, and whether it is outdoors.
    let mut top: HashMap<i32, usize> = HashMap::new();
    for k in (0..reach.cell.len()).filter(|&k| reach.reached[k]) {
        top.entry(reach.cell[k]).or_insert(k);
    }
    let is_outdoor: HashMap<i32, bool> = top
        .iter()
        .map(|(&cell, &k)| {
            let [x, y] = reach.centre(cell);
            (cell, outdoor(level, [x, y, reach.z[k] + 40.0]))
        })
        .collect();
    let mut outdoor_z: Vec<f32> = top
        .iter()
        .filter(|(c, _)| is_outdoor[c])
        .map(|(_, &k)| reach.z[k])
        .collect();
    outdoor_z.sort_by(f32::total_cmp);
    let bands = [percentile(&outdoor_z, 0.40), percentile(&outdoor_z, 0.85)];
    let band_of = |z: f32| bands.iter().filter(|&&b| b < z).count();
    // An area's kind: indoor, or the outdoor band.
    let kind = |cell: i32| -> i32 {
        if is_outdoor[&cell] {
            band_of(reach.z[top[&cell]]) as i32
        } else {
            -1
        }
    };

    // Areas: connected cells of one kind at similar heights.
    let mut area_of: HashMap<i32, u32> = HashMap::new();
    let mut areas: Vec<Area> = Vec::new();
    let mut cells: Vec<i32> = top.keys().copied().collect();
    cells.sort();
    for &seed in &cells {
        if area_of.contains_key(&seed) {
            continue;
        }
        let id = areas.len() as u32;
        let seed_kind = kind(seed);
        let reach_dz = if seed_kind < 0 { 64.0 } else { 24.0 };
        let mut stack = vec![seed];
        area_of.insert(seed, id);
        let mut members = Vec::new();
        while let Some(c) = stack.pop() {
            members.push(c);
            let (i, j) = (c / reach.ny, c % reach.ny);
            for (ii, jj) in [(i + 1, j), (i - 1, j), (i, j + 1), (i, j - 1)] {
                if !(0..reach.nx).contains(&ii) || !(0..reach.ny).contains(&jj) {
                    continue;
                }
                let c2 = ii * reach.ny + jj;
                if area_of.contains_key(&c2) || !top.contains_key(&c2) || kind(c2) != seed_kind {
                    continue;
                }
                if (reach.z[top[&c2]] - reach.z[top[&c]]).abs() > reach_dz {
                    continue;
                }
                area_of.insert(c2, id);
                stack.push(c2);
            }
        }
        // The member nearest the middle, so the anchor is inside the area.
        let n = members.len() as f32;
        let (mx, my) = members.iter().fold((0.0, 0.0), |a, &c| {
            let [x, y] = reach.centre(c);
            (a.0 + x / n, a.1 + y / n)
        });
        let anchor = members
            .iter()
            .map(|&c| reach.centre(c))
            .min_by(|a, b| {
                let da = (a[0] - mx).powi(2) + (a[1] - my).powi(2);
                let db = (b[0] - mx).powi(2) + (b[1] - my).powi(2);
                da.total_cmp(&db)
            })
            .unwrap_or([mx, my]);
        areas.push(Area {
            id,
            indoor: seed_kind < 0,
            colour: if seed_kind < 0 {
                SMALL
            } else {
                OUTDOOR[seed_kind as usize]
            },
            anchor,
            cells: members.len(),
        });
    }

    // Indoor colours: the least-used one no touching indoor area has.
    let mut touch: HashMap<u32, HashSet<u32>> = HashMap::new();
    for (&c, &a) in &area_of {
        let (i, j) = (c / reach.ny, c % reach.ny);
        for c2 in [(i + 1) * reach.ny + j, c + 1] {
            if let Some(&b) = area_of.get(&c2)
                && b != a
            {
                touch.entry(a).or_default().insert(b);
                touch.entry(b).or_default().insert(a);
            }
        }
    }
    let mut order: Vec<u32> = areas.iter().filter(|a| a.indoor).map(|a| a.id).collect();
    order.sort_by_key(|&id| std::cmp::Reverse(areas[id as usize].cells));
    let mut used: HashMap<[u8; 3], usize> = HashMap::new();
    for id in order {
        if areas[id as usize].cells < SMALL_CELLS {
            continue;
        }
        let taken: HashSet<[u8; 3]> = touch
            .get(&id)
            .map(|t| t.iter().map(|&b| areas[b as usize].colour).collect())
            .unwrap_or_default();
        let colour = INDOOR
            .iter()
            .filter(|c| !taken.contains(*c))
            .min_by_key(|c| used.get(*c).copied().unwrap_or(0))
            .copied()
            .unwrap_or(INDOOR[id as usize % INDOOR.len()]);
        *used.entry(colour).or_default() += 1;
        areas[id as usize].colour = colour;
    }

    // Faces: drawn if reachable; their area is the one most of their top
    // cells are in. A floor always under another is drawn first, under it.
    let mut votes: HashMap<usize, HashMap<u32, usize>> = HashMap::new();
    let mut under: HashSet<usize> = HashSet::new();
    for k in (0..reach.cell.len()).filter(|&k| reach.reached[k]) {
        let face = reach.floors[reach.floor[k] as usize];
        let cell = reach.cell[k];
        if top[&cell] == k {
            *votes
                .entry(face)
                .or_default()
                .entry(area_of[&cell])
                .or_default() += 1;
        } else {
            under.insert(face);
        }
    }
    // A face drawn whole would paint all of one reached only along an edge
    // (dod_harrington's rock slope by Bridge, clipped off, is reached only
    // where it meets the grass). Under a tenth reached: only its reached
    // cells are drawn, each cut out of the face.
    let mut cells: HashMap<usize, (usize, Vec<usize>)> = HashMap::new();
    for k in 0..reach.cell.len() {
        let entry = cells
            .entry(reach.floors[reach.floor[k] as usize])
            .or_default();
        entry.0 += 1;
        if reach.reached[k] {
            entry.1.push(k);
        }
    }
    let partial: HashMap<usize, Vec<usize>> = cells
        .into_iter()
        .filter(|(_, (all, hit))| !hit.is_empty() && hit.len() * 10 < *all)
        .map(|(face, (_, hit))| (face, hit))
        .collect();
    votes.retain(|face, _| !partial.contains_key(face));
    under.retain(|face| !partial.contains_key(face));
    let half = super::reach::GRID / 2.0;
    let pieces: Vec<(usize, u32, Vec<[f32; 2]>)> = partial
        .iter()
        .flat_map(|(&face, hit)| {
            let outline: Vec<[f32; 2]> = level.faces[face]
                .points
                .iter()
                .map(|p| [p[0], p[1]])
                .collect();
            hit.iter()
                .filter_map(|&k| {
                    let [x, y] = reach.centre(reach.cell[k]);
                    let cut = clip_to_box(&outline, [x - half, y - half], [x + half, y + half]);
                    let area = area_of.get(&reach.cell[k]).copied().unwrap_or(0);
                    (cut.len() >= 3).then_some((face, area, cut))
                })
                .collect::<Vec<_>>()
        })
        .collect();

    // Faces too small for a cell (stair treads, ledges): the area of a
    // reachable floor right beside them, at about their height.
    for &face in &reach.floors {
        if votes.contains_key(&face) || under.contains(&face) || partial.contains_key(&face) {
            continue;
        }
        let points = &level.faces[face].points;
        let n = points.len() as f32;
        let (cx, cy, cz) = points.iter().fold((0.0, 0.0, 0.0), |a, p| {
            (a.0 + p[0] / n, a.1 + p[1] / n, a.2 + p[2] / n)
        });
        let Some(cell) = reach.cell_at(cx, cy) else {
            continue;
        };
        let (i, j) = (cell / reach.ny, cell % reach.ny);
        'near: for (ii, jj) in [(i, j), (i + 1, j), (i - 1, j), (i, j + 1), (i, j - 1)] {
            let c2 = ii * reach.ny + jj;
            for k in reach.nodes_in(c2) {
                if reach.reached[k] && (reach.z[k] - cz).abs() <= 24.0 {
                    if let Some(&area) = area_of.get(&c2) {
                        *votes.entry(face).or_default().entry(area).or_default() += 1;
                    }
                    break 'near;
                }
            }
        }
    }

    // Fit what will be drawn: the faces themselves reach past the cell
    // centres, a whole face past its last reachable cell.
    let drawn: Vec<[f32; 2]> = votes
        .keys()
        .chain(under.iter())
        .flat_map(|&f| level.faces[f].points.iter().map(|p| [p[0], p[1]]))
        .chain(pieces.iter().flat_map(|(_, _, cut)| cut.iter().copied()))
        .collect();
    let mut transform =
        Transform::fit(&drawn, 24.0).ok_or("nothing is reachable from the spawns")?;
    let mut zs: Vec<f32> = (0..reach.cell.len())
        .filter(|&k| reach.reached[k])
        .map(|k| reach.z[k])
        .collect();
    zs.sort_by(f32::total_cmp);
    transform.height = percentile(&zs, 0.5).round();

    let mut faces: Vec<SceneFace> = Vec::new();
    for (&face, tally) in &votes {
        let area = tally
            .iter()
            .max_by_key(|(id, n)| (**n, std::cmp::Reverse(**id)))
            .map(|(id, _)| *id)
            .unwrap_or(0);
        faces.push(scene_face(level, &reach, &transform, face, area));
    }
    for &face in &under {
        if votes.contains_key(&face) {
            continue;
        }
        // Under another floor everywhere: its own cells' area is the top one.
        let points = &level.faces[face].points;
        let area = reach
            .cell_at(points[0][0], points[0][1])
            .and_then(|c| area_of.get(&c).copied())
            .unwrap_or(0);
        faces.push(scene_face(level, &reach, &transform, face, area));
    }
    for (face, area, cut) in &pieces {
        let points = &level.faces[*face].points;
        faces.push(SceneFace {
            points: cut.iter().map(|p| transform.to_pixel(p[0], p[1])).collect(),
            z: points.iter().map(|p| p[2]).sum::<f32>() / points.len() as f32,
            area: *area,
            stairs: false,
            face: *face as u32,
        });
    }
    faces.sort_by(|a, b| a.z.total_cmp(&b.z).then(a.face.cmp(&b.face)));

    let water = level
        .faces
        .iter()
        .filter(|f| f.texture.starts_with('!') && f.normal_z > 0.7 && f.points.len() >= 3)
        .map(|f| Water {
            points: f
                .points
                .iter()
                .map(|p| transform.to_pixel(p[0], p[1]))
                .collect(),
            z: f.points.iter().map(|p| p[2]).sum::<f32>() / f.points.len() as f32,
        })
        .collect();

    let marker = |name: String, origin: [f32; 3]| Marker {
        name,
        at: transform.to_pixel(origin[0], origin[1]),
        world: origin,
    };
    let flags = level
        .points("dod_control_point")
        .into_iter()
        .map(|(e, o)| marker(flag_name(e.get("point_name").unwrap_or(""), strings), o))
        .collect();
    let allies = level
        .points("info_player_allies")
        .into_iter()
        .map(|(_, o)| marker("Allies".to_string(), o))
        .collect();
    let axis = level
        .points("info_player_axis")
        .into_iter()
        .map(|(_, o)| marker("Axis".to_string(), o))
        .collect();

    let breakable_edges = breakable_edges(level, &faces, &transform);
    let slope_edges = super::slopes::slope_edges(level, &reach, &transform);
    Ok(Scene {
        map: level.name.clone(),
        transform,
        width: super::transform::WIDTH,
        height: super::transform::HEIGHT,
        faces,
        areas,
        water,
        cap_zones: cap_zones(level, &transform),
        breakable_edges,
        slope_edges,
        checksum: 0,
        flags,
        allies,
        axis,
        palette: PALETTE.to_vec(),
        background: BACKGROUND,
        void: VOID,
    })
}

/// The outer edges of each breakable's drawn floors: every edge of its
/// faces that no other of its faces shares.
fn breakable_edges(level: &Level, faces: &[SceneFace], t: &Transform) -> Vec<[[f32; 2]; 2]> {
    let mut by_model: HashMap<usize, Vec<&Vec<[f32; 3]>>> = HashMap::new();
    for face in faces {
        let f = &level.faces[face.face as usize];
        if level.models.get(f.model).is_some_and(|m| m.breakable) {
            by_model.entry(f.model).or_default().push(&f.points);
        }
    }
    let mut out = Vec::new();
    for polygons in by_model.values() {
        out.extend(
            outline(polygons)
                .into_iter()
                .map(|[a, b]| [t.to_pixel(a[0], a[1]), t.to_pixel(b[0], b[1])]),
        );
    }
    out
}

/// Edges (in x and y) used by exactly one of `polygons`.
pub(super) fn outline(polygons: &[&Vec<[f32; 3]>]) -> Vec<[[f32; 2]; 2]> {
    let key = |p: [f32; 3]| ((p[0] * 2.0).round() as i64, (p[1] * 2.0).round() as i64);
    let mut count: HashMap<((i64, i64), (i64, i64)), (usize, [[f32; 2]; 2])> = HashMap::new();
    for points in polygons {
        for i in 0..points.len() {
            let (a, b) = (points[i], points[(i + 1) % points.len()]);
            let (ka, kb) = (key(a), key(b));
            if ka == kb {
                continue;
            }
            let k = if ka < kb { (ka, kb) } else { (kb, ka) };
            count
                .entry(k)
                .or_insert((0, [[a[0], a[1]], [b[0], b[1]]]))
                .0 += 1;
        }
    }
    let mut edges: Vec<_> = count.into_iter().filter(|(_, (n, _))| *n == 1).collect();
    edges.sort_by_key(|(k, _)| *k);
    edges.into_iter().map(|(_, (_, e))| e).collect()
}

/// `polygon` cut to the box from `lo` to `hi` (Sutherland-Hodgman, one
/// side at a time).
fn clip_to_box(polygon: &[[f32; 2]], lo: [f32; 2], hi: [f32; 2]) -> Vec<[f32; 2]> {
    let mut out = polygon.to_vec();
    for (axis, bound, keep_above) in [
        (0, lo[0], true),
        (0, hi[0], false),
        (1, lo[1], true),
        (1, hi[1], false),
    ] {
        let inside = |p: &[f32; 2]| (p[axis] >= bound) == keep_above || p[axis] == bound;
        let input = std::mem::take(&mut out);
        for i in 0..input.len() {
            let (a, b) = (input[i], input[(i + 1) % input.len()]);
            let (ia, ib) = (inside(&a), inside(&b));
            if ia {
                out.push(a);
            }
            if ia != ib {
                let t = (bound - a[axis]) / (b[axis] - a[axis]);
                out.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
            }
        }
        if out.is_empty() {
            break;
        }
    }
    out
}

fn scene_face(level: &Level, reach: &Reach, t: &Transform, face: usize, area: u32) -> SceneFace {
    let points = &level.faces[face].points;
    let z = points.iter().map(|p| p[2]).sum::<f32>() / points.len() as f32;
    SceneFace {
        points: points.iter().map(|p| t.to_pixel(p[0], p[1])).collect(),
        z,
        area,
        stairs: is_tread(points, reach),
        face: face as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_triangle_cut_to_a_box_keeps_only_the_part_inside() {
        let triangle = [[0.0, 0.0], [10.0, 0.0], [0.0, 10.0]];
        let cut = clip_to_box(&triangle, [2.0, 2.0], [6.0, 6.0]);
        // The box's corner past the hypotenuse is cut off: a pentagon.
        assert_eq!(cut.len(), 5);
        assert!(
            cut.iter()
                .all(|p| (2.0..=6.0).contains(&p[0]) && (2.0..=6.0).contains(&p[1]))
        );
        assert!(cut.iter().all(|p| p[0] + p[1] <= 10.0 + 1e-4));
        assert!(clip_to_box(&triangle, [8.0, 8.0], [9.0, 9.0]).is_empty());
    }

    #[test]
    fn a_breakables_outline_leaves_out_the_edge_its_faces_share() {
        let left = vec![
            [0.0, 0.0, 0.0],
            [10.0, 0.0, 0.0],
            [10.0, 10.0, 0.0],
            [0.0, 10.0, 0.0],
        ];
        let right = vec![
            [10.0, 0.0, 0.0],
            [20.0, 0.0, 0.0],
            [20.0, 10.0, 0.0],
            [10.0, 10.0, 0.0],
        ];
        let edges = outline(&[&left, &right]);
        assert_eq!(edges.len(), 6);
        let shared = |e: &[[f32; 2]; 2]| e.iter().all(|p| p[0] == 10.0);
        assert!(!edges.iter().any(shared));
    }

    #[test]
    fn flag_names_drop_the_and_take_a_capital() {
        let mut strings = HashMap::new();
        strings.insert("point_anzio_plaza".to_string(), "the plaza".to_string());
        assert_eq!(flag_name("POINT_ANZIO_PLAZA", &strings), "Plaza");
        assert_eq!(flag_name("#POINT_ANZIO_PLAZA", &strings), "Plaza");
        assert_eq!(flag_name("Town Centre", &strings), "Town Centre");
        assert_eq!(flag_name("the cliffs", &strings), "Cliffs");
    }

    #[test]
    fn percentile_picks_from_the_sorted_list() {
        let v = [1.0, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(percentile(&v, 0.0), 1.0);
        assert_eq!(percentile(&v, 0.5), 3.0);
        assert_eq!(percentile(&v, 1.0), 5.0);
        assert_eq!(percentile(&[], 0.5), 0.0);
    }

    #[test]
    fn the_palette_holds_every_default_colour() {
        for c in OUTDOOR
            .iter()
            .chain(INDOOR.iter())
            .chain([SMALL, STAIRS, WATER].iter())
        {
            assert!(PALETTE.contains(c), "{c:?}");
        }
    }

    #[test]
    fn a_grid_cell_is_eight_units() {
        assert_eq!(super::reach::GRID, 8.0);
    }
}
