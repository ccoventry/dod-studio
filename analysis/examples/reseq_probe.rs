//! Renumber a stitched demo's tail so its delta sequence continues the prefix.
//!
//! Every `svc_deltapacketentities` names the snapshot it was encoded against.
//! Across a join that numbering jumps backwards (measured on the #15 recovery:
//! 81 just before, 45 just after), so the engine resolves each tail delta
//! against a stale snapshot from the wrong part of the match, and svc_bad
//! follows within a few frames.
//!
//! This shifts every sequence number after the join by a constant so the
//! numbering runs on instead of restarting. Whether that is *sufficient* is the
//! open question -- the deltas still describe a world the client has not seen --
//! but the references at least point at snapshots it holds.
//!
//!     cargo run --release -p analysis --example reseq_probe -- <in.dem> <out.dem>
use dem::bit::BitSliceCast;
use dem::open_demo_from_bytes;
use dem::types::{EngineMessage, FrameData, MessageData, NetMessage};

fn main() {
    let mut a = std::env::args().skip(1);
    let input = a.next().expect("usage: reseq_probe <in.dem> <out.dem>");
    let output = a.next().expect("out.dem");
    let bytes = std::fs::read(&input).expect("read");
    let mut demo = open_demo_from_bytes(&bytes).expect("parse");

    // Locate the join: the largest forward jump in frame time.
    let mut join_time = 0.0f32;
    let mut best = 0.0f32;
    for entry in demo.directory.entries.iter().skip(1) {
        let mut prev: Option<f32> = None;
        for f in &entry.frames {
            if let Some(p) = prev {
                if f.time - p > best { best = f.time - p; join_time = p; }
            }
            prev = Some(f.time);
        }
    }
    println!("join after t={join_time:.2}s (jump of {best:.2}s)");

    // Last sequence before the join, first after.
    let (mut last_before, mut first_after) = (None::<u32>, None::<u32>);
    for entry in demo.directory.entries.iter().skip(1) {
        for f in &entry.frames {
            let FrameData::NetworkMessage(bt) = &f.frame_data else { continue };
            let MessageData::Parsed(msgs) = &bt.1.messages else { continue };
            for m in msgs {
                let NetMessage::EngineMessage(em) = m else { continue };
                if let EngineMessage::SvcDeltaPacketEntities(pe) = &**em {
                    let s = pe.delta_sequence.to_u32();
                    if f.time <= join_time { last_before = Some(s); }
                    else if first_after.is_none() { first_after = Some(s); }
                }
            }
        }
    }
    let (Some(lb), Some(fa)) = (last_before, first_after) else {
        println!("could not find sequences either side of the join"); return };
    let shift = (lb + 1).wrapping_sub(fa) & 0xff;
    println!("last before = {lb}, first after = {fa}; shifting the tail by +{shift} (mod 256)");

    let mut rewritten = 0usize;
    for entry in demo.directory.entries.iter_mut().skip(1) {
        for f in entry.frames.iter_mut() {
            if f.time <= join_time { continue }
            let FrameData::NetworkMessage(bt) = &mut f.frame_data else { continue };
            let MessageData::Parsed(msgs) = &mut bt.1.messages else { continue };
            for m in msgs.iter_mut() {
                let NetMessage::EngineMessage(em) = m else { continue };
                if let EngineMessage::SvcDeltaPacketEntities(pe) = &mut **em {
                    let cur = pe.delta_sequence.to_u32();
                    let new = (cur + shift) & 0xff;
                    pe.delta_sequence = dem::nbit_num!(new, 8);
                    rewritten += 1;
                }
            }
        }
    }
    println!("rewrote {rewritten} delta_sequence values");

    let out = demo.write_to_bytes();
    std::fs::write(&output, &out).expect("write");
    println!("wrote {output} ({:.1} MB)", out.len() as f64 / 1e6);
    match open_demo_from_bytes(&out) {
        Ok(d) => println!("re-parse OK: {} frames",
            d.directory.entries.iter().map(|e| e.frames.len()).sum::<usize>()),
        Err(e) => println!("re-parse failed: {e}"),
    }
    match analysis::Analysis::try_from_bytes(&out) {
        Ok(an) => println!("analysis OK: map={:?} players={}", an.state.initial_map_name, an.state.players.len()),
        Err(e) => println!("analysis failed: {e}"),
    }
}
