//! Drops a set of entities from every packet-entities message.
//!
//! Three things per packet: leave the dropped entities' entries out,
//! re-encode the next entry's index (indices are differences from the
//! previous listed entity), and restate `entity_count`, which is the size of
//! the snapshot the packet results in, not the number it lists. `hw.dll`'s
//! `CL_ParsePacketEntities` allocates the new frame from the declared count
//! and copies carried entities into it without a bounds check
//! (`rep movsd` at `hw.dll+0x13453`), so a count that is too small overruns
//! the heap (#15's `m1_h1` crash). The count is worked out by carrying every
//! snapshot forward the way the engine does ([`Carry`]).
//!
//! A full packet's entry can also take its baseline from an entry listed
//! earlier in the same packet, a 6-bit count of entries back (the `offset` of
//! `SV_WriteDeltaHeader`, `has_baseline_index` / `baseline_index` in `dem`;
//! the players of the `dod_lennon2` second halves use it). Leaving an entry
//! out between the two moves that base onto another entity, which the packet
//! still parses as but decodes into the wrong state, so the offset is restated
//! to name the same entity ([`restate_offsets`]). An entity another entry
//! bases on is never dropped (`select`).

use std::collections::{HashMap, HashSet};

use dem::bit::BitSliceCast;
use dem::types::{
    Demo, EngineMessage, EntityState, EntityStateDelta, FrameData, MessageData, NetMessage,
};

use super::carry::Carry;

/// A packet-entities index difference fits in 6 bits; more is written whole.
const DIFFERENCE_LIMIT: u16 = 64;
const DIFFERENCE_BITS: u32 = 6;
const ABSOLUTE_BITS: u32 = 11;
const COUNT_BITS: u32 = 16;
/// A full entry's baseline offset: how many entries back its base is listed.
const OFFSET_BITS: u32 = 6;
const OFFSET_LIMIT: usize = 1 << OFFSET_BITS;

/// What a walk over the demo saw.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct Tally {
    pub packets: usize,
    /// Packets whose declared count isn't the carried set's size.
    pub count_mismatch: usize,
    /// The largest declared count before the walk changed it.
    pub peak_before: u32,
    /// The largest declared count after.
    pub peak_after: u32,
    pub entries_dropped: usize,
    /// Entries of `drop`'s entities still listed after the walk.
    pub dropped_still_listed: usize,
    /// Every full-packet entry that takes its baseline from an earlier entry,
    /// as (entity, the entity it bases on), in demo order, as read before the
    /// walk changed anything.
    pub offset_bases: Vec<(u16, u16)>,
}

/// Each entry of a full packet that bases on an earlier entry, as (entity,
/// the entity it bases on). An offset reaching before the first entry names
/// nothing and is left out.
pub(super) fn offset_bases(entries: &[EntityState]) -> impl Iterator<Item = (u16, u16)> + '_ {
    entries.iter().enumerate().filter_map(|(at, e)| {
        if !e.has_baseline_index {
            return None;
        }
        let back = e.baseline_index.as_ref()?.to_u32() as usize;
        let base = at.checked_sub(back)?;
        Some((e.entity_index, entries[base].entity_index))
    })
}

/// Points every kept entry's baseline offset back at the entity it named
/// before entries were left out (`bases`, from [`offset_bases`]). An entry
/// whose base is gone keeps its old offset, which the read-back check
/// rejects.
fn restate_offsets(entries: &mut [EntityState], bases: &HashMap<u16, u16>) {
    let at: HashMap<u16, usize> = entries
        .iter()
        .enumerate()
        .map(|(i, e)| (e.entity_index, i))
        .collect();
    for (i, e) in entries.iter_mut().enumerate() {
        if !e.has_baseline_index {
            continue;
        }
        let back = bases
            .get(&e.entity_index)
            .and_then(|base| at.get(base))
            .and_then(|&b| i.checked_sub(b))
            .filter(|&back| (1..OFFSET_LIMIT).contains(&back));
        if let Some(back) = back {
            e.baseline_index = Some(dem::nbit_num!(back, OFFSET_BITS));
        }
    }
}

