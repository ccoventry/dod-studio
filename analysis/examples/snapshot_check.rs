//! Does an injected snapshot hold as many entities as the stream expects? (#224)
//!
//! `snapshot_inject` rebuilds the client's entity table by replaying every
//! update before a join. Nothing so far checks the *size* of what it produces,
//! and it can be short without any of the other tests noticing: the demo still
//! parses, no packet is discarded, and the analyzer still reads it.
//!
//! The stream states the answer itself. A packet's `entity_count` is the size of
//! the snapshot that results from applying it, not the number of entities it
//! lists, so the delta packets that follow an injected snapshot say how large
//! the client's table ought to be. Compare the two.
//!
//!     cargo run --release -p analysis --example snapshot_check -- <demo.dem>

use dem::bit::BitSliceCast;
use dem::open_demo_from_bytes;
use dem::types::{EngineMessage, FrameData, MessageData, NetMessage};

fn main() {
    let path = std::env::args().nth(1).expect("usage: snapshot_check <demo.dem>");
    let bytes = std::fs::read(&path).expect("read");
    let demo = open_demo_from_bytes(&bytes).expect("parse");

    // Walk the playback entries, remembering the last few declared counts, and
    // report each full snapshot against what surrounds it.
    let mut recent: Vec<u32> = Vec::new();
    let mut pending: Option<(f32, usize, u32, Vec<u32>)> = None;
    let mut after: Vec<u32> = Vec::new();
    let mut report = |p: &(f32, usize, u32, Vec<u32>), after: &[u32]| {
        let (time, listed, declared, before) = p;
        println!("\nfull snapshot at t={time:.2}s");
        println!("  lists {listed} entities, declares entity_count {declared}");
        println!("  entity_count declared just before: {before:?}");
        println!("  entity_count declared just after:  {after:?}");
        if let Some(want) = after.first() {
            if (*want as i64 - *listed as i64).abs() > 2 {
                println!("  MISMATCH: the stream expects ~{want} entities, the snapshot supplies {listed}");
            } else {
                println!("  consistent with what the stream expects");
            }
        }
    };

    for entry in demo.directory.entries.iter().skip(1) {
        for f in &entry.frames {
            let FrameData::NetworkMessage(bt) = &f.frame_data else { continue };
            let MessageData::Parsed(msgs) = &bt.1.messages else { continue };
            for m in msgs {
                let NetMessage::EngineMessage(em) = m else { continue };
                match &**em {
                    EngineMessage::SvcPacketEntities(pe) => {
                        if let Some(p) = pending.take() { report(&p, &after) }
                        after.clear();
                        let before = recent.iter().rev().take(4).copied().collect::<Vec<_>>();
                        pending = Some((f.time, pe.entity_states.len(), pe.entity_count.to_u32(), before));
                    }
                    EngineMessage::SvcDeltaPacketEntities(pe) => {
                        let c = pe.entity_count.to_u32();
                        recent.push(c);
                        if pending.is_some() && after.len() < 4 { after.push(c) }
                        if pending.is_some() && after.len() == 4 {
                            if let Some(p) = pending.take() { report(&p, &after) }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    if let Some(p) = pending.take() { report(&p, &after) }
}
