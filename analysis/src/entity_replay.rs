//! The client's own view of the entity table, replayed from a demo's entity
//! snapshots (#448).
//!
//! `svc_deltapacketentities` does not carry the world, only what changed since
//! an earlier snapshot the client acknowledged. To know where anything *is* at
//! a given moment, every packet has to be applied against the snapshot it
//! names, the way `CL_ParsePacketEntities` does it:
//!
//! - each snapshot is stored in a ring of `CL_UPDATE_BACKUP` (64) slots,
//!   indexed by the network frame's `incoming_sequence`;
//! - a delta packet starts from the slot its `delta_sequence` names, copies
//!   every entity it does not mention, drops the ones it removes, and decodes
//!   the rest against the old state -- or against the entity's spawn baseline
//!   when the old snapshot did not have it (an enemy walking back into view);
//! - a packet whose reference is 63 or more sequences old is thrown away
//!   (`CL_FlushEntityPacket`; the exact predicate is in
//!   `examples/flush_predict.rs`).
//!
//! Applying each delta over "the latest state" instead is close, and wrong in
//! exactly the cases that matter: an entity that re-enters the snapshot would
//! keep the fields it had when it left.
//!
//! `ClientDataReplay` does the same for `svc_clientdata`, the recording
//! player's own state, which is delta-encoded against the same frames.
//!
//! Generic over what is kept per entity, so a caller pays only for the fields
//! it reads. The analyzer keeps three floats per player (`PlayerPose`); the
//! corrupt-demo and splicing tools (#15, #224, #41) can keep the whole field
//! map by using `Delta` itself.
//!
//! Pure: no I/O, no threads, so it builds for `wasm32` like the rest of the
//! crate.

use dem::bit::BitSliceCast;
use dem::types::{Delta, EngineMessage};
use std::ops::RangeInclusive;

/// `CL_UPDATE_BACKUP`: how many snapshots the client keeps.
const UPDATE_BACKUP: usize = 64;

/// A packet is discarded when its reference is at least this many sequences
/// old. `CL_UPDATE_BACKUP - 1`, read from `hw.dll`; see `flush_predict.rs`.
const FLUSH_LIMIT: i32 = 63;

/// What an entity keeps between snapshots.
pub trait EntityFields: Clone + Default {
    /// Overwrites the fields `delta` carries. A delta lists only what changed.
    fn apply(&mut self, delta: &Delta);
}

/// Every field, as decoded. The expensive choice: one map per entity per
/// snapshot. Meant for tools that rebuild or re-encode snapshots.
impl EntityFields for Delta {
    fn apply(&mut self, delta: &Delta) {
        for (k, v) in delta {
            self.insert(k.clone(), v.clone());
        }
    }
}

/// One field of a delta, by name. The decoder keeps names as they arrive off
/// the wire, NUL terminator included -- `"origin[0]\0"` -- so a plain
/// `get("origin[0]")` misses every one. Pass the terminated form; the bare
/// one is tried too.
pub fn field<'a>(delta: &'a Delta, key_with_nul: &str) -> Option<&'a [u8]> {
    delta
        .get(key_with_nul)
        .or_else(|| delta.get(key_with_nul.trim_end_matches('\0')))
        .map(Vec::as_slice)
}

