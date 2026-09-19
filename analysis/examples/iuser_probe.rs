//! Where does the spectator target come from during HLTV playback?
//!
//! `HUD_ProcessPlayerState` copies the local player's `iuser1`/`iuser2` entity
//! fields into the globals the whole client reads as "spectator mode" and
//! "spectated target", every frame. If those fields are carried in the demo,
//! the recording itself is steering the camera and a DLL write would be
//! overwritten continuously.
//!
//!     cargo run --release -p analysis --example iuser_probe -- <demo>...
use dem::open_demo_from_bytes;
use dem::types::{EngineMessage, FrameData, MessageData, NetMessage};
use std::collections::BTreeMap;

fn key(k: &str) -> String { k.trim_matches(|c: char| c=='\0'||c.is_whitespace()).to_string() }
fn as_i32(v: &[u8]) -> Option<i32> { (v.len()>=4).then(|| i32::from_le_bytes([v[0],v[1],v[2],v[3]])) }

fn main() {
    for path in std::env::args().skip(1) {
        let Ok(bytes) = std::fs::read(&path) else { continue };
        let Ok(demo) = open_demo_from_bytes(&bytes) else { continue };
        // field -> entity index -> count of distinct values seen
        let mut per_entity: BTreeMap<String, BTreeMap<u16, BTreeMap<i32, usize>>> = BTreeMap::new();
        let mut cd_iuser: BTreeMap<String, BTreeMap<i32, usize>> = BTreeMap::new();

        for entry in &demo.directory.entries {
            for frame in &entry.frames {
                let FrameData::NetworkMessage(bt) = &frame.frame_data else { continue };
                let MessageData::Parsed(msgs) = &bt.1.messages else { continue };
                for m in msgs {
                    let NetMessage::EngineMessage(em) = m else { continue };
                    match &**em {
                        EngineMessage::SvcClientData(cd) => {
                            for (k,v) in cd.client_data.iter() {
                                let kk = key(k);
                                if kk.starts_with("iuser") {
                                    if let Some(i)=as_i32(v) { *cd_iuser.entry(kk).or_default().entry(i).or_insert(0)+=1; }
                                }
                            }
                        }
                        EngineMessage::SvcPacketEntities(pe) => {
                            for es in &pe.entity_states {
                                for (k,v) in es.delta.iter() {
                                    let kk=key(k);
                                    if kk.starts_with("iuser") {
                                        if let Some(i)=as_i32(v) { *per_entity.entry(kk).or_default().entry(es.entity_index).or_default().entry(i).or_insert(0)+=1; }
                                    }
                                }
                            }
                        }
                        EngineMessage::SvcDeltaPacketEntities(pe) => {
                            for es in &pe.entity_states {
                                let Some(d)=&es.delta else { continue };
                                for (k,v) in d.iter() {
                                    let kk=key(k);
                                    if kk.starts_with("iuser") {
                                        if let Some(i)=as_i32(v) { *per_entity.entry(kk).or_default().entry(es.entity_index).or_default().entry(i).or_insert(0)+=1; }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        println!("\n{path}");
        if cd_iuser.is_empty() { println!("  clientdata iuser*: absent"); }
        for (k,vals) in &cd_iuser {
            let mut v: Vec<_> = vals.iter().collect(); v.sort_by(|a,b| b.1.cmp(a.1));
            println!("  clientdata {k}: {:?}", v.iter().take(8).map(|(val,n)| format!("{val}x{n}")).collect::<Vec<_>>());
        }
        if per_entity.is_empty() { println!("  entity iuser*: ABSENT on every entity"); }
        for (k, ents) in &per_entity {
            println!("  entity {k}: on {} entities", ents.len());
            for (ent, vals) in ents.iter().take(6) {
                let mut v: Vec<_> = vals.iter().collect(); v.sort_by(|a,b| b.1.cmp(a.1));
                println!("     ent {:>3}: {:?}", ent, v.iter().take(8).map(|(val,n)| format!("{val}x{n}")).collect::<Vec<_>>());
            }
        }
    }
}
