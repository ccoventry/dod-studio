//! What `map_old_build` reads out of the engine: the loaded map's brush
//! pieces, the demo's model list and its entities' spawn baselines.
//!
//! Every `model_t` field read sits at the SDK's offset (`com_model.h`) in
//! both engine builds: `Mod_LoadSubmodels` (pre-Anniversary `hw.dll+0x40ba0`;
//! inlined into `Mod_LoadBrushModel`, `+0x23f060`, on the 25th Anniversary
//! build) stores the submodel array at +0x7c and its count at +0x78, padding
//! each one's bounds by one unit; `Mod_LoadBrushModel` (pre-Anniversary
//! `+0x41bb0`) copies a submodel's bounds into `mins` +0x54 and `maxs` +0x60
//! of each `*N` model it makes. `type` (+0x44) and `entities` (+0x180) are
//! the offsets `map_sounds` and `map_light` read.
//! `tools/verify_map_old_build_offsets.py` checks them against both
//! `hw.dll`s, and [`read_models`] checks every precached `*N` model's bounds
//! against the submodel array at run time, so a build whose layout moved
//! reads as unusable and nothing is changed.

#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use dod::brush_pieces::{Bounds, Piece, Spawned};

use crate::engine::{self, EntityStateS, ModelSPartial};

/// `model_t.type`; 0 is `mod_brush`.
pub(super) const MODEL_TYPE_AT: usize = 0x44;
const MOD_BRUSH: i32 = 0;
/// `model_t.mins` and `maxs`: the model's padded bounds.
pub(super) const MODEL_MINS_AT: usize = 0x54;
pub(super) const MODEL_MAXS_AT: usize = 0x60;
/// `model_t.numsubmodels` and `submodels`, the world's `dmodel_t` array.
pub(super) const MODEL_NUMSUBMODELS_AT: usize = 0x78;
pub(super) const MODEL_SUBMODELS_AT: usize = 0x7c;
/// `model_t.entities`: the engine's NUL-terminated copy of the entity lump.
pub(super) const MODEL_ENTITIES_AT: usize = 0x180;
/// How far into a `model_t` the world's read goes.
const WORLD_READ_LEN: usize = MODEL_ENTITIES_AT + 4;
/// How far into a `*N` model's `model_t` its check goes.
const PIECE_READ_LEN: usize = MODEL_MAXS_AT + 12;
/// `sizeof(dmodel_t)` in memory: `mins`, `maxs` first, as on disk.
pub(super) const DMODEL_SIZE: usize = 64;
/// More pieces than any map has (the engine's own limit is 400): a count past
/// this is not a submodel count.
const MAX_PIECES: usize = 1024;
/// `MAX_MODELS`: the length of the client's model list.
const MAX_MODELS: i32 = 512;
/// More entities than the engine keeps: the walk stops where
/// `GetEntityByIndex` returns null (it checks `cl.max_edicts`, both builds).
const MAX_ENTITIES: i32 = 8192;
/// Largest entity lump read, as in `map_sounds`.
const MAX_ENTITY_TEXT: usize = 2 * 1024 * 1024;

/// Why the map's pieces couldn't be read.
pub(super) enum NotRead {
    /// The level is still loading: try again on a later frame.
    NotYet,
    /// This world can't be read, and won't be until another loads.
    Unusable(String),
}

/// The loaded map's pieces, `*N` at index `N`.
pub(super) struct Pieces {
    pub pieces: Vec<Piece>,
    /// Why the pieces carry no origins, when the entity lump couldn't be read.
    pub no_origins: Option<String>,
}

/// The world model the engine draws, entity 0's, or 0 before one loads.
pub(super) fn world_model() -> usize {
    let Some(engfuncs) = engine::engfuncs() else {
        return 0;
    };
    // Safety: the engine's own accessor, bounds-checked by the engine.
    let world = unsafe { (engfuncs.get_entity_by_index)(0) };
    if world.is_null() {
        return 0;
    }
    // Safety: a cl_entity_t the engine owns; `model` is the SDK layout.
    unsafe { (*world).model as usize }
}

fn read_i32(at: usize) -> i32 {
    // Safety: every caller checked `at` is readable first.
    unsafe { std::ptr::read_unaligned(at as *const i32) }
}

fn read_vec3(at: usize) -> [f32; 3] {
    // Safety: as `read_i32`.
    unsafe { std::ptr::read_unaligned(at as *const [f32; 3]) }
}

