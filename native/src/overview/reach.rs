//! Where a player can get to: every floor a player can stand on, rasterised
//! onto an 8-unit grid, kept where a crouching player fits, then flood-filled
//! from the spawns over steps, jumps, drops and ladders.
//!
//! The rules that mattered, each found on a real map:
//!
//! - The crouch hull is tested with its bottom a step above the floor (plus
//!   however far a slope rises under its corner). Tested on the floor itself,
//!   every stair riser and curb beside a node reads as solid and the fill
//!   stops at the first step.
//! - Walking off an edge carries a player forward while he falls, through
//!   open air only, up to twelve cells. Without it, dod_railroad2_s10a's
//!   spawns are sealed in by the high end of a ramp; with only four,
//!   dod_anjou's are, behind rock too steep to stand on (normal below 0.7),
//!   which a player slides down rather than walks.
//! - Stairs under a clip ramp: the player stands on the ramp, up to a step
//!   above the tread, and a sloped clip plane is pushed out further still, so
//!   the crouch hull is also tried 12 and 24 units higher.
//! - A floor above that a player doesn't fit on (a sill, a fence top beside a
//!   wall) isn't climbed onto, so it doesn't hide the floor under it.

use std::collections::{HashMap, VecDeque};

use super::level::{CONTENTS_SOLID, Face, Level};

pub const GRID: f32 = 8.0;
/// A stair step (18) plus slack for slopes.
pub const STEP: f32 = 20.0;
/// DoD's highest climb onto a ledge: a full-stamina jump rises 45 units
/// (`pm_shared.c` PM_Jump: sqrt(45 * 1600) up against gravity 800), and
/// ducking in the air lifts the feet 18 more (the crouch box is 36 shorter,
/// shrinking from both ends).
pub const JUMP: f32 = 63.0;
/// Hull 3, the crouching player: 32x32x36, origin in the middle.
const CROUCH_HALF: f32 = 18.0;
const CROUCH_HULL: usize = 3;
/// How far a fall (or a slide down rock too steep to stand on) carries a
/// player past the edge, in cells.
const FALL_CELLS: i32 = 12;
/// A running jump, as measured from a demo of one (dod_harrington, a
/// player jumping and ducking onto the crates by Bridge): he leaves the
/// ground at 264 units/s upward and 215-220 across; gravity 800; ducking in
/// the air lifts his feet 18. His feet top out 63 above where he left at
/// 0.32 s, and he lands on a ledge 58-60 up 78-92 units on, in the air
/// 0.39-0.43 s; on level ground he is in the air 0.73 s.
/// sqrt(2 * 800 * 45): the 45 units a jump rises before the duck.
const JUMP_SPEED: f32 = 268.33;
const RUN_SPEED: f32 = 220.0;
const GRAVITY: f32 = 800.0;
const DUCK_LIFT: f32 = 18.0;
/// The furthest a running jump is looked along, in cells: across level
/// ground, about 160 units.
const LEAP_CELLS: i32 = 22;

/// How far across a running jump carries a player before his feet come
/// back down to `rise` above where he left (ducked at the top), plus half
/// his box, which lands on a ledge before his centre is over it. None above
/// the top of the jump.
pub fn leap_reach(rise: f32) -> Option<f32> {
    if rise > JUMP {
        return None;
    }
    let lift = rise - DUCK_LIFT;
    let disc = (JUMP_SPEED * JUMP_SPEED - 2.0 * GRAVITY * lift).max(0.0);
    Some(RUN_SPEED * (JUMP_SPEED + disc.sqrt()) / GRAVITY + 16.0)
}
/// Where a player's origin can be, from a cell's centre, with his box (16
/// either side) still over it: centred first.
const OFFSETS: [(f32, f32); 9] = [
    (0.0, 0.0),
    (12.0, 0.0),
    (-12.0, 0.0),
    (0.0, 12.0),
    (0.0, -12.0),
    (12.0, 12.0),
    (12.0, -12.0),
    (-12.0, 12.0),
    (-12.0, -12.0),
];

