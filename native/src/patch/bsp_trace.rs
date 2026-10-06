//! Line of sight through a map: whether a straight line between two points
//! passes through solid world or a solid brush entity (R&D, highlight
//! detection: wall shots and prefires).
//!
//! The trace walks hull 0, the point hull the renderer and bullets use: each
//! node splits space by a plane, and a leaf's `contents` says what is there
//! (`CONTENTS_SOLID` is -2). It is `SV_RecursiveHullCheck` reduced to a yes or
//! no. Brush entities (doors, `func_wall`, breakables) have their own trees,
//! one per submodel, which the world's does not contain: a bullet through a
//! closed door crosses one of those, not the world.
//!
//! Brush entities are traced where the map places them. A door that was open
//! at the time is still closed here, so "through an object" is a weaker claim
//! than "through a wall".

use super::bsp::{Bsp, Leaf, Node, Plane};
use super::bsp_entities::MapEntity;

pub const CONTENTS_SOLID: i32 = -2;

/// Brush entities that stop a line of sight while closed or standing.
const SOLID_BRUSH_CLASSES: &[&str] = &[
    "func_door",
    "func_door_rotating",
    "func_wall",
    "func_wall_toggle",
    "func_breakable",
    "momentary_door",
    "func_pushable",
    "func_rotating",
];

/// Whether the segment `a`→`b` crosses a solid leaf of the tree at `node`.
pub fn segment_blocked(
    planes: &[Plane],
    nodes: &[Node],
    leaves: &[Leaf],
    node: i32,
    a: [f32; 3],
    b: [f32; 3],
) -> bool {
    blocked(planes, nodes, leaves, node, a, b, 0)
}

fn blocked(
    planes: &[Plane],
    nodes: &[Node],
    leaves: &[Leaf],
    node: i32,
    a: [f32; 3],
    b: [f32; 3],
    depth: u32,
) -> bool {
    // A malformed tree must not recurse forever.
    if depth > 4096 {
        return false;
    }
    if node < 0 {
        let leaf = (-1 - node) as usize;
        return leaves
            .get(leaf)
            .is_some_and(|l| l.contents == CONTENTS_SOLID);
    }
    let Some(n) = nodes.get(node as usize) else {
        return false;
    };
    let Some(plane) = planes.get(n.plane as usize) else {
        return false;
    };
    let side = |p: [f32; 3]| {
        plane.normal[0] * p[0] + plane.normal[1] * p[1] + plane.normal[2] * p[2] - plane.dist
    };
    let (t1, t2) = (side(a), side(b));
    if t1 >= 0.0 && t2 >= 0.0 {
        return blocked(planes, nodes, leaves, n.children[0], a, b, depth + 1);
    }
    if t1 < 0.0 && t2 < 0.0 {
        return blocked(planes, nodes, leaves, n.children[1], a, b, depth + 1);
    }
    let frac = t1 / (t1 - t2);
    let mid = [
        a[0] + (b[0] - a[0]) * frac,
        a[1] + (b[1] - a[1]) * frac,
        a[2] + (b[2] - a[2]) * frac,
    ];
    let near = usize::from(t1 < 0.0);
    blocked(planes, nodes, leaves, n.children[near], a, mid, depth + 1)
        || blocked(
            planes,
            nodes,
            leaves,
            n.children[1 - near],
            mid,
            b,
            depth + 1,
        )
}

/// What stands between two points.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub enum Sight {
    Clear,
    /// A brush entity (a door, a table, a breakable) and nothing else.
    Object,
    World,
}

/// A map ready for line-of-sight checks.
pub struct SightMap {
    bsp: Bsp,
    /// Submodel head node and the offset its coordinates are relative to.
    solids: Vec<(i32, [f32; 3])>,
}

impl SightMap {
    pub fn new(bsp: Bsp, entities: &[MapEntity]) -> Self {
        let solids = entities
            .iter()
            .filter(|e| SOLID_BRUSH_CLASSES.contains(&e.classname()))
            .filter_map(|e| {
                let model = bsp.models.get(e.brush_submodel()? as usize)?;
                let origin = e
                    .get("origin")
                    .map(|o| {
                        let v: Vec<f32> = o
                            .split_whitespace()
                            .filter_map(|x| x.parse().ok())
                            .collect();
                        [
                            v.first().copied().unwrap_or(0.0),
                            v.get(1).copied().unwrap_or(0.0),
                            v.get(2).copied().unwrap_or(0.0),
                        ]
                    })
                    .unwrap_or([0.0; 3]);
                Some((model.head_node, origin))
            })
            .collect();
        Self { bsp, solids }
    }

    pub fn sight(&self, a: [f32; 3], b: [f32; 3]) -> Sight {
        let b_ = &self.bsp;
        if segment_blocked(&b_.planes, &b_.nodes, &b_.leaves, b_.head_node, a, b) {
            return Sight::World;
        }
        let local = |p: [f32; 3], o: [f32; 3]| [p[0] - o[0], p[1] - o[1], p[2] - o[2]];
        if self.solids.iter().any(|(head, origin)| {
            segment_blocked(
                &b_.planes,
                &b_.nodes,
                &b_.leaves,
                *head,
                local(a, *origin),
                local(b, *origin),
            )
        }) {
            return Sight::Object;
        }
        Sight::Clear
    }

