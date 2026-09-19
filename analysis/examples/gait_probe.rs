//! Which movement states are replicated for *other* players, and can sprint be
//! told apart in an HLTV recording?
//!
//! Asked while working out whether the crosshair can be hidden at the right
//! moments during HLTV first-person spectating (#219). The local client knows
//! it is sprinting from `clientdata.maxspeed` -- DoD sets a discrete speed per
//! movement state rather than a continuous one -- but `maxspeed` lives in
//! `svc_clientdata`, which only ever describes the recording client. The
//! question was whether a spectated player's sprint could be recovered instead,
//! either from their speed or from something else replicated.
//!
//! The answer is `gaitsequence`, which **is** replicated per player and is an
//! exact state, not an inference. Player models share one sequence table
//! (verified: `us-inf`, `us-para`, `axis-inf`, `axis-para`, `brit-inf` all carry
//! the same 345 sequences in the same order), so the index means the same thing
//! for every player:
//!
//! ```text
//!    0 look_idle     9 walk          13 dod_crawl        (prone, moving)
//!    1 dod_idle1    10 dod_walk      14 dod_crouch_idle
//!    6 jump         11 dod_jog       15 prone_idle
//!                   12 dod_sprint    16 prone_forward
//! ```
//!
//! This probe reports, per demo: the local player's `maxspeed` distribution
//! (present only in a POV demo) and the `gaitsequence` histogram for player
//! entities (present in both, and the useful one for HLTV).
//!
//!     cargo run --release -p analysis --example gait_probe -- <demo>...

use dem::open_demo_from_bytes;
use dem::types::{EngineMessage, FrameData, MessageData, NetMessage};
use std::collections::BTreeMap;

/// Sequence indices shared by every DoD player model.
fn gait_name(i: i32) -> &'static str {
    match i {
        0 => "look_idle",
        1 => "dod_idle1",
        2..=5 => "dod_idle*",
        6 => "jump",
        7 => "swim",
        8 => "swim_idle",
        9 => "walk",
        10 => "dod_walk",
        11 => "dod_jog",
        12 => "dod_sprint",
        13 => "dod_crawl",
        14 => "dod_crouch_idle",
        15 => "prone_idle",
        16 => "prone_forward",
        17 => "get_down",
        18 => "get_up",
        _ => "(other)",
    }
}

fn key(k: &str) -> String {
    k.trim_matches(|c: char| c == '\0' || c.is_whitespace()).to_string()
}
fn as_f32(v: &[u8]) -> Option<f32> {
    (v.len() >= 4).then(|| f32::from_le_bytes([v[0], v[1], v[2], v[3]]))
}
fn as_i32(v: &[u8]) -> Option<i32> {
    (v.len() >= 4).then(|| i32::from_le_bytes([v[0], v[1], v[2], v[3]]))
}

#[derive(Default)]
struct Tally {
    maxspeed: BTreeMap<i32, usize>,
    gait: BTreeMap<i32, usize>,
    /// Whether these fields ever appear on a *player* entity, which is the
    /// difference between "could drive a spectator-side fix" and "could not".
    ent_maxspeed: usize,
    ent_fuser4: usize,
}

fn note_player_field(t: &mut Tally, k: &str, v: &[u8]) {
    match k {
        "gaitsequence" => {
            if let Some(i) = as_i32(v) {
                *t.gait.entry(i).or_insert(0) += 1;
            }
        }
        "maxspeed" => t.ent_maxspeed += 1,
        "fuser4" => t.ent_fuser4 += 1,
        _ => {}
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: gait_probe <demo>...");
        std::process::exit(2);
    }

    for path in &args {
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("{path}: unreadable");
            continue;
        };
        let Ok(demo) = open_demo_from_bytes(&bytes) else {
            eprintln!("{path}: unparseable");
            continue;
        };
        let mut t = Tally::default();

        for entry in &demo.directory.entries {
            for frame in &entry.frames {
                let FrameData::NetworkMessage(bt) = &frame.frame_data else { continue };
                let MessageData::Parsed(msgs) = &bt.1.messages else { continue };
                for m in msgs {
                    let NetMessage::EngineMessage(em) = m else { continue };
                    match &**em {
                        EngineMessage::SvcClientData(cd) => {
                            if let Some((_, v)) =
                                cd.client_data.iter().find(|(k, _)| key(k) == "maxspeed")
                            {
                                if let Some(f) = as_f32(v) {
                                    *t.maxspeed.entry(f.round() as i32).or_insert(0) += 1;
                                }
                            }
                        }
                        // Player entities are indices 1..=32. Most updates
                        // arrive as deltas, not full snapshots, so both carry.
                        EngineMessage::SvcPacketEntities(pe) => {
                            for es in &pe.entity_states {
                                if !(1..=32).contains(&es.entity_index) {
                                    continue;
                                }
                                for (k, v) in es.delta.iter() {
                                    note_player_field(&mut t, &key(k), v);
                                }
                            }
                        }
                        EngineMessage::SvcDeltaPacketEntities(pe) => {
                            for es in &pe.entity_states {
                                if !(1..=32).contains(&es.entity_index) {
                                    continue;
                                }
                                let Some(d) = &es.delta else { continue };
                                for (k, v) in d.iter() {
                                    note_player_field(&mut t, &key(k), v);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        println!("\n{path}");
        if t.maxspeed.is_empty() {
            println!("  clientdata maxspeed: absent (HLTV recording -- no local player)");
        } else {
            let total: usize = t.maxspeed.values().sum();
            print!("  clientdata maxspeed ({total} updates):");
            for (v, n) in &t.maxspeed {
                print!(" {v}u/s x{n}");
            }
            println!();
        }

        let gtotal: usize = t.gait.values().sum();
        println!("  player gaitsequence ({gtotal} updates across player entities):");
        let mut rows: Vec<_> = t.gait.iter().collect();
        rows.sort_by(|a, b| b.1.cmp(a.1));
        for (idx, n) in rows {
            let pctg = 100.0 * *n as f64 / gtotal.max(1) as f64;
            println!("     {:>3} {:<16} x{:<8} {:>5.1}%", idx, gait_name(*idx), n, pctg);
        }
        println!(
            "  maxspeed on player entities: {}   fuser4 (stamina) on player entities: {}",
            if t.ent_maxspeed == 0 { "ABSENT".to_string() } else { format!("{}", t.ent_maxspeed) },
            if t.ent_fuser4 == 0 { "ABSENT".to_string() } else { format!("{}", t.ent_fuser4) },
        );
    }
}
