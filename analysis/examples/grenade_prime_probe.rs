//! Can a recording tell a primed grenade from a plain throw without the
//! thrower's own viewmodel?
//!
//! Priming is rolling a grenade out and catching it again with USE, which
//! starts its fuse; the player then throws the live one. A POV demo shows it
//! as `throw`, `exploding_idle` (the catch), `exploding_pinpull` (the wind-up)
//! and `exploding_throw`. A spectated view has none of those, so this looks
//! at what every recording has, the grenade in the world:
//!
//! - a world grenade (`w_grenade`, `w_stick`, `w_mills`) appearing is a throw;
//! - one that vanishes within a second, with no explosion's worth of fuse, was
//!   caught;
//! - the next one to appear beside the same player, with no `grenthrow.wav`,
//!   is the primed throw.
//!
//! For each plain throw it prints how long the grenade lived, and for the
//! caught ones what the thrower's replicated state did at the catch and at
//! the primed throw (body sequence, held model).
//!
//!     cargo run --release -p analysis --example grenade_prime_probe -- <demo> [--list]

use dem::bit::BitSliceCast;
use dem::open_demo_from_bytes;
use dem::types::{Delta, EngineMessage, FrameData, MessageData, NetMessage};
use std::collections::{BTreeMap, HashMap};

/// A grenade that lives less than this was caught, not left to its fuse.
const CAUGHT_WITHIN: f32 = 1.5;
/// How far from a player a new grenade can be and still be theirs.
const ARM_REACH: f32 = 96.0;

fn clean(name: &str) -> &str {
    name.trim_matches(|c: char| c == '\0' || c.is_whitespace())
}

fn field<'a>(delta: &'a Delta, name: &str) -> Option<&'a [u8]> {
    delta
        .iter()
        .find(|(k, _)| clean(k) == name)
        .map(|(_, v)| v.as_slice())
        .filter(|v| v.len() >= 4)
}

fn field_u32(delta: &Delta, name: &str) -> Option<u32> {
    field(delta, name).map(|v| u32::from_le_bytes([v[0], v[1], v[2], v[3]]))
}

fn field_f32(delta: &Delta, name: &str) -> Option<f32> {
    field(delta, name).map(|v| f32::from_le_bytes([v[0], v[1], v[2], v[3]]))
}

fn spread(values: &mut [f32]) -> String {
    if values.is_empty() {
        return "none".to_string();
    }
    values.sort_by(|a, b| a.total_cmp(b));
    format!(
        "{:+.3}s / {:+.3}s / {:+.3}s (min / median / max, {})",
        values[0],
        values[values.len() / 2],
        values[values.len() - 1],
        values.len()
    )
}

#[derive(Default, Clone)]
struct Entity {
    model: u32,
    origin: [f32; 3],
    sequence: u32,
    held: u32,
}

/// One grenade's time in the world.
struct Grenade {
    born: f32,
    gone: Option<f32>,
    origin: [f32; 3],
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: grenade_prime_probe <demo> [--list]");
    let list = std::env::args().any(|a| a == "--list");
    let bytes = std::fs::read(&path).expect("read demo");
    let demo = open_demo_from_bytes(&bytes).expect("parse demo");

    let mut models: HashMap<u32, String> = HashMap::new();
    let mut sounds: HashMap<u32, String> = HashMap::new();
    let mut entities: BTreeMap<u16, Entity> = BTreeMap::new();
    // Grenades by entity index while alive, then moved to `grenades`.
    let mut alive: HashMap<u16, Grenade> = HashMap::new();
    let mut grenades: Vec<Grenade> = Vec::new();
    // (time, player)
    let mut throw_sounds: Vec<(f32, u16)> = Vec::new();
    // player -> (time, sequence) / (time, held model index) / (time, origin)
    let mut sequences: HashMap<u16, Vec<(f32, u32)>> = HashMap::new();
    let mut held: HashMap<u16, Vec<(f32, u32)>> = HashMap::new();
    let mut positions: HashMap<u16, Vec<(f32, [f32; 3])>> = HashMap::new();

    let is_grenade_model = |models: &HashMap<u32, String>, index: u32| {
        models.get(&index).is_some_and(|name| {
            ["w_grenade", "w_stick", "w_mills"]
                .iter()
                .any(|g| name.contains(g))
        })
    };

