//! How high the camera is above a player's origin as he goes into and out
//! of a crouch, in a POV demo and in an HLTV demo (issue #1144).
//!
//! The in-eye camera of a spectator is the watched player's origin plus 22,
//! or plus 18 in the small hull (`usehull` 1), so it changes height only where
//! `usehull` and the origin change. The player's own camera follows
//! `view_ofs`, which player movement eases over the duck time. This prints
//! the raw material for laying the two side by side, as tab-separated rows:
//!
//! - `F time duck onground origin_z view_z` -- every frame's own header
//!   (the recording player's keys and camera; meaningful in a POV demo only);
//! - `E time entity origin_z usehull solid gait sequence` -- every player entity (1-32) in
//!   every snapshot, with fields carried over from the last update;
//! - `V time view_ofs2` -- `clientdata`'s `view_ofs[2]` when it changes;
//! - `N entity name` -- a player's name, from userinfo.
//!
//!     cargo run --release -p analysis --example crouch_eye_probe -- <demo> > rows.tsv
use dem::open_demo_from_bytes;
use dem::types::{Delta, EngineMessage, FrameData, MessageData, NetMessage};
use std::collections::HashMap;

fn clean(name: &str) -> &str {
    name.trim_matches(|c: char| c == '\0' || c.is_whitespace())
}

fn raw(delta: &Delta, name: &str) -> Option<u32> {
    let (_, v) = delta.iter().find(|(k, _)| clean(k) == name)?;
    Some(u32::from_le_bytes([
        *v.first()?,
        v.get(1).copied().unwrap_or(0),
        v.get(2).copied().unwrap_or(0),
        v.get(3).copied().unwrap_or(0),
    ]))
}

#[derive(Clone, Copy, Default)]
struct Player {
    origin_z: f32,
    usehull: u32,
    solid: u32,
    gait: u32,
    sequence: u32,
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: crouch_eye_probe <demo>");
    let bytes = std::fs::read(path).expect("read demo");
    let demo = open_demo_from_bytes(&bytes).expect("parse demo");
    let mut players: HashMap<u16, Player> = HashMap::new();
    let mut view_ofs: Option<f32> = None;

    let update = |players: &mut HashMap<u16, Player>, time: f32, entity: u16, d: &Delta| {
        if !(1..=32).contains(&entity) {
            return;
        }
        let p = players.entry(entity).or_default();
        if let Some(v) = raw(d, "origin[2]") {
            p.origin_z = f32::from_bits(v);
        }
        if let Some(v) = raw(d, "usehull") {
            p.usehull = v;
        }
        if let Some(v) = raw(d, "solid") {
            p.solid = v;
        }
        if let Some(v) = raw(d, "gaitsequence") {
            p.gait = v;
        }
        if let Some(v) = raw(d, "sequence") {
            p.sequence = v;
        }
        println!(
            "E\t{time:.4}\t{entity}\t{:.2}\t{}\t{}\t{}\t{}",
            p.origin_z, p.usehull, p.solid, p.gait, p.sequence
        );
    };

    for entry in &demo.directory.entries {
        for frame in &entry.frames {
            let FrameData::NetworkMessage(bt) = &frame.frame_data else {
                continue;
            };
            let time = frame.time;
            let info = &bt.1.info;
            let p = &info.refparams;
            let g = |v: &Vec<f32>, i: usize| v.get(i).copied().unwrap_or(0.0);
            println!(
                "F\t{time:.4}\t{}\t{}\t{:.2}\t{:.2}",
                u8::from(info.usercmd.buttons & 4 != 0),
                p.on_ground,
                g(&p.sim_org, 2),
                g(&p.view_origin, 2),
            );
            let MessageData::Parsed(msgs) = &bt.1.messages else {
                continue;
            };
            for m in msgs {
                let NetMessage::EngineMessage(em) = m else {
                    continue;
                };
                match &**em {
                    EngineMessage::SvcUpdateUserInfo(ui) => {
                        let text = String::from_utf8_lossy(ui.user_info.as_slice()).to_string();
                        let parts: Vec<&str> = text
                            .trim_matches(|c| c == '\0' || c == '\\')
                            .split('\\')
                            .collect();
                        for pair in parts.chunks(2) {
                            if pair.len() == 2 && pair[0] == "name" {
                                println!("N\t{}\t{}", u16::from(ui.index) + 1, pair[1]);
                            }
                        }
                    }
                    EngineMessage::SvcClientData(cd) => {
                        if let Some(v) = raw(&cd.client_data, "view_ofs[2]") {
                            let v = f32::from_bits(v);
                            if view_ofs != Some(v) {
                                view_ofs = Some(v);
                                println!("V\t{time:.4}\t{v}");
                            }
                        }
                    }
                    EngineMessage::SvcPacketEntities(pe) => {
                        for es in &pe.entity_states {
                            update(&mut players, time, es.entity_index, &es.delta);
                        }
                    }
                    EngineMessage::SvcDeltaPacketEntities(pe) => {
                        for es in &pe.entity_states {
                            if let Some(delta) = &es.delta {
                                update(&mut players, time, es.entity_index, delta);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}
