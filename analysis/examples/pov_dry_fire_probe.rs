//! What a POV demo carries around each of the recorder's shots, for #1029
//! (a shot that plays the empty-gun click on playback).
//!
//! Prints, in file order, for a window of the viewdemo clock (the last
//! `svc_time`):
//!
//! - the recorded usercmd's attack and reload buttons when they change;
//! - every `clientdata_t` / `weapon_data_t` field that changes;
//! - `svc_event` and `Dem_Event` fire events with their arguments;
//! - `Dem_WeaponAnim` and `Dem_Sound` frames, and the recorder's `svc_sound`.
//!
//!     cargo run --release -p analysis --example pov_dry_fire_probe -- <demo> <from> <to>

use dem::bit::BitSliceCast;
use dem::open_demo_from_bytes;
use dem::types::{Delta, EngineMessage, FrameData, MessageData, NetMessage};
use std::collections::{BTreeMap, HashMap};

fn clean(s: &str) -> String {
    s.trim_matches(|c: char| c == '\0' || c.is_whitespace())
        .to_string()
}

/// A delta field as text: floats for `m_fl*`/`fuser*`/`f*param*` and the
/// float-typed clientdata fields, integers otherwise.
fn value(name: &str, v: &[u8]) -> String {
    let float = name.starts_with("m_fl")
        || name.starts_with("fuser")
        || name.starts_with("fparam")
        || name.starts_with("m_fReloadTime")
        || name.starts_with("m_fNextAimBonus")
        || name.starts_with("m_fAimedDamage")
        || matches!(
            name,
            "origin[0]"
                | "origin[1]"
                | "origin[2]"
                | "velocity[0]"
                | "velocity[1]"
                | "velocity[2]"
                | "angles[0]"
                | "angles[1]"
                | "angles[2]"
                | "punchangle[0]"
                | "punchangle[1]"
                | "punchangle[2]"
                | "fov"
                | "maxspeed"
                | "view_ofs[2]"
                | "vuser1[0]"
                | "vuser1[1]"
                | "vuser1[2]"
                | "vuser2[0]"
                | "vuser2[1]"
                | "vuser2[2]"
                | "vuser3[0]"
                | "vuser3[1]"
                | "vuser3[2]"
                | "vuser4[0]"
                | "vuser4[1]"
                | "vuser4[2]"
                | "m_flNextAttack"
        );
    match v.len() {
        4 if float => format!("{:.3}", f32::from_le_bytes([v[0], v[1], v[2], v[3]])),
        4 => format!("{}", i32::from_le_bytes([v[0], v[1], v[2], v[3]])),
        2 => format!("{}", i16::from_le_bytes([v[0], v[1]])),
        1 => format!("{}", v[0] as i8),
        _ => format!("{v:?}"),
    }
}

fn fields(delta: &Delta) -> BTreeMap<String, &[u8]> {
    delta
        .iter()
        .map(|(k, v)| (clean(k), v.as_slice()))
        .collect()
}

