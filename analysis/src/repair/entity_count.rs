//! Restating each packet's `entity_count` as the size of the snapshot the
//! engine really builds from it (#15). Moved from the `entity_count_fix`
//! probe.
//!
//! `hw.dll`'s `CL_ParsePacketEntities` allocates the new frame from the
//! declared count and copies every carried entity into it with no bounds
//! check (`rep movsd` of 0x154 bytes at `hw.dll+0x13453`). After a join, the
//! tail's deltas declare the size the server's own snapshot had while the
//! client carries the injected snapshot's entities, so the copy runs off the
//! end of the heap block: an access violation, or heap corruption that faults
//! a few seconds later. All three `_v10` builds died that way.

use dem::bit::BitSliceCast;
use dem::types::{Demo, EngineMessage, FrameData, MessageData, NetMessage};

use super::entity_carry::Carry;

/// One packet whose declared count disagrees with the carried set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Disagreement {
    entry: usize,
    frame: usize,
    message: usize,
    carried: usize,
}

/// What a walk over every packet-entities message found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct CountWalk {
    pub packets: usize,
    /// Packets whose declared count is not the carried set's size.
    pub disagreements: usize,
    /// Delta packets the engine throws away (`CL_FlushEntityPacket`).
    pub flushed: usize,
}

/// The carried size for one message, with its declared count, or `None` for
/// a message that is not packet entities or that the engine throws away.
fn carried_count(em: &EngineMessage, seq: i32, carry: &mut Carry) -> Option<(usize, usize)> {
    match em {
        EngineMessage::SvcPacketEntities(pe) => {
            let set = carry.full(seq, pe.entity_states.iter().map(|e| e.entity_index));
            Some((pe.entity_count.to_u32() as usize, set.len()))
        }
        EngineMessage::SvcDeltaPacketEntities(dpe) => {
            let entries: Vec<(u16, bool)> = dpe
                .entity_states
                .iter()
                .map(|e| (e.entity_index, e.remove_entity))
                .collect();
            // A packet the engine throws away keeps its count: it is never used.
            let set = carry.delta(seq, dpe.delta_sequence.to_u8(), &entries)?;
            Some((dpe.entity_count.to_u32() as usize, set.len()))
        }
        _ => None,
    }
}

/// Every packet whose count disagrees, and the walk's totals.
fn find_disagreements(demo: &Demo) -> (Vec<Disagreement>, CountWalk) {
    let mut carry = Carry::default();
    let mut found = Vec::new();
    let mut walk = CountWalk::default();
    for (entry_index, entry) in demo.directory.entries.iter().enumerate() {
        for (frame_index, frame) in entry.frames.iter().enumerate() {
            let FrameData::NetworkMessage(bt) = &frame.frame_data else {
                continue;
            };
            let seq = bt.1.sequence_info.incoming_sequence;
            let MessageData::Parsed(msgs) = &bt.1.messages else {
                continue;
            };
            for (message, m) in msgs.iter().enumerate() {
                let NetMessage::EngineMessage(em) = m else {
                    continue;
                };
                let Some((declared, carried)) = carried_count(em, seq, &mut carry) else {
                    continue;
                };
                walk.packets += 1;
                if declared != carried {
                    found.push(Disagreement {
                        entry: entry_index,
                        frame: frame_index,
                        message,
                        carried,
                    });
                }
            }
        }
    }
    walk.disagreements = found.len();
    walk.flushed = carry.flushed;
    (found, walk)
}

/// Counts the packets whose declared count disagrees with the set the engine
/// carries: the check a repaired demo must pass with none.
pub(crate) fn check_entity_counts(demo: &Demo) -> CountWalk {
    find_disagreements(demo).1
}

/// Writes the carried size into every packet whose declared count differs,
/// and returns what the walk found before the fix.
pub(crate) fn restate_entity_counts(demo: &mut Demo) -> CountWalk {
    let (found, walk) = find_disagreements(demo);
    for d in found {
        let FrameData::NetworkMessage(bt) =
            &mut demo.directory.entries[d.entry].frames[d.frame].frame_data
        else {
            continue;
        };
        let MessageData::Parsed(msgs) = bt.1.messages_mut() else {
            continue;
        };
        let count = dem::nbit_num!(d.carried as u32, 16);
        if let NetMessage::EngineMessage(em) = &mut msgs[d.message] {
            match &mut **em {
                EngineMessage::SvcPacketEntities(pe) => pe.entity_count = count,
                EngineMessage::SvcDeltaPacketEntities(dpe) => dpe.entity_count = count,
                _ => {}
            }
        }
    }
    walk
}
