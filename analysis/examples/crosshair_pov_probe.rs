//! What was the recording player doing, moment by moment, as far as the
//! crosshair rule cares? (issue #310)
//!
//! `goldsrc-hooks`' spectator crosshair hides when the player's own view
//! would have hidden it. The reference for "would have" is a POV demo played
//! back and recorded to frames, where the crosshair is simply there or not.
//! This prints the other half: every change, in time order, to the state the
//! rule reads, so the two can be laid side by side.
//!
//! For the recording player's own entity it prints the replicated fields a
//! spectator also gets (`sequence`, `gaitsequence`, `movetype`,
//! `weaponmodel`), and from `clientdata`, which only the player gets, the
//! ones POV's rule really tests (`flags` for on-ground, `waterlevel`,
//! `m_iId`, `fov`, `iuser3` for prone, `health`) and the viewmodel. From each
//! frame's own header it prints `buttons` (the keys held, which is what POV's
//! sprint and crawl tests read, and which playback replays) and `onground`.
//!
//! It also prints where the camera was, for the in-eye camera's height
//! (#329): `view_ofs[2]` and `usehull` as the server sent them, and from each
//! frame `eye` (camera above the origin), `camera_z` and `origin_z`. Those
//! three change nearly every frame while the player moves; filter them out
//! when reading the rest.
//!
//! Output is tab-separated: `time  field  value`. Model fields print the
//! model's name; everything else the number.
//!
//! `<player entity>` 0 means the recording player.
//!
//!     cargo run --release -p analysis --example crosshair_pov_probe -- <pov demo> <player entity> [from] [to]

use dem::bit::BitSliceCast;
use dem::open_demo_from_bytes;
use dem::types::{Delta, EngineMessage, FrameData, MessageData, NetMessage};
use std::collections::HashMap;

/// Replicated for every player, so a spectator has them too.
const ENTITY_FIELDS: [&str; 5] = [
    "sequence",
    "gaitsequence",
    "movetype",
    "weaponmodel",
    "usehull",
];
/// Sent to the player alone.
const CLIENT_FIELDS: [&str; 8] = [
    "view_ofs[2]",
    "viewmodel",
    "m_iId",
    "flags",
    "waterlevel",
    "fov",
    "iuser3",
    "health",
];

fn clean(name: &str) -> &str {
    name.trim_matches(|c: char| c == '\0' || c.is_whitespace())
}

/// A delta field as a number, whatever width it was sent at. Floats (`fov`,
/// `health`) are sent as 4 bytes of f32.
fn field(delta: &Delta, name: &str) -> Option<f64> {
    let (_, v) = delta.iter().find(|(k, _)| clean(k) == name)?;
    let raw = u32::from_le_bytes([
        *v.first()?,
        v.get(1).copied().unwrap_or(0),
        v.get(2).copied().unwrap_or(0),
        v.get(3).copied().unwrap_or(0),
    ]);
    Some(if matches!(name, "fov" | "health" | "view_ofs[2]") {
        f64::from(f32::from_bits(raw))
    } else {
        f64::from(raw)
    })
}

fn short(model: &str) -> &str {
    let file = model.rsplit('/').next().unwrap_or(model);
    file.strip_suffix(".mdl").unwrap_or(file)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let usage = "usage: crosshair_pov_probe <pov demo> <player entity> [from] [to]";
    let path = args.get(1).expect(usage);
    // 0 picks the recording player, from the first frame's own header.
    let mut player: u16 = args.get(2).and_then(|a| a.parse().ok()).expect(usage);
    let from: f32 = args.get(3).and_then(|a| a.parse().ok()).unwrap_or(0.0);
    let to: f32 = args.get(4).and_then(|a| a.parse().ok()).unwrap_or(f32::MAX);
    let bytes = std::fs::read(path).expect("read demo");
    let demo = open_demo_from_bytes(&bytes).expect("parse demo");

    let mut models: HashMap<u32, String> = HashMap::new();
    let mut last: HashMap<&'static str, f64> = HashMap::new();
    let mut frame_last: HashMap<&'static str, f64> = HashMap::new();
    let mut note = |time: f32, name: &'static str, delta: &Delta, models: &HashMap<u32, String>| {
        let Some(value) = field(delta, name) else {
            return;
        };
        if last.insert(name, value) == Some(value) || time < from || time > to {
            return;
        }
        if matches!(name, "weaponmodel" | "viewmodel") {
            let model = models.get(&(value as u32)).map_or("?", |m| short(m));
            println!("{time:.3}\t{name}\t{model}");
        } else {
            println!("{time:.3}\t{name}\t{value}");
        }
    };

    for entry in &demo.directory.entries {
        for frame in &entry.frames {
            if let FrameData::WeaponAnimation(wa) = &frame.frame_data
                && frame.time >= from
                && frame.time <= to
            {
                println!("{:.3}\tweaponanim\t{}", frame.time, wa.anim);
            }
            let FrameData::NetworkMessage(bt) = &frame.frame_data else {
                continue;
            };
            if player == 0 {
                player = bt.1.info.refparams.player_num as u16 + 1;
                eprintln!("recording player: entity {player}");
            }
            // What the player's own client had each frame, and what playback
            // hands back to the client: the keys held and whether on the ground.
            // `eye` is where the camera really was above the player's origin
            // (to a tenth of a unit), view bob and stance changes included.
            let params = &bt.1.info.refparams;
            let eye = match (params.view_origin.get(2), params.sim_org.get(2)) {
                (Some(view), Some(origin)) => f64::from(((view - origin) * 10.0).round() / 10.0),
                _ => 0.0,
            };
            for (name, value) in [
                ("buttons", f64::from(bt.1.info.usercmd.buttons)),
                ("onground", f64::from(params.on_ground)),
                ("eye", eye),
                (
                    "camera_z",
                    f64::from(
                        (params.view_origin.get(2).copied().unwrap_or(0.0) * 10.0).round() / 10.0,
                    ),
                ),
                (
                    "origin_z",
                    f64::from(
                        (params.sim_org.get(2).copied().unwrap_or(0.0) * 10.0).round() / 10.0,
                    ),
                ),
            ] {
                if frame_last.insert(name, value) != Some(value)
                    && frame.time >= from
                    && frame.time <= to
                {
                    println!("{:.3}	{name}	{value}", frame.time);
                }
            }
            let MessageData::Parsed(msgs) = &bt.1.messages else {
                continue;
            };
            for m in msgs {
                let NetMessage::EngineMessage(em) = m else {
                    continue;
                };
                match &**em {
                    EngineMessage::SvcResourceList(rl) => {
                        for r in &rl.resources {
                            if r.type_.to_u8() == 2 {
                                models.insert(
                                    r.index.to_u32(),
                                    clean(&r.name.get_string()).to_string(),
                                );
                            }
                        }
                    }
                    EngineMessage::SvcClientData(cd) => {
                        for name in CLIENT_FIELDS {
                            note(frame.time, name, &cd.client_data, &models);
                        }
                    }
                    EngineMessage::SvcPacketEntities(pe) => {
                        for es in &pe.entity_states {
                            if es.entity_index == player {
                                for name in ENTITY_FIELDS {
                                    note(frame.time, name, &es.delta, &models);
                                }
                            }
                        }
                    }
                    EngineMessage::SvcDeltaPacketEntities(pe) => {
                        for es in &pe.entity_states {
                            if es.entity_index == player
                                && let Some(delta) = &es.delta
                            {
                                for name in ENTITY_FIELDS {
                                    note(frame.time, name, delta, &models);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}
