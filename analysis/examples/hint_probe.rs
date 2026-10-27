//! Where a POV demo opens DoD's auto-help hint panel (#286): prints every
//! `HudText` user message whose flag byte (after the text) is nonzero, which
//! is the only thing that opens the panel, against the viewdemo clock (the
//! last `SvcTime`). Ends with a count per demo, so a folder can be scanned
//! for a demo that shows hints.
//!
//!     cargo run --release -p analysis --example hint_probe -- [--all] demo.dem...

use dem::open_demo_from_bytes;
use dem::types::{EngineMessage, FrameData, MessageData, NetMessage};

fn clock(t: f32) -> String {
    let cs = (t.max(0.0) * 100.0) as u32;
    format!("{}:{:02}:{:02}", cs / 6000, cs / 100 % 60, cs % 100)
}

/// The text and flag byte of a `HudText` payload: a NUL-ended string, then
/// one byte. No flag byte reads as 0 (an ordinary HUD message).
fn hud_text(data: &[u8]) -> (String, u8) {
    let end = data.iter().position(|&b| b == 0).unwrap_or(data.len());
    let text = String::from_utf8_lossy(&data[..end]).to_string();
    (text, data.get(end + 1).copied().unwrap_or(0))
}

fn main() {
    // `--all` lists every HudText, flagged or not.
    let all = std::env::args().any(|a| a == "--all");
    for path in std::env::args().skip(1).filter(|a| a != "--all") {
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let Ok(demo) = open_demo_from_bytes(&bytes) else {
            println!("== {path}: does not parse");
            continue;
        };
        let mut svc_time = 0.0f32;
        let mut hints = Vec::new();
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
                            if let EngineMessage::SvcTime(t) = &**em {
                                svc_time = t.time;
                            }
                        }
                        NetMessage::UserMessage(um) => {
                            let name = String::from_utf8_lossy(&um.name);
                            if name.trim_end_matches('\0') != "HudText" {
                                continue;
                            }
                            let (text, flag) = hud_text(&um.data);
                            if flag != 0 || all {
                                hints.push(format!("  {} flag {flag} {text:?}", clock(svc_time)));
                            }
                        }
                    }
                }
            }
        }
        let what = if all { "HudText message(s)" } else { "hint(s)" };
        println!("== {path}: {} {what}", hints.len());
        for line in hints.iter().take(40) {
            println!("{line}");
        }
        if hints.len() > 40 {
            println!("  ... and {} more", hints.len() - 40);
        }
    }
}
