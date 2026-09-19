//! Every SvcTempEntity in a time window, tagged by variant, to find what's
//! spamming a "Sprite: no such frame"/"Bad model on beam" crash (#15).
use dem::open_demo_from_bytes;
use dem::types::{EngineMessage, FrameData, MessageData, NetMessage, TempEntity};

fn variant_name(t: &TempEntity) -> &'static str {
    match t {
        TempEntity::TeBeamPoints(_) => "TeBeamPoints",
        TempEntity::TeBeamEntPoint(_) => "TeBeamEntPoint",
        TempEntity::TeGunshot(_) => "TeGunshot",
        TempEntity::TeExplosion(_) => "TeExplosion",
        TempEntity::TeTarExplosion(_) => "TeTarExplosion",
        TempEntity::TeSmoke(_) => "TeSmoke",
        TempEntity::TeTracer(_) => "TeTracer",
        TempEntity::TeLightning(_) => "TeLightning",
        TempEntity::TeBeamEnts(_) => "TeBeamEnts",
        TempEntity::TeSparks(_) => "TeSparks",
        TempEntity::TeLavaSplash(_) => "TeLavaSplash",
        TempEntity::TeTeleport(_) => "TeTeleport",
        TempEntity::TeExplosion2(_) => "TeExplosion2",
        TempEntity::TeBspDecal(_) => "TeBspDecal",
        TempEntity::TeImplosion(_) => "TeImplosion",
        TempEntity::TeSpriteTrail(_) => "TeSpriteTrail",
        TempEntity::TeSprite(_) => "TeSprite",
        TempEntity::TeBeamSprite(_) => "TeBeamSprite",
        TempEntity::TeBeamTorus(_) => "TeBeamTorus",
        TempEntity::TeBeamDisk(_) => "TeBeamDisk",
        TempEntity::TeBeamCylinder(_) => "TeBeamCylinder",
        TempEntity::TeBeamFollow(_) => "TeBeamFollow",
        TempEntity::TeGlowSprite(_) => "TeGlowSprite",
        TempEntity::TeBeamRing(_) => "TeBeamRing",
        _ => "Other",
    }
}

fn main() {
    let mut a = std::env::args().skip(1);
    let path = a.next().expect("usage: tempentity_scan <demo.dem> <t_lo> <t_hi>");
    let t_lo: f32 = a.next().expect("t_lo").parse().unwrap();
    let t_hi: f32 = a.next().expect("t_hi").parse().unwrap();
    let bytes = std::fs::read(&path).expect("read");
    let demo = open_demo_from_bytes(&bytes).expect("parse");
    let mut idx = 0usize;
    for entry in demo.directory.entries.iter().skip(1) {
        for f in &entry.frames {
            let FrameData::NetworkMessage(bt) = &f.frame_data else { idx += 1; continue };
            let time = f.time;
            let seq = bt.1.sequence_info.incoming_sequence;
            let MessageData::Parsed(msgs) = &bt.1.messages else { idx += 1; continue };
            if time >= t_lo && time <= t_hi {
                for m in msgs {
                    if let NetMessage::EngineMessage(em) = m {
                        if let EngineMessage::SvcTempEntity(te) = &**em {
                            println!("frame_idx={idx} t={time:.2}s seq={seq}: {} raw_len_hint={:?}",
                                variant_name(&te.entity), &te.entity);
                        }
                    }
                }
            }
            idx += 1;
        }
    }
}