/// Index encoding for a full snapshot entry: +1 by a flag, a 6-bit
/// difference, or an 11-bit absolute index, as the engine writes them.
fn encode_full(e: &mut EntityState, prev: u16) {
    let diff = e.entity_index.wrapping_sub(prev);
    e.increment_entity_number = diff == 1;
    if diff == 1 {
        e.is_absolute_entity_index = None;
        e.absolute_entity_index = None;
        e.entity_index_difference = None;
    } else if diff < DIFFERENCE_LIMIT {
        e.is_absolute_entity_index = Some(false);
        e.absolute_entity_index = None;
        e.entity_index_difference = Some(dem::nbit_num!(diff, DIFFERENCE_BITS));
    } else {
        e.is_absolute_entity_index = Some(true);
        e.absolute_entity_index = Some(dem::nbit_num!(e.entity_index, ABSOLUTE_BITS));
        e.entity_index_difference = None;
    }
}

/// Index encoding for a delta entry: a 6-bit difference or an 11-bit absolute.
fn encode_delta(e: &mut EntityStateDelta, prev: u16) {
    let diff = e.entity_index.wrapping_sub(prev);
    if diff < DIFFERENCE_LIMIT {
        e.is_absolute_entity_index = false;
        e.absolute_entity_index = None;
        e.entity_index_difference = Some(dem::nbit_num!(diff, DIFFERENCE_BITS));
    } else {
        e.is_absolute_entity_index = true;
        e.absolute_entity_index = Some(dem::nbit_num!(e.entity_index, ABSOLUTE_BITS));
        e.entity_index_difference = None;
    }
}

fn has_packet_entities(msgs: &[NetMessage]) -> bool {
    msgs.iter().any(|m| {
        matches!(m, NetMessage::EngineMessage(em) if matches!(&**em,
            EngineMessage::SvcPacketEntities(_) | EngineMessage::SvcDeltaPacketEntities(_)))
    })
}

/// One packet's numbers.
struct Packet {
    declared: u32,
    carried: usize,
    after: u32,
    dropped: usize,
}

/// The declared count less the dropped entities in this snapshot: equal to
/// the carried set's size wherever the original agreed with it, and any
/// original disagreement left as it was.
fn count_after(declared: u32, present: usize) -> u32 {
    declared.saturating_sub(present as u32)
}

/// Rewrites one packet-entities message without `drop`'s entities.
fn trim_message(
    em: &mut EngineMessage,
    seq: i32,
    drop: &HashSet<u16>,
    carry: &mut Carry,
) -> Option<Packet> {
    match em {
        EngineMessage::SvcPacketEntities(pe) => {
            let declared = pe.entity_count.to_u32();
            let set = carry.full(seq, pe.entity_states.iter().map(|e| e.entity_index));
            let carried = set.len();
            let after = count_after(declared, set.iter().filter(|i| drop.contains(i)).count());
            let bases: HashMap<u16, u16> = offset_bases(&pe.entity_states).collect();
            let before_len = pe.entity_states.len();
            pe.entity_states.retain(|e| !drop.contains(&e.entity_index));
            let dropped = before_len - pe.entity_states.len();
            if dropped > 0 {
                let mut prev = 0u16;
                for e in &mut pe.entity_states {
                    encode_full(e, prev);
                    prev = e.entity_index;
                }
                restate_offsets(&mut pe.entity_states, &bases);
            }
            if after != declared {
                pe.entity_count = dem::nbit_num!(after, COUNT_BITS);
            }
            Some(Packet {
                declared,
                carried,
                after,
                dropped,
            })
        }
        EngineMessage::SvcDeltaPacketEntities(dpe) => {
            let declared = dpe.entity_count.to_u32();
            let from = dpe.delta_sequence.to_u8();
            let entries: Vec<(u16, bool)> = dpe
                .entity_states
                .iter()
                .map(|e| (e.entity_index, e.remove_entity))
                .collect();
            let (carried, present) = match carry.delta(seq, from, &entries) {
                Some(set) => (set.len(), set.iter().filter(|i| drop.contains(i)).count()),
                // The engine throws this packet away, so its count is never used.
                None => (declared as usize, 0),
            };
            let after = count_after(declared, present);
            let before_len = dpe.entity_states.len();
            dpe.entity_states
                .retain(|e| !drop.contains(&e.entity_index));
            let dropped = before_len - dpe.entity_states.len();
            if dropped > 0 {
                let mut prev = 0u16;
                for e in &mut dpe.entity_states {
                    encode_delta(e, prev);
                    prev = e.entity_index;
                }
            }
            if after != declared {
                dpe.entity_count = dem::nbit_num!(after, COUNT_BITS);
            }
            Some(Packet {
                declared,
                carried,
                after,
                dropped,
            })
        }
        _ => None,
    }
}

