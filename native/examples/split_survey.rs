//! R&D for #624: what a demo's directory entries hold at their edges, and
//! around each `SvcServerInfo` (a map change re-sends the signon).
//!
//!     cargo run --release -p native --example split_survey -- <demo>

use dem::open_demo_from_bytes;
use dem::types::{EngineMessage, FrameData, MessageData, NetMessage};

fn describe(fd: &FrameData) -> String {
    match fd {
        FrameData::NetworkMessage(b) => {
            let kind = format!("{:?}", b.0);
            let msgs = match &b.1.messages {
                MessageData::Parsed(m) => m
                    .iter()
                    .map(|n| match n {
                        NetMessage::EngineMessage(e) => {
                            let s = format!("{e:?}");
                            s.split(['(', ' ', '{']).next().unwrap_or("").to_string()
                        }
                        NetMessage::UserMessage(_) => "User".to_string(),
                    })
                    .collect::<Vec<_>>()
                    .join(","),
                _ => "raw".into(),
            };
            format!(
                "Net[{kind}] seq_in={} {msgs}",
                b.1.sequence_info.incoming_sequence
            )
        }
        other => format!("{other:?}").chars().take(60).collect(),
    }
}

fn main() {
    let path = std::env::args().nth(1).expect("demo");
    let bytes = std::fs::read(&path).unwrap();
    let demo = open_demo_from_bytes(&bytes).unwrap();
    let h = &demo.header;
    println!(
        "header map {:?} crc {:#x} proto {}",
        String::from_utf8_lossy(&h.map_name.0).trim_end_matches('\0'),
        h.map_checksum,
        h.network_protocol
    );
    for (ei, e) in demo.directory.entries.iter().enumerate() {
        println!(
            "entry {ei}: type {} desc {:?} frames {} track_time {} offset {} len {}",
            e.type_,
            String::from_utf8_lossy(&e.description.0).trim_end_matches('\0'),
            e.frames.len(),
            e.track_time,
            e.frame_offset,
            e.file_length
        );
        let n = e.frames.len();
        for (fi, f) in e.frames.iter().enumerate() {
            let has_info = matches!(&f.frame_data, FrameData::NetworkMessage(b) if matches!(&b.1.messages, MessageData::Parsed(m) if m.iter().any(|x| matches!(x, NetMessage::EngineMessage(em) if matches!(**em, EngineMessage::SvcServerInfo(_))))));
            if fi < 4 || fi + 4 >= n || has_info {
                println!(
                    "  [{ei}:{fi}] t={:.3} frame={} {}",
                    f.time,
                    f.frame,
                    describe(&f.frame_data)
                );
            }
            if has_info && ei > 0 {
                for (j, g) in e.frames.iter().enumerate().skip(fi + 1).take(70) {
                    println!(
                        "     +[{ei}:{j}] t={:.3} frame={} {}",
                        g.time,
                        g.frame,
                        describe(&g.frame_data)
                    );
                }
            }
        }
    }
}
