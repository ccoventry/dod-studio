//! Which overview-map commands does a POV demo replay, and when? (#1441)
//!
//! DoD's `+map` cycles `_cl_minimap` (0 -> 1 full map -> 2 minimap with
//! `cl_dmsmallmap 1` -> 0) and `+mapzoom` zooms the minimap. A POV demo records
//! the recorder's presses as type-3 `ConsoleCommand` frames, and `viewdemo`
//! runs them again in the viewer's game, so the viewer's `_cl_minimap` ends
//! wherever the recorder's presses leave it. This lists every such frame with
//! the viewdemo clock (the last `svc_time` before it) and the parity of `+map`
//! presses so far.
//!
//! Usage:
//!
//!     cargo run --release -p native --example map_command_probe -- <demo>...

use dem::open_demo_from_bytes;
use dem::types::{EngineMessage, FrameData, MessageData, NetMessage};

fn last_svc_time(frame_data: &FrameData) -> Option<f32> {
    let FrameData::NetworkMessage(boxed) = frame_data else {
        return None;
    };
    let MessageData::Parsed(messages) = &boxed.1.messages else {
        return None;
    };
    messages.iter().rev().find_map(|m| match m {
        NetMessage::EngineMessage(eng) => match eng.as_ref() {
            EngineMessage::SvcTime(t) => Some(t.time),
            _ => None,
        },
        _ => None,
    })
}

fn main() {
    for path in std::env::args().skip(1) {
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                println!("{path}: could not read: {e}");
                continue;
            }
        };
        let demo = match open_demo_from_bytes(&bytes) {
            Ok(d) => d,
            Err(e) => {
                println!("{path}: could not parse: {e}");
                continue;
            }
        };
        let name = std::path::Path::new(&path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let mut svc = 0.0f32;
        let mut presses = 0;
        let mut lines = Vec::new();
        for entry in &demo.directory.entries {
            for frame in &entry.frames {
                if let Some(t) = last_svc_time(&frame.frame_data) {
                    svc = t;
                }
                if let FrameData::ConsoleCommand(cmd) = &frame.frame_data {
                    let text = cmd.command.to_str().unwrap_or("").trim_end_matches('\0');
                    let lower = text.trim().to_ascii_lowercase();
                    if lower.contains("map") {
                        if lower.starts_with("+map") && !lower.starts_with("+mapzoom") {
                            presses += 1;
                        }
                        lines.push(format!(
                            "  {svc:9.2}  {text}  (+map presses so far {presses})"
                        ));
                    }
                }
            }
        }
        println!(
            "{name}: {} map command(s), {presses} +map press(es)",
            lines.len()
        );
        for l in lines {
            println!("{l}");
        }
    }
}