/// Counts what is listed of `drop`'s entities, changing nothing.
fn listed_of(em: &EngineMessage, drop: &HashSet<u16>) -> usize {
    match em {
        EngineMessage::SvcPacketEntities(pe) => pe
            .entity_states
            .iter()
            .filter(|e| drop.contains(&e.entity_index))
            .count(),
        EngineMessage::SvcDeltaPacketEntities(d) => d
            .entity_states
            .iter()
            .filter(|e| drop.contains(&e.entity_index))
            .count(),
        _ => 0,
    }
}

/// Measures every packet-entities message against the carried sets, and
/// counts the entries of `listed`'s entities still in them. Changes nothing.
pub(super) fn measure(demo: &mut Demo, listed: &HashSet<u16>) -> Tally {
    walk(demo, &HashSet::new(), listed)
}

/// Drops `drop`'s entities from every packet-entities message.
pub(super) fn apply(demo: &mut Demo, drop: &HashSet<u16>) -> Tally {
    walk(demo, drop, &HashSet::new())
}

fn walk(demo: &mut Demo, drop: &HashSet<u16>, listed: &HashSet<u16>) -> Tally {
    let mut tally = Tally::default();
    let mut carry = Carry::default();
    for entry in demo.directory.entries.iter_mut() {
        for frame in entry.frames.iter_mut() {
            let FrameData::NetworkMessage(bt) = &mut frame.frame_data else {
                continue;
            };
            let seq = bt.1.sequence_info.incoming_sequence;
            if !matches!(&bt.1.messages, MessageData::Parsed(m) if has_packet_entities(m)) {
                continue;
            }
            let MessageData::Parsed(msgs) = &mut bt.1.messages else {
                continue;
            };
            let mut changed = false;
            for m in msgs.iter_mut() {
                let NetMessage::EngineMessage(em) = m else {
                    continue;
                };
                let still_listed = listed_of(em, listed);
                if let EngineMessage::SvcPacketEntities(pe) = &**em {
                    tally.offset_bases.extend(offset_bases(&pe.entity_states));
                }
                if let Some(p) = trim_message(em, seq, drop, &mut carry) {
                    changed |= p.dropped > 0 || p.after != p.declared;
                    tally.packets += 1;
                    tally.count_mismatch += usize::from(p.declared as usize != p.carried);
                    tally.peak_before = tally.peak_before.max(p.declared);
                    tally.peak_after = tally.peak_after.max(p.after);
                    tally.entries_dropped += p.dropped;
                    tally.dropped_still_listed += still_listed;
                }
            }
            // The edit went through the field, so the frame's source bytes
            // still stand: forget them, or the writer ships the original
            // bytes instead of the edit. A frame left as it was keeps them
            // and is copied verbatim.
            if changed {
                bt.1.messages_mut();
            }
        }
    }
    tally
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full_entry(index: u16) -> EntityState {
        EntityState {
            entity_index: index,
            increment_entity_number: false,
            is_absolute_entity_index: None,
            absolute_entity_index: None,
            entity_index_difference: None,
            has_custom_delta: false,
            has_baseline_index: false,
            baseline_index: None,
            delta: Default::default(),
        }
    }

    fn delta_entry(index: u16) -> EntityStateDelta {
        EntityStateDelta {
            entity_index: index,
            remove_entity: false,
            is_absolute_entity_index: false,
            absolute_entity_index: None,
            entity_index_difference: None,
            has_custom_delta: Some(false),
            delta: Some(Default::default()),
        }
    }

    #[test]
    fn a_full_entry_right_after_the_last_takes_the_increment_flag() {
        let mut e = full_entry(41);
        encode_full(&mut e, 40);
        assert!(e.increment_entity_number);
        assert_eq!(e.is_absolute_entity_index, None);
    }

    #[test]
    fn a_full_entry_under_64_on_takes_a_six_bit_difference() {
        let mut e = full_entry(100);
        encode_full(&mut e, 40);
        assert!(!e.increment_entity_number);
        assert_eq!(e.is_absolute_entity_index, Some(false));
        assert_eq!(
            e.entity_index_difference.as_ref().map(|b| b.to_u32()),
            Some(60)
        );
    }

    #[test]
    fn an_entry_64_or_more_on_is_written_whole() {
        let mut full = full_entry(200);
        encode_full(&mut full, 40);
        assert_eq!(full.is_absolute_entity_index, Some(true));
        assert_eq!(
            full.absolute_entity_index.as_ref().map(|b| b.to_u32()),
            Some(200)
        );
        let mut delta = delta_entry(200);
        encode_delta(&mut delta, 40);
        assert!(delta.is_absolute_entity_index);
        assert_eq!(
            delta.absolute_entity_index.as_ref().map(|b| b.to_u32()),
            Some(200)
        );
    }

    /// A full packet listing `entries`, each (index, entries back it bases on).
    fn full_packet(entries: &[(u16, u32)]) -> EngineMessage {
        let mut prev = 0;
        let entity_states = entries
            .iter()
            .map(|&(index, back)| {
                let mut e = full_entry(index);
                encode_full(&mut e, prev);
                prev = index;
                e.has_baseline_index = back > 0;
                e.baseline_index = (back > 0).then(|| dem::nbit_num!(back, OFFSET_BITS));
                e
            })
            .collect();
        EngineMessage::SvcPacketEntities(dem::types::SvcPacketEntities {
            entity_count: dem::nbit_num!(entries.len(), COUNT_BITS),
            entity_states,
        })
    }

    fn bases_of(em: &EngineMessage) -> Vec<(u16, u16)> {
        match em {
            EngineMessage::SvcPacketEntities(pe) => offset_bases(&pe.entity_states).collect(),
            _ => unreachable!(),
        }
    }

    #[test]
    fn an_offset_across_a_dropped_entry_still_names_its_base() {
        // 50 bases on 2, three entries back; 40 goes from between them.
        let mut em = full_packet(&[(2, 0), (40, 0), (41, 0), (50, 3)]);
        assert_eq!(bases_of(&em), vec![(50, 2)]);
        trim_message(&mut em, 1, &HashSet::from([40]), &mut Carry::default());
        assert_eq!(bases_of(&em), vec![(50, 2)]);
        let EngineMessage::SvcPacketEntities(pe) = &em else {
            unreachable!()
        };
        assert_eq!(
            pe.entity_states[2]
                .baseline_index
                .as_ref()
                .map(|b| b.to_u32()),
            Some(2)
        );
    }

    #[test]
    fn an_entry_whose_base_is_dropped_no_longer_names_it() {
        // The read-back check compares these, so a dropped base is caught.
        let mut em = full_packet(&[(2, 0), (40, 0), (50, 2)]);
        trim_message(&mut em, 1, &HashSet::from([2]), &mut Carry::default());
        assert!(!bases_of(&em).contains(&(50, 2)));
    }

    #[test]
    fn the_count_loses_only_the_dropped_entities_the_snapshot_holds() {
        assert_eq!(count_after(300, 54), 246);
        assert_eq!(count_after(3, 5), 0);
    }
}
