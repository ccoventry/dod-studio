//! Every raw UserMessage tagged DeathMsg in a time window, printing both the
//! successfully-typed decode AND raw bytes when typed decode fails -- to find
//! whether find_crash_spot.rs is silently dropping events my simple script
//! can't decode, vs. them genuinely not being in the file (#15).
use dem::open_demo_from_bytes;
use dem::types::{FrameData, MessageData, NetMessage};
use dod::UserMessage;
use std::collections::HashMap;

fn main() {
    let mut a = std::env::args().skip(1);
    let path = a.next().expect("usage: deathmsg_diag <demo.dem> <t_lo> <t_hi>");
    let t_lo: f32 = a.next().expect("t_lo").parse().unwrap();
    let t_hi: f32 = a.next().expect("t_hi").parse().unwrap();
    let bytes = std::fs::read(&path).expect("read");
    let demo = open_demo_from_bytes(&bytes).expect("parse");

    let mut names: HashMap<u8, String> = HashMap::new();
    let mut idx = 0usize;
    for entry in demo.directory.entries.iter().skip(1) {
        for f in &entry.frames {
            let FrameData::NetworkMessage(bt) = &f.frame_data else { idx += 1; continue };
            let time = f.time;
            let MessageData::Parsed(msgs) = &bt.1.messages else { idx += 1; continue };
            for m in msgs {
                if let NetMessage::EngineMessage(em) = m {
                    if let dem::types::EngineMessage::SvcUpdateUserInfo(ui) = &**em {
                        let raw = String::from_utf8_lossy(ui.user_info.as_slice()).to_string();
                        let parts: Vec<&str> = raw.trim_matches(|c| c == '\0' || c == '\\').split('\\').collect();
                        let fmap: HashMap<&str, &str> = parts.chunks_exact(2).map(|c| (c[0], c[1])).collect();
                        if let Some(n) = fmap.get("name") { names.insert(ui.index, n.to_string()); }
                    }
                }
                if let NetMessage::UserMessage(um) = m {
                    if time < t_lo || time > t_hi { continue }
                    let name_str = String::from_utf8_lossy(&um.name).trim_matches('\0').to_string();
                    if name_str != "DeathMsg" { continue }
                    match UserMessage::new(&um.name, &um.data) {
                        Ok(UserMessage::DeathMsg(d)) => {
                            // DeathMsg's client indices are 1-based; SvcUpdateUserInfo.index is
                            // not (analysis/src/lib.rs subtracts 1 before its own lookup, but
                            // SvcUpdateUserInfo.index is used raw to populate the table) -- so a
                            // direct un-adjusted match against `names` is off by one slot.
                            let k_raw = d.killer_client_index.wrapping_sub(1);
                            let v_raw = d.victim_client_index.wrapping_sub(1);
                            let killer_fixed = names.get(&k_raw).cloned().unwrap_or(format!("#{k_raw}"));
                            let victim_fixed = names.get(&v_raw).cloned().unwrap_or(format!("#{v_raw}"));
                            let killer_unfixed = names.get(&d.killer_client_index).cloned().unwrap_or(format!("#{}", d.killer_client_index));
                            let victim_unfixed = names.get(&d.victim_client_index).cloned().unwrap_or(format!("#{}", d.victim_client_index));
                            println!("frame_idx={idx} t={time:.2}s id={} OK: [-1 fixed] {killer_fixed} killed {victim_fixed} | [unfixed] {killer_unfixed} killed {victim_unfixed} with {:?} raw={:?}",
                                um.id, d.weapon, um.data);
                        }
                        Ok(other) => {
                            println!("frame_idx={idx} t={time:.2}s id={} name matched DeathMsg but decoded as a DIFFERENT variant: {other:?} raw={:?}",
                                um.id, um.data);
                        }
                        Err(_) => {
                            println!("frame_idx={idx} t={time:.2}s id={} PARSE ERRORED raw={:?} name_bytes={:?}",
                                um.id, um.data, um.name);
                        }
                    }
                }
            }
            idx += 1;
        }
    }
}
