//! The parts of a GoldSrc BSP (version 30) an overview needs: the faces as
//! polygons in world space, the clip hulls a player collides with, the point
//! hull's contents (sky, water), and the entities.
//!
//! `patch::bsp` reads a different subset for the decal flush (visibility, no
//! clip hulls); this one is kept separate so neither has to carry the other's
//! fields.

use std::collections::HashMap;

use crate::patch::bsp_entities::{self, MapEntity};

const LUMP_ENTITIES: usize = 0;
const LUMP_PLANES: usize = 1;
const LUMP_TEXTURES: usize = 2;
const LUMP_VERTICES: usize = 3;
const LUMP_NODES: usize = 5;
const LUMP_TEXINFO: usize = 6;
const LUMP_FACES: usize = 7;
const LUMP_CLIPNODES: usize = 9;
const LUMP_LEAVES: usize = 10;
const LUMP_EDGES: usize = 12;
const LUMP_SURFEDGES: usize = 13;
const LUMP_MODELS: usize = 14;
const LUMP_COUNT: usize = 15;

/// Point contents, as the engine numbers them.
pub const CONTENTS_EMPTY: i32 = -1;
pub const CONTENTS_SOLID: i32 = -2;
pub const CONTENTS_SKY: i32 = -6;

#[derive(Debug, Clone, Copy)]
pub struct Plane {
    pub normal: [f32; 3],
    pub dist: f32,
    /// 0..2: the plane is axial on x, y or z; anything else is not.
    pub kind: i32,
}

#[derive(Debug, Clone)]
pub struct Model {
    pub mins: [f32; 3],
    pub maxs: [f32; 3],
    /// Root nodes: hull 0 into `nodes`, hulls 1..3 into `clipnodes`.
    pub heads: [i32; 4],
    pub first_face: usize,
    pub face_count: usize,
    /// The owning entity's `origin`: a brush entity with one stores its
    /// faces relative to it (a rotating door), so they are moved by it here.
    pub offset: [f32; 3],
    /// The owning entity's classname (`worldspawn` for model 0).
    pub class: String,
}

#[derive(Debug, Clone)]
pub struct Face {
    /// Lower-cased texture name.
    pub texture: String,
    /// World-space polygon, brush-entity origin applied.
    pub points: Vec<[f32; 3]>,
    /// The z of the face's outward normal: above 0.7 is ground a player can
    /// stand on.
    pub normal_z: f32,
    pub model: usize,
}

pub struct Level {
    pub name: String,
    pub planes: Vec<Plane>,
    pub faces: Vec<Face>,
    pub models: Vec<Model>,
    pub entities: Vec<MapEntity>,
    /// Hull 0: (plane, [child; 2]); a negative child is `!leaf`.
    nodes: Vec<(u32, [i32; 2])>,
    leaf_contents: Vec<i32>,
    /// Hulls 1..3: (plane, [child; 2]); a negative child is the contents.
    clipnodes: Vec<(u32, [i32; 2])>,
}

