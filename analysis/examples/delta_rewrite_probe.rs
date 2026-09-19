//! Can entity fields in a demo be rewritten and survive re-serialisation?
//!
//! The "splice, then teleport everyone into place" idea depends on being able to
//! author entity state into the stream, not just read it. This shifts every
//! player entity's origin[0] by a fixed amount, writes the demo back out, and
//! re-reads it to see whether the change survived and the file still decodes.
//!
//!     cargo run --release -p analysis --example delta_rewrite_probe -- <in.dem> <out.dem>
use dem::open_demo_from_bytes;
use dem::types::{EngineMessage, FrameData, MessageData, NetMessage};

const SHIFT: f32 = 128.0;

fn key(k: &str) -> String { k.trim_matches(|c: char| c=='\0'||c.is_whitespace()).to_string() }

fn sample_origins(bytes: &[u8], limit: usize) -> Vec<f32> {
    let Ok(demo) = open_demo_from_bytes(bytes) else { return vec![] };
    let mut out = Vec::new();
    for entry in &demo.directory.entries {
        for frame in &entry.frames {
            let FrameData::NetworkMessage(bt) = &frame.frame_data else { continue };
            let MessageData::Parsed(msgs) = &bt.1.messages else { continue };
            for m in msgs {
                let NetMessage::EngineMessage(em) = m else { continue };
                if let EngineMessage::SvcDeltaPacketEntities(pe) = &**em {
                    for es in &pe.entity_states {
                        if !(1..=32).contains(&es.entity_index) { continue }
                        let Some(d) = &es.delta else { continue };
                        for (k, v) in d.iter() {
                            if key(k) == "origin[0]" && v.len() >= 4 {
                                out.push(f32::from_le_bytes([v[0],v[1],v[2],v[3]]));
                                if out.len() >= limit { return out }
                            }
                        }
                    }
                }
            }
        }
    }
    out
}

fn main() {
    let mut args = std::env::args().skip(1);
    let input = args.next().expect("usage: delta_rewrite_probe <in.dem> <out.dem>");
    let output = args.next().expect("out.dem");
    let bytes = std::fs::read(&input).expect("read");
    let before = sample_origins(&bytes, 8);

    let mut demo = open_demo_from_bytes(&bytes).expect("parse");
    let mut edited = 0usize;
    for entry in demo.directory.entries.iter_mut() {
        for frame in entry.frames.iter_mut() {
            let FrameData::NetworkMessage(bt) = &mut frame.frame_data else { continue };
            let MessageData::Parsed(msgs) = &mut bt.1.messages else { continue };
            for m in msgs.iter_mut() {
                let NetMessage::EngineMessage(em) = m else { continue };
                if let EngineMessage::SvcDeltaPacketEntities(pe) = &mut **em {
                    for es in pe.entity_states.iter_mut() {
                        if !(1..=32).contains(&es.entity_index) { continue }
                        let Some(d) = es.delta.as_mut() else { continue };
                        for (k, v) in d.iter_mut() {
                            if key(k) == "origin[0]" && v.len() >= 4 {
                                let cur = f32::from_le_bytes([v[0],v[1],v[2],v[3]]);
                                let new = cur + SHIFT;
                                v[..4].copy_from_slice(&new.to_le_bytes());
                                edited += 1;
                            }
                        }
                    }
                }
            }
        }
    }
    println!("rewrote {edited} origin[0] fields (+{SHIFT})");

    let out = demo.write_to_bytes();
    std::fs::write(&output, &out).expect("write");
    println!("bytes {} -> {}", bytes.len(), out.len());

    match open_demo_from_bytes(&out) {
        Ok(_) => println!("re-parse OK"),
        Err(e) => { println!("RE-PARSE FAILED: {e}"); return }
    }
    let after = sample_origins(&out, 8);
    println!("before: {:?}", before.iter().map(|v| format!("{v:.1}")).collect::<Vec<_>>());
    println!("after : {:?}", after.iter().map(|v| format!("{v:.1}")).collect::<Vec<_>>());
    let ok = before.len()==after.len() && before.iter().zip(&after).all(|(b,a)| (a-b-SHIFT).abs() < 0.75);
    println!("shift preserved: {}", if ok {"YES"} else {"NO (encoding is lossy or bit-packed against a reference)"});
    match analysis::Analysis::try_from_bytes(&out) {
        Ok(a) => println!("analysis OK: map={:?} players={}", a.state.initial_map_name, a.state.players.len()),
        Err(e) => println!("ANALYSIS FAILED: {e}"),
    }
}
