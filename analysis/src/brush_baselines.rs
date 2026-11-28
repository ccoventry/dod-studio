//! Every brush entity's spawn baseline in a demo: the `*N` it names, its
//! bounds and origin (`dod::brush_pieces::Spawned`) and its render settings.
//! What the hook DLL's `ds_map_fix_old_build` reads out of the engine during
//! playback (#1540), read here from the file, for
//! `examples/brush_bounds_probe.rs` and the tests that check the hook's rule
//! on real demos.

use dem::bit::BitSliceCast;
use dem::types::{Delta, Demo, EngineMessage, FrameData, MessageData, NetMessage};
use dod::brush_pieces::{Bounds, Spawned};
use std::collections::{BTreeMap, HashMap};

/// `svc_resourcelist`'s type for a model.
const RESOURCE_MODEL: u8 = 2;

/// One brush entity's first spawn baseline.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrushBaseline {
    pub entity: u16,
    pub spawned: Spawned,
    pub rendermode: i32,
    pub renderamt: i32,
    pub renderfx: i32,
    pub skin: i32,
    pub solid: i32,
    pub effects: i32,
    /// How many packets carried the entity: one the server never sends
    /// (`EF_NODRAW` triggers) is never drawn, whatever its baseline says.
    pub packets: u32,
}

fn field<'a>(delta: &'a Delta, name: &str) -> Option<&'a [u8]> {
    delta
        .iter()
        .find(|(k, _)| k.trim_matches(|c: char| c == '\0' || c.is_whitespace()) == name)
        .map(|(_, v)| v.as_slice())
}

fn f32_of(delta: &Delta, name: &str) -> f32 {
    field(delta, name)
        .and_then(|v| v.get(..4))
        .map(|v| f32::from_le_bytes([v[0], v[1], v[2], v[3]]))
        .unwrap_or(0.0)
}

fn i32_of(delta: &Delta, name: &str) -> i32 {
    field(delta, name)
        .map(|v| match v {
            [] => 0,
            [a] => i32::from(*a),
            [a, b] | [a, b, _] => i32::from(i16::from_le_bytes([*a, *b])),
            [a, b, c, d, ..] => i32::from_le_bytes([*a, *b, *c, *d]),
        })
        .unwrap_or(0)
}

fn vec3_of(delta: &Delta, name: &str) -> [f32; 3] {
    [0, 1, 2].map(|i| f32_of(delta, &format!("{name}[{i}]")))
}

