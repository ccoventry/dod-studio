//! Verify the join's injected CurWeapon user message actually landed and
//! decode which weapon/ammo it names.
use dem::open_demo_from_bytes;
use dem::types::{FrameData, MessageData, NetMessage};
use dod::UserMessage;

fn main() {
    let path = std::env::args().nth(1).expect("usage: verify_curweapon <demo.dem>");
    let bytes = std::fs::read(&path).expect("read");
    let demo = open_demo_from_bytes(&bytes).expect("parse");
    let mut idx = 0usize;
    let mut last_before_join: Option<(usize, f32)> = None;
    for entry in demo.directory.entries.iter().skip(1) {
        for f in &entry.frames {
            let FrameData::NetworkMessage(bt) = &f.frame_data else { idx += 1; continue };
            let time = f.time;
            let MessageData::Parsed(msgs) = &bt.1.messages else { idx += 1; continue };
            for m in msgs {
                if let NetMessage::UserMessage(um) = m {
                    if let Ok(UserMessage::CurWeapon(cw)) = UserMessage::new(&um.name, &um.data) {
                        println!("frame_idx={idx} t={time:.2}s: CurWeapon active={} weapon={:?} clip_ammo={}",
                            cw.is_active, cw.weapon, cw.clip_ammo);
                        last_before_join = Some((idx, time));
                    }
                }
            }
            idx += 1;
        }
    }
    let _ = last_before_join;
}
