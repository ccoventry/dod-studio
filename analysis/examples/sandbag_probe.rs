//! What a demo carries for the non-player entities around one spot over a
//! window of the viewdemo clock: the model precache, the spawn baselines, and
//! every entity the packets hold, with model name, origin and render fields.
//! It showed that a map's `env_model`s (dod_avalanche's sandbags) are never
//! networked: neither a 2004 server's demo nor today's holds a baseline for
//! one, so their missing under `dem_forcehltv 1` was the client's drawing
//! (`goldsrc-hooks/src/env_models.rs`), not the demo. `SB_VIEW=1` also prints
//! the recorder's view origin and angles every 0.5 s, to find a moment
//! looking at a spot.
//!
//!     cargo run --release -p analysis --example sandbag_probe -- <demo> <from> <to> [<track-ent>]

use dem::bit::BitSliceCast;
use dem::open_demo_from_bytes;
use dem::types::{EngineMessage, FrameData, MessageData, NetMessage};
use std::collections::{BTreeMap, BTreeSet, HashMap};

type State = BTreeMap<String, Vec<u8>>;

fn key(k: &str) -> String {
    k.trim_matches(|c: char| c == '\0' || c.is_whitespace())
        .to_string()
}

fn is_float(k: &str) -> bool {
    [
        "origin",
        "angles",
        "scale",
        "fuser",
        "frame",
        "animtime",
        "framerate",
        "mins",
        "maxs",
        "velocity",
        "vuser",
        "startpos",
        "endpos",
        "impacttime",
        "starttime",
    ]
    .iter()
    .any(|p| k.starts_with(p))
}

fn val(k: &str, v: &[u8]) -> String {
    let mut b = [0u8; 4];
    for (i, x) in v.iter().take(4).enumerate() {
        b[i] = *x;
    }
    if is_float(k) {
        format!("{:.1}", f32::from_le_bytes(b))
    } else {
        format!("{}", i32::from_le_bytes(b))
    }
}

fn f(s: &State, k: &str) -> f32 {
    s.get(k)
        .map(|v| {
            let mut b = [0u8; 4];
            for (i, x) in v.iter().take(4).enumerate() {
                b[i] = *x;
            }
            f32::from_le_bytes(b)
        })
        .unwrap_or(0.0)
}

fn i(s: &State, k: &str) -> i32 {
    s.get(k)
        .map(|v| {
            let mut b = [0u8; 4];
            for (i, x) in v.iter().take(4).enumerate() {
                b[i] = *x;
            }
            i32::from_le_bytes(b)
        })
        .unwrap_or(0)
}

fn describe(s: &State, models: &HashMap<i32, String>) -> String {
    let mi = i(s, "modelindex");
    let mut out = format!(
        "mdl={mi}:{} org=({:.0} {:.0} {:.0})",
        models.get(&mi).map(String::as_str).unwrap_or("?"),
        f(s, "origin[0]"),
        f(s, "origin[1]"),
        f(s, "origin[2]")
    );
    for (k, v) in s {
        if k == "modelindex" || k.starts_with("origin") || k.starts_with("animtime") {
            continue;
        }
        if v.iter().any(|b| *b != 0) {
            out.push_str(&format!(" {k}={}", val(k, v)));
        }
    }
    out
}

