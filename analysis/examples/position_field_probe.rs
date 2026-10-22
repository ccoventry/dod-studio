//! Which movement fields does a demo actually carry, and how often? (#448)
//!
//! Step 1 of the position-stats work: before designing any stat, list the
//! real field names `entity_state_player_t`, `clientdata_t` and
//! `weapon_data_t` decode to -- they come from the server's own
//! `svc_deltadescription`, not the SDK headers -- and how often each one
//! changes. That decides which stats are possible, and whether a position is
//! exact at a given moment or has to be interpolated between updates.
//!
//! Also reports, for player entities, how often the snapshot drops one (a
//! `remove_entity`: out of the recording player's PVS on a POV demo) and how
//! stale a player's position is at the moment of each `DeathMsg`.
//!
//!     cargo run --release -p analysis --example position_field_probe -- <demo.dem>

use dem::bit::BitSliceCast;
use dem::open_demo_from_bytes;
use dem::types::{Delta, EngineMessage, FrameData, MessageData, NetMessage};
use std::collections::{BTreeMap, HashMap};

fn trim(k: &str) -> &str {
    k.trim_end_matches('\0')
}

fn tally(counts: &mut BTreeMap<String, usize>, d: &Delta) {
    for k in d.keys() {
        *counts.entry(trim(k).to_string()).or_default() += 1;
    }
}

fn f32_of(d: &Delta, key: &str) -> Option<f32> {
    d.iter()
        .find(|(k, _)| trim(k) == key)
        .and_then(|(_, v)| v.get(..4))
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn print(title: &str, counts: &BTreeMap<String, usize>, of: usize, seconds: f32) {
    println!("\n{title}: {of} deltas");
    let mut v: Vec<_> = counts.iter().collect();
    v.sort_by(|a, b| b.1.cmp(a.1));
    for (k, n) in v {
        println!(
            "  {k:<24} {n:>9}  {:>6.1}%  {:>7.2}/s",
            *n as f64 * 100.0 / of.max(1) as f64,
            *n as f32 / seconds.max(1.0)
        );
    }
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: position_field_probe <demo.dem>");
    let bytes = std::fs::read(&path).expect("read");
    let demo = open_demo_from_bytes(&bytes).expect("parse");

    let mut player = BTreeMap::new();
    let mut player_n = 0usize;
    let mut other = BTreeMap::new();
    let mut other_n = 0usize;
    let mut client = BTreeMap::new();
    let mut client_n = 0usize;
    let mut weapon = BTreeMap::new();
    let mut weapon_n = 0usize;
    let mut full_snapshots = 0usize;
    let mut delta_packets = 0usize;
    let mut player_removals = 0usize;
    let mut per_slot_updates: BTreeMap<u16, usize> = BTreeMap::new();
    let mut last_origin_time: HashMap<u16, f32> = HashMap::new();
    let mut present: HashMap<u16, bool> = HashMap::new();
    let mut deaths = 0usize;
    let mut death_age: Vec<f32> = Vec::new();
    let mut death_absent = 0usize;
    let mut last_time = 0f32;
    let mut pov_slot: Option<u8> = None;

    for entry in &demo.directory.entries {
        for f in &entry.frames {
            last_time = last_time.max(f.time);
            let FrameData::NetworkMessage(bt) = &f.frame_data else {
                continue;
            };
            let MessageData::Parsed(msgs) = &bt.1.messages else {
                continue;
            };
            for m in msgs {
                match m {
                    NetMessage::EngineMessage(em) => match &**em {
                        EngineMessage::SvcServerInfo(si) => pov_slot = Some(si.player_index),
                        EngineMessage::SvcPacketEntities(pe) => {
                            full_snapshots += 1;
                            for es in &pe.entity_states {
                                if es.entity_index >= 1 && es.entity_index <= 32 {
                                    present.insert(es.entity_index, true);
                                }
                            }
                        }
                        EngineMessage::SvcDeltaPacketEntities(pe) => {
                            delta_packets += 1;
                            for es in &pe.entity_states {
                                let idx = es.entity_index;
                                let is_player = (1..=32).contains(&idx);
                                if es.remove_entity {
                                    if is_player {
                                        player_removals += 1;
                                        present.insert(idx, false);
                                    }
                                    continue;
                                }
                                let Some(d) = &es.delta else { continue };
                                if is_player {
                                    present.insert(idx, true);
                                    player_n += 1;
                                    *per_slot_updates.entry(idx).or_default() += 1;
                                    tally(&mut player, d);
                                    if f32_of(d, "origin[0]").is_some()
                                        || f32_of(d, "origin[1]").is_some()
                                    {
                                        last_origin_time.insert(idx, f.time);
                                    }
                                } else {
                                    other_n += 1;
                                    tally(&mut other, d);
                                }
                            }
                        }
                        EngineMessage::SvcClientData(cd) => {
                            client_n += 1;
                            tally(&mut client, &cd.client_data);
                            for w in cd.weapon_data.iter().flatten() {
                                weapon_n += 1;
                                tally(&mut weapon, &w.weapon_data);
                                let _ = w.weapon_index.to_u32();
                            }
                        }
                        _ => {}
                    },
                    NetMessage::UserMessage(um) => {
                        if um.name.starts_with(b"DeathMsg") && um.data.len() >= 2 {
                            deaths += 1;
                            let victim = um.data[1] as u16;
                            if present.get(&victim) != Some(&true) {
                                death_absent += 1;
                            } else if let Some(t) = last_origin_time.get(&victim) {
                                death_age.push(f.time - t);
                            }
                        }
                    }
                }
            }
        }
    }

    println!("{path}");
    println!(
        "{:.0}s of demo, {full_snapshots} full snapshots, {delta_packets} delta packets, POV slot {pov_slot:?}",
        last_time
    );
    print(
        "entity_state_player_t (entities 1..=32)",
        &player,
        player_n,
        last_time,
    );
    print(
        "entity_state_t / custom (entities > 32)",
        &other,
        other_n,
        last_time,
    );
    print("clientdata_t", &client, client_n, last_time);
    print("weapon_data_t", &weapon, weapon_n, last_time);
    println!("\nplayer entity removals (left the snapshot): {player_removals}");
    println!("updates per player slot: {per_slot_updates:?}");
    death_age.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let pct = |p: f32| {
        death_age
            .get(((death_age.len() as f32 - 1.0) * p) as usize)
            .copied()
            .unwrap_or(f32::NAN)
    };
    println!(
        "DeathMsg: {deaths}, victim absent from snapshot {death_absent}; seconds since victim's last origin change: p50 {:.3} p90 {:.3} max {:.3}",
        pct(0.5),
        pct(0.9),
        pct(1.0)
    );
}