/// A decoded float field: the decoder stores every float as 4 LE bytes.
pub fn field_f32(value: &[u8]) -> Option<f32> {
    value
        .get(..4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

#[derive(Clone, Debug)]
struct Snapshot<S> {
    sequence: i32,
    /// Sorted by entity index, as the engine keeps them.
    entities: Vec<(u16, S)>,
}

/// How the replay went: a demo that discards many packets would make every
/// position stale, and this is where that shows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReplayCounters {
    pub full_snapshots: usize,
    pub delta_snapshots: usize,
    /// Discarded because the reference was too old (`CL_FlushEntityPacket`).
    pub flushed: usize,
    /// Discarded because the referenced snapshot was never seen, e.g. the
    /// first packets after a splice.
    pub missing_reference: usize,
}

#[derive(Clone, Debug)]
pub struct EntityReplay<S> {
    tracked: RangeInclusive<u16>,
    frames: Vec<Option<Snapshot<S>>>,
    /// Sorted by entity index.
    baselines: Vec<(u16, S)>,
    incoming_sequence: i32,
    current: Option<usize>,
    pub counters: ReplayCounters,
}

impl<S: EntityFields> Default for EntityReplay<S> {
    /// Tracks the 32 player slots GoldSrc can have.
    fn default() -> Self {
        Self::new(1..=32)
    }
}

impl<S: EntityFields> EntityReplay<S> {
    /// Keeps only the entities in `tracked`; everything else is skipped
    /// without being decoded any further than the parser already did.
    pub fn new(tracked: RangeInclusive<u16>) -> Self {
        Self {
            tracked,
            frames: vec![None; UPDATE_BACKUP],
            baselines: Vec::new(),
            incoming_sequence: 0,
            current: None,
            counters: ReplayCounters::default(),
        }
    }

    /// Forgets everything, for a new signon: sequences restart and baselines
    /// are re-sent.
    pub fn reset(&mut self, tracked: RangeInclusive<u16>) {
        *self = Self::new(tracked);
    }

    /// Call once per network-message frame, before its messages: the
    /// snapshot a packet produces is filed under the frame's
    /// `incoming_sequence`.
    pub fn begin_frame(&mut self, incoming_sequence: i32) {
        self.incoming_sequence = incoming_sequence;
    }

    /// The entities in the latest valid snapshot, sorted by index.
    pub fn entities(&self) -> &[(u16, S)] {
        self.current
            .and_then(|slot| self.frames[slot].as_ref())
            .map(|s| s.entities.as_slice())
            .unwrap_or(&[])
    }

    /// One entity's state in the latest valid snapshot, or `None` when that
    /// snapshot does not have it (on a POV demo: out of the recording
    /// player's view).
    pub fn get(&self, index: u16) -> Option<&S> {
        let entities = self.entities();
        entities
            .binary_search_by_key(&index, |(i, _)| *i)
            .ok()
            .map(|at| &entities[at].1)
    }

    /// Applies one engine message. Everything but the baseline and the two
    /// snapshot messages is ignored.
    pub fn apply(&mut self, message: &EngineMessage) {
        match message {
            EngineMessage::SvcSpawnBaseline(sb) => {
                for es in &sb.entities {
                    if !self.tracked.contains(&es.entity_index) {
                        continue;
                    }
                    let mut state = S::default();
                    state.apply(&es.delta);
                    match self
                        .baselines
                        .binary_search_by_key(&es.entity_index, |(i, _)| *i)
                    {
                        Ok(at) => self.baselines[at].1 = state,
                        Err(at) => self.baselines.insert(at, (es.entity_index, state)),
                    }
                }
            }
            EngineMessage::SvcPacketEntities(pe) => {
                self.counters.full_snapshots += 1;
                let slot = self.slot(self.incoming_sequence);
                let mut out = self.take_buffer(slot);
                for es in &pe.entity_states {
                    if !self.tracked.contains(&es.entity_index) {
                        continue;
                    }
                    let mut state = self.baseline(es.entity_index);
                    state.apply(&es.delta);
                    out.push((es.entity_index, state));
                }
                out.sort_by_key(|(i, _)| *i);
                out.dedup_by_key(|(i, _)| *i);
                self.store(slot, out);
            }
            EngineMessage::SvcDeltaPacketEntities(pe) => {
                let delta_sequence = pe.delta_sequence.to_u32() as i32;
                if (self.incoming_sequence - delta_sequence) & 0xff >= FLUSH_LIMIT {
                    self.counters.flushed += 1;
                    self.invalidate(self.slot(self.incoming_sequence));
                    return;
                }
                let slot = self.slot(self.incoming_sequence);
                let reference = self.slot(delta_sequence);
                let has_reference = slot != reference
                    && self.frames[reference]
                        .as_ref()
                        .is_some_and(|s| s.sequence & 0xff == delta_sequence);
                if !has_reference {
                    self.counters.missing_reference += 1;
                    self.invalidate(slot);
                    return;
                }
                self.counters.delta_snapshots += 1;

                let mut out = self.take_buffer(slot);
                let old = &self.frames[reference]
                    .as_ref()
                    .expect("checked above")
                    .entities;
                let mut o = 0;
                for es in &pe.entity_states {
                    let index = es.entity_index;
                    if !self.tracked.contains(&index) {
                        continue;
                    }
                    // Everything the packet does not mention carries over.
                    while o < old.len() && old[o].0 < index {
                        out.push(old[o].clone());
                        o += 1;
                    }
                    let previous = if o < old.len() && old[o].0 == index {
                        o += 1;
                        Some(&old[o - 1].1)
                    } else {
                        None
                    };
                    if es.remove_entity {
                        continue;
                    }
                    let Some(delta) = &es.delta else { continue };
                    let mut state = match previous {
                        Some(s) => s.clone(),
                        None => baseline_of(&self.baselines, index),
                    };
                    state.apply(delta);
                    // Indices arrive ascending; a repeat replaces, never
                    // duplicates.
                    match out.last_mut() {
                        Some((last, s)) if *last == index => *s = state,
                        _ => out.push((index, state)),
                    }
                }
                out.extend(old[o..].iter().cloned());
                self.store(slot, out);
            }
            _ => {}
        }
    }

    fn slot(&self, sequence: i32) -> usize {
        (sequence as usize) & (UPDATE_BACKUP - 1)
    }

    fn baseline(&self, index: u16) -> S {
        baseline_of(&self.baselines, index)
    }

    /// Reuses the vector the slot held 64 packets ago, so a long demo does
    /// not allocate one per packet.
    fn take_buffer(&mut self, slot: usize) -> Vec<(u16, S)> {
        let mut v = self.frames[slot]
            .take()
            .map(|s| s.entities)
            .unwrap_or_default();
        v.clear();
        if self.current == Some(slot) {
            self.current = None;
        }
        v
    }

    fn store(&mut self, slot: usize, entities: Vec<(u16, S)>) {
        self.frames[slot] = Some(Snapshot {
            sequence: self.incoming_sequence,
            entities,
        });
        self.current = Some(slot);
    }

    /// A discarded packet leaves its slot unusable as a later reference.
    /// `current` stays on the last good snapshot: the client keeps drawing
    /// the world it last had, and so does this.
    fn invalidate(&mut self, slot: usize) {
        if self.current != Some(slot) {
            self.frames[slot] = None;
        }
    }
}

/// The recording player's own `clientdata_t`, replayed the same way.
///
/// A POV demo's own player is in the entity snapshots too, but without an
/// origin: the server leaves it out because the client predicts its own
/// movement. `svc_clientdata` is where that player's position lives, encoded
/// against the same acknowledged frame as the entities (its first byte is the
/// delta sequence; dem-patch calls it `delta_update_mask`), or against zero
/// when it has none.
#[derive(Clone, Debug)]
pub struct ClientDataReplay<S> {
    frames: Vec<Option<(i32, S)>>,
    incoming_sequence: i32,
    current: Option<S>,
}

impl<S: EntityFields> Default for ClientDataReplay<S> {
    fn default() -> Self {
        Self {
            frames: vec![None; UPDATE_BACKUP],
            incoming_sequence: 0,
            current: None,
        }
    }
}

impl<S: EntityFields> ClientDataReplay<S> {
    /// As `EntityReplay::begin_frame`.
    pub fn begin_frame(&mut self, incoming_sequence: i32) {
        self.incoming_sequence = incoming_sequence;
    }

    /// The latest state, `None` before the first `svc_clientdata`.
    pub fn current(&self) -> Option<&S> {
        self.current.as_ref()
    }

    pub fn apply(&mut self, message: &EngineMessage) {
        let EngineMessage::SvcClientData(cd) = message else {
            return;
        };
        let mut state = match &cd.delta_update_mask {
            Some(mask) if cd.has_delta_update_mask => {
                let delta_sequence = mask.to_u32() as i32;
                match &self.frames[(delta_sequence as usize) & (UPDATE_BACKUP - 1)] {
                    Some((sequence, s)) if sequence & 0xff == delta_sequence => s.clone(),
                    // The reference is gone; the engine would have dropped
                    // this frame too.
                    _ => return,
                }
            }
            _ => S::default(),
        };
        state.apply(&cd.client_data);
        let slot = (self.incoming_sequence as usize) & (UPDATE_BACKUP - 1);
        self.frames[slot] = Some((self.incoming_sequence, state.clone()));
        self.current = Some(state);
    }
}

fn baseline_of<S: EntityFields>(baselines: &[(u16, S)], index: u16) -> S {
    baselines
        .binary_search_by_key(&index, |(i, _)| *i)
        .ok()
        .map(|at| baselines[at].1.clone())
        .unwrap_or_default()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use dem::nbit_num;
    use dem::types::{
        EntityS, EntityState, EntityStateDelta, SvcDeltaPacketEntities, SvcPacketEntities,
        SvcSpawnBaseline,
    };

    /// A delta as the decoder produces one: float fields, NUL-terminated names.
    pub(crate) fn delta(fields: &[(&str, f32)]) -> Delta {
        fields
            .iter()
            .map(|(k, v)| (format!("{k}\0"), v.to_le_bytes().to_vec()))
            .collect()
    }

    pub(crate) fn full(entities: &[(u16, Delta)]) -> EngineMessage {
        EngineMessage::SvcPacketEntities(SvcPacketEntities {
            entity_count: nbit_num!(entities.len(), 16),
            entity_states: entities
                .iter()
                .map(|(i, d)| EntityState {
                    entity_index: *i,
                    increment_entity_number: false,
                    is_absolute_entity_index: Some(true),
                    absolute_entity_index: None,
                    entity_index_difference: None,
                    has_custom_delta: false,
                    has_baseline_index: false,
                    baseline_index: None,
                    delta: d.clone(),
                })
                .collect(),
        })
    }

    /// `None` in a change removes the entity.
    pub(crate) fn changes(delta_sequence: i32, changes: &[(u16, Option<Delta>)]) -> EngineMessage {
        EngineMessage::SvcDeltaPacketEntities(SvcDeltaPacketEntities {
            entity_count: nbit_num!(0, 16),
            delta_sequence: nbit_num!(delta_sequence & 0xff, 8),
            entity_states: changes
                .iter()
                .map(|(i, d)| EntityStateDelta {
                    entity_index: *i,
                    remove_entity: d.is_none(),
                    is_absolute_entity_index: true,
                    absolute_entity_index: None,
                    entity_index_difference: None,
                    has_custom_delta: d.as_ref().map(|_| false),
                    delta: d.clone(),
                })
                .collect(),
        })
    }

    fn x(replay: &EntityReplay<Delta>, index: u16) -> Option<f32> {
        replay
            .get(index)
            .and_then(|d| field(d, "x\0"))
            .and_then(field_f32)
    }

    fn step(replay: &mut EntityReplay<Delta>, sequence: i32, message: EngineMessage) {
        replay.begin_frame(sequence);
        replay.apply(&message);
    }

    #[test]
    fn a_delta_carries_over_what_it_does_not_mention() {
        let mut r = EntityReplay::<Delta>::new(1..=32);
        step(
            &mut r,
            1,
            full(&[(1, delta(&[("x", 1.0)])), (2, delta(&[("x", 2.0)]))]),
        );
        step(&mut r, 2, changes(1, &[(2, Some(delta(&[("x", 20.0)])))]));
        assert_eq!(x(&r, 1), Some(1.0));
        assert_eq!(x(&r, 2), Some(20.0));
        assert_eq!(r.counters.delta_snapshots, 1);
    }

    #[test]
    fn a_delta_applies_to_the_snapshot_it_names_not_the_latest() {
        // The server encoded packet 3 against packet 1, which the client had
        // acknowledged; packet 2 moved entity 1, and by packet 3 it is back
        // where packet 1 had it -- so packet 3 does not mention it. Applying
        // each delta over the latest state would leave it at 5.
        let mut r = EntityReplay::<Delta>::new(1..=32);
        step(&mut r, 1, full(&[(1, delta(&[("x", 1.0)]))]));
        step(&mut r, 2, changes(1, &[(1, Some(delta(&[("x", 5.0)])))]));
        assert_eq!(x(&r, 1), Some(5.0));
        step(&mut r, 3, changes(1, &[]));
        assert_eq!(x(&r, 1), Some(1.0));
    }

    #[test]
    fn a_removed_entity_is_gone_and_returns_from_its_baseline() {
        let mut r = EntityReplay::<Delta>::new(1..=32);
        r.apply(&EngineMessage::SvcSpawnBaseline(SvcSpawnBaseline {
            entities: vec![EntityS {
                entity_index: 3,
                index: nbit_num!(3, 11),
                type_: nbit_num!(1, 2),
                delta: delta(&[("y", 7.0)]),
            }],
            total_extra_data: nbit_num!(0, 6),
            extra_data: vec![],
        }));
        step(&mut r, 1, full(&[(3, delta(&[("x", 1.0), ("y", 100.0)]))]));
        step(&mut r, 2, changes(1, &[(3, None)]));
        assert!(r.get(3).is_none());
        // Back in view: decoded against the baseline, so `y` is the
        // baseline's 7, not the 100 it had when it left.
        step(&mut r, 3, changes(2, &[(3, Some(delta(&[("x", 9.0)])))]));
        assert_eq!(x(&r, 3), Some(9.0));
        let y = r.get(3).and_then(|d| field(d, "y\0")).and_then(field_f32);
        assert_eq!(y, Some(7.0));
    }

    #[test]
    fn a_packet_against_a_too_old_snapshot_is_discarded() {
        let mut r = EntityReplay::<Delta>::new(1..=32);
        step(&mut r, 1, full(&[(1, delta(&[("x", 1.0)]))]));
        // 63 sequences on: CL_FlushEntityPacket.
        step(&mut r, 64, changes(1, &[(1, Some(delta(&[("x", 2.0)])))]));
        assert_eq!(r.counters.flushed, 1);
        // The last good snapshot still stands.
        assert_eq!(x(&r, 1), Some(1.0));
    }

    #[test]
    fn a_packet_against_an_unseen_snapshot_is_discarded() {
        let mut r = EntityReplay::<Delta>::new(1..=32);
        step(&mut r, 10, full(&[(1, delta(&[("x", 1.0)]))]));
        step(&mut r, 12, changes(11, &[(1, Some(delta(&[("x", 2.0)])))]));
        assert_eq!(r.counters.missing_reference, 1);
        assert_eq!(x(&r, 1), Some(1.0));
    }

    #[test]
    fn untracked_entities_are_skipped() {
        let mut r = EntityReplay::<Delta>::new(1..=12);
        step(
            &mut r,
            1,
            full(&[(1, delta(&[("x", 1.0)])), (13, delta(&[("x", 13.0)]))]),
        );
        step(&mut r, 2, changes(1, &[(40, Some(delta(&[("x", 40.0)])))]));
        assert_eq!(r.entities().len(), 1);
        assert!(r.get(13).is_none());
    }

    #[test]
    fn sequence_numbers_wrap_at_one_byte() {
        let mut r = EntityReplay::<Delta>::new(1..=32);
        step(&mut r, 255, full(&[(1, delta(&[("x", 1.0)]))]));
        step(
            &mut r,
            256,
            changes(255, &[(1, Some(delta(&[("x", 2.0)])))]),
        );
        step(&mut r, 257, changes(256, &[]));
        assert_eq!(x(&r, 1), Some(2.0));
        assert_eq!(r.counters.delta_snapshots, 2);
    }
}