/// The pieces of `world`, the world model of `level` (`maps/<map>.bsp`).
pub(super) fn read_pieces(world: usize, level: &str) -> Result<Pieces, NotRead> {
    if world == 0 || !crate::crash::readable(world, WORLD_READ_LEN) {
        return Err(NotRead::NotYet);
    }
    // Safety: readable, checked above; `name` is a NUL-padded char[64].
    let name = unsafe { (*(world as *const ModelSPartial)).name_str() };
    // Until the new map's world model replaces the old one, the old one's
    // arrays are hunk memory the new map may already be reusing.
    if !name.eq_ignore_ascii_case(level) || read_i32(world + MODEL_TYPE_AT) != MOD_BRUSH {
        return Err(NotRead::NotYet);
    }
    let count = read_i32(world + MODEL_NUMSUBMODELS_AT);
    let Some(count) = usize::try_from(count)
        .ok()
        .filter(|&c| (1..=MAX_PIECES).contains(&c))
    else {
        return Err(NotRead::Unusable(format!(
            "the world model says it has {count} pieces"
        )));
    };
    // A 32-bit pointer: this DLL only ever runs in the 32-bit engine.
    let submodels = read_i32(world + MODEL_SUBMODELS_AT) as u32 as usize;
    if submodels == 0 || !crate::crash::readable(submodels, count * DMODEL_SIZE) {
        return Err(NotRead::Unusable(
            "the world model's piece list isn't readable".into(),
        ));
    }
    let bounds: Vec<Bounds> = (0..count)
        .map(|n| {
            let at = submodels + n * DMODEL_SIZE;
            Bounds {
                mins: read_vec3(at),
                maxs: read_vec3(at + 12),
            }
        })
        .collect();
    let entities = read_i32(world + MODEL_ENTITIES_AT) as u32 as usize;
    let origins = crate::crash::read_c_string(entities, MAX_ENTITY_TEXT)
        .ok_or_else(|| "its entity text isn't readable".to_string())
        .and_then(|text| {
            dod::map_entities::parse_entity_text(&dod::map_entities::text_up_to_nul(&text))
        })
        .map(|parsed| dod::brush_pieces::piece_origins(&parsed, count));
    let (origins, no_origins) = match origins {
        Ok(origins) => (origins.into_iter().map(Some).collect(), None),
        Err(why) => (vec![None; count], Some(why)),
    };
    let pieces = bounds
        .into_iter()
        .zip(origins)
        .map(|(bounds, origin)| Piece { bounds, origin })
        .collect();
    Ok(Pieces { pieces, no_origins })
}

/// The demo's model list, as far as the map's pieces go.
pub(super) struct Models {
    /// The piece number `N` of each model-list entry named `*N`, by index.
    piece_at: Vec<Option<usize>>,
    /// The engine's `model_t` for piece `N`, or 0 when the demo's list has
    /// no `*N` (a piece only the installed build has).
    model_of: Vec<usize>,
}

impl Models {
    /// The piece a model-list index names, when it names one.
    pub fn piece_at(&self, index: i32) -> Option<usize> {
        let index = usize::try_from(index).ok()?;
        self.piece_at.get(index).copied().flatten()
    }

    /// The engine's model for piece `n`, when the demo's list has it.
    pub fn model_of(&self, n: usize) -> Option<usize> {
        self.model_of.get(n).copied().filter(|&m| m != 0)
    }
}

