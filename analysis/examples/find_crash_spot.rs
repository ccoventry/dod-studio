//! Dump every DeathMsg/score event in order with its frame index and time, so
//! a live crash's console tail (an ordered sequence of kills) can be matched
//! against our own byte/frame structure exactly (#15).
use dem::open_demo_from_bytes;
use dem::types::{EngineMessage, FrameData, MessageData, NetMessage};
use dod::UserMessage;
use std::collections::HashMap;

fn main() {
    let path = std::env::args().nth(1).expect("usage: find_crash_spot <demo.dem>");
    let bytes = std::fs::read(&path).expect("read");
    let demo = open_demo_from_bytes(&bytes).expect("parse");

    let mut names: HashMap<u8, String> = HashMap::new();

    let mut idx = 0usize;
    for entry in demo.directory.entries.iter().skip(1) {
        for f in &entry.frames {
            let FrameData::NetworkMessage(bt) = &f.frame_data else { idx += 1; continue };
            let seq = bt.1.sequence_info.incoming_sequence;
            let time = f.time;
            let MessageData::Parsed(msgs) = &bt.1.messages else { idx += 1; continue };
            for m in msgs {
                match m {
                    NetMessage::EngineMessage(em) => {
                        if let EngineMessage::SvcUpdateUserInfo(ui) = &**em {
                            let raw = String::from_utf8_lossy(ui.user_info.as_slice()).to_string();
                            let parts: Vec<&str> = raw.trim_matches(|c| c == '\0' || c == '\\').split('\\').collect();
                            let fmap: HashMap<&str, &str> = parts.chunks_exact(2).map(|c| (c[0], c[1])).collect();
                            if let Some(n) = fmap.get("name") {
                                names.insert(ui.index, n.to_string());
                            }
                        }
                    }
                    NetMessage::UserMessage(um) => {
                        if let Ok(UserMessage::DeathMsg(d)) = UserMessage::new(&um.name, &um.data) {
                            // DeathMsg's client indices are 1-based (analysis/src/lib.rs
                            // subtracts 1 before its own player lookup); SvcUpdateUserInfo.index,
                            // used un-adjusted to populate `names` above, is not -- a direct
                            // match here was off by one slot for every kill.
                            let killer_idx = d.killer_client_index.wrapping_sub(1);
                            let victim_idx = d.victim_client_index.wrapping_sub(1);
                            let killer = names.get(&killer_idx).cloned().unwrap_or(format!("#{killer_idx}"));
                            let victim = names.get(&victim_idx).cloned().unwrap_or(format!("#{victim_idx}"));
                            println!("frame_idx={idx} t={time:.2}s seq={seq}: {killer} killed {victim} with {:?}", d.weapon);
                        }
                    }
                }
            }
            idx += 1;
        }
    }
}
