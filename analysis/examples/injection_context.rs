//! Dump the message-type sequence immediately around a demo's injected join
//! frame(s), so a live crash's tick/time can be matched against known
//! structure instead of guessed at (#224).
use dem::open_demo_from_bytes;
use dem::types::{EngineMessage, FrameData, MessageData, NetMessage};

fn main() {
    let path = std::env::args().nth(1).expect("usage: injection_context <demo.dem>");
    let bytes = std::fs::read(&path).expect("read");
    let demo = open_demo_from_bytes(&bytes).expect("parse");

    // Flatten (global frame index, time, message-type summary) for every
    // network-message frame, and find every SvcPacketEntities (our injected
    // full snapshot; the demo otherwise only has these at t=0).
    let mut rows: Vec<(usize, f32, String)> = Vec::new();
    let mut injection_points: Vec<usize> = Vec::new();
    let mut idx = 0usize;
    for entry in demo.directory.entries.iter() {
        for f in &entry.frames {
            if let FrameData::NetworkMessage(bt) = &f.frame_data {
                if let MessageData::Parsed(msgs) = &bt.1.messages {
                    let names: Vec<&str> = msgs.iter().map(|m| match m {
                        NetMessage::EngineMessage(em) => match &**em {
                            EngineMessage::SvcPacketEntities(_) => "svc_packetentities(FULL)",
                            EngineMessage::SvcDeltaPacketEntities(_) => "svc_deltapacketentities",
                            EngineMessage::SvcTime(_) => "svc_time",
                            EngineMessage::SvcClientData(_) => "svc_clientdata",
                            EngineMessage::SvcSound(_) => "svc_sound",
                            EngineMessage::SvcPrint(_) => "svc_print",
                            _ => "other",
                        },
                        NetMessage::UserMessage(_) => "usermsg",
                    }).collect();
                    if names.iter().any(|n| *n == "svc_packetentities(FULL)") {
                        injection_points.push(idx);
                    }
                    rows.push((idx, f.time, names.join(",")));
                }
            }
            idx += 1;
        }
    }

    println!("{path}");
    println!("{} network-message frames total, {} full SvcPacketEntities found",
        rows.len(), injection_points.len());
    for &at in &injection_points {
        let row = rows.iter().position(|(i, ..)| *i == at).unwrap();
        let lo = row.saturating_sub(10);
        let hi = (row + 15).min(rows.len());
        println!("\n=== injection at network-frame {at} (t={:.2}s) ===", rows[row].1);
        for r in lo..hi {
            let marker = if r == row { " <-- INJECTED" } else { "" };
            println!("  frame {:>7} t={:>8.2}s  {}{marker}", rows[r].0, rows[r].1, rows[r].2);
        }
    }
}