/// Textures no player stands on.
const TOOL_TEXTURES: [&str; 8] = [
    "aaatrigger",
    "clip",
    "null",
    "sky",
    "origin",
    "hint",
    "skip",
    "bevel",
];

/// Brush entities whose upper faces are floors.
const FLOOR_CLASSES: [&str; 7] = [
    "worldspawn",
    "func_wall",
    "func_breakable",
    "func_wall_toggle",
    "func_door",
    "func_door_rotating",
    "func_detail",
];

/// Brush entities that block a player. Doors open, so they don't.
const SOLID_CLASSES: [&str; 3] = ["func_wall", "func_breakable", "func_wall_toggle"];

/// Whether `face` is ground a player could stand on.
pub fn is_floor(level: &Level, face: &Face) -> bool {
    face.normal_z > 0.7
        && !face.texture.starts_with('!')
        && !TOOL_TEXTURES.iter().any(|t| face.texture.starts_with(t))
        && level
            .models
            .get(face.model)
            .is_some_and(|m| FLOOR_CLASSES.contains(&m.class.as_str()))
}

/// The plane through a polygon (Newell's method): unit normal and distance.
pub fn plane_of(points: &[[f32; 3]]) -> ([f32; 3], f32) {
    let mut n = [0.0f32; 3];
    let mut centre = [0.0f32; 3];
    for (i, a) in points.iter().enumerate() {
        let b = points[(i + 1) % points.len()];
        n[0] += (a[1] - b[1]) * (a[2] + b[2]);
        n[1] += (a[2] - b[2]) * (a[0] + b[0]);
        n[2] += (a[0] - b[0]) * (a[1] + b[1]);
        for k in 0..3 {
            centre[k] += a[k] / points.len() as f32;
        }
    }
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-9);
    let n = [n[0] / len, n[1] / len, n[2] / len];
    (n, n[0] * centre[0] + n[1] * centre[1] + n[2] * centre[2])
}

/// The grid and its nodes: one per floor per cell.
pub struct Reach {
    pub x0: f32,
    pub y0: f32,
    pub nx: i32,
    pub ny: i32,
    /// Index into `Level::faces` of every floor face, in `floors` order.
    pub floors: Vec<usize>,
    /// Per node, sorted by cell, then highest first.
    pub cell: Vec<i32>,
    pub z: Vec<f32>,
    /// Index into `floors`.
    pub floor: Vec<u32>,
    /// A crouching player fits here.
    pub fits: Vec<bool>,
    pub reached: Vec<bool>,
    starts: HashMap<i32, (usize, usize)>,
}

impl Reach {
    pub fn centre(&self, cell: i32) -> [f32; 2] {
        let (i, j) = (cell / self.ny, cell % self.ny);
        [
            self.x0 + (i as f32 + 0.5) * GRID,
            self.y0 + (j as f32 + 0.5) * GRID,
        ]
    }

    pub fn cell_at(&self, x: f32, y: f32) -> Option<i32> {
        let i = ((x - self.x0) / GRID).floor() as i32;
        let j = ((y - self.y0) / GRID).floor() as i32;
        (0..self.nx)
            .contains(&i)
            .then_some(())
            .filter(|_| (0..self.ny).contains(&j))
            .map(|_| i * self.ny + j)
    }

    /// The nodes in `cell`, highest first.
    pub fn nodes_in(&self, cell: i32) -> std::ops::Range<usize> {
        self.starts.get(&cell).map(|&(a, b)| a..b).unwrap_or(0..0)
    }
}