/// Skips the fields that change every frame and say nothing about a shot.
fn noisy(name: &str) -> bool {
    name.starts_with("origin")
        || name.starts_with("velocity")
        || name.starts_with("punchangle")
        || name.starts_with("view_ofs")
        || name == "flTimeStepSound"
        || name == "flSwimTime"
        || name == "waterjumptime"
        || name == "flDuckTime"
        || name == "bInDuck"
        || name == "flags"
        || name == "maxspeed"
        || name == "health"
        || name == "fuser2"
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = &args[1];
    let from: f32 = args[2].parse().unwrap();
    let to: f32 = args[3].parse().unwrap();
    let bytes = std::fs::read(path).unwrap();
    let demo = open_demo_from_bytes(&bytes).unwrap();

    let mut event_names: HashMap<u32, String> = HashMap::new();
    let mut sound_names: HashMap<u32, String> = HashMap::new();
    let mut svc_time = 0f32;
    // The recorder's entity: the fourth argument, else the first frame's
    // `playernum + 1`.
    let mut local: Option<u32> = args.get(4).and_then(|a| a.parse().ok());
    let mut last_buttons = 0u16;
    let mut cd_last: HashMap<String, String> = HashMap::new();

    for entry in &demo.directory.entries {
        for frame in &entry.frames {
            let inside = svc_time >= from && svc_time <= to;
            let t = frame.time;
            let stamp = format!("{svc_time:8.3} f{t:8.3}");
            match &frame.frame_data {
                FrameData::WeaponAnimation(wa) if inside => {
                    println!("{stamp} Dem_WeaponAnim anim {} body {}", wa.anim, wa.body);
                }
                FrameData::Sound(s) if inside => {
                    println!(
                        "{stamp} Dem_Sound {} ch {} vol {:.2}",
                        clean(&String::from_utf8_lossy(&s.sample)),
                        s.channel,
                        s.volume
                    );
                }
                FrameData::Event(ev) if inside => {
                    let a = &ev.args;
                    println!(
                        "{stamp} Dem_Event {} flags {} delay {:.3} ent {} i1 {} i2 {} b1 {} b2 {} f1 {:.3} f2 {:.3}",
                        event_names
                            .get(&(ev.index as u32))
                            .cloned()
                            .unwrap_or(format!("#{}", ev.index)),
                        ev.flags,
                        ev.delay,
                        a.entity_index,
                        a.iparam1,
                        a.iparam2,
                        a.bparam1,
                        a.bparam2,
                        a.fparam1,
                        a.fparam2
                    );
                }
                FrameData::NetworkMessage(bt) => {
                    let info = &bt.1.info;
                    if local.is_none() && info.refparams.player_num >= 0 {
                        local = Some(info.refparams.player_num as u32 + 1);
                    }
                    let b = info.usercmd.buttons & (1 | 2048 | 8192);
                    if inside && b != last_buttons {
                        println!(
                            "{stamp} usercmd buttons attack {} attack2 {} reload {}",
                            b & 1 != 0,
                            b & 2048 != 0,
                            b & 8192 != 0
                        );
                    }
                    last_buttons = b;
                    let MessageData::Parsed(msgs) = &bt.1.messages else {
                        continue;
                    };
                    for m in msgs {
                        if let NetMessage::UserMessage(um) = m
                            && inside
                            && clean(&String::from_utf8_lossy(&um.name)) == "CurWeapon"
                        {
                            println!("{stamp} CurWeapon {:?}", um.data);
                        }
                        let NetMessage::EngineMessage(em) = m else {
                            continue;
                        };
                        match &**em {
                            EngineMessage::SvcTime(st) => svc_time = st.time,
                            EngineMessage::SvcResourceList(rl) => {
                                for r in &rl.resources {
                                    let name = clean(&r.name.get_string());
                                    match r.type_.to_u8() {
                                        0 => {
                                            sound_names.insert(r.index.to_u32(), name);
                                        }
                                        5 => {
                                            event_names.insert(r.index.to_u32(), name);
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            EngineMessage::SvcClientData(cd) => {
                                let mut note = |key: String, v: String| {
                                    if cd_last.get(&key) != Some(&v) {
                                        if inside {
                                            println!("{stamp} cd {key} = {v}");
                                        }
                                        cd_last.insert(key, v);
                                    }
                                };
                                for (k, v) in fields(&cd.client_data) {
                                    if !noisy(&k) {
                                        let s = value(&k, v);
                                        note(k, s);
                                    }
                                }
                                for w in cd.weapon_data.iter().flatten() {
                                    let slot = w.weapon_index.to_u32();
                                    for (k, v) in fields(&w.weapon_data) {
                                        let s = value(&k, v);
                                        note(format!("wd[{slot}].{k}"), s);
                                    }
                                }
                            }
                            EngineMessage::SvcEvent(e) if inside => {
                                for ev in &e.events {
                                    let idx = ev.event_index.to_u32();
                                    let d = ev
                                        .delta
                                        .as_ref()
                                        .map(|d| {
                                            fields(d)
                                                .into_iter()
                                                .filter(|(k, _)| {
                                                    !k.starts_with("origin")
                                                        && !k.starts_with("angles")
                                                        && !k.starts_with("velocity")
                                                })
                                                .map(|(k, v)| format!("{k}={}", value(&k, v)))
                                                .collect::<Vec<_>>()
                                                .join(" ")
                                        })
                                        .unwrap_or_default();
                                    println!(
                                        "{stamp} svc_event {} {d}",
                                        event_names.get(&idx).cloned().unwrap_or(format!("#{idx}"))
                                    );
                                }
                            }
                            EngineMessage::SvcSound(s) if inside => {
                                let ent = s.entity_index.to_u32();
                                let mark = if Some(ent) == local { "(local)" } else { "" };
                                if Some(ent) != local && ent != 0 {
                                    continue;
                                }
                                let idx =
                                    s.sound_index_long.as_ref().map(|b| b.to_u32()).or_else(|| {
                                        s.sound_index_short.as_ref().map(|b| b.to_u32())
                                    });
                                let name = idx
                                    .and_then(|i| sound_names.get(&i).cloned())
                                    .unwrap_or_else(|| format!("{idx:?}"));
                                println!("{stamp} svc_sound {mark} ent {ent} {name}");
                            }
                            EngineMessage::SvcWeaponAnim(wa) if inside => {
                                println!("{stamp} svc_weaponanim {}", wa.sequence_number);
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
    }
    eprintln!("local player entity {local:?}");
}
