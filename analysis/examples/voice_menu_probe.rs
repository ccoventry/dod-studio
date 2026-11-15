//! Where a POV demo's voice-command and command menus come from: prints every
//! recorded console command that opens or answers a menu, and every
//! `ShowMenu` user message, against the viewdemo clock (the last `SvcTime`).
//!
//!     cargo run --release -p analysis --example voice_menu_probe -- demo.dem...

use dem::open_demo_from_bytes;
use dem::types::{EngineMessage, FrameData, MessageData, NetMessage};

fn clock(t: f32) -> String {
    let cs = (t.max(0.0) * 100.0) as u32;
    format!("{}:{:02}:{:02}", cs / 6000, cs / 100 % 60, cs % 100)
}

fn main() {
    for path in std::env::args().skip(1) {
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let Ok(demo) = open_demo_from_bytes(&bytes) else {
            println!("== {path}: does not parse");
            continue;
        };
        println!("== {path}");
        let mut svc_time = 0.0f32;
        let mut lines = 0;
        let mut last_menu = f32::NEG_INFINITY;
        for entry in &demo.directory.entries {
            for frame in &entry.frames {
                match &frame.frame_data {
                    FrameData::ConsoleCommand(c) => {
                        let text = String::from_utf8_lossy(c.command.as_slice())
                            .trim_end_matches('\0')
                            .to_string();
                        let lower = text.to_ascii_lowercase();
                        let near_menu = svc_time - last_menu < 15.0;
                        if ["voice", "menu", "signal"]
                            .iter()
                            .any(|k| lower.contains(k))
                            || (near_menu && lower.starts_with("slot"))
                        {
                            println!("  {} cmd   {text:?}", clock(svc_time));
                            lines += 1;
                        }
                    }
                    FrameData::NetworkMessage(bt) => {
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
                                    let name = String::from_utf8_lossy(&um.name)
                                        .trim_end_matches('\0')
                                        .to_string();
                                    if name == "ShowMenu" || name == "HandSignal" {
                                        last_menu = svc_time;
                                        let d = &um.data;
                                        let text =
                                            String::from_utf8_lossy(d.get(4..).unwrap_or(&[]))
                                                .trim_end_matches('\0')
                                                .replace('\n', "|");
                                        println!(
                                            "  {} {name} {:02x?} {text:?}",
                                            clock(svc_time),
                                            &d[..d.len().min(4)]
                                        );
                                        lines += 1;
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
                if lines > 80 {
                    println!("  ...");
                    break;
                }
            }
        }
    }
}