    /// How far along `a`→`b` the line first meets solid world, as a fraction,
    /// or `None` when it never does.
    pub fn first_world_hit(&self, a: [f32; 3], b: [f32; 3]) -> Option<f32> {
        let m = &self.bsp;
        let at = |f: f32| {
            [
                a[0] + (b[0] - a[0]) * f,
                a[1] + (b[1] - a[1]) * f,
                a[2] + (b[2] - a[2]) * f,
            ]
        };
        if !segment_blocked(&m.planes, &m.nodes, &m.leaves, m.head_node, a, b) {
            return None;
        }
        let (mut lo, mut hi) = (0.0f32, 1.0f32);
        for _ in 0..24 {
            let mid = (lo + hi) / 2.0;
            if segment_blocked(&m.planes, &m.nodes, &m.leaves, m.head_node, a, at(mid)) {
                hi = mid;
            } else {
                lo = mid;
            }
        }
        Some(hi)
    }

    /// The texture of the world face where `a`→`b` first meets solid, if any.
    pub fn world_hit_texture(&self, a: [f32; 3], b: [f32; 3]) -> Option<String> {
        let f = self.first_world_hit(a, b)?;
        let p = [
            a[0] + (b[0] - a[0]) * f,
            a[1] + (b[1] - a[1]) * f,
            a[2] + (b[2] - a[2]) * f,
        ];
        let m = &self.bsp;
        m.world_faces()
            .filter_map(|i| m.point_on_face(i, &p, 2.0).map(|d| (i, d)))
            .min_by(|x, y| x.1.total_cmp(&y.1))
            .and_then(|(i, _)| m.texture_name(i).map(str::to_string))
    }

    /// Whether `a` sits inside solid world (a bad eye position).
    pub fn in_solid(&self, a: [f32; 3]) -> bool {
        let b = &self.bsp;
        segment_blocked(&b.planes, &b.nodes, &b.leaves, b.head_node, a, a)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One plane, x = 10: in front (x > 10) is empty, behind is solid.
    fn half_space() -> (Vec<Plane>, Vec<Node>, Vec<Leaf>) {
        let planes = vec![Plane {
            normal: [1.0, 0.0, 0.0],
            dist: 10.0,
        }];
        let nodes = vec![Node {
            plane: 0,
            children: [-1, -2],
        }];
        let leaves = vec![
            Leaf {
                contents: -1,
                vis_offset: -1,
            },
            Leaf {
                contents: CONTENTS_SOLID,
                vis_offset: -1,
            },
        ];
        (planes, nodes, leaves)
    }

    #[test]
    fn a_line_in_empty_space_is_clear() {
        let (p, n, l) = half_space();
        assert!(!segment_blocked(
            &p,
            &n,
            &l,
            0,
            [20.0, 0.0, 0.0],
            [50.0, 5.0, 0.0]
        ));
    }

    #[test]
    fn a_line_into_solid_is_blocked() {
        let (p, n, l) = half_space();
        assert!(segment_blocked(
            &p,
            &n,
            &l,
            0,
            [20.0, 0.0, 0.0],
            [0.0, 0.0, 0.0]
        ));
        assert!(segment_blocked(
            &p,
            &n,
            &l,
            0,
            [0.0, 0.0, 0.0],
            [5.0, 0.0, 0.0]
        ));
    }

    /// A slab: solid between x = 10 and x = 20, empty either side.
    #[test]
    fn a_line_through_a_wall_is_blocked() {
        let planes = vec![
            Plane {
                normal: [1.0, 0.0, 0.0],
                dist: 10.0,
            },
            Plane {
                normal: [1.0, 0.0, 0.0],
                dist: 20.0,
            },
        ];
        // Node 0: x >= 10 goes to node 1, else empty. Node 1: x >= 20 empty,
        // else solid.
        let nodes = vec![
            Node {
                plane: 0,
                children: [1, -1],
            },
            Node {
                plane: 1,
                children: [-1, -2],
            },
        ];
        let leaves = vec![
            Leaf {
                contents: -1,
                vis_offset: -1,
            },
            Leaf {
                contents: CONTENTS_SOLID,
                vis_offset: -1,
            },
        ];
        assert!(segment_blocked(
            &planes,
            &nodes,
            &leaves,
            0,
            [0.0; 3],
            [30.0, 0.0, 0.0]
        ));
        assert!(!segment_blocked(
            &planes,
            &nodes,
            &leaves,
            0,
            [0.0; 3],
            [5.0, 0.0, 0.0]
        ));
        assert!(!segment_blocked(
            &planes,
            &nodes,
            &leaves,
            0,
            [25.0, 0.0, 0.0],
            [40.0, 0.0, 0.0]
        ));
    }
}
