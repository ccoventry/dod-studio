//! How many entities does a snapshot actually describe, and how does the set
//! evolve? Checking the assumption behind snapshot_inject's reconstruction.
use dem::bit::BitSliceCast;
use dem::open_demo_from_bytes;
use dem::types::{EngineMessage, FrameData, MessageData, NetMessage};
use std::collections::BTreeSet;

fn main() {
    let path = std::env::args().nth(1).expect("demo");
    let bytes = std::fs::read(&path).expect("read");
    let demo = open_demo_from_bytes(&bytes).expect("parse");
    let mut seen: BTreeSet<u16> = BTreeSet::new();
    let mut removals = 0usize;
    let mut mentions = 0usize;
    let mut per_packet: Vec<usize> = Vec::new();
    let mut declared: Vec<u32> = Vec::new();
    let mut before_join: Vec<u32> = Vec::new();
    // join time, passed in
    let join: f32 = std::env::args().nth(2).map(|s| s.parse().unwrap()).unwrap_or(f32::MAX);
    for entry in demo.directory.entries.iter().skip(1) {
        for f in &entry.frames {
            let FrameData::NetworkMessage(bt) = &f.frame_data else { continue };
            let MessageData::Parsed(msgs) = &bt.1.messages else { continue };
            for m in msgs {
                let NetMessage::EngineMessage(em) = m else { continue };
                match &**em {
                    EngineMessage::SvcPacketEntities(pe) => {
                        declared.push(pe.entity_count.to_u32());
                        if f.time <= join { before_join.push(pe.entity_count.to_u32()); }
                        per_packet.push(pe.entity_states.len());
                        for es in &pe.entity_states { seen.insert(es.entity_index); mentions += 1; }
                    }
                    EngineMessage::SvcDeltaPacketEntities(pe) => {
                        declared.push(pe.entity_count.to_u32());
                        if f.time <= join { before_join.push(pe.entity_count.to_u32()); }
                        per_packet.push(pe.entity_states.len());
                        for es in &pe.entity_states {
                            seen.insert(es.entity_index);
                            mentions += 1;
                            if es.remove_entity { removals += 1; }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    // What does the last packet before the join claim the snapshot size is?
    println!("last 6 entity_count values before the join: {:?}",
        before_join.iter().rev().take(6).collect::<Vec<_>>());
    per_packet.sort();
    declared.sort();
    println!("distinct entity indices ever mentioned: {}", seen.len());
    println!("total mentions: {mentions}, of which removals: {removals}");
    if !per_packet.is_empty() {
        println!("entities listed per packet: median {}, max {}", per_packet[per_packet.len()/2], per_packet[per_packet.len()-1]);
        println!("entity_count field:         median {}, max {}", declared[declared.len()/2], declared[declared.len()-1]);
    }
    let lo: Vec<_> = seen.iter().take(24).collect();
    println!("lowest indices: {lo:?}");
    println!("highest index: {:?}", seen.iter().last());
}