/// Whether a crouching player fits with his origin at `p`: hull 3 of the
/// world and of every solid brush entity. Breakables count as broken: a
/// player can shoot through (dod_anzio's drain has a breakable floor at one
/// end), though one still holds him up before it goes (they stay floors).
fn fits(level: &Level, p: [f32; 3]) -> bool {
    if level.hull_contents(0, CROUCH_HULL, p) == CONTENTS_SOLID {
        return false;
    }
    for (index, model) in level.models.iter().enumerate().skip(1) {
        if !SOLID_CLASSES.contains(&model.class.as_str()) || model.breakable {
            continue;
        }
        let local = [
            p[0] - model.offset[0],
            p[1] - model.offset[1],
            p[2] - model.offset[2],
        ];
        let near =
            (0..3).all(|k| local[k] >= model.mins[k] - 20.0 && local[k] <= model.maxs[k] + 20.0);
        if near && level.hull_contents(index, CROUCH_HULL, local) == CONTENTS_SOLID {
            return false;
        }
    }
    true
}

/// Whether a crouching player falls clear down the column at `x, y` from a
/// hull centre at `top` to one at `bottom`: no street, clip brush or gap too
/// narrow for a player between a ledge and the floor far below it
/// (dod_anzio's drain south of the bridge runs under the street, and walking
/// off the kerb used to land in it).
fn drop_clear(level: &Level, x: f32, y: f32, top: f32, bottom: f32) -> bool {
    let mut h = top;
    while h > bottom {
        if !fits(level, [x, y, h]) {
            return false;
        }
        h -= 16.0;
    }
    true
}

/// Whether `x, y` at hull height `h` is inside solid itself, not just
/// within a player's half-width of it: a player's box centred near each of
/// the box's own corners hits something. (The hulls are solid out to 16
/// units past a brush, so a point beside a wall tests solid too; 14, not
/// 16, since past a slanted wall the hull reaches exactly as far as the
/// corner and rounding decides it.)
fn buried(level: &Level, x: f32, y: f32, h: f32) -> bool {
    [(14.0, 14.0), (14.0, -14.0), (-14.0, 14.0), (-14.0, -14.0)]
        .iter()
        .all(|&(dx, dy)| !fits(level, [x + dx, y + dy, h]))
}

fn ladders(level: &Level, reach: &Reach) -> HashMap<i32, (f32, f32)> {
    let mut out = HashMap::new();
    for model in level.models.iter().filter(|m| m.class == "func_ladder") {
        let o = model.offset;
        let i0 = ((model.mins[0] + o[0] - 24.0 - reach.x0) / GRID).floor() as i32;
        let i1 = ((model.maxs[0] + o[0] + 24.0 - reach.x0) / GRID).floor() as i32;
        let j0 = ((model.mins[1] + o[1] - 24.0 - reach.y0) / GRID).floor() as i32;
        let j1 = ((model.maxs[1] + o[1] + 24.0 - reach.y0) / GRID).floor() as i32;
        let span = (model.mins[2] + o[2] - 40.0, model.maxs[2] + o[2] + 8.0);
        for i in i0.max(0)..=i1.min(reach.nx - 1) {
            for j in j0.max(0)..=j1.min(reach.ny - 1) {
                out.insert(i * reach.ny + j, span);
            }
        }
    }
    out
}

/// What a build stopped by its caller returns: the Overviews page asked
/// for another map before this one was done.
pub const CANCELLED: &str = "cancelled";

/// Builds the grid, tests every node, and floods from the spawns.
pub fn build(level: &Level) -> Result<Reach, String> {
    build_until(level, &|_| false)
}

