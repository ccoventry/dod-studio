//! How does a REAL svc_packetentities encode entity 0 (world)? (#224)
//!
//! `snapshot_inject` sets `is_absolute_entity_index: Some(true)` for every
//! entity in its synthesized packet, entity 0 included -- deliberate, per the
//! module doc, to avoid depending on incremental-index arithmetic. But entity 0
//! is always the FIRST entity in a real full snapshot, and a real server may
//! have a special-cased way of encoding "the first entity, index 0" that never
//! appears anywhere else in a whole recording -- untested by anything, because
//! nothing except our own injector ever needs to construct one from scratch.
//!
//! Dumps the index-encoding flags for entity 0 in every real SvcPacketEntities
//! this demo contains (only ever at t=0), so the pattern our injector should
//! match is directly visible rather than guessed at.
//!
//!     cargo run --release -p analysis --example entity0_encoding -- <healthy.dem>

use dem::open_demo_from_bytes;
use dem::types::{EngineMessage, FrameData, MessageData, NetMessage};

fn main() {
    let path = std::env::args().nth(1).expect("usage: entity0_encoding <healthy.dem>");
    let bytes = std::fs::read(&path).expect("read");
    let demo = open_demo_from_bytes(&bytes).expect("parse");

    let mut found = 0;
    for entry in demo.directory.entries.iter() {
        for f in &entry.frames {
            let FrameData::NetworkMessage(bt) = &f.frame_data else { continue };
            let MessageData::Parsed(msgs) = &bt.1.messages else { continue };
            for m in msgs {
                let NetMessage::EngineMessage(em) = m else { continue };
                let EngineMessage::SvcPacketEntities(pe) = &**em else { continue };
                found += 1;
                println!("SvcPacketEntities at t={:.2}s, {} entities listed", f.time, pe.entity_states.len());
                for (i, es) in pe.entity_states.iter().take(3).enumerate() {
                    println!(
                        "  [{i}] entity_index={} increment_entity_number={} is_absolute_entity_index={:?} \
                         has_absolute_index_bits={} entity_index_difference_present={} \
                         has_custom_delta={} has_baseline_index={} fields={}",
                        es.entity_index,
                        es.increment_entity_number,
                        es.is_absolute_entity_index,
                        es.absolute_entity_index.is_some(),
                        es.entity_index_difference.is_some(),
                        es.has_custom_delta,
                        es.has_baseline_index,
                        es.delta.iter().count(),
                    );
                }
            }
        }
    }
    if found == 0 { println!("no SvcPacketEntities found in this demo") }
}
