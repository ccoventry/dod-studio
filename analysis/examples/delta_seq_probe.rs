//! Do the tail's entity deltas reference snapshots the client no longer has?
//!
//! `svc_deltapacketentities` names the snapshot it is encoded against. The
//! client looks that up in its own history; if it is missing, it cannot decode
//! and bails (svc_bad). A parser does not care -- it decodes the fields
//! regardless -- which is why a stitched demo can parse cleanly and still be
//! rejected by the engine.
//!
//!     cargo run --release -p analysis --example delta_seq_probe -- <demo> [around-time]
use dem::bit::BitSliceCast;
use dem::open_demo_from_bytes;
use dem::types::{EngineMessage, FrameData, MessageData, NetMessage};

fn main() {
    let path = std::env::args().nth(1).expect("demo");
    let around: Option<f32> = std::env::args().nth(2).and_then(|s| s.parse().ok());
    let bytes = std::fs::read(&path).expect("read");
    let demo = open_demo_from_bytes(&bytes).expect("parse");

    let mut seq: Vec<(f32, u32, bool)> = Vec::new(); // time, sequence, is_full
    for entry in demo.directory.entries.iter().skip(1) {
        for frame in &entry.frames {
            let FrameData::NetworkMessage(bt) = &frame.frame_data else { continue };
            let MessageData::Parsed(msgs) = &bt.1.messages else { continue };
            for m in msgs {
                let NetMessage::EngineMessage(em) = m else { continue };
                match &**em {
                    EngineMessage::SvcPacketEntities(_) => seq.push((frame.time, 0, true)),
                    EngineMessage::SvcDeltaPacketEntities(pe) =>
                        seq.push((frame.time, pe.delta_sequence.to_u32(), false)),
                    _ => {}
                }
            }
        }
    }
    println!("{} entity-snapshot messages", seq.len());
    let fulls: Vec<_> = seq.iter().filter(|s| s.2).map(|s| s.0).collect();
    println!("full snapshots at: {:?}", fulls.iter().take(8).map(|t| format!("{t:.1}s")).collect::<Vec<_>>());

    let _ = around;
    // The biggest forward time jump is the join; show what the sequence
    // numbering does across it.
    let mut best = (0usize, 0.0f32);
    for (i, w) in seq.windows(2).enumerate() {
        let d = w[1].0 - w[0].0;
        if d > best.1 { best = (i, d); }
    }
    println!("
largest time jump: {:.2}s", best.1);
    let lo = best.0.saturating_sub(4);
    let hi = (best.0 + 6).min(seq.len());
    for i in lo..hi {
        let (t, sq, f) = seq[i];
        let mark = if i == best.0 { "   <-- last before join" }
            else if i == best.0 + 1 { "   <-- first after join" } else { "" };
        println!("   t={t:<9.2} seq={sq:<4} {}{mark}", if f { "FULL" } else { "delta" });
    }
}
