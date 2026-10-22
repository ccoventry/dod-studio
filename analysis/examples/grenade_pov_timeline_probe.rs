//! What does a player's own recording show around a grenade throw?
//!
//! The spectated view (`goldsrc-hooks`' `anim_fix`) has to invent the
//! grenade's first-person animations, and the only honest reference for what
//! they should look like is a POV demo, which recorded them. This lines up,
//! for every throw in a POV demo:
//!
//! - the viewmodel animations the recording player's client played
//!   (`Dem_WeaponAnim` frames): `pinpull`, `throw`, and whatever came next;
//! - when the viewmodel itself changed (`clientdata.viewmodel`);
//! - and the three things an HLTV demo also has for that player: the body
//!   sequence entering its grenade attack, `weapons/grenthrow.wav`, and the
//!   third-person `weaponmodel` changing.
//!
//! Everything is printed relative to the `throw` animation, so the output
//! reads as "in POV, X happens this long before or after the throw", and the
//! HLTV-visible columns say what the spectated view can time it from.
//!
//!     cargo run --release -p analysis --example grenade_pov_timeline_probe -- <pov demo> [--list]
//!     cargo run --release -p analysis --example grenade_pov_timeline_probe -- <pov demo> --dump <from> <to>

use dem::bit::BitSliceCast;
use dem::open_demo_from_bytes;
use dem::types::{Delta, EngineMessage, FrameData, MessageData, NetMessage};
use std::collections::HashMap;

/// How long after a throw to keep following the viewmodel.
const WINDOW: f32 = 8.0;

/// The grenade viewmodels share one sequence layout.
const LABELS: [&str; 9] = [
    "idle",
    "draw",
    "pinpull",
    "holster",
    "throw",
    "exploding_idle",
    "exploding_draw",
    "exploding_pinpull",
    "exploding_throw",
];

fn clean(name: &str) -> &str {
    name.trim_matches(|c: char| c == '\0' || c.is_whitespace())
}

fn field_u32(delta: &Delta, name: &str) -> Option<u32> {
    delta
        .iter()
        .find(|(k, _)| clean(k) == name)
        .and_then(|(_, v)| (v.len() >= 4).then(|| u32::from_le_bytes([v[0], v[1], v[2], v[3]])))
}

fn is_grenade(viewmodel: &str) -> bool {
    ["v_grenade", "v_stick", "v_mills"]
        .iter()
        .any(|g| viewmodel.contains(g))
}

fn short(model: &str) -> &str {
    let file = model.rsplit('/').next().unwrap_or(model);
    file.strip_suffix(".mdl").unwrap_or(file)
}