/// [`build`], telling `progress` how far along it is (0 to 0.95, every few
/// thousand nodes) and giving up with [`CANCELLED`] when it answers true.
pub fn build_until(level: &Level, progress: &dyn Fn(f32) -> bool) -> Result<Reach, String> {
    let floors: Vec<usize> = (0..level.faces.len())
        .filter(|&i| is_floor(level, &level.faces[i]) && level.faces[i].points.len() >= 3)
        .collect();
    if floors.is_empty() {
        return Err("the map has no floors".to_string());
    }
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for &f in &floors {
        for p in &level.faces[f].points {
            x0 = x0.min(p[0]);
            y0 = y0.min(p[1]);
            x1 = x1.max(p[0]);
            y1 = y1.max(p[1]);
        }
    }
    let (x0, y0) = (x0 - GRID, y0 - GRID);
    let nx = ((x1 - x0) / GRID) as i32 + 2;
    let ny = ((y1 - y0) / GRID) as i32 + 2;

    // Rasterise: every cell centre inside a floor's footprint.
    let mut raw: Vec<(i32, f32, u32, f32)> = Vec::new();
    for (fi, &f) in floors.iter().enumerate() {
        let points = &level.faces[f].points;
        let (n, d) = plane_of(points);
        if n[2].abs() < 1e-3 {
            continue;
        }
        let rise = 16.0 * (n[0].abs() + n[1].abs()) / n[2].abs().max(0.7);
        let (mut a0, mut b0, mut a1, mut b1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for p in points {
            a0 = a0.min(p[0]);
            b0 = b0.min(p[1]);
            a1 = a1.max(p[0]);
            b1 = b1.max(p[1]);
        }
        let i0 = (((a0 - x0) / GRID) as i32).max(0);
        let i1 = (((a1 - x0) / GRID) as i32 + 1).min(nx);
        let j0 = (((b0 - y0) / GRID) as i32).max(0);
        let j1 = (((b1 - y0) / GRID) as i32 + 1).min(ny);
        for i in i0..i1 {
            for j in j0..j1 {
                let cx = x0 + (i as f32 + 0.5) * GRID;
                let cy = y0 + (j as f32 + 0.5) * GRID;
                let (mut pos, mut neg) = (true, true);
                for k in 0..points.len() {
                    let a = points[k];
                    let b = points[(k + 1) % points.len()];
                    let cross = (b[0] - a[0]) * (cy - a[1]) - (b[1] - a[1]) * (cx - a[0]);
                    pos &= cross >= -1e-4;
                    neg &= cross <= 1e-4;
                }
                if pos || neg {
                    let z = (d - n[0] * cx - n[1] * cy) / n[2];
                    raw.push((i * ny + j, z, fi as u32, rise));
                }
            }
        }
    }
    raw.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.total_cmp(&a.1)));
    // One node per floor per cell: drop one within 4 units under the last.
    let mut kept: Vec<(i32, f32, u32, f32)> = Vec::with_capacity(raw.len());
    for node in raw {
        if let Some(last) = kept.last()
            && last.0 == node.0
            && last.1 - node.1 < 4.0
        {
            continue;
        }
        kept.push(node);
    }

    let mut reach = Reach {
        x0,
        y0,
        nx,
        ny,
        floors,
        cell: kept.iter().map(|n| n.0).collect(),
        z: kept.iter().map(|n| n.1).collect(),
        floor: kept.iter().map(|n| n.2).collect(),
        fits: Vec::new(),
        reached: vec![false; kept.len()],
        starts: HashMap::new(),
    };
    let mut start = 0;
    for k in 1..=kept.len() {
        if k == kept.len() || kept[k].0 != kept[start].0 {
            reach.starts.insert(kept[start].0, (start, k));
            start = k;
        }
    }
    // A step above the floor first; a crouch resting on it for low passages.
    // Then with the player off to one side: his 32-unit box stands on any
    // sliver of floor under it, so a ledge too thin for him to be centred on
    // (against a wall, a beam) still holds him. Not when the floor there is
    // itself inside something solid: dod_harrington's rock slope by Bridge
    // is boxed in by a clip brush whose wall runs along its edge, and a box
    // beside that wall stands on the grass, not on the slope.
    reach.fits = kept
        .iter()
        .enumerate()
        .map(|(n, &(cell, z, _, rise))| {
            if n.is_multiple_of(4096) && progress(0.75 * n as f32 / kept.len() as f32) {
                return Err(CANCELLED.to_string());
            }
            let [cx, cy] = reach.centre(cell);
            Ok([
                z + STEP + 2.0 + rise + CROUCH_HALF,
                z + 2.0 + rise + CROUCH_HALF,
                z + STEP + 2.0 + rise + CROUCH_HALF + 12.0,
                z + STEP + 2.0 + rise + CROUCH_HALF + 24.0,
            ]
            .into_iter()
            .any(|h| {
                fits(level, [cx, cy, h])
                    || (!buried(level, cx, cy, h)
                        && OFFSETS[1..]
                            .iter()
                            .any(|&(dx, dy)| fits(level, [cx + dx, cy + dy, h])))
            }))
        })
        .collect::<Result<_, _>>()?;

    flood(level, &mut reach, progress)?;
    Ok(reach)
}

