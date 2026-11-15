//! What the client holds at each join (#15): every entity's fields and the
//! viewing player's own client data, replayed from the recording before it.
//!
//! The tail after a hole is a run of deltas against entity state the client
//! never built, so the repair hands it a full snapshot at the join
//! (`snapshot.rs`). This builds that state: one pass over the demo, merging
//! each entity's fields into one accumulated state and taking a copy as each
//! join goes past.

use std::collections::{BTreeMap, BTreeSet};

use dem::bit::BitSliceCast;
use dem::types::{Delta, Demo, EngineMessage, FrameData, MessageData, NetMessage};

use super::joins::Join;

/// One entity's accumulated fields.
#[derive(Clone, Debug, Default)]
pub(crate) struct EntityFields {
    pub fields: Delta,
    pub custom: bool,
}

/// The viewing player's own client data, accumulated like the entities. The
/// entity snapshot rebuilds the world; this rebuilds the one thing it does not
/// cover, the player's weapon, ammo and health.
#[derive(Clone, Debug, Default)]
pub(crate) struct ClientState {
    pub fields: Delta,
    /// Weapon index to that weapon's accumulated fields (ammo, clip).
    pub weapons: BTreeMap<u32, Delta>,
    /// The last `CurWeapon` user message saying a weapon is in hand, as its
    /// id (per file, from `SvcNewUserMsg`) and 3-byte payload. Client data
    /// says what each weapon holds, not which is in hand: only this does.
    /// Without it the client keeps predicting the weapon from before the hole
    /// and plays its fire sound over the right one.
    pub cur_weapon: Option<(u8, Vec<u8>)>,
}

/// What the client holds at one join.
#[derive(Clone, Debug, Default)]
pub(crate) struct JoinState {
    /// Entities some real packet before the join listed, with their fields
    /// (entity 0 included; the snapshot leaves it out itself).
    pub entities: BTreeMap<u16, EntityFields>,
    /// Entities the map's baseline names that no real packet before the join
    /// listed, left out of `entities`.
    pub left_out: usize,
    pub client: ClientState,
}

/// The fields `SvcSpawnBaseline` gives each edict, merged into `entities`.
fn seed_baseline(
    entities: &mut BTreeMap<u16, EntityFields>,
    baseline: &dem::types::SvcSpawnBaseline,
) {
    for es in &baseline.entities {
        let e = entities.entry(es.entity_index).or_default();
        for (k, v) in es.delta.iter() {
            e.fields.insert(k.clone(), v.clone());
        }
    }
}

/// `CurWeapon` payload: `[is_active, weapon, clip_ammo]`. Only the
/// `is_active` half of a switch pair says what is in hand.
fn active_cur_weapon(m: &NetMessage) -> Option<(u8, Vec<u8>)> {
    let NetMessage::UserMessage(um) = m else {
        return None;
    };
    (um.name.starts_with(b"CurWeapon") && um.data.len() == 3 && um.data[0] != 0)
        .then(|| (um.id, um.data.clone()))
}

/// The replay's running state.
#[derive(Default)]
struct Replay {
    entities: BTreeMap<u16, EntityFields>,
    /// Entities some real packet has listed since the last signon.
    carried: BTreeSet<u16>,
    client: ClientState,
}

