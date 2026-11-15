//! Every svc_updateuserinfo whose name matches, with its directory entry and
//! time, plus the clears (empty userinfo) for the same slot: when a player
//! joined a recording, which a forward seek must replay (#596).
//!
//!     cargo run --release -p analysis --example userinfo_times -- demo.dem m00cat

use dem::open_demo_from_bytes;
use dem::types::{EngineMessage, FrameData, MessageData, NetMessage};
use std::collections::HashSet;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("usage: userinfo_times <demo> <name>");
    let needle = args
        .next()
        .expect("usage: userinfo_times <demo> <name>")
        .to_lowercase();
    let bytes = std::fs::read(&path).expect("read");
    let demo = open_demo_from_bytes(&bytes).expect("parse");
    let mut slots: HashSet<u8> = HashSet::new();
    let mut last_time = 0.0f32;
    for (ei, entry) in demo.directory.entries.iter().enumerate() {
        for frame in &entry.frames {
            last_time = last_time.max(frame.time);
            let FrameData::NetworkMessage(bt) = &frame.frame_data else {
                continue;
            };
            let MessageData::Parsed(msgs) = &bt.1.messages else {
                continue;
            };
            for m in msgs {
                let NetMessage::EngineMessage(em) = m else {
                    continue;
                };
                let EngineMessage::SvcUpdateUserInfo(ui) = &**em else {
                    continue;
                };
                let raw = String::from_utf8_lossy(ui.user_info.as_slice()).to_string();
                let name = raw
                    .trim_matches(|c| c == '\0' || c == '\\')
                    .split('\\')
                    .collect::<Vec<_>>()
                    .chunks(2)
                    .find(|c| c.len() == 2 && c[0] == "name")
                    .map(|c| c[1].to_string());
                match name {
                    Some(n) if n.to_lowercase().contains(&needle) => {
                        slots.insert(ui.index);
                        println!(
                            "entry {ei} t={:.1}s ({}:{:02}) slot {} name {n}",
                            frame.time,
                            (frame.time as u32) / 60,
                            (frame.time as u32) % 60,
                            ui.index
                        );
                    }
                    None if slots.contains(&ui.index) => {
                        println!("entry {ei} t={:.1}s slot {} CLEARED", frame.time, ui.index);
                    }
                    _ => {}
                }
            }
        }
    }
    println!(
        "last frame time {:.1}s ({}:{:02})",
        last_time,
        (last_time as u32) / 60,
        (last_time as u32) % 60
    );
}
