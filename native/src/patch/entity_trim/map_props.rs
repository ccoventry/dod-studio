//! What a map's `.bsp` says about its brush models (`*N`) that the demo can't:
//! where each one sits, and the class of the entity that draws it. Brush
//! entities are networked at origin 0, and `solid` is 0 on the wire for every
//! one of them, the solid ones included (measured on
//! `wsod25_grp3_h1_hltv.dem`, #207), so only the map tells a non-solid
//! `func_illusionary` from a wall.

use std::collections::HashMap;

/// The models lump of a version 30 BSP.
const LUMP_MODELS: usize = 14;
/// One model: mins, maxs, origin, 4 head nodes, vis leaves, first face, faces.
const MODEL_BYTES: usize = 64;

/// Each brush model's centre and the class of the entity that uses it.
#[derive(Debug, Default, Clone)]
pub struct MapProps {
    centres: Vec<[f32; 3]>,
    classes: HashMap<u32, String>,
}

impl MapProps {
    /// Reads a GoldSrc BSP's models and entity lumps. `Err` names what is
    /// wrong with the file.
    pub fn parse(bsp: &[u8]) -> Result<Self, String> {
        let (offset, length) = dod::map_entities::lump_span(bsp, LUMP_MODELS)?;
        let models = bsp
            .get(offset..offset.saturating_add(length))
            .ok_or("the models lump runs past the end of the file")?;
        // `as_chunks` hands out whole 64-byte models, so every read below is
        // inside its chunk; a partial model at the end is left out.
        let centres = models
            .as_chunks::<MODEL_BYTES>()
            .0
            .iter()
            .map(|m| {
                let read = |at: usize| f32::from_le_bytes([m[at], m[at + 1], m[at + 2], m[at + 3]]);
                [0, 1, 2].map(|axis| (read(axis * 4) + read(12 + axis * 4)) / 2.0)
            })
            .collect();
        let classes = dod::map_entities::parse_entities(bsp)?
            .iter()
            .filter_map(|e| Some((e.brush_submodel()?, e.classname().to_string())))
            .collect();
        Ok(Self { centres, classes })
    }

    /// [`MapProps::parse`] of the file at `path`.
    pub fn from_file(path: &std::path::Path) -> Result<Self, String> {
        let bytes =
            std::fs::read(path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
        Self::parse(&bytes).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// The class of the entity drawing brush model `*n`.
    pub fn class_of(&self, n: u32) -> Option<&str> {
        self.classes.get(&n).map(String::as_str)
    }

    /// The centre of brush model `*n`'s bounds.
    pub fn centre_of(&self, n: u32) -> Option<[f32; 3]> {
        self.centres.get(n as usize).copied()
    }

    /// Built from parts, for tests.
    #[cfg(test)]
    pub(super) fn from_parts(centres: Vec<[f32; 3]>, classes: &[(u32, &str)]) -> Self {
        Self {
            centres,
            classes: classes.iter().map(|&(n, c)| (n, c.to_string())).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal BSP: an entity lump and a models lump, every other lump empty.
    fn bsp(entities: &str, models: &[([f32; 3], [f32; 3])]) -> Vec<u8> {
        let header = dod::map_entities::HEADER_SIZE;
        let mut model_bytes = Vec::new();
        for (mins, maxs) in models {
            for v in mins.iter().chain(maxs.iter()) {
                model_bytes.extend_from_slice(&v.to_le_bytes());
            }
            model_bytes.resize(model_bytes.len() + MODEL_BYTES - 24, 0);
        }
        let mut text = entities.as_bytes().to_vec();
        text.push(0);
        let mut out = vec![0u8; header];
        out[0..4].copy_from_slice(&30i32.to_le_bytes());
        let put = |out: &mut Vec<u8>, lump: usize, bytes: &[u8]| {
            let at = out.len();
            out[4 + lump * 8..8 + lump * 8].copy_from_slice(&(at as i32).to_le_bytes());
            out[8 + lump * 8..12 + lump * 8].copy_from_slice(&(bytes.len() as i32).to_le_bytes());
            out.extend_from_slice(bytes);
        };
        put(&mut out, 0, &text);
        put(&mut out, LUMP_MODELS, &model_bytes);
        out
    }

    #[test]
    fn reads_each_brush_models_centre_and_class() {
        let file = bsp(
            "{\n\"classname\" \"worldspawn\"\n}\n{\n\"classname\" \"func_illusionary\"\n\"model\" \"*1\"\n}\n",
            &[([0.0; 3], [0.0; 3]), ([0.0, 10.0, -4.0], [20.0, 30.0, 4.0])],
        );
        let props = MapProps::parse(&file).unwrap();
        assert_eq!(props.class_of(1), Some("func_illusionary"));
        assert_eq!(props.class_of(2), None);
        assert_eq!(props.centre_of(1), Some([10.0, 20.0, 0.0]));
        assert_eq!(props.centre_of(2), None);
    }

    #[test]
    fn a_truncated_file_is_an_error_not_a_panic() {
        let mut file = bsp("{\n}\n", &[([0.0; 3], [1.0; 3])]);
        file.truncate(file.len() - 10);
        assert!(MapProps::parse(&file).is_err());
        assert!(MapProps::parse(b"short").is_err());
    }
}
