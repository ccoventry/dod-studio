//! Survey every entity-index encoding used across a whole demo's real packets,
//! full snapshots and deltas both (#224).
use dem::bit::BitSliceCast;
use dem::open_demo_from_bytes;
use dem::types::{EngineMessage, FrameData, MessageData, NetMessage};

fn main() {
    let path = std::env::args().nth(1).expect("demo");
    let bytes = std::fs::read(&path).expect("read");
    let demo = open_demo_from_bytes(&bytes).expect("parse");

    let mut full_total = 0u64;
    let mut full_absolute_true = 0u64;
    let mut delta_total = 0u64;
    let mut delta_absolute_true = 0u64;
    let mut max_diff_seen = 0u32;
    let mut entity0_appearances = 0u64;
    let mut min_index_seen = u16::MAX;

    for entry in demo.directory.entries.iter() {
        for f in &entry.frames {
            let FrameData::NetworkMessage(bt) = &f.frame_data else { continue };
            let MessageData::Parsed(msgs) = &bt.1.messages else { continue };
            for m in msgs {
                let NetMessage::EngineMessage(em) = m else { continue };
                match &**em {
                    EngineMessage::SvcPacketEntities(pe) => {
                        for es in &pe.entity_states {
                            full_total += 1;
                            if es.is_absolute_entity_index == Some(true) { full_absolute_true += 1 }
                            if let Some(d) = &es.entity_index_difference {
                                max_diff_seen = max_diff_seen.max(d.to_u32());
                            }
                            if es.entity_index == 0 { entity0_appearances += 1 }
                            min_index_seen = min_index_seen.min(es.entity_index);
                        }
                    }
                    EngineMessage::SvcDeltaPacketEntities(pe) => {
                        for es in &pe.entity_states {
                            delta_total += 1;
                            if es.is_absolute_entity_index { delta_absolute_true += 1 }
                            if let Some(d) = &es.entity_index_difference {
                                max_diff_seen = max_diff_seen.max(d.to_u32());
                            }
                            if es.entity_index == 0 { entity0_appearances += 1 }
                            min_index_seen = min_index_seen.min(es.entity_index);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    println!("{path}");
    println!("  full snapshots: {full_total} entities, absolute=true: {full_absolute_true}");
    println!("  deltas: {delta_total} entities, absolute=true: {delta_absolute_true}");
    println!("  max 6-bit difference value ever seen: {max_diff_seen} (cap is 63)");
    println!("  entity_index==0 appearances (whole demo): {entity0_appearances}");
    println!("  lowest entity_index ever seen (whole demo): {min_index_seen}");
}