fn rd_i32(b: &[u8], at: usize) -> Result<i32, String> {
    b.get(at..at + 4)
        .map(|s| i32::from_le_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or_else(|| format!("truncated at byte {at}"))
}

fn rd_i16(b: &[u8], at: usize) -> Result<i16, String> {
    b.get(at..at + 2)
        .map(|s| i16::from_le_bytes([s[0], s[1]]))
        .ok_or_else(|| format!("truncated at byte {at}"))
}

fn rd_f32(b: &[u8], at: usize) -> Result<f32, String> {
    rd_i32(b, at).map(|v| f32::from_bits(v as u32))
}

fn rd_vec3(b: &[u8], at: usize) -> Result<[f32; 3], String> {
    Ok([rd_f32(b, at)?, rd_f32(b, at + 4)?, rd_f32(b, at + 8)?])
}

fn lump(bytes: &[u8], index: usize) -> Result<&[u8], String> {
    let at = 4 + index * 8;
    let offset = rd_i32(bytes, at)? as usize;
    let length = rd_i32(bytes, at + 4)? as usize;
    bytes
        .get(offset..offset.saturating_add(length))
        .ok_or_else(|| format!("lump {index} runs past the end of the file"))
}

fn entries<T>(
    data: &[u8],
    stride: usize,
    mut parse: impl FnMut(&[u8], usize) -> Result<T, String>,
) -> Result<Vec<T>, String> {
    (0..data.len() / stride)
        .map(|i| parse(data, i * stride))
        .collect()
}

fn texture_names(data: &[u8]) -> Result<Vec<String>, String> {
    let count = rd_i32(data, 0)?.max(0) as usize;
    let mut names = Vec::with_capacity(count);
    for i in 0..count {
        let offset = rd_i32(data, 4 + i * 4)?;
        let name = if offset < 0 {
            String::new()
        } else {
            let at = offset as usize;
            let raw = data.get(at..at + 16).unwrap_or(&[]);
            let end = raw.iter().position(|&c| c == 0).unwrap_or(raw.len());
            String::from_utf8_lossy(&raw[..end]).to_ascii_lowercase()
        };
        names.push(name);
    }
    Ok(names)
}

fn parse_origin(text: &str) -> Option<[f32; 3]> {
    let mut parts = text.split_whitespace().map(|p| p.parse::<f32>().ok());
    Some([parts.next()??, parts.next()??, parts.next()??])
}

impl Level {
    pub fn parse(name: &str, bytes: &[u8]) -> Result<Level, String> {
        if bytes.len() < 4 + LUMP_COUNT * 8 {
            return Err("file is shorter than a BSP header".to_string());
        }
        let version = rd_i32(bytes, 0)?;
        if version != 30 {
            return Err(format!("BSP version {version} is not GoldSrc's 30"));
        }
        let planes = entries(lump(bytes, LUMP_PLANES)?, 20, |b, at| {
            Ok(Plane {
                normal: rd_vec3(b, at)?,
                dist: rd_f32(b, at + 12)?,
                kind: rd_i32(b, at + 16)?,
            })
        })?;
        let vertices = entries(lump(bytes, LUMP_VERTICES)?, 12, rd_vec3)?;
        let edges = entries(lump(bytes, LUMP_EDGES)?, 4, |b, at| {
            Ok([rd_i16(b, at)? as u16, rd_i16(b, at + 2)? as u16])
        })?;
        let surfedges = entries(lump(bytes, LUMP_SURFEDGES)?, 4, rd_i32)?;
        let texinfo = entries(lump(bytes, LUMP_TEXINFO)?, 40, |b, at| rd_i32(b, at + 32))?;
        let names = texture_names(lump(bytes, LUMP_TEXTURES)?)?;
        let nodes = entries(lump(bytes, LUMP_NODES)?, 24, |b, at| {
            Ok((
                rd_i32(b, at)? as u32,
                [rd_i16(b, at + 4)? as i32, rd_i16(b, at + 6)? as i32],
            ))
        })?;
        let leaf_contents = entries(lump(bytes, LUMP_LEAVES)?, 28, rd_i32)?;
        let clipnodes = entries(lump(bytes, LUMP_CLIPNODES)?, 8, |b, at| {
            Ok((
                rd_i32(b, at)? as u32,
                [rd_i16(b, at + 4)? as i32, rd_i16(b, at + 6)? as i32],
            ))
        })?;
        let entities =
            bsp_entities::parse_entity_text(&String::from_utf8_lossy(lump(bytes, LUMP_ENTITIES)?))?;

        // Brush entities: which model each owns, and its origin.
        let mut owner: HashMap<usize, (String, [f32; 3])> = HashMap::new();
        for entity in &entities {
            if let Some(model) = entity.brush_submodel() {
                let origin = entity
                    .get("origin")
                    .and_then(parse_origin)
                    .unwrap_or([0.0; 3]);
                owner.insert(model as usize, (entity.classname().to_string(), origin));
            }
        }
        let models = entries(lump(bytes, LUMP_MODELS)?, 64, |b, at| {
            Ok(Model {
                mins: rd_vec3(b, at)?,
                maxs: rd_vec3(b, at + 12)?,
                heads: [
                    rd_i32(b, at + 36)?,
                    rd_i32(b, at + 40)?,
                    rd_i32(b, at + 44)?,
                    rd_i32(b, at + 48)?,
                ],
                first_face: rd_i32(b, at + 56)?.max(0) as usize,
                face_count: rd_i32(b, at + 60)?.max(0) as usize,
                offset: [0.0; 3],
                class: String::new(),
            })
        })?;
        let mut models = models;
        for (index, model) in models.iter_mut().enumerate() {
            if index == 0 {
                model.class = "worldspawn".to_string();
            } else if let Some((class, origin)) = owner.get(&index) {
                model.class = class.clone();
                model.offset = *origin;
            }
        }
        let mut face_model = HashMap::new();
        for (index, model) in models.iter().enumerate() {
            for face in model.first_face..model.first_face + model.face_count {
                face_model.insert(face, index);
            }
        }

        let raw_faces = lump(bytes, LUMP_FACES)?;
        let mut faces = Vec::with_capacity(raw_faces.len() / 20);
        for index in 0..raw_faces.len() / 20 {
            let at = index * 20;
            let plane = rd_i16(raw_faces, at)? as u16 as usize;
            let side = rd_i16(raw_faces, at + 2)?;
            let first_edge = rd_i32(raw_faces, at + 4)?.max(0) as usize;
            let edge_count = rd_i16(raw_faces, at + 8)?.max(0) as usize;
            let tex = rd_i16(raw_faces, at + 10)? as u16 as usize;
            let model = face_model.get(&index).copied().unwrap_or(0);
            let offset = models.get(model).map(|m| m.offset).unwrap_or([0.0; 3]);
            let mut points = Vec::with_capacity(edge_count);
            for &surfedge in surfedges
                .get(first_edge..first_edge + edge_count)
                .unwrap_or(&[])
            {
                let edge = edges
                    .get(surfedge.unsigned_abs() as usize)
                    .ok_or("a face names an edge past the end")?;
                let vertex = if surfedge >= 0 { edge[0] } else { edge[1] };
                let v = vertices
                    .get(vertex as usize)
                    .ok_or("an edge names a vertex past the end")?;
                points.push([v[0] + offset[0], v[1] + offset[1], v[2] + offset[2]]);
            }
            let normal_z = planes
                .get(plane)
                .map(|p| if side != 0 { -p.normal[2] } else { p.normal[2] })
                .unwrap_or(0.0);
            let texture = texinfo
                .get(tex)
                .and_then(|&miptex| names.get(miptex.max(0) as usize))
                .cloned()
                .unwrap_or_default();
            faces.push(Face {
                texture,
                points,
                normal_z,
                model,
            });
        }

        Ok(Level {
            name: name.to_string(),
            planes,
            faces,
            models,
            entities,
            nodes,
            leaf_contents,
            clipnodes,
        })
    }

    pub fn from_file(path: &std::path::Path) -> Result<Level, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        Level::parse(&name, &bytes)
    }

    fn side(&self, plane: u32, p: [f32; 3]) -> bool {
        let Some(plane) = self.planes.get(plane as usize) else {
            return true;
        };
        let d = match plane.kind {
            0..=2 => p[plane.kind as usize],
            _ => plane.normal[0] * p[0] + plane.normal[1] * p[1] + plane.normal[2] * p[2],
        };
        d - plane.dist >= 0.0
    }

    /// Contents of `p` in clip hull 1..3 of `model`: [`CONTENTS_SOLID`] or
    /// [`CONTENTS_EMPTY`] (water counts as empty here, as it does for a player).
    pub fn hull_contents(&self, model: usize, hull: usize, p: [f32; 3]) -> i32 {
        let mut node = self.models.get(model).map(|m| m.heads[hull]).unwrap_or(-1);
        let mut steps = 0;
        while node >= 0 {
            let Some(&(plane, children)) = self.clipnodes.get(node as usize) else {
                return CONTENTS_EMPTY;
            };
            node = if self.side(plane, p) {
                children[0]
            } else {
                children[1]
            };
            steps += 1;
            if steps > 4096 {
                return CONTENTS_EMPTY;
            }
        }
        node
    }

    /// Point-hull contents of `p` in the world: empty, solid, water, sky...
    pub fn point_contents(&self, p: [f32; 3]) -> i32 {
        let mut node = self.models.first().map(|m| m.heads[0]).unwrap_or(-1);
        let mut steps = 0;
        while node >= 0 {
            let Some(&(plane, children)) = self.nodes.get(node as usize) else {
                return CONTENTS_EMPTY;
            };
            node = if self.side(plane, p) {
                children[0]
            } else {
                children[1]
            };
            steps += 1;
            if steps > 4096 {
                return CONTENTS_EMPTY;
            }
        }
        self.leaf_contents
            .get((!node) as usize)
            .copied()
            .unwrap_or(CONTENTS_SOLID)
    }

    /// Point-hull contents of `p` inside brush model `model` (its own tree,
    /// in its own space): anything but empty is inside the brush.
    pub fn model_point_contents(&self, model: usize, p: [f32; 3]) -> i32 {
        let mut node = self.models.get(model).map(|m| m.heads[0]).unwrap_or(-1);
        let mut steps = 0;
        while node >= 0 {
            let Some(&(plane, children)) = self.nodes.get(node as usize) else {
                return CONTENTS_EMPTY;
            };
            node = if self.side(plane, p) {
                children[0]
            } else {
                children[1]
            };
            steps += 1;
            if steps > 4096 {
                return CONTENTS_EMPTY;
            }
        }
        self.leaf_contents
            .get((!node) as usize)
            .copied()
            .unwrap_or(CONTENTS_SOLID)
    }

    /// `(classname, origin)` for every entity with an origin.
    pub fn points(&self, class: &str) -> Vec<(&MapEntity, [f32; 3])> {
        self.entities
            .iter()
            .filter(|e| e.classname() == class)
            .filter_map(|e| e.get("origin").and_then(parse_origin).map(|o| (e, o)))
            .collect()
    }
}