impl Replay {
    fn apply(&mut self, em: &EngineMessage) {
        match em {
            // A level change re-sends the signon inline, and the new map's
            // edicts replace the old map's outright. Carrying the old map's
            // entities into a later join's snapshot spammed "Tried to link
            // edict 93 without model" at `m1_h2`'s third join (#15).
            EngineMessage::SvcSpawnBaseline(sb) => {
                self.entities.clear();
                self.carried.clear();
                seed_baseline(&mut self.entities, sb);
            }
            EngineMessage::SvcClientData(cd) => {
                for (k, v) in cd.client_data.iter() {
                    self.client.fields.insert(k.clone(), v.clone());
                }
                for w in cd.weapon_data.iter().flatten() {
                    let slot = self
                        .client
                        .weapons
                        .entry(w.weapon_index.to_u32())
                        .or_default();
                    for (k, v) in w.weapon_data.iter() {
                        slot.insert(k.clone(), v.clone());
                    }
                }
            }
            EngineMessage::SvcPacketEntities(pe) => {
                for es in &pe.entity_states {
                    self.carried.insert(es.entity_index);
                    let e = self.entities.entry(es.entity_index).or_default();
                    e.custom = es.has_custom_delta;
                    for (k, v) in es.delta.iter() {
                        e.fields.insert(k.clone(), v.clone());
                    }
                }
            }
            EngineMessage::SvcDeltaPacketEntities(pe) => {
                for es in &pe.entity_states {
                    if es.remove_entity {
                        self.entities.remove(&es.entity_index);
                        continue;
                    }
                    let Some(d) = &es.delta else { continue };
                    self.carried.insert(es.entity_index);
                    let e = self.entities.entry(es.entity_index).or_default();
                    if let Some(c) = es.has_custom_delta {
                        e.custom = c;
                    }
                    for (k, v) in d.iter() {
                        e.fields.insert(k.clone(), v.clone());
                    }
                }
            }
            _ => {}
        }
    }

    /// The state to hand the client at a join. The baseline gives fields,
    /// never membership: `SvcSpawnBaseline` names every edict the map spawned,
    /// trigger brushes included, which the server never sends. A snapshot
    /// listing one has the client draw it, and `m1_h2` died 7 s after its
    /// first join on `lightmap for texture aaatrigger too large` (#15).
    fn at_join(&self) -> JoinState {
        let mut entities = self.entities.clone();
        let before = entities.len();
        entities.retain(|i, _| *i == 0 || self.carried.contains(i));
        JoinState {
            left_out: before - entities.len(),
            entities,
            client: self.client.clone(),
        }
    }
}

/// The state at each of `joins` (sorted by frame), from one replay of `demo`.
///
/// Seeded from entry 0's `SvcSpawnBaseline` first: a field set at spawn and
/// never resent (worldspawn's `modelindex`) is invisible to a replay of the
/// packets alone. The client builds each entity from exactly the fields a
/// packet lists, and a world with no `modelindex` is `Tried to link edict 0
/// without model` once a tick from the join on, then the game gone (#15).
pub(crate) fn states_at_joins(demo: &Demo, joins: &[Join]) -> Vec<JoinState> {
    let mut replay = Replay::default();
    if let Some(entry0) = demo.directory.entries.first() {
        for em in engine_messages(entry0.frames.iter()) {
            if let EngineMessage::SvcSpawnBaseline(sb) = em {
                seed_baseline(&mut replay.entities, sb);
            }
        }
    }
    let mut states = Vec::with_capacity(joins.len());
    let frames = demo
        .directory
        .entries
        .iter()
        .skip(1)
        .flat_map(|e| e.frames.iter());
    for (index, frame) in frames.enumerate() {
        while states.len() < joins.len() && index > joins[states.len()].frame_index {
            states.push(replay.at_join());
        }
        if states.len() == joins.len() {
            break;
        }
        let FrameData::NetworkMessage(bt) = &frame.frame_data else {
            continue;
        };
        let MessageData::Parsed(msgs) = &bt.1.messages else {
            continue;
        };
        for m in msgs {
            match m {
                NetMessage::EngineMessage(em) => replay.apply(em),
                other => {
                    if let Some(cur) = active_cur_weapon(other) {
                        replay.client.cur_weapon = Some(cur);
                    }
                }
            }
        }
    }
    while states.len() < joins.len() {
        states.push(replay.at_join());
    }
    states
}

/// Every engine message in `frames`, in order.
fn engine_messages<'a>(
    frames: impl Iterator<Item = &'a dem::types::Frame>,
) -> impl Iterator<Item = &'a EngineMessage> {
    frames
        .filter_map(|f| match &f.frame_data {
            FrameData::NetworkMessage(bt) => match &bt.1.messages {
                MessageData::Parsed(msgs) => Some(msgs.iter()),
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