/// min / median / max of a list, as text.
fn spread(values: &mut [f32]) -> String {
    if values.is_empty() {
        return "none".to_string();
    }
    values.sort_by(|a, b| a.total_cmp(b));
    format!(
        "{:+.3}s / {:+.3}s / {:+.3}s (min / median / max, {} throws)",
        values[0],
        values[values.len() / 2],
        values[values.len() - 1],
        values.len()
    )
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: grenade_pov_timeline_probe <pov demo> [--list]");
    let list = std::env::args().any(|a| a == "--list");
    let bytes = std::fs::read(&path).expect("read demo");
    let demo = open_demo_from_bytes(&bytes).expect("parse demo");

    let mut models: HashMap<u32, String> = HashMap::new();
    let mut sounds: HashMap<u32, String> = HashMap::new();
    let mut viewmodel = String::new();
    // (time, sequence, viewmodel at the time)
    let mut anims: Vec<(f32, i32, String)> = Vec::new();
    // (time, new viewmodel)
    let mut viewmodel_changes: Vec<(f32, String)> = Vec::new();
    // (time, entity)
    let mut throw_sounds: Vec<(f32, u16)> = Vec::new();
    // (time, entity): the body sequence entering a grenade attack (82-96).
    let mut windups: Vec<(f32, u16)> = Vec::new();
    // (time, entity, model index)
    let mut held_changes: Vec<(f32, u16, u32)> = Vec::new();
    let mut last_sequence: HashMap<u16, u32> = HashMap::new();
    let mut last_held: HashMap<u16, u32> = HashMap::new();

    for entry in &demo.directory.entries {
        for frame in &entry.frames {
            if let FrameData::WeaponAnimation(wa) = &frame.frame_data {
                anims.push((frame.time, wa.anim, viewmodel.clone()));
            }
            let FrameData::NetworkMessage(bt) = &frame.frame_data else {
                continue;
            };
            let MessageData::Parsed(msgs) = &bt.1.messages else {
                continue;
            };
            let mut body = |entity: u16, delta: &Delta| {
                if entity == 0 || entity > 32 {
                    return;
                }
                if let Some(sequence) = field_u32(delta, "sequence")
                    && last_sequence.insert(entity, sequence) != Some(sequence)
                    && (82..=96).contains(&sequence)
                {
                    windups.push((frame.time, entity));
                }
                if let Some(held) = field_u32(delta, "weaponmodel")
                    && last_held.insert(entity, held) != Some(held)
                {
                    held_changes.push((frame.time, entity, held));
                }
            };
            for m in msgs {
                let NetMessage::EngineMessage(em) = m else {
                    continue;
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
                    EngineMessage::SvcClientData(cd) => {
                        if let Some(name) = field_u32(&cd.client_data, "viewmodel")
                            .and_then(|index| models.get(&index))
                            && *name != viewmodel
                        {
                            viewmodel = name.clone();
                            viewmodel_changes.push((frame.time, viewmodel.clone()));
                        }
                    }
                    EngineMessage::SvcPacketEntities(pe) => {
                        for es in &pe.entity_states {
                            body(es.entity_index, &es.delta);
                        }
                    }
                    EngineMessage::SvcDeltaPacketEntities(pe) => {
                        for es in &pe.entity_states {
                            if let Some(delta) = &es.delta {
                                body(es.entity_index, delta);
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
                            throw_sounds.push((frame.time, s.entity_index.to_u32() as u16));
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    // `--dump <from> <to>`: everything in a window, in time order, for holding
    // against frames recorded from the same stretch of the demo.
    let args: Vec<String> = std::env::args().collect();
    if let Some(at) = args.iter().position(|a| a == "--dump") {
        let from: f32 = args[at + 1].parse().expect("--dump <from> <to>");
        let to: f32 = args[at + 2].parse().expect("--dump <from> <to>");
        let mut lines: Vec<(f32, String)> = Vec::new();
        for (t, anim, vm) in &anims {
            let label = if is_grenade(vm) {
                LABELS
                    .get(*anim as usize)
                    .copied()
                    .unwrap_or("?")
                    .to_string()
            } else {
                format!("sequence {anim}")
            };
            lines.push((*t, format!("animation  {} {label}", short(vm))));
        }
        for (t, vm) in &viewmodel_changes {
            lines.push((*t, format!("viewmodel  -> {}", short(vm))));
        }
        for (t, entity) in &throw_sounds {
            lines.push((*t, format!("grenthrow.wav from entity {entity}")));
        }
        for (t, entity) in &windups {
            lines.push((
                *t,
                format!("body       entity {entity} enters a grenade attack"),
            ));
        }
        for (t, entity, model) in &held_changes {
            lines.push((
                *t,
                format!(
                    "held       entity {entity} -> {}",
                    models.get(model).map_or("?", |m| short(m))
                ),
            ));
        }
        lines.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (t, line) in lines.iter().filter(|(t, _)| *t >= from && *t <= to) {
            println!("{t:9.3}  {line}");
        }
        return;
    }

    // The throws: `throw` or `exploding_throw` on a grenade viewmodel.
    let throws: Vec<(usize, f32)> = anims
        .iter()
        .enumerate()
        .filter(|(_, (_, anim, vm))| matches!(anim, 4 | 8) && is_grenade(vm))
        .map(|(i, (time, _, _))| (i, *time))
        .collect();
    if throws.is_empty() {
        println!("no grenade throw animations in this demo -- is it a POV recording?");
        return;
    }

    // The recording player is whoever's throw sound sits beside the throws.
    let mut votes: HashMap<u16, usize> = HashMap::new();
    for (_, time) in &throws {
        for (t, entity) in &throw_sounds {
            if (t - time).abs() <= 0.3 {
                *votes.entry(*entity).or_insert(0) += 1;
            }
        }
    }
    let me = votes.iter().max_by_key(|(_, n)| **n).map(|(e, _)| *e);

    println!("=== {path} ===");
    println!(
        "{} throws; the recording player is entity {}\n",
        throws.len(),
        me.map(|e| e.to_string()).unwrap_or_else(|| "?".to_string())
    );

    let (mut cook, mut from_windup, mut sound_after) = (Vec::new(), Vec::new(), Vec::new());
    // What the viewmodel did after each throw, as a chain of animations up to
    // whatever ended it: chain -> (each step's offsets, how it ended, the
    // ending's offsets).
    let mut chains: HashMap<String, (Vec<Vec<f32>>, Vec<f32>)> = HashMap::new();
    // When the viewmodel changed against when the replicated held model did.
    let mut held_behind_viewmodel = Vec::new();

    for (index, time) in &throws {
        let exploding = anims[*index].1 == 8;
        let pull = anims[..*index]
            .iter()
            .rev()
            .take_while(|(t, _, _)| time - t <= 15.0)
            .find(|(_, anim, vm)| *anim == 2 && is_grenade(vm))
            .map(|(t, _, _)| time - t);
        let windup = windups
            .iter()
            .rev()
            .find(|(t, e)| Some(*e) == me && *t <= time + 0.2 && time - t <= 2.0)
            .map(|(t, _)| time - t);
        let sound = throw_sounds
            .iter()
            .find(|(t, e)| Some(*e) == me && (t - time).abs() <= 1.0)
            .map(|(t, _)| t - time);
        let swap = viewmodel_changes
            .iter()
            .find(|(t, _)| *t > *time && t - time <= WINDOW);
        let held = held_changes
            .iter()
            .find(|(t, e, _)| Some(*e) == me && *t >= time - 0.3 && t - time <= WINDOW);
        // The grenade's own animations after the throw, until the viewmodel
        // changes, the next pin pull or throw, or the window runs out.
        let limit = swap.map_or(time + WINDOW, |(t, _)| *t);
        let after: Vec<&(f32, i32, String)> = anims[index + 1..]
            .iter()
            .take_while(|(t, anim, vm)| *t < limit && is_grenade(vm) && !matches!(anim, 2 | 4))
            .collect();
        let next_pull = anims[index + 1..]
            .iter()
            .find(|(t, anim, vm)| *t < limit && is_grenade(vm) && matches!(anim, 2 | 4))
            .map(|(t, _, _)| t - time);

        if exploding {
            continue;
        }
        if let Some(v) = pull {
            cook.push(v);
        }
        if let Some(v) = windup {
            from_windup.push(v);
        }
        if let Some(v) = sound {
            sound_after.push(v);
        }
        if let (Some((v, _)), Some((h, _, _))) = (swap, held) {
            held_behind_viewmodel.push(h - v);
        }

        let label = |anim: i32| LABELS.get(anim as usize).copied().unwrap_or("?");
        let (ending, ended_at) = match (next_pull, swap) {
            (Some(t), _) => ("the next pin pull", Some(t)),
            (None, Some((t, _))) => ("another weapon", Some(t - time)),
            (None, None) => ("nothing more", None),
        };
        let chain = format!(
            "throw -> {}{}[{ending}]",
            after
                .iter()
                .map(|(_, anim, _)| label(*anim))
                .collect::<Vec<_>>()
                .join(" -> "),
            if after.is_empty() { "" } else { " -> " }
        );
        let entry = chains
            .entry(chain)
            .or_insert_with(|| (vec![Vec::new(); after.len()], Vec::new()));
        for (step, (t, _, _)) in after.iter().enumerate() {
            entry.0[step].push(t - time);
        }
        if let Some(t) = ended_at {
            entry.1.push(t);
        }

        if list {
            let text = |v: Option<f32>| v.map_or("    ?".to_string(), |v| format!("{v:+.3}"));
            let next: Vec<String> = after
                .iter()
                .map(|(t, anim, _)| format!("{} {:+.2}", label(*anim), t - time))
                .collect();
            println!(
                "{time:8.2}s  pull {}  body {}  sound {}  then: {}  ends: {ending} {}",
                text(pull.map(|v| -v)),
                text(windup.map(|v| -v)),
                text(sound),
                next.join(", "),
                text(ended_at)
            );
        }
    }

    println!("All times are relative to the `throw` animation (plain throws only).\n");
    println!("before the throw:");
    println!("  pinpull to throw (the cook):       {}", spread(&mut cook));
    println!(
        "  body enters its grenade attack:    {}",
        spread(&mut from_windup.iter().map(|v| -v).collect::<Vec<_>>())
    );
    println!(
        "  grenthrow.wav:                     {}",
        spread(&mut sound_after)
    );
    println!(
        "  held model change after viewmodel: {}",
        spread(&mut held_behind_viewmodel)
    );

    println!("\nafter the throw, what the viewmodel played, by how often:");
    let mut rows: Vec<(String, (Vec<Vec<f32>>, Vec<f32>))> = chains.into_iter().collect();
    rows.sort_by_key(|(_, (steps, ends))| {
        std::cmp::Reverse(steps.first().map_or(ends.len(), Vec::len).max(ends.len()))
    });
    for (chain, (mut steps, mut ends)) in rows {
        println!("  {chain}");
        let names: Vec<&str> = chain.split(" -> ").skip(1).collect();
        for (step, gaps) in steps.iter_mut().enumerate() {
            println!(
                "      {:<18} {}",
                names.get(step).copied().unwrap_or("?"),
                spread(gaps)
            );
        }
        if !ends.is_empty() {
            println!("      {:<18} {}", "ends", spread(&mut ends));
        }
    }
}
