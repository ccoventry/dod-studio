//! Which entities a trim may drop, and which it does.
//!
//! Only entities that are safe to lose are eligible: present from the first
//! snapshot, never updated and never removed afterwards (static map props, so
//! dropping one changes nothing else and its index is never reused), never
//! the entry another entry of a full packet takes its baseline from, and
//! never a player or the world. In the order they go:
//!
//! - [`Tier::Invisible`]: a render mode with `renderamt` 15 or less (on
//!   `dod_lennon2`, `func_button` collision hulls round props, and two
//!   near-invisible `func_illusionary`);
//! - [`Tier::Sprite`]: static `.spr` entities (its fog puffs);
//! - [`Tier::Decoration`]: `func_illusionary` brush props, non-solid by
//!   definition so nothing ever collided with one, farthest from anywhere a
//!   player stood first. Needs the map's `.bsp` ([`MapProps`]): without it,
//!   this tier is left out.

use std::collections::{HashMap, HashSet};

use dem::bit::BitSliceCast;
use dem::types::{
    Delta, Demo, EngineMessage, FrameData, MessageData, NetMessage, SvcPacketEntities,
};

use super::MapProps;

/// Player positions are deduplicated on this grid, in world units.
const GRID_UNITS: f32 = 64.0;
/// At or under this `renderamt`, an entity with a render mode is invisible.
const INVISIBLE_RENDERAMT: i32 = 15;

/// Why an entity may be dropped, in the order tiers are used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tier {
    Invisible,
    Sprite,
    Decoration,
}

/// One entity a trim drops.
#[derive(Debug, Clone, PartialEq)]
pub struct Dropped {
    pub index: u16,
    pub tier: Tier,
    /// Its model: `*N` for a brush model, else the precached file.
    pub model: String,
    /// How far it sits from the nearest place a player stood, in units;
    /// infinite when the demo holds no player positions.
    pub distance_units: f32,
}

/// What the demo says about one entity: its spawn state merged with the
/// first snapshot, and whether anything about it changes afterwards.
#[derive(Default)]
struct Entity {
    fields: Delta,
    in_first: bool,
    updates: u32,
    removed: bool,
    /// Another entry of a full packet takes its baseline from this one
    /// (`rewrite`'s module doc), so leaving it out would change that entity.
    offset_base: bool,
}

/// Everything the first pass over the demo learns.
pub(super) struct Survey {
    models: HashMap<u32, String>,
    entities: HashMap<u16, Entity>,
    max_players: u16,
    /// Grid cells a player stood in.
    player_cells: HashSet<(i32, i32, i32)>,
}

/// Keys carry the decoder's trailing NUL (`origin[0]\0`).
fn field<'a>(delta: &'a Delta, name: &str) -> Option<&'a [u8]> {
    delta
        .iter()
        .find(|(k, _)| k.trim_end_matches('\0') == name)
        .map(|(_, v)| v.as_slice())
}

fn field_i32(delta: &Delta, name: &str) -> Option<i32> {
    field(delta, name)
        .and_then(|v| v.get(..4))
        .map(|v| i32::from_le_bytes([v[0], v[1], v[2], v[3]]))
}

fn field_f32(delta: &Delta, name: &str) -> Option<f32> {
    field(delta, name)
        .and_then(|v| v.get(..4))
        .map(|v| f32::from_le_bytes([v[0], v[1], v[2], v[3]]))
}

/// Every engine message in the demo's network frames, in order.
pub(super) fn engine_messages(demo: &Demo) -> impl Iterator<Item = &EngineMessage> {
    demo.directory
        .entries
        .iter()
        .flat_map(|e| e.frames.iter())
        .filter_map(|f| match &f.frame_data {
            FrameData::NetworkMessage(bt) => match &bt.1.messages {
                MessageData::Parsed(m) => Some(m.iter()),
                _ => None,
            },
            _ => None,
        })
        .flatten()
        .filter_map(|m| match m {
            NetMessage::EngineMessage(em) => Some(&**em),
            _ => None,
        })
}

