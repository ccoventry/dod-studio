//! Spawn protection (#371): the `trigger_hurt` brushes round each team's
//! spawn that hurt the other team, for the Overviews page to outline.
//!
//! DoD marks the team a `trigger_hurt` is for with spawnflag 64 or 128.
//! Matched against the spawns across a library of 156 maps, 64 sits by
//! the Allies' spawn 280 times out of 286 and 128 by the Axis' 292 of 295:
//! 64 protects the Allies' spawn (hurts Axis players), 128 the Axis'. One
//! that does no damage protects nothing and is left out.

use serde::{Deserialize, Serialize};

use super::level::Level;
use super::transform::Transform;

const ALLIES: i32 = 64;
const AXIS: i32 = 128;

/// One team's spawn protection, as outline segments in image pixels.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpawnZone {
    /// `"allies"` or `"axis"`: whose spawn it protects.
    pub team: String,
    pub edges: Vec<[[f32; 2]; 2]>,
}

/// Every protecting `trigger_hurt`'s footprint: the outline of its upward
/// faces, else its bounding box.
pub fn spawn_zones(level: &Level, t: &Transform) -> Vec<SpawnZone> {
    let mut out = Vec::new();
    for entity in &level.entities {
        if entity.classname() != "trigger_hurt" {
            continue;
        }
        let number = |key: &str| entity.get(key).and_then(|v| v.trim().parse::<f32>().ok());
        let flags = number("spawnflags").unwrap_or(0.0) as i32;
        let team = match flags & (ALLIES | AXIS) {
            ALLIES => "allies",
            AXIS => "axis",
            _ => continue,
        };
        if number("dmg").unwrap_or(10.0) <= 0.0 {
            continue;
        }
        let Some(model) = entity
            .brush_submodel()
            .and_then(|m| level.models.get(m as usize))
        else {
            continue;
        };
        let tops: Vec<&Vec<[f32; 3]>> = level.faces
            [model.first_face..model.first_face + model.face_count]
            .iter()
            .filter(|f| f.normal_z > 0.7 && f.points.len() >= 3)
            .map(|f| &f.points)
            .collect();
        let world = if tops.is_empty() {
            let (lo, hi) = (model.mins, model.maxs);
            let o = model.offset;
            let corners = [
                [lo[0] + o[0], lo[1] + o[1]],
                [hi[0] + o[0], lo[1] + o[1]],
                [hi[0] + o[0], hi[1] + o[1]],
                [lo[0] + o[0], hi[1] + o[1]],
            ];
            (0..4).map(|i| [corners[i], corners[(i + 1) % 4]]).collect()
        } else {
            super::scene::outline(&tops)
        };
        out.push(SpawnZone {
            team: team.to_string(),
            edges: world
                .into_iter()
                .map(|[a, b]| [t.to_pixel(a[0], a[1]), t.to_pixel(b[0], b[1])])
                .collect(),
        });
    }
    out
}