/// The baseline of every entity in the demo's first `svc_spawnbaseline`
/// whose model is a brush piece (`*N` in the demo's model list), by entity
/// number.
pub fn brush_baselines(demo: &Demo) -> Vec<BrushBaseline> {
    let mut models: HashMap<u32, String> = HashMap::new();
    let mut rows: BTreeMap<u16, BrushBaseline> = BTreeMap::new();
    let mut seen: HashMap<u16, u32> = HashMap::new();
    let mut have_baseline = false;
    for entry in &demo.directory.entries {
        for frame in &entry.frames {
            let FrameData::NetworkMessage(bt) = &frame.frame_data else {
                continue;
            };
            let MessageData::Parsed(msgs) = &bt.1.messages else {
                continue;
            };
            for m in msgs {
                let NetMessage::EngineMessage(em) = m else {
                    continue;
                };
                match &**em {
                    EngineMessage::SvcResourceList(rl) => {
                        for r in rl
                            .resources
                            .iter()
                            .filter(|r| r.type_.to_u8() == RESOURCE_MODEL)
                        {
                            models.insert(
                                r.index.to_u32(),
                                r.name.get_string().trim_matches('\0').to_string(),
                            );
                        }
                    }
                    EngineMessage::SvcPacketEntities(pe) => {
                        for s in &pe.entity_states {
                            *seen.entry(s.entity_index).or_default() += 1;
                        }
                    }
                    EngineMessage::SvcDeltaPacketEntities(pe) => {
                        for s in pe.entity_states.iter().filter(|s| !s.remove_entity) {
                            *seen.entry(s.entity_index).or_default() += 1;
                        }
                    }
                    EngineMessage::SvcSpawnBaseline(sb) if !have_baseline => {
                        have_baseline = true;
                        for e in &sb.entities {
                            let d = &e.delta;
                            let index = u32::try_from(i32_of(d, "modelindex")).unwrap_or(0);
                            let Some(named) = models
                                .get(&index)
                                .and_then(|name| name.strip_prefix('*'))
                                .and_then(|n| n.parse().ok())
                            else {
                                continue;
                            };
                            rows.insert(
                                e.entity_index,
                                BrushBaseline {
                                    entity: e.entity_index,
                                    spawned: Spawned {
                                        named,
                                        bounds: Bounds {
                                            mins: vec3_of(d, "mins"),
                                            maxs: vec3_of(d, "maxs"),
                                        },
                                        origin: vec3_of(d, "origin"),
                                    },
                                    rendermode: i32_of(d, "rendermode"),
                                    renderamt: i32_of(d, "renderamt"),
                                    renderfx: i32_of(d, "renderfx"),
                                    skin: i32_of(d, "skin"),
                                    solid: i32_of(d, "solid"),
                                    effects: i32_of(d, "effects"),
                                    packets: 0,
                                },
                            );
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    for (entity, row) in &mut rows {
        row.packets = seen.get(entity).copied().unwrap_or(0);
    }
    rows.into_values().collect()
}

#[cfg(test)]
mod tests {
    //! The hook's rule on real demos. None is in the tree (community demos
    //! are not ours to commit); paths come from env vars, else the PRE
    //! install's `dod\`:
    //!
    //!     DOD_OLD_CHEMILLE_DEMO=<chemille-gamewyze-fx-1.dem>
    //!     DOD_CURRENT_CHEMILLE_DEMO=<a demo on today's dod_chemille>
    //!     DOD_CHEMILLE_BSP=<dod_chemille.bsp>
    //!     cargo test -p analysis -- --ignored brush_baselines

    use super::*;
    use dod::brush_pieces::{self, Counts};
    use std::path::PathBuf;

    const PRE_DOD: &str =
        r"C:\Program Files (x86)\Steam\steamapps\common\Half-Life - PRE-Anniversary for Movies\dod";

    fn path(var: &str, fallback: &str) -> PathBuf {
        std::env::var_os(var)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(PRE_DOD).join(fallback))
    }

    fn read(var: &str, fallback: &str) -> Vec<u8> {
        let path = path(var, fallback);
        std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e} (set {var})", path.display()))
    }

    fn counts_on_chemille(demo_var: &str, demo_fallback: &str) -> Counts {
        let demo = dem::open_demo_from_bytes(&read(demo_var, demo_fallback)).unwrap();
        let pieces =
            brush_pieces::pieces_from_bsp(&read("DOD_CHEMILLE_BSP", r"maps\dod_chemille.bsp"))
                .unwrap();
        let spawned: Vec<Spawned> = brush_baselines(&demo).iter().map(|b| b.spawned).collect();
        brush_pieces::plan(&spawned, &pieces).counts
    }

    #[test]
    #[ignore = "needs chemille-gamewyze-fx-1.dem (2004) and dod_chemille.bsp"]
    fn an_old_chemille_demo_moves_58_pieces_and_hides_14() {
        let counts = counts_on_chemille("DOD_OLD_CHEMILLE_DEMO", "chemille-gamewyze-fx-1.dem");
        assert_eq!(
            counts,
            Counts {
                named: 112,
                other: 58,
                nothing: 14,
                ..Counts::default()
            }
        );
        assert!(counts.is_other_build());
    }

    #[test]
    #[ignore = "needs a demo on today's dod_chemille and dod_chemille.bsp"]
    fn a_demo_on_todays_chemille_fits_every_piece() {
        let counts = counts_on_chemille(
            "DOD_CURRENT_CHEMILLE_DEMO",
            "k4-phillyday1-draft-map5-chemille-allies.dem",
        );
        assert!(counts.named > 0, "{counts:?}");
        assert_eq!(
            counts,
            Counts {
                named: counts.named,
                ..Counts::default()
            }
        );
        assert!(!counts.is_other_build());
    }
}
