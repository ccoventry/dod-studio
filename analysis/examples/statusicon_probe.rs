//! Finds every `StatusIcon` user message in a demo and reports the icon name
//! DoD actually sent, plus enable/disable and color -- `CHudStatusIcons`
//! (`dodstudio_hide_hudelement statusicons`) draws whatever icon name arrives
//! here. Not in the `dod` crate's typed `UserMessage` enum, so this reads the
//! wire format directly: byte enable, C-string icon name, then [r,g,b] only
//! if enabling (confirmed against the real client.dll's
//! `CHudStatusIcons::EnableIcon`/`MsgFunc_StatusIcon`, not just source).
//!
//!     cargo run --release -p analysis --example statusicon_probe -- path/to/demo.dem

use dem::open_demo_from_bytes;
use dem::types::{EngineMessage, FrameData, MessageData, NetMessage};

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: statusicon_probe <demo>");
    let bytes = std::fs::read(&path).expect("read demo");
    let demo = open_demo_from_bytes(&bytes).expect("parse demo");

    let mut viewdemo_offset = 0.0f32;
    let mut hits: Vec<(f32, bool, String, Option<(u8, u8, u8)>)> = Vec::new();

    for entry in &demo.directory.entries {
        for frame in &entry.frames {
            let FrameData::NetworkMessage(bt) = &frame.frame_data else {
                continue;
            };
            let MessageData::Parsed(msgs) = &bt.1.messages else {
                continue;
            };
            for m in msgs {
                match m {
                    NetMessage::EngineMessage(em) => {
                        if let EngineMessage::SvcTime(t) = em.as_ref() {
                            viewdemo_offset = t.time;
                        }
                    }
                    NetMessage::UserMessage(um) => {
                        let mut n: Vec<u8> = um.name.clone();
                        while n.last() == Some(&0) {
                            n.pop();
                        }
                        if n != b"StatusIcon" {
                            continue;
                        }
                        let data = &um.data;
                        let Some(&enable) = data.first() else {
                            continue;
                        };
                        let enable = enable != 0;
                        let Some(nul) = data[1..].iter().position(|&b| b == 0) else {
                            continue;
                        };
                        let name = String::from_utf8_lossy(&data[1..1 + nul]).to_string();
                        let rgb = if enable {
                            let rest = &data[1 + nul + 1..];
                            (rest.len() >= 3).then(|| (rest[0], rest[1], rest[2]))
                        } else {
                            None
                        };
                        hits.push((viewdemo_offset, enable, name, rgb));
                    }
                }
            }
        }
    }

    println!("== {} ==", path);
    println!("{} StatusIcon message(s)\n", hits.len());
    if hits.is_empty() {
        println!("Never fires in this demo -- pick a different one.");
        return;
    }
    println!(
        "{:>12}  {:<8} {:<20} rgb",
        "viewdemo (s)", "action", "icon name"
    );
    for (offset, enable, name, rgb) in &hits {
        let action = if *enable { "enable" } else { "disable" };
        let rgb_s = rgb
            .map(|(r, g, b)| format!("({r},{g},{b})"))
            .unwrap_or_default();
        println!("{:>12.2}  {:<8} {:<20} {}", offset, action, name, rgb_s);
    }
}