/// The `N` of a model named `*N`.
fn piece_number(name: &str) -> Option<usize> {
    let digits = name.strip_prefix('*')?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// The demo's model list, each `*N` checked against the map's piece `N`:
/// `Err` when one doesn't match, which means this build's `model_t` isn't
/// laid out as checked.
pub(super) fn read_models(pieces: &[Piece]) -> Result<Models, String> {
    let Some(studio) = engine::engine_studio() else {
        return Err("the engine's studio interface isn't captured".into());
    };
    let mut models = Models {
        piece_at: vec![None; MAX_MODELS as usize],
        model_of: vec![0; pieces.len()],
    };
    for index in 1..MAX_MODELS {
        // Safety: the engine's own accessor; null for an index the demo's
        // list doesn't fill, and every index is inside the engine's array.
        let model = unsafe { (studio.get_model_by_index)(index) } as usize;
        if model == 0 || !crate::crash::readable(model, PIECE_READ_LEN) {
            continue;
        }
        // Safety: readable, checked above.
        let name = unsafe { (*(model as *const ModelSPartial)).name_str() };
        let Some(n) = piece_number(&name) else {
            continue;
        };
        models.piece_at[index as usize] = Some(n);
        // A number past the map's last piece is one only the demo's build
        // had: nothing to draw it with, and nothing to check.
        let Some(piece) = pieces.get(n) else {
            continue;
        };
        let bounds = Bounds {
            mins: read_vec3(model + MODEL_MINS_AT),
            maxs: read_vec3(model + MODEL_MAXS_AT),
        };
        if read_i32(model + MODEL_TYPE_AT) != MOD_BRUSH || bounds != piece.bounds {
            return Err(format!(
                "model *{n}'s bounds aren't the map's piece {n}: this engine's model layout isn't the one checked"
            ));
        }
        models.model_of[n] = model;
    }
    Ok(models)
}

/// What an entity's baseline was when its action was decided: a new demo's
/// baselines differ, so its entities are read again.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Fingerprint {
    pub modelindex: i32,
    pub(super) bounds_and_origin: [u32; 9],
}

pub(super) fn fingerprint(baseline: &EntityStateS) -> Fingerprint {
    let mut bits = [0u32; 9];
    for (slot, value) in bits.iter_mut().zip(
        [baseline.mins, baseline.maxs, baseline.origin]
            .iter()
            .flat_map(|v| [v.x, v.y, v.z]),
    ) {
        *slot = value.to_bits();
    }
    Fingerprint {
        modelindex: baseline.modelindex,
        bounds_and_origin: bits,
    }
}

/// One entity slot's spawn baseline.
pub(super) struct Baseline {
    pub fingerprint: Fingerprint,
    /// The brush entity it spawned, when it names a piece.
    pub spawned: Option<Spawned>,
}

/// Every entity slot's baseline, by entity index (the world's, at 0, is
/// never a brush entity).
pub(super) fn read_baselines(models: &Models) -> Vec<Baseline> {
    let Some(engfuncs) = engine::engfuncs() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for index in 0..MAX_ENTITIES {
        // Safety: the engine's own accessor, null past `cl.max_edicts`.
        let entity = unsafe { (engfuncs.get_entity_by_index)(index) };
        if entity.is_null() {
            break;
        }
        // Safety: a cl_entity_t the engine owns, on its thread.
        let baseline = unsafe { &(*entity).baseline };
        let spawned = (index != 0)
            .then(|| models.piece_at(baseline.modelindex))
            .flatten()
            .map(|named| Spawned {
                named,
                bounds: Bounds {
                    mins: [baseline.mins.x, baseline.mins.y, baseline.mins.z],
                    maxs: [baseline.maxs.x, baseline.maxs.y, baseline.maxs.z],
                },
                origin: [baseline.origin.x, baseline.origin.y, baseline.origin.z],
            });
        out.push(Baseline {
            fingerprint: fingerprint(baseline),
            spawned,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_piece_number_is_the_digits_after_the_star() {
        assert_eq!(piece_number("*0"), Some(0));
        assert_eq!(piece_number("*184"), Some(184));
        assert_eq!(piece_number("*"), None);
        assert_eq!(piece_number("*12a"), None);
        assert_eq!(piece_number("models/player.mdl"), None);
        assert_eq!(piece_number("*-1"), None);
    }

    #[test]
    fn a_model_list_index_outside_the_list_names_no_piece() {
        let models = Models {
            piece_at: vec![None, Some(0), Some(1)],
            model_of: vec![0, 0x1000],
        };
        assert_eq!(models.piece_at(2), Some(1));
        assert_eq!(models.piece_at(3), None);
        assert_eq!(models.piece_at(-1), None);
        assert_eq!(models.model_of(1), Some(0x1000));
        assert_eq!(models.model_of(0), None);
        assert_eq!(models.model_of(5), None);
    }

    #[test]
    fn a_baselines_fingerprint_changes_with_its_bounds() {
        // Safety: a plain-data struct, all zeros is a valid value.
        let mut state: EntityStateS = unsafe { std::mem::zeroed() };
        state.modelindex = 5;
        let before = fingerprint(&state);
        state.maxs.z = 64.0;
        assert_ne!(fingerprint(&state), before);
        assert_eq!(fingerprint(&state).modelindex, 5);
    }
}