impl Survey {
    /// Reads the demo once: models, every entity's history, player positions.
    pub(super) fn of(demo: &Demo) -> Self {
        let mut s = Survey {
            models: HashMap::new(),
            entities: HashMap::new(),
            max_players: 32,
            player_cells: HashSet::new(),
        };
        let mut first_seen = false;
        let mut origins: HashMap<u16, [f32; 3]> = HashMap::new();
        for em in engine_messages(demo) {
            match em {
                EngineMessage::SvcServerInfo(si) => s.max_players = u16::from(si.max_players),
                EngineMessage::SvcResourceList(rl) => {
                    // Type 2 is a model.
                    for r in rl.resources.iter().filter(|r| r.type_.to_u8() == 2) {
                        let name = r.name.get_string();
                        s.models
                            .insert(r.index.to_u32(), name.trim_matches('\0').to_string());
                    }
                }
                EngineMessage::SvcSpawnBaseline(sb) => {
                    for e in &sb.entities {
                        let ent = s.entities.entry(e.entity_index).or_default();
                        ent.fields.extend(e.delta.clone());
                    }
                }
                EngineMessage::SvcPacketEntities(pe) if !first_seen => {
                    first_seen = true;
                    s.mark_offset_bases(pe);
                    for e in &pe.entity_states {
                        let ent = s.entities.entry(e.entity_index).or_default();
                        ent.fields.extend(e.delta.clone());
                        ent.in_first = true;
                    }
                }
                EngineMessage::SvcPacketEntities(pe) => {
                    s.mark_offset_bases(pe);
                    for e in &pe.entity_states {
                        s.entities.entry(e.entity_index).or_default().updates += 1;
                    }
                }
                EngineMessage::SvcDeltaPacketEntities(dpe) => {
                    for e in &dpe.entity_states {
                        let ent = s.entities.entry(e.entity_index).or_default();
                        if e.remove_entity {
                            ent.removed = true;
                            continue;
                        }
                        ent.updates += 1;
                        if let (true, Some(delta)) = (s.is_player(e.entity_index), &e.delta) {
                            let at = origins.entry(e.entity_index).or_insert([0.0; 3]);
                            if move_to(at, delta) {
                                let [x, y, z] = at.map(|v| (v / GRID_UNITS).round() as i32);
                                s.player_cells.insert((x, y, z));
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        s
    }

    fn mark_offset_bases(&mut self, pe: &SvcPacketEntities) {
        for (_, base) in super::rewrite::offset_bases(&pe.entity_states) {
            self.entities.entry(base).or_default().offset_base = true;
        }
    }

    fn is_player(&self, index: u16) -> bool {
        (1..=self.max_players).contains(&index)
    }

    fn model_of(&self, e: &Entity) -> &str {
        field_i32(&e.fields, "modelindex")
            .and_then(|mi| self.models.get(&(mi as u32)))
            .map(String::as_str)
            .unwrap_or("")
    }

    /// The tier `index` falls in, or `None` when it must stay.
    fn tier(&self, map: Option<&MapProps>, index: u16, e: &Entity) -> Option<Tier> {
        if !e.in_first
            || e.removed
            || e.updates > 0
            || e.offset_base
            || index == 0
            || self.is_player(index)
        {
            return None;
        }
        let model = self.model_of(e);
        let rendermode = field_i32(&e.fields, "rendermode").unwrap_or(0);
        let renderamt = field_i32(&e.fields, "renderamt").unwrap_or(0);
        if rendermode != 0 && renderamt <= INVISIBLE_RENDERAMT {
            Some(Tier::Invisible)
        } else if model.ends_with(".spr") {
            Some(Tier::Sprite)
        } else if brush_model(model)
            .and_then(|n| map?.class_of(n))
            .is_some_and(|c| c == "func_illusionary")
        {
            Some(Tier::Decoration)
        } else {
            None
        }
    }

    /// Where an entity sits: a brush model's centre (from the map) moved by
    /// the entity's origin, or a point entity's origin. `None` for a brush
    /// model the map doesn't have.
    fn position(&self, map: Option<&MapProps>, e: &Entity) -> Option<[f32; 3]> {
        let origin =
            [0, 1, 2].map(|a| field_f32(&e.fields, &format!("origin[{a}]")).unwrap_or(0.0));
        match brush_model(self.model_of(e)) {
            Some(n) => {
                let centre = map?.centre_of(n)?;
                Some([0, 1, 2].map(|a| centre[a] + origin[a]))
            }
            None => Some(origin),
        }
    }

    fn distance_from_players(&self, at: [f32; 3]) -> f32 {
        self.player_cells
            .iter()
            .map(|&(x, y, z)| {
                let d = [
                    x as f32 * GRID_UNITS - at[0],
                    y as f32 * GRID_UNITS - at[1],
                    z as f32 * GRID_UNITS - at[2],
                ];
                (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
            })
            .fold(f32::INFINITY, f32::min)
    }

    /// Every entity that may be dropped, in the order they go: tier by tier,
    /// farthest from the players first, then by index so the choice is the
    /// same every run.
    pub(super) fn candidates(&self, map: Option<&MapProps>) -> Vec<Dropped> {
        let mut pool: Vec<Dropped> = self
            .entities
            .iter()
            .filter_map(|(&index, e)| {
                let tier = self.tier(map, index, e)?;
                let distance_units = self
                    .position(map, e)
                    .map_or(0.0, |at| self.distance_from_players(at));
                Some(Dropped {
                    index,
                    tier,
                    model: self.model_of(e).to_string(),
                    distance_units,
                })
            })
            .collect();
        pool.sort_by(|a, b| {
            a.tier
                .cmp(&b.tier)
                .then(b.distance_units.total_cmp(&a.distance_units))
                .then(a.index.cmp(&b.index))
        });
        pool
    }
}

/// Applies a delta's origin fields to `at`; whether it held any.
fn move_to(at: &mut [f32; 3], delta: &Delta) -> bool {
    let mut moved = false;
    for (axis, slot) in at.iter_mut().enumerate() {
        if let Some(v) = field_f32(delta, &format!("origin[{axis}]")) {
            *slot = v;
            moved = true;
        }
    }
    moved
}

/// `n` for a brush model name `*n`.
fn brush_model(model: &str) -> Option<u32> {
    model.strip_prefix('*')?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entity(fields: &[(&str, i32)], in_first: bool, updates: u32) -> Entity {
        Entity {
            fields: fields
                .iter()
                .map(|(k, v)| (format!("{k}\0"), v.to_le_bytes().to_vec()))
                .collect(),
            in_first,
            updates,
            removed: false,
            offset_base: false,
        }
    }

    fn survey(entities: Vec<(u16, Entity)>, models: &[(u32, &str)]) -> Survey {
        Survey {
            models: models.iter().map(|&(i, m)| (i, m.to_string())).collect(),
            entities: entities.into_iter().collect(),
            max_players: 32,
            player_cells: HashSet::new(),
        }
    }

    #[test]
    fn an_invisible_static_entity_is_tier_invisible() {
        let s = survey(vec![], &[]);
        let e = entity(&[("rendermode", 2), ("renderamt", 0)], true, 0);
        assert_eq!(s.tier(None, 40, &e), Some(Tier::Invisible));
    }

    #[test]
    fn a_static_sprite_is_tier_sprite() {
        let s = survey(vec![], &[(3, "sprites/fog6.spr")]);
        let e = entity(&[("modelindex", 3)], true, 0);
        assert_eq!(s.tier(None, 40, &e), Some(Tier::Sprite));
    }

    #[test]
    fn a_func_illusionary_is_tier_decoration_only_with_the_map() {
        let s = survey(vec![], &[(5, "*12")]);
        let e = entity(&[("modelindex", 5)], true, 0);
        let map = MapProps::from_parts(vec![[0.0; 3]; 13], &[(12, "func_illusionary")]);
        assert_eq!(s.tier(Some(&map), 40, &e), Some(Tier::Decoration));
        assert_eq!(s.tier(None, 40, &e), None);
        let wall = MapProps::from_parts(vec![[0.0; 3]; 13], &[(12, "func_wall")]);
        assert_eq!(s.tier(Some(&wall), 40, &e), None);
    }

    #[test]
    fn players_the_world_and_anything_that_changes_stay() {
        let s = survey(vec![], &[]);
        let invisible = [("rendermode", 2), ("renderamt", 0)];
        assert_eq!(
            s.tier(None, 5, &entity(&invisible, true, 0)),
            None,
            "a player"
        );
        assert_eq!(
            s.tier(None, 0, &entity(&invisible, true, 0)),
            None,
            "the world"
        );
        assert_eq!(
            s.tier(None, 40, &entity(&invisible, true, 1)),
            None,
            "updated"
        );
        assert_eq!(
            s.tier(None, 40, &entity(&invisible, false, 0)),
            None,
            "came later"
        );
        let mut removed = entity(&invisible, true, 0);
        removed.removed = true;
        assert_eq!(s.tier(None, 40, &removed), None, "removed");
        let mut base = entity(&invisible, true, 0);
        base.offset_base = true;
        assert_eq!(s.tier(None, 40, &base), None, "another entry's baseline");
    }

    #[test]
    fn candidates_go_tier_by_tier_then_farthest_from_players_first() {
        let map = MapProps::from_parts(
            vec![[0.0; 3], [100.0, 0.0, 0.0], [5000.0, 0.0, 0.0]],
            &[(1, "func_illusionary"), (2, "func_illusionary")],
        );
        let mut s = survey(
            vec![
                (50, entity(&[("modelindex", 1)], true, 0)),
                (51, entity(&[("modelindex", 2)], true, 0)),
                (52, entity(&[("modelindex", 3)], true, 0)),
                (53, entity(&[("rendermode", 4), ("renderamt", 0)], true, 0)),
            ],
            &[(1, "*1"), (2, "*2"), (3, "sprites/glow01.spr")],
        );
        s.player_cells.insert((0, 0, 0));
        let order: Vec<(u16, Tier)> = s
            .candidates(Some(&map))
            .iter()
            .map(|d| (d.index, d.tier))
            .collect();
        assert_eq!(
            order,
            vec![
                (53, Tier::Invisible),
                (52, Tier::Sprite),
                (51, Tier::Decoration),
                (50, Tier::Decoration),
            ]
        );
    }
}