/// Reports 0.75 to 0.95 to `progress`, by the share of standable nodes
/// taken from the queue (most of them are reached).
fn flood(level: &Level, reach: &mut Reach, progress: &dyn Fn(f32) -> bool) -> Result<(), String> {
    let standable = reach.fits.iter().filter(|&&f| f).count().max(1);
    let ladders = ladders(level, reach);
    let mut queue = VecDeque::new();
    for class in ["info_player_allies", "info_player_axis"] {
        for (_, origin) in level.points(class) {
            let Some(cell) = reach.cell_at(origin[0], origin[1]) else {
                continue;
            };
            let below = reach
                .nodes_in(cell)
                .find(|&k| reach.z[k] <= origin[2] && reach.fits[k]);
            if let Some(k) = below
                && !reach.reached[k]
            {
                reach.reached[k] = true;
                queue.push_back(k);
            }
        }
    }
    let mut done = 0usize;
    while let Some(k) = queue.pop_front() {
        done += 1;
        if done.is_multiple_of(4096)
            && progress(0.75 + 0.2 * (done as f32 / standable as f32).min(1.0))
        {
            return Err(CANCELLED.to_string());
        }
        let cell = reach.cell[k];
        let (i, j) = (cell / reach.ny, cell % reach.ny);
        let z = reach.z[k];
        let on_ladder = ladders.contains_key(&cell);
        for (di, dj) in [(1, 0), (-1, 0), (0, 1), (0, -1), (0, 0)] {
            let (ii, jj) = (i + di, j + dj);
            if !(0..reach.nx).contains(&ii) || !(0..reach.ny).contains(&jj) {
                continue;
            }
            let c2 = ii * reach.ny + jj;
            if on_ladder && let Some(&(lo, hi)) = ladders.get(&c2) {
                for k2 in reach.nodes_in(c2) {
                    let z2 = reach.z[k2];
                    if ((lo..=hi).contains(&z2) || z2 <= z + JUMP)
                        && reach.fits[k2]
                        && !reach.reached[k2]
                    {
                        reach.reached[k2] = true;
                        queue.push_back(k2);
                    }
                }
                continue;
            }
            if (di, dj) == (0, 0) {
                continue;
            }
            for step in 1..=FALL_CELLS {
                let (ii, jj) = (i + di * step, j + dj * step);
                if !(0..reach.nx).contains(&ii) || !(0..reach.ny).contains(&jj) {
                    break;
                }
                let c2 = ii * reach.ny + jj;
                // The first floor at or below a jump's reach; one above this
                // floor that a player doesn't fit on isn't climbed onto.
                let land = reach.nodes_in(c2).find(|&k2| {
                    let z2 = reach.z[k2];
                    !(z2 > z + JUMP || (!reach.fits[k2] && z2 > z + 2.0))
                });
                if let Some(k2) = land
                    && reach.fits[k2]
                {
                    let z2 = reach.z[k2];
                    let [x, y] = reach.centre(c2);
                    // From resting on the ledge, crouched: a window's sill
                    // leaves room for no more (dod_anjou_a4's windows by
                    // Factory are crouch-jumped through, then dropped from).
                    let top = z + 2.0 + CROUCH_HALF;
                    if z2 < z - STEP && !drop_clear(level, x, y, top, z2 + STEP + 2.0 + CROUCH_HALF)
                    {
                        // No way down here, but still room to go on at the
                        // ledge's height: through a thick wall's window, over
                        // the floor that runs on under the wall.
                        if fits(level, [x, y, top]) {
                            continue;
                        }
                        break;
                    }
                    if !reach.reached[k2] {
                        reach.reached[k2] = true;
                        queue.push_back(k2);
                    }
                    break;
                }
                let falling = land.is_none_or(|k2| reach.z[k2] < z - STEP);
                if !falling {
                    break;
                }
                // Only through open air, never through a wall: a step above
                // the ledge, or crouched on it through a window.
                let [x, y] = reach.centre(c2);
                if !fits(level, [x, y, z + STEP + 2.0 + CROUCH_HALF])
                    && !fits(level, [x, y, z + 2.0 + CROUCH_HALF])
                {
                    break;
                }
            }
            // A running jump (see `leap_reach`): onto a ledge above, over
            // lower ground or a gap, as far as the jump carries a player to
            // that height. Across a gap only if every cell before the far
            // side is a drop (otherwise he would just walk).
            let next = (i + di) * reach.ny + (j + dj);
            let mut gap = (0..reach.nx).contains(&(i + di))
                && (0..reach.ny).contains(&(j + dj))
                && !reach
                    .nodes_in(next)
                    .any(|k2| (reach.z[k2] - z).abs() <= STEP);
            // First the landing, from the grid alone; only then the air on
            // the way, which costs hull tests (most scans find nothing).
            let mut landing = None;
            for step in 2..=LEAP_CELLS {
                let (ii, jj) = (i + di * step, j + dj * step);
                if !(0..reach.nx).contains(&ii) || !(0..reach.ny).contains(&jj) {
                    break;
                }
                let c2 = ii * reach.ny + jj;
                let far = step as f32 * GRID;
                let carries = |z2: f32| leap_reach(z2 - z).is_some_and(|d| far <= d);
                let ledge = reach.nodes_in(c2).find(|&k2| {
                    let z2 = reach.z[k2];
                    reach.fits[k2]
                        && carries(z2)
                        && (z2 > z + STEP || (gap && z2 >= z - STEP && z2 <= z + STEP))
                });
                if let Some(k2) = ledge {
                    landing = Some((step, k2));
                    break;
                }
                // Ground on the way as high as the take-off, or a step above
                // it, that he can stand on: he would walk on and jump from
                // there, and that node's own scan covers it. Only jumps over
                // ground lower than where he left are worth following.
                if reach
                    .nodes_in(c2)
                    .any(|k2| reach.fits[k2] && (z - 2.0..=z + STEP).contains(&reach.z[k2]))
                {
                    break;
                }
                if reach.nodes_in(c2).any(|k2| (reach.z[k2] - z).abs() <= STEP) {
                    gap = false;
                }
            }
            let Some((at, k2)) = landing else {
                continue;
            };
            if reach.reached[k2] {
                continue;
            }
            // Through open air, low or at the top of the jump (over the edge
            // of the very ledge he lands on).
            let open = (2..at).all(|step| {
                let [x, y] = reach.centre((i + di * step) * reach.ny + (j + dj * step));
                fits(level, [x, y, z + STEP + 2.0 + CROUCH_HALF])
                    || fits(level, [x, y, z + JUMP + CROUCH_HALF])
            });
            if open {
                reach.reached[k2] = true;
                queue.push_back(k2);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_running_jump_carries_as_far_as_the_demo_showed() {
        // Onto the crate 58-60 above: landed 78-92 units on.
        assert!(leap_reach(60.0).unwrap() >= 92.0);
        // Not past the top of a ducked jump.
        assert!(leap_reach(63.0).is_some());
        assert!(leap_reach(64.0).is_none());
        // Level ground: 0.73 s in the air at a run.
        let level = leap_reach(0.0).unwrap();
        assert!((150.0..190.0).contains(&level), "{level}");
    }

    #[test]
    fn newell_finds_an_upward_floor() {
        let square = [
            [0.0, 0.0, 10.0],
            [64.0, 0.0, 10.0],
            [64.0, 64.0, 10.0],
            [0.0, 64.0, 10.0],
        ];
        let (n, d) = plane_of(&square);
        assert!((n[2] - 1.0).abs() < 1e-5, "{n:?}");
        assert!((d - 10.0).abs() < 1e-3);
    }
}
