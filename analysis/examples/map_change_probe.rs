//! R&D for #1372: what a demo holds at each map change, as `viewdemo`'s
//! loader meets it.
//!
//! `Core.dll`'s loader stops at a level change in two places: the server's
//! `reconnect` stufftext, and a `svc_serverinfo` that arrives while it is
//! running (`docs/goldsrc_viewdemo_map_change.md`). The hook's R&D setting
//! holds the loader at the first. This prints, for every `svc_serverinfo`
//! after a demo's first, whether a `reconnect` came before it and how far
//! before, so a survey can say how many map changes that covers.
//!
//!     cargo run --release -p analysis --example map_change_probe -- <demo>...
//!
//! One tab-separated row per later signon: demo, signon number, map, the
//! demo frame time and server time just before it, network frames since the
//! last `reconnect` (`-` when there was none since the previous signon),
//! messages after the `reconnect` in its own packet, and the network frames
//! the previous map held.

use dem::open_demo_from_bytes;
use dem::types::{EngineMessage, FrameData, MessageData, NetMessage};

/// The last `reconnect` stufftext seen since the previous signon.
struct Reconnect {
    /// Network frames read when it came.
    at_frame: usize,
    /// Messages after it in the same packet.
    messages_after: usize,
}

fn probe(path: &str) {
    let Ok(bytes) = std::fs::read(path) else {
        println!("{path}\tUNREADABLE");
        return;
    };
    let Ok(demo) = open_demo_from_bytes(&bytes) else {
        println!("{path}\tPARSEFAIL");
        return;
    };
    drop(bytes);
    let name = std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();

    let mut net_frames = 0usize;
    let mut signons = 0usize;
    let mut frames_at_last_signon = 0usize;
    let mut svc_time = 0.0f32;
    let mut reconnect: Option<Reconnect> = None;
    for entry in &demo.directory.entries {
        for frame in &entry.frames {
            let FrameData::NetworkMessage(boxed) = &frame.frame_data else {
                continue;
            };
            net_frames += 1;
            let MessageData::Parsed(messages) = &boxed.1.messages else {
                continue;
            };
            for (i, message) in messages.iter().enumerate() {
                let NetMessage::EngineMessage(engine) = message else {
                    continue;
                };
                match &**engine {
                    EngineMessage::SvcTime(t) => svc_time = t.time,
                    EngineMessage::SvcStuffText(text) => {
                        let command = String::from_utf8_lossy(&text.command.0);
                        if command.trim_end_matches(['\0', '\n', ' ']).trim() == "reconnect" {
                            reconnect = Some(Reconnect {
                                at_frame: net_frames,
                                messages_after: messages.len() - i - 1,
                            });
                        }
                    }
                    EngineMessage::SvcServerInfo(info) => {
                        signons += 1;
                        if signons > 1 {
                            let map = String::from_utf8_lossy(&info.map_file_name);
                            let (since, after) = match &reconnect {
                                Some(r) => (
                                    (net_frames - r.at_frame).to_string(),
                                    r.messages_after.to_string(),
                                ),
                                None => ("-".to_string(), "-".to_string()),
                            };
                            println!(
                                "{name}\t{signons}\t{}\t{:.3}\t{svc_time:.3}\t{since}\t{after}\t{}",
                                map.trim_end_matches('\0'),
                                frame.time,
                                net_frames - frames_at_last_signon
                            );
                        }
                        frames_at_last_signon = net_frames;
                        reconnect = None;
                    }
                    _ => {}
                }
            }
        }
    }
    if signons <= 1 {
        println!("{name}\t1\t(one map)");
    }
}

fn main() {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    if paths.is_empty() {
        eprintln!("usage: map_change_probe <demo>...");
        return;
    }
    for path in &paths {
        probe(path);
    }
}