fn overlay(s: &mut State, d: &dem::types::Delta) {
    for (k, v) in d.iter() {
        s.insert(key(k), v.to_vec());
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = &args[1];
    let from: f32 = args[2].parse().unwrap();
    let to: f32 = args[3].parse().unwrap();
    let track: u16 = args.get(4).and_then(|v| v.parse().ok()).unwrap_or(0);
    let bytes = std::fs::read(path).expect("read demo");
    let demo = open_demo_from_bytes(&bytes).expect("parse demo");
    println!(
        "header: map={} checksum={:08x} demo_proto={} net_proto={}",
        String::from_utf8_lossy(&demo.header.map_name.0).trim_matches('\0'),
        demo.header.map_checksum,
        demo.header.demo_protocol,
        demo.header.network_protocol
    );
    let mut models: HashMap<i32, String> = HashMap::new();
    let mut baselines: HashMap<u16, State> = HashMap::new();
    let mut states: HashMap<u16, State> = HashMap::new();
    let mut present: BTreeSet<u16> = BTreeSet::new();
    let mut maxc: u16 = 32;
    let mut t = 0.0f32;
    let mut window_started = false;
    let mut last_track = -1.0f32;
    let mut seen_in_window: BTreeSet<u16> = BTreeSet::new();
    let view = std::env::var("SB_VIEW").is_ok();
    let mut last_view = -1.0f32;
    for entry in &demo.directory.entries {
        for frame in &entry.frames {
            let FrameData::NetworkMessage(bt) = &frame.frame_data else {
                continue;
            };
            if view && t - last_view >= 0.5 {
                last_view = t;
                let r = &bt.1.info.refparams;
                println!(
                    "view t={t:.2} org=({:.0} {:.0} {:.0}) ang=({:.0} {:.0})",
                    r.view_origin[0],
                    r.view_origin[1],
                    r.view_origin[2],
                    r.view_angles[0],
                    r.view_angles[1]
                );
            }
            let MessageData::Parsed(msgs) = &bt.1.messages else {
                continue;
            };
            for m in msgs {
                let NetMessage::EngineMessage(em) = m else {
                    continue;
                };
                match &**em {
                    EngineMessage::SvcTime(st) => t = st.time,
                    EngineMessage::SvcServerInfo(si) => {
                        maxc = u16::from(si.max_players);
                        println!(
                            "serverinfo: proto={} checksum={:08x} max={} slot={} map={}",
                            si.protocol,
                            si.map_checksum as u32,
                            si.max_players,
                            si.player_index,
                            String::from_utf8_lossy(&si.map_file_name).trim_matches('\0')
                        );
                    }
                    EngineMessage::SvcResourceList(rl) => {
                        let mut list = Vec::new();
                        for r in &rl.resources {
                            let name = r.name.get_string().trim_matches('\0').to_string();
                            if r.type_.to_u8() == 2 {
                                models.insert(r.index.to_u32() as i32, name.clone());
                                list.push(format!("{}={}", r.index.to_u32(), name));
                            }
                        }
                        println!("model precache ({}): {}", list.len(), list.join(" "));
                    }
                    EngineMessage::SvcSpawnBaseline(sb) => {
                        for e in &sb.entities {
                            let mut s = State::new();
                            overlay(&mut s, &e.delta);
                            baselines.insert(e.entity_index, s);
                        }
                        println!("baselines: {}", sb.entities.len());
                        let mut idx: Vec<_> = baselines.keys().copied().collect();
                        idx.sort_unstable();
                        for e in idx {
                            if e > maxc {
                                println!("  base {e}: {}", describe(&baselines[&e], &models));
                            }
                        }
                    }
                    EngineMessage::SvcPacketEntities(pe) => {
                        present.clear();
                        for es in &pe.entity_states {
                            let mut s =
                                baselines.get(&es.entity_index).cloned().unwrap_or_default();
                            overlay(&mut s, &es.delta);
                            states.insert(es.entity_index, s);
                            present.insert(es.entity_index);
                        }
                    }
                    EngineMessage::SvcDeltaPacketEntities(pe) => {
                        for es in &pe.entity_states {
                            if es.remove_entity {
                                present.remove(&es.entity_index);
                                continue;
                            }
                            let s = states.entry(es.entity_index).or_insert_with(|| {
                                baselines.get(&es.entity_index).cloned().unwrap_or_default()
                            });
                            if !present.contains(&es.entity_index)
                                && let Some(b) = baselines.get(&es.entity_index)
                            {
                                *s = b.clone();
                            }
                            if let Some(d) = &es.delta {
                                overlay(s, d);
                            }
                            present.insert(es.entity_index);
                        }
                    }
                    _ => continue,
                }
                if !(from..=to).contains(&t) {
                    continue;
                }
                if !matches!(
                    &**em,
                    EngineMessage::SvcPacketEntities(_) | EngineMessage::SvcDeltaPacketEntities(_)
                ) {
                    continue;
                }
                if !window_started {
                    window_started = true;
                    println!("--- window start t={t:.2}: {} present", present.len());
                    for e in &present {
                        println!("  {e}: {}", describe(&states[e], &models));
                    }
                }
                for e in &present {
                    if *e > maxc && seen_in_window.insert(*e) {
                        println!(
                            "  t={t:.2} non-player {e}: {}",
                            describe(&states[e], &models)
                        );
                    }
                }
                if t - last_track >= 0.25 {
                    last_track = t;
                    let mut line = format!("t={t:.2} n={}", present.len());
                    for e in present.iter().filter(|e| **e >= 1 && **e <= maxc) {
                        let s = &states[e];
                        let mark = if *e == track { "*" } else { "" };
                        line.push_str(&format!(
                            " {mark}{e}:({:.0} {:.0} {:.0} m{} s{})",
                            f(s, "origin[0]"),
                            f(s, "origin[1]"),
                            f(s, "origin[2]"),
                            i(s, "modelindex"),
                            i(s, "sequence")
                        ));
                    }
                    println!("{line}");
                }
            }
        }
    }
}