    for entry in &demo.directory.entries {
        for frame in &entry.frames {
            let FrameData::NetworkMessage(bt) = &frame.frame_data else {
                continue;
            };
            let MessageData::Parsed(msgs) = &bt.1.messages else {
                continue;
            };
            let time = frame.time;
            for m in msgs {
                let NetMessage::EngineMessage(em) = m else {
                    continue;
                };
                let mut update =
                    |index: u16,
                     delta: &Delta,
                     entities: &mut BTreeMap<u16, Entity>,
                     alive: &mut HashMap<u16, Grenade>| {
                        let e = entities.entry(index).or_default();
                        if let Some(model) = field_u32(delta, "modelindex") {
                            e.model = model;
                        }
                        for (axis, name) in
                            ["origin[0]", "origin[1]", "origin[2]"].iter().enumerate()
                        {
                            if let Some(v) = field_f32(delta, name) {
                                e.origin[axis] = v;
                            }
                        }
                        if (1..=32).contains(&index) {
                            if let Some(sequence) = field_u32(delta, "sequence")
                                && sequence != e.sequence
                            {
                                e.sequence = sequence;
                                sequences.entry(index).or_default().push((time, sequence));
                            }
                            if let Some(model) = field_u32(delta, "weaponmodel")
                                && model != e.held
                            {
                                e.held = model;
                                held.entry(index).or_default().push((time, model));
                            }
                            positions.entry(index).or_default().push((time, e.origin));
                        } else if is_grenade_model(&models, e.model) && !alive.contains_key(&index)
                        {
                            alive.insert(
                                index,
                                Grenade {
                                    born: time,
                                    gone: None,
                                    origin: e.origin,
                                },
                            );
                        }
                    };
                match &**em {
                    EngineMessage::SvcResourceList(rl) => {
                        for r in &rl.resources {
                            let name = clean(&r.name.get_string()).to_string();
                            match r.type_.to_u8() {
                                2 => {
                                    models.insert(r.index.to_u32(), name);
                                }
                                0 => {
                                    sounds.insert(r.index.to_u32(), name);
                                }
                                _ => {}
                            }
                        }
                    }
                    EngineMessage::SvcPacketEntities(pe) => {
                        // A full packet: whatever it leaves out is gone.
                        let present: Vec<u16> =
                            pe.entity_states.iter().map(|es| es.entity_index).collect();
                        let dropped: Vec<u16> = alive
                            .keys()
                            .copied()
                            .filter(|index| !present.contains(index))
                            .collect();
                        for index in dropped {
                            if let Some(mut g) = alive.remove(&index) {
                                g.gone = Some(time);
                                grenades.push(g);
                            }
                            entities.remove(&index);
                        }
                        for es in &pe.entity_states {
                            update(es.entity_index, &es.delta, &mut entities, &mut alive);
                        }
                    }
                    EngineMessage::SvcDeltaPacketEntities(pe) => {
                        for es in &pe.entity_states {
                            if es.remove_entity {
                                if let Some(mut g) = alive.remove(&es.entity_index) {
                                    g.gone = Some(time);
                                    grenades.push(g);
                                }
                                entities.remove(&es.entity_index);
                            } else if let Some(delta) = &es.delta {
                                update(es.entity_index, delta, &mut entities, &mut alive);
                            }
                        }
                    }
                    EngineMessage::SvcSound(s) => {
                        let index = s
                            .sound_index_long
                            .as_ref()
                            .map(|b| b.to_u32())
                            .or_else(|| s.sound_index_short.as_ref().map(|b| b.to_u32()));
                        if index
                            .and_then(|i| sounds.get(&i))
                            .is_some_and(|name| name.contains("grenthrow"))
                        {
                            throw_sounds.push((time, s.entity_index.to_u32() as u16));
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    grenades.extend(alive.into_values());
    grenades.sort_by(|a, b| a.born.total_cmp(&b.born));

    let position_at = |player: u16, time: f32| -> Option<[f32; 3]> {
        positions.get(&player).and_then(|list| {
            list.iter()
                .rev()
                .find(|(t, _)| *t <= time + 0.05)
                .map(|(_, p)| *p)
        })
    };
    let distance = |a: [f32; 3], b: [f32; 3]| {
        ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
    };
    let model_name = |index: u32| {
        models
            .get(&index)
            .map(|m| m.rsplit('/').next().unwrap_or(m).to_string())
            .unwrap_or_else(|| format!("#{index}"))
    };

    println!("=== {path} ===");
    println!(
        "{} grenthrow sounds, {} world grenades\n",
        throw_sounds.len(),
        grenades.len()
    );

    let (mut lifetimes_kept, mut lifetimes_caught) = (Vec::new(), Vec::new());
    let (mut appears_after_sound, mut catch_to_throw, mut held_after_primed) =
        (Vec::new(), Vec::new(), Vec::new());
    let mut body_at_primed: HashMap<String, usize> = HashMap::new();
    let (mut caught, mut primed_found, mut no_grenade) = (0usize, 0usize, 0usize);

    for (time, player) in &throw_sounds {
        let Some(here) = position_at(*player, *time) else {
            continue;
        };
        // The grenade this throw put in the world.
        let Some(thrown) = grenades
            .iter()
            .find(|g| (g.born - time).abs() <= 0.15 && distance(g.origin, here) <= ARM_REACH)
        else {
            no_grenade += 1;
            continue;
        };
        appears_after_sound.push(thrown.born - time);
        let life = thrown.gone.map(|gone| gone - thrown.born);
        let was_caught = life.is_some_and(|l| l <= CAUGHT_WITHIN);
        match life {
            Some(l) if was_caught => lifetimes_caught.push(l),
            Some(l) => lifetimes_kept.push(l),
            None => {}
        }
        if !was_caught {
            continue;
        }
        caught += 1;
        let catch = thrown.gone.unwrap();
        // The primed throw: the next grenade to appear at this player, with
        // no throw sound of theirs beside it.
        let primed = grenades.iter().find(|g| {
            g.born > catch
                && g.born - catch <= 8.0
                && position_at(*player, g.born).is_some_and(|p| distance(g.origin, p) <= ARM_REACH)
                && !throw_sounds
                    .iter()
                    .any(|(t, p)| p == player && (t - g.born).abs() <= 0.15)
        });
        let mut line = format!(
            "{time:8.2}s player {player:2}: caught after {:.3}s",
            catch - thrown.born
        );
        if let Some(primed) = primed {
            primed_found += 1;
            catch_to_throw.push(primed.born - catch);
            let swap = held
                .get(player)
                .and_then(|list| list.iter().find(|(t, _)| *t >= primed.born - 0.5))
                .filter(|(t, _)| t - primed.born <= 3.0);
            if let Some((t, _)) = swap {
                held_after_primed.push(t - primed.born);
            }
            // What the body did in the half second before the primed throw.
            let body: Vec<String> = sequences
                .get(player)
                .map(|list| {
                    list.iter()
                        .filter(|(t, _)| *t >= primed.born - 0.6 && *t <= primed.born + 0.1)
                        .map(|(t, s)| format!("{s} at {:+.2}", t - primed.born))
                        .collect()
                })
                .unwrap_or_default();
            let key = sequences
                .get(player)
                .and_then(|list| {
                    list.iter()
                        .filter(|(t, _)| *t >= primed.born - 0.6 && *t <= primed.born + 0.1)
                        .map(|(_, s)| s.to_string())
                        .next_back()
                })
                .unwrap_or_else(|| "no change".to_string());
            *body_at_primed.entry(key).or_insert(0) += 1;
            line += &format!(
                ", thrown {:.3}s later; held model {}; body [{}]",
                primed.born - catch,
                swap.map_or("stays".to_string(), |(t, m)| format!(
                    "-> {} {:+.2}",
                    model_name(*m),
                    t - primed.born
                )),
                body.join(", ")
            );
        } else {
            line += ", no primed throw found";
        }
        if list {
            println!("{line}");
        }
    }

    println!(
        "the thrown grenade appears, after grenthrow.wav: {}",
        spread(&mut appears_after_sound)
    );
    println!("throws with no world grenade beside them:        {no_grenade}");
    println!("\nhow long the thrown grenade stays in the world:");
    println!("  left to its fuse: {}", spread(&mut lifetimes_kept));
    println!("  caught:           {}", spread(&mut lifetimes_caught));
    println!("\n{caught} caught; the primed throw found for {primed_found}:");
    println!(
        "  catch to primed throw:                 {}",
        spread(&mut catch_to_throw)
    );
    println!(
        "  held model changes, after the throw:   {}",
        spread(&mut held_after_primed)
    );
    let mut rows: Vec<(String, usize)> = body_at_primed.into_iter().collect();
    rows.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    println!("  last body sequence change in the 0.6s before the primed throw:");
    for (sequence, n) in rows.iter().take(8) {
        println!("    {sequence:<10} {n}");
    }
}
