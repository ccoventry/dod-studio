//! Rewriting what a player does in a demo (#1469): move a player another
//! player recorded, or give a player data for seconds the demo never got.
//!
//! Every mode reads the demo's entity snapshots the way the client does
//! (`analysis::entity_replay`), so the state it prints or edits is the one the
//! engine would build, not "the latest delta".
//!
//! - `find <demo>`: stretches of 4 s or more where another player is in front
//!   of the recorder's camera (within 30 degrees of where he looks, 150-1500
//!   units away) and walking a fairly straight line: candidates for `bend`.
//! - `track <demo> <entity> <from> <to>`: one line per entity packet in the
//!   window (viewdemo clock): is the player listed, which fields, and his
//!   replayed origin, angles and animation fields.
//! - `deaths <demo> <from> <to>`: the `DeathMsg`s in the window, with which of
//!   the two players the recorder's client held at that moment.
//! - `bend <demo> <out> <entity> <from> <to> <units>`: step 1. Offsets his
//!   origin sideways (to his left for a positive `units`, right for a
//!   negative) by `units * sin^2(pi u)` over the window, so he swerves and
//!   comes back to where the demo has him. Every packet in the window that
//!   holds him lists his full origin, so no packet carries a stale one.
//! - `fill <demo> <out> <entity> <first> <secs> borrow|<walk_yaw> <speed>`:
//!   step 2. Gives him data for the `secs` before `first` (the time he first
//!   appears after a gap), with every field he has when he arrives and his
//!   upper-body frame running on. `borrow` walks him along another player's
//!   real track from this demo that leads into where he arrives (the ground,
//!   the walls and the pace are real); `<walk_yaw> <speed>` walks a straight
//!   line on that heading instead, heights from the nearest footprint.
//!
//! Both edits restate every packet's `entity_count` from the carried set
//! afterwards (a count below it is a crash, `docs/goldsrc_demo_salvage.md`),
//! re-parse the result and report the edited player's track again.
//!
//!     cargo run --release -p analysis --example entity_edit_probe -- find <demo>

use analysis::entity_replay::{EntityReplay, field, field_f32};
use analysis::repair::entity_carry::Carry;
use dem::bit::BitSliceCast;
use dem::open_demo_from_bytes;
use dem::types::{
    Delta, Demo, EngineMessage, EntityStateDelta, FieldName, FrameData, MessageData, NetMessage,
};
use std::collections::HashMap;

const ORIGIN: [&str; 3] = ["origin[0]\0", "origin[1]\0", "origin[2]\0"];
const ANGLES: [&str; 3] = ["angles[0]\0", "angles[1]\0", "angles[2]\0"];
/// A delta packet's index difference is 6 bits; a wider gap is written
/// absolute, as the engine does.
const MAX_INDEX_DIFFERENCE: u16 = 63;
/// How far across (units) a borrowed track may end from where he arrives,
/// and how far its last heading may turn from his (degrees).
const BORROW_REACH: f32 = 128.0;
const BORROW_TURN: f32 = 60.0;
/// Over how long before he arrives the borrowed angles turn into his own.
const ANGLE_BLEND_SECS: f32 = 0.5;
/// The key `tracks` files the recorder's own track under: his entity has no
/// origin in a POV demo, his position is the frame header's `simorg`.
const RECORDER_TRACK: u16 = 0;

/// Where one entity-packet message sits and what the client held after it.
struct PacketAt {
    entry: usize,
    frame: usize,
    message: usize,
    svc_time: f32,
    /// The tracked player's replayed state after this packet, if held.
    state: Option<Delta>,
    /// Whether this packet lists him (and which fields).
    listed: Option<Vec<String>>,
    /// The recorder's camera: origin and angles from the frame header.
    view_origin: [f32; 3],
    view_angles: [f32; 3],
    /// The recorder's own origin (`simorg`).
    recorder_origin: [f32; 3],
    /// Every player's replayed origin after this packet (for `find`).
    players: Vec<(u16, [f32; 3], [f32; 3])>,
}

fn trimmed(k: &str) -> String {
    k.trim_matches(|c: char| c == '\0' || c.is_whitespace())
        .to_string()
}

fn vec3(v: &[f32]) -> [f32; 3] {
    [
        v.first().copied().unwrap_or(0.0),
        v.get(1).copied().unwrap_or(0.0),
        v.get(2).copied().unwrap_or(0.0),
    ]
}

fn get_f32(d: &Delta, key: &str) -> Option<f32> {
    field(d, key).and_then(field_f32)
}

fn get_u32(d: &Delta, key: &str) -> Option<u32> {
    field(d, key).map(|v| {
        let mut b = [0u8; 4];
        for (i, x) in v.iter().take(4).enumerate() {
            b[i] = *x;
        }
        u32::from_le_bytes(b)
    })
}

fn origin_of(d: &Delta) -> Option<[f32; 3]> {
    Some([
        get_f32(d, ORIGIN[0])?,
        get_f32(d, ORIGIN[1])?,
        get_f32(d, ORIGIN[2])?,
    ])
}

fn angles_of(d: &Delta) -> [f32; 3] {
    [
        get_f32(d, ANGLES[0]).unwrap_or(0.0),
        get_f32(d, ANGLES[1]).unwrap_or(0.0),
        get_f32(d, ANGLES[2]).unwrap_or(0.0),
    ]
}

/// A player model's angles for a view: the model's pitch is a third of the
/// view's, the other way up (GoldSrc's `pev->angles.x = -v_angle.x / 3`;
/// DoD's in-eye camera multiplies it back by -3).
fn model_angles_from_view(view: [f32; 3]) -> [f32; 3] {
    let pitch = if view[0] > 180.0 { view[0] - 360.0 } else { view[0] };
    [(-pitch / 3.0).rem_euclid(360.0), view[1].rem_euclid(360.0), 0.0]
}

/// Every network frame's entity packet, replayed. `tracked` is the player
/// whose state each `PacketAt` keeps in full.
fn timeline(demo: &Demo, tracked: u16) -> Vec<PacketAt> {
    let mut replay: EntityReplay<Delta> = EntityReplay::new(1..=32);
    let mut svc_time = 0.0f32;
    let mut out = Vec::new();
    for (entry_index, entry) in demo.directory.entries.iter().enumerate() {
        for (frame_index, frame) in entry.frames.iter().enumerate() {
            let FrameData::NetworkMessage(bt) = &frame.frame_data else {
                continue;
            };
            replay.begin_frame(bt.1.sequence_info.incoming_sequence);
            let MessageData::Parsed(msgs) = &bt.1.messages else {
                continue;
            };
            for (message_index, m) in msgs.iter().enumerate() {
                let NetMessage::EngineMessage(em) = m else {
                    continue;
                };
                if let EngineMessage::SvcTime(t) = &**em {
                    svc_time = t.time;
                }
                if let EngineMessage::SvcServerInfo(_) = &**em {
                    // A new signon restarts the sequences and re-sends baselines.
                    replay.reset(1..=32);
                }
                replay.apply(em);
                let listed = match &**em {
                    EngineMessage::SvcDeltaPacketEntities(pe) => pe
                        .entity_states
                        .iter()
                        .find(|e| e.entity_index == tracked)
                        .map(|e| match &e.delta {
                            Some(d) => d.keys().map(|k| trimmed(k)).collect(),
                            None => vec!["REMOVE".to_string()],
                        }),
                    EngineMessage::SvcPacketEntities(pe) => pe
                        .entity_states
                        .iter()
                        .find(|e| e.entity_index == tracked)
                        .map(|e| e.delta.keys().map(|k| trimmed(k)).collect()),
                    _ => continue,
                };
                let info = &bt.1.info.refparams;
                out.push(PacketAt {
                    entry: entry_index,
                    frame: frame_index,
                    message: message_index,
                    svc_time,
                    state: replay.get(tracked).cloned(),
                    listed,
                    view_origin: vec3(&info.view_origin),
                    view_angles: vec3(&info.view_angles),
                    recorder_origin: vec3(&info.sim_org),
                    players: replay
                        .entities()
                        .iter()
                        .filter_map(|(i, d)| origin_of(d).map(|o| (*i, o, angles_of(d))))
                        .collect(),
                });
            }
        }
    }
    out
}

/// The field names the player decoder uses, NUL included, by trimmed name.
fn player_field_names(demo: &Demo) -> HashMap<String, FieldName> {
    let mut names = HashMap::new();
    for entry in &demo.directory.entries {
        for frame in &entry.frames {
            let FrameData::NetworkMessage(bt) = &frame.frame_data else {
                continue;
            };
            let MessageData::Parsed(msgs) = &bt.1.messages else {
                continue;
            };
            for m in msgs {
                let NetMessage::EngineMessage(em) = m else {
                    continue;
                };
                if let EngineMessage::SvcDeltaDescription(dd) = &**em
                    && String::from_utf8_lossy(&dd.name).starts_with("entity_state_player_t")
                {
                    for f in &dd.fields {
                        names.insert(trimmed(&f.key), f.key.clone());
                    }
                    return names;
                }
            }
        }
    }
    names
}

fn summary(d: &Delta) -> String {
    let o = origin_of(d).unwrap_or([f32::NAN; 3]);
    format!(
        "org {:.1} {:.1} {:.1} ang {:.0} {:.0} seq {} gait {} frame {:.1} anim {:.3} rate {:.2} wm {} mins_z {:.0}",
        o[0],
        o[1],
        o[2],
        get_f32(d, ANGLES[0]).unwrap_or(f32::NAN),
        get_f32(d, ANGLES[1]).unwrap_or(f32::NAN),
        get_u32(d, "sequence\0").map(|v| v as i32).unwrap_or(-1),
        get_u32(d, "gaitsequence\0").map(|v| v as i32).unwrap_or(-1),
        get_f32(d, "frame\0").unwrap_or(f32::NAN),
        get_f32(d, "animtime\0").unwrap_or(f32::NAN),
        get_f32(d, "framerate\0").unwrap_or(f32::NAN),
        get_u32(d, "weaponmodel\0").map(|v| v as i32).unwrap_or(-1),
        get_f32(d, "mins[2]\0").unwrap_or(f32::NAN),
    )
}

fn print_track(packets: &[PacketAt], from: f32, to: f32) {
    let mut last: Option<[f32; 3]> = None;
    let mut last_t = 0.0;
    for p in packets.iter().filter(|p| p.svc_time >= from && p.svc_time <= to) {
        let Some(state) = &p.state else {
            println!("{:.3}\t-", p.svc_time);
            last = None;
            continue;
        };
        let o = origin_of(state).unwrap_or([0.0; 3]);
        let speed = match last {
            Some(l) if p.svc_time > last_t => {
                ((o[0] - l[0]).powi(2) + (o[1] - l[1]).powi(2)).sqrt() / (p.svc_time - last_t)
            }
            _ => f32::NAN,
        };
        last = Some(o);
        last_t = p.svc_time;
        let listed = match &p.listed {
            Some(f) => f.join(","),
            None => "(carried)".to_string(),
        };
        println!(
            "{:.3}\t{}\tv {:.0}\t[{}]",
            p.svc_time,
            summary(state),
            speed,
            listed
        );
    }
}

/// The angle in degrees between where the camera looks and the player.
fn off_center(view_origin: [f32; 3], view_angles: [f32; 3], target: [f32; 3]) -> (f32, f32) {
    let (pitch, yaw) = (view_angles[0].to_radians(), view_angles[1].to_radians());
    let fwd = [pitch.cos() * yaw.cos(), pitch.cos() * yaw.sin(), -pitch.sin()];
    // Aim at his chest, not his feet: the origin is the hull's centre.
    let to = [
        target[0] - view_origin[0],
        target[1] - view_origin[1],
        target[2] + 16.0 - view_origin[2],
    ];
    let dist = (to[0] * to[0] + to[1] * to[1] + to[2] * to[2]).sqrt();
    if dist < 1.0 {
        return (180.0, dist);
    }
    let dot = (fwd[0] * to[0] + fwd[1] * to[1] + fwd[2] * to[2]) / dist;
    (dot.clamp(-1.0, 1.0).acos().to_degrees(), dist)
}

fn find(packets: &[PacketAt], recorder: u16) {
    // entity -> (start, last time, path length, start origin, last origin, angle sum, n)
    struct Run {
        start: f32,
        last_t: f32,
        path: f32,
        first: [f32; 3],
        last: [f32; 3],
        angle_sum: f32,
        dist_sum: f32,
        n: u32,
    }
    let mut runs: HashMap<u16, Run> = HashMap::new();
    let mut found = Vec::new();
    let close = |e: u16, r: Run, found: &mut Vec<String>| {
        let secs = r.last_t - r.start;
        let net = ((r.last[0] - r.first[0]).powi(2) + (r.last[1] - r.first[1]).powi(2)).sqrt();
        if secs >= 4.0 && r.path / secs > 120.0 && net / r.path.max(1.0) > 0.85 {
            found.push(format!(
                "ent {e}\t{:.2}-{:.2}\t{:.1}s\tspeed {:.0}\tstraight {:.2}\toff {:.0}deg\tdist {:.0}",
                r.start,
                r.last_t,
                secs,
                r.path / secs,
                net / r.path.max(1.0),
                r.angle_sum / r.n as f32,
                r.dist_sum / r.n as f32
            ));
        }
    };
    for p in packets {
        let mut seen = Vec::new();
        for (e, o, _) in &p.players {
            if *e == recorder {
                continue;
            }
            let (angle, dist) = off_center(p.view_origin, p.view_angles, *o);
            if angle > 30.0 || !(150.0..=1500.0).contains(&dist) {
                continue;
            }
            seen.push(*e);
            let r = runs.entry(*e).or_insert(Run {
                start: p.svc_time,
                last_t: p.svc_time,
                path: 0.0,
                first: *o,
                last: *o,
                angle_sum: 0.0,
                dist_sum: 0.0,
                n: 0,
            });
            if p.svc_time - r.last_t > 0.3 {
                // A break in view starts a new run.
                *r = Run {
                    start: p.svc_time,
                    last_t: p.svc_time,
                    path: 0.0,
                    first: *o,
                    last: *o,
                    angle_sum: 0.0,
                    dist_sum: 0.0,
                    n: 0,
                };
            }
            r.path += ((o[0] - r.last[0]).powi(2) + (o[1] - r.last[1]).powi(2)).sqrt();
            r.last = *o;
            r.last_t = p.svc_time;
            r.angle_sum += angle;
            r.dist_sum += dist;
            r.n += 1;
        }
        let gone: Vec<u16> = runs
            .keys()
            .copied()
            .filter(|e| !seen.contains(e) && p.svc_time - runs[e].last_t > 0.3)
            .collect();
        for e in gone {
            if let Some(r) = runs.remove(&e) {
                close(e, r, &mut found);
            }
        }
    }
    for (e, r) in runs.drain() {
        close(e, r, &mut found);
    }
    for f in found {
        println!("{f}");
    }
}

/// The recorder's entity: `svc_serverinfo`'s player index + 1.
fn recorder_entity(demo: &Demo) -> u16 {
    for entry in &demo.directory.entries {
        for frame in &entry.frames {
            let FrameData::NetworkMessage(bt) = &frame.frame_data else {
                continue;
            };
            let MessageData::Parsed(msgs) = &bt.1.messages else {
                continue;
            };
            for m in msgs {
                if let NetMessage::EngineMessage(em) = m
                    && let EngineMessage::SvcServerInfo(si) = &**em
                {
                    return si.player_index as u16 + 1;
                }
            }
        }
    }
    0
}

fn deaths(demo: &Demo, packets: &[PacketAt], from: f32, to: f32) {
    // DeathMsg is [killer, victim, weapon]; indices are 1-based slots, so
    // they are the entity indices directly.
    let mut svc_time = 0.0;
    let mut held: Vec<u16> = Vec::new();
    let mut pi = 0;
    for entry in &demo.directory.entries {
        for frame in &entry.frames {
            let FrameData::NetworkMessage(bt) = &frame.frame_data else {
                continue;
            };
            let MessageData::Parsed(msgs) = &bt.1.messages else {
                continue;
            };
            for m in msgs {
                match m {
                    NetMessage::EngineMessage(em) => {
                        if let EngineMessage::SvcTime(t) = &**em {
                            svc_time = t.time;
                            while pi < packets.len() && packets[pi].svc_time <= svc_time {
                                held = packets[pi].players.iter().map(|(e, _, _)| *e).collect();
                                pi += 1;
                            }
                        }
                    }
                    NetMessage::UserMessage(um) => {
                        let name = String::from_utf8_lossy(&um.name);
                        if !name.starts_with("DeathMsg") || svc_time < from || svc_time > to {
                            continue;
                        }
                        let (k, v) = (
                            um.data.first().copied().unwrap_or(0) as u16,
                            um.data.get(1).copied().unwrap_or(0) as u16,
                        );
                        println!(
                            "{svc_time:.2}\tkiller {k} (held {})\tvictim {v} (held {})\tdata {:?}",
                            held.contains(&k),
                            held.contains(&v),
                            um.data
                        );
                    }
                }
            }
        }
    }
}

/// The packet message an edit goes into.
fn packet_mut(
    demo: &mut Demo,
    entry: usize,
    frame: usize,
    message: usize,
) -> Option<&mut EngineMessage> {
    let FrameData::NetworkMessage(bt) =
        &mut demo.directory.entries[entry].frames[frame].frame_data
    else {
        return None;
    };
    let MessageData::Parsed(msgs) = bt.1.messages_mut() else {
        return None;
    };
    match &mut msgs[message] {
        NetMessage::EngineMessage(em) => Some(&mut **em),
        _ => None,
    }
}

/// Lists `entity` in the delta packet with `fields` set over whatever the
/// packet already lists for him (or as a new entry), re-encoding the packet's
/// index differences. Returns false for a message that is not a delta packet.
fn set_entity_fields(em: &mut EngineMessage, entity: u16, fields: &Delta) -> bool {
    let EngineMessage::SvcDeltaPacketEntities(pe) = em else {
        return false;
    };
    match pe
        .entity_states
        .iter_mut()
        .find(|e| e.entity_index == entity)
    {
        Some(e) if !e.remove_entity => {
            let d = e.delta.get_or_insert_with(Delta::new);
            for (k, v) in fields {
                d.insert(k.clone(), v.clone());
            }
        }
        Some(_) => return false,
        None => {
            pe.entity_states.push(EntityStateDelta {
                entity_index: entity,
                remove_entity: false,
                is_absolute_entity_index: false,
                absolute_entity_index: None,
                entity_index_difference: None,
                has_custom_delta: Some(false),
                delta: Some(fields.clone()),
            });
            pe.entity_states.sort_by_key(|e| e.entity_index);
        }
    }
    let mut running = 0u16;
    for e in pe.entity_states.iter_mut() {
        let gap = e.entity_index - running;
        if gap <= MAX_INDEX_DIFFERENCE {
            e.is_absolute_entity_index = false;
            e.absolute_entity_index = None;
            e.entity_index_difference = Some(dem::nbit_num!(gap, 6));
        } else {
            e.is_absolute_entity_index = true;
            e.absolute_entity_index = Some(dem::nbit_num!(e.entity_index, 11));
            e.entity_index_difference = None;
        }
        running = e.entity_index;
    }
    true
}

/// Restates every packet's `entity_count` as the carried set's size (the
/// repair's `entity_count` step, which is crate-private). Returns how many
/// changed.
fn restate_counts(demo: &mut Demo) -> usize {
    let mut carry = Carry::default();
    let mut fixes = Vec::new();
    for (ei, entry) in demo.directory.entries.iter().enumerate() {
        for (fi, frame) in entry.frames.iter().enumerate() {
            let FrameData::NetworkMessage(bt) = &frame.frame_data else {
                continue;
            };
            let seq = bt.1.sequence_info.incoming_sequence;
            let MessageData::Parsed(msgs) = &bt.1.messages else {
                continue;
            };
            for (mi, m) in msgs.iter().enumerate() {
                let NetMessage::EngineMessage(em) = m else {
                    continue;
                };
                let (declared, carried) = match &**em {
                    EngineMessage::SvcPacketEntities(pe) => {
                        let set = carry.full(seq, pe.entity_states.iter().map(|e| e.entity_index));
                        (pe.entity_count.to_u32() as usize, set.len())
                    }
                    EngineMessage::SvcDeltaPacketEntities(pe) => {
                        let entries: Vec<(u16, bool)> = pe
                            .entity_states
                            .iter()
                            .map(|e| (e.entity_index, e.remove_entity))
                            .collect();
                        let Some(set) = carry.delta(seq, pe.delta_sequence.to_u8(), &entries)
                        else {
                            continue;
                        };
                        (pe.entity_count.to_u32() as usize, set.len())
                    }
                    _ => continue,
                };
                if declared != carried {
                    fixes.push((ei, fi, mi, carried));
                }
            }
        }
    }
    let n = fixes.len();
    for (ei, fi, mi, carried) in fixes {
        if let Some(em) = packet_mut(demo, ei, fi, mi) {
            let count = dem::nbit_num!(carried as u32, 16);
            match em {
                EngineMessage::SvcPacketEntities(pe) => pe.entity_count = count,
                EngineMessage::SvcDeltaPacketEntities(pe) => pe.entity_count = count,
                _ => {}
            }
        }
    }
    n
}

fn f32_value(v: f32) -> dem::types::DeltaValue {
    v.to_le_bytes().into()
}

fn bend(demo: &mut Demo, entity: u16, from: f32, to: f32, units: f32) -> Result<usize, String> {
    let names = player_field_names(demo);
    let key = |n: &str| -> Result<FieldName, String> {
        names
            .get(n)
            .cloned()
            .ok_or_else(|| format!("no player field {n}"))
    };
    let origin_keys = [key("origin[0]")?, key("origin[1]")?, key("origin[2]")?];
    let packets = timeline(demo, entity);
    let window: Vec<&PacketAt> = packets
        .iter()
        .filter(|p| p.svc_time >= from && p.svc_time <= to && p.state.is_some())
        .collect();
    let first = window
        .first()
        .and_then(|p| p.state.as_ref().and_then(origin_of))
        .ok_or("he is not in the window")?;
    let last = window
        .last()
        .and_then(|p| p.state.as_ref().and_then(origin_of))
        .ok_or("he is not in the window")?;
    let (dx, dy) = (last[0] - first[0], last[1] - first[1]);
    let len = (dx * dx + dy * dy).sqrt().max(1.0);
    // Left of the way he walks, z up.
    let left = [-dy / len, dx / len];
    println!(
        "walks {:.0} units on heading {:.0} deg; offset towards {:.0} deg",
        len,
        dy.atan2(dx).to_degrees(),
        (left[1] * units.signum()).atan2(left[0] * units.signum()).to_degrees()
    );
    let edits: Vec<(usize, usize, usize, Delta)> = window
        .iter()
        .filter_map(|p| {
            let o = origin_of(p.state.as_ref()?)?;
            let u = (p.svc_time - from) / (to - from);
            let s = (std::f32::consts::PI * u).sin().powi(2) * units;
            let mut d = Delta::new();
            d.insert(origin_keys[0].clone(), f32_value(o[0] + left[0] * s));
            d.insert(origin_keys[1].clone(), f32_value(o[1] + left[1] * s));
            d.insert(origin_keys[2].clone(), f32_value(o[2]));
            Some((p.entry, p.frame, p.message, d))
        })
        .collect();
    // The demo has no world geometry to test against: count the bent
    // positions no player ever stood near, a rough sign of a wall or a drop.
    let stood = stood(&packets);
    let off_ground = edits
        .iter()
        .filter(|(_, _, _, d)| {
            let o = origin_of(d).unwrap_or([0.0; 3]);
            ground_z(&stood, o[0], o[1], o[2]).is_none()
        })
        .count();
    println!(
        "{} of {} bent positions have no footprint within 64 units",
        off_ground,
        edits.len()
    );
    let mut done = 0;
    for (e, f, m, d) in edits {
        if let Some(em) = packet_mut(demo, e, f, m)
            && set_entity_fields(em, entity, &d)
        {
            done += 1;
        }
    }
    Ok(done)
}

/// Every place a player stood in the demo, for the height of a made-up path.
fn stood(packets: &[PacketAt]) -> Vec<[f32; 3]> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for p in packets {
        for (_, o, _) in &p.players {
            let key = ((o[0] / 16.0) as i32, (o[1] / 16.0) as i32, (o[2] / 16.0) as i32);
            if seen.insert(key) {
                out.push(*o);
            }
        }
    }
    out
}

/// The height of the nearest place someone stood (within 64 units across),
/// preferring the one closest to `z_hint`, and how far across it was.
fn ground_z(stood: &[[f32; 3]], x: f32, y: f32, z_hint: f32) -> Option<(f32, f32)> {
    stood
        .iter()
        .map(|o| (((o[0] - x).powi(2) + (o[1] - y).powi(2)).sqrt(), o[2]))
        .filter(|(d, z)| *d < 64.0 && (z - z_hint).abs() < 40.0)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(d, z)| (z, d))
}

/// How the made-up stretch moves.
enum Route {
    /// A straight walk on `walk_yaw` (degrees, the way he moves) at `speed`
    /// units/s, heights from the nearest footprint.
    Line { walk_yaw: f32, speed: f32 },
    /// Another player's real track from this demo that ends where he
    /// arrives, heading his way: real ground, real walls, real pace.
    Borrow,
}

struct FillPlan {
    entity: u16,
    first: f32,
    secs: f32,
    route: Route,
}

/// A straight walk back from `o1`, one point per made-up packet time, and how
/// many points found no footprint for their height.
fn line_path(
    times: &[f32],
    t1: f32,
    o1: [f32; 3],
    (walk_yaw, speed): (f32, f32),
    stood: &[[f32; 3]],
) -> (Vec<[f32; 3]>, usize) {
    let (hx, hy) = (walk_yaw.to_radians().cos(), walk_yaw.to_radians().sin());
    // Heights walking back from where he arrives: each point's hint is the
    // last one's, so a slope is followed instead of lost after 40 units.
    let mut misses = 0;
    let mut hint = o1[2];
    let mut path: Vec<[f32; 3]> = times
        .iter()
        .rev()
        .map(|t| {
            let back = (t1 - t) * speed;
            let (x, y) = (o1[0] - hx * back, o1[1] - hy * back);
            match ground_z(stood, x, y, hint) {
                Some((z, _)) => hint = z,
                None => misses += 1,
            }
            [x, y, hint]
        })
        .collect();
    path.reverse();
    // Footprints mix standing and crouched heights 18 units apart: smooth.
    let raw: Vec<f32> = path.iter().map(|o| o[2]).collect();
    for (i, o) in path.iter_mut().enumerate() {
        let lo = i.saturating_sub(8);
        let hi = (i + 9).min(raw.len());
        o[2] = raw[lo..hi].iter().sum::<f32>() / (hi - lo) as f32;
    }
    (path, misses)
}

/// One point of a player's replayed track.
#[derive(Clone, Copy)]
struct Sample {
    t: f32,
    origin: [f32; 3],
    /// Model angles (pitch already the model's, a third of the view's).
    angles: [f32; 3],
}

/// Every other player's replayed track, one sample per packet he is in, and
/// the recorder's own under `RECORDER_TRACK`.
fn tracks(packets: &[PacketAt], skip: u16) -> HashMap<u16, Vec<Sample>> {
    let mut out: HashMap<u16, Vec<Sample>> = HashMap::new();
    for p in packets {
        if p.recorder_origin != [0.0; 3] {
            out.entry(RECORDER_TRACK).or_default().push(Sample {
                t: p.svc_time,
                origin: p.recorder_origin,
                angles: model_angles_from_view(p.view_angles),
            });
        }
        for (e, o, a) in &p.players {
            if *e != skip {
                out.entry(*e).or_default().push(Sample {
                    t: p.svc_time,
                    origin: *o,
                    angles: *a,
                });
            }
        }
    }
    out
}

fn heading(a: [f32; 3], b: [f32; 3]) -> f32 {
    (b[1] - a[1]).atan2(b[0] - a[0]).to_degrees()
}

/// The track at time `t`: origin interpolated, angles from the sample at or
/// after `t`.
fn sample_at(track: &[Sample], t: f32) -> Option<Sample> {
    let i = track.partition_point(|s| s.t < t);
    let b = *track.get(i)?;
    if i == 0 {
        return Some(b);
    }
    let a = track[i - 1];
    let u = if b.t > a.t { (t - a.t) / (b.t - a.t) } else { 0.0 };
    Some(Sample {
        t,
        origin: [
            a.origin[0] + (b.origin[0] - a.origin[0]) * u,
            a.origin[1] + (b.origin[1] - a.origin[1]) * u,
            a.origin[2] + (b.origin[2] - a.origin[2]) * u,
        ],
        angles: b.angles,
    })
}

/// The real track of another player that best leads into `o1`: a stretch of
/// `secs` with no gap over 0.3 s, walking 120-320 units/s, ending within
/// `BORROW_REACH` of `o1` (40 in height) on a heading within `BORROW_TURN`
/// of his. Shifted so it ends exactly at `o1`, the shift growing from nothing
/// at its start, so its first point is a place someone really stood. Angles
/// are the borrowed player's: where he looked, not a fixed stare.
fn borrowed_path(
    packets: &[PacketAt],
    entity: u16,
    times: &[f32],
    (t1, o1, arrival_heading): (f32, [f32; 3], f32),
) -> Result<Vec<([f32; 3], Option<[f32; 3]>)>, String> {
    let secs = t1 - times.first().copied().ok_or("nothing to fill")?;
    let all = tracks(packets, entity);
    let mut best: Option<(f32, u16, f32, [f32; 3])> = None;
    for (e, track) in &all {
        for (j, end) in track.iter().enumerate() {
            let oj = end.origin;
            let across = ((oj[0] - o1[0]).powi(2) + (oj[1] - o1[1]).powi(2)).sqrt();
            if across > BORROW_REACH || (oj[2] - o1[2]).abs() > 40.0 {
                continue;
            }
            let i = track.partition_point(|s| s.t < end.t - secs);
            if track[i].t > end.t - secs + 0.3 {
                continue;
            }
            let seg = &track[i..=j];
            if seg.windows(2).any(|w| w[1].t - w[0].t > 0.3) {
                continue;
            }
            let length: f32 = seg
                .windows(2)
                .map(|w| {
                    ((w[1].origin[0] - w[0].origin[0]).powi(2)
                        + (w[1].origin[1] - w[0].origin[1]).powi(2))
                    .sqrt()
                })
                .sum();
            if !(120.0..=320.0).contains(&(length / secs)) {
                continue;
            }
            let Some(before) = sample_at(track, end.t - 0.15) else {
                continue;
            };
            let turn = (heading(before.origin, oj) - arrival_heading + 540.0).rem_euclid(360.0)
                - 180.0;
            if turn.abs() > BORROW_TURN {
                continue;
            }
            if best.as_ref().is_none_or(|b| across < b.0) {
                best = Some((across, *e, end.t, oj));
            }
        }
    }
    let (across, e, tj, oj) = best.ok_or("no other player's track leads into his arrival")?;
    println!(
        "borrowing entity {e}'s track {:.2}-{:.2} (ends {across:.0} units from his arrival)",
        tj - secs,
        tj
    );
    let track = &all[&e];
    let shift = [o1[0] - oj[0], o1[1] - oj[1], o1[2] - oj[2]];
    Ok(times
        .iter()
        .map(|t| {
            let u = 1.0 - (t1 - t) / secs;
            let s = sample_at(track, tj - (t1 - t)).unwrap_or(Sample {
                t: tj,
                origin: oj,
                angles: [0.0; 3],
            });
            (
                [
                    s.origin[0] + shift[0] * u,
                    s.origin[1] + shift[1] * u,
                    s.origin[2] + shift[2] * u,
                ],
                Some(s.angles),
            )
        })
        .collect())
}

/// `a` turned towards `b` by `u` (0..1), each angle the short way round.
fn blend_angles(a: [f32; 3], b: [f32; 3], u: f32) -> [f32; 3] {
    let mut out = [0.0; 3];
    for i in 0..3 {
        let d = (b[i] - a[i] + 540.0).rem_euclid(360.0) - 180.0;
        out[i] = (a[i] + d * u).rem_euclid(360.0);
    }
    out
}

fn fill(demo: &mut Demo, plan: &FillPlan) -> Result<usize, String> {
    let packets = timeline(demo, plan.entity);
    let stood = stood(&packets);
    let first_at = packets
        .iter()
        .position(|p| p.svc_time >= plan.first && p.state.is_some())
        .ok_or("he never appears after `first`")?;
    let first = &packets[first_at];
    // Everything he is at his first packet, origin and angles included: the
    // first real packet after the gap is a delta from his baseline, and the
    // fields it leaves out must come out as the baseline's, not as whatever
    // the made-up frames set.
    let arrival = first.state.clone().ok_or("no state")?;
    let o1 = origin_of(&arrival).ok_or("no origin")?;
    println!(
        "first real packet {:.3}: {}",
        first.svc_time,
        summary(&arrival)
    );
    let start = first.svc_time - plan.secs;
    // His upper-body frame runs on at the rate the next real packet shows
    // (the field wraps at 256), so the made-up stretch is not one frozen pose.
    let frame0 = get_f32(&arrival, "frame\0").unwrap_or(0.0);
    let frame_rate = packets[first_at + 1..]
        .iter()
        .find(|p| p.svc_time > first.svc_time)
        .and_then(|p| {
            let f = get_f32(p.state.as_ref()?, "frame\0")?;
            Some((f - frame0).rem_euclid(256.0) / (p.svc_time - first.svc_time))
        })
        .unwrap_or(0.0);
    let made_up: Vec<&PacketAt> = packets[..first_at]
        .iter()
        .filter(|p| p.svc_time >= start && p.svc_time < first.svc_time)
        .collect();
    if let Some(p) = made_up.iter().find(|p| p.state.is_some()) {
        return Err(format!(
            "he is already in the demo at {:.3}: shorten the fill",
            p.svc_time
        ));
    }
    let times: Vec<f32> = made_up.iter().map(|p| p.svc_time).collect();
    let (path, misses) = match plan.route {
        Route::Line { walk_yaw, speed } => {
            let (path, misses) = line_path(&times, first.svc_time, o1, (walk_yaw, speed), &stood);
            (path.into_iter().map(|o| (o, None)).collect(), misses)
        }
        Route::Borrow => {
            let later = packets[first_at..]
                .iter()
                .find(|p| p.svc_time >= first.svc_time + 0.15)
                .and_then(|p| origin_of(p.state.as_ref()?))
                .ok_or("he leaves again within 0.15 s")?;
            let path =
                borrowed_path(&packets, plan.entity, &times, (first.svc_time, o1, heading(o1, later)))?;
            (path, 0)
        }
    };
    let arrival_angles = angles_of(&arrival);
    let mut edits = Vec::new();
    for (p, &([x, y, z], borrowed)) in made_up.iter().zip(&path) {
        let frame = (frame0 - frame_rate * (first.svc_time - p.svc_time)).rem_euclid(256.0);
        // Borrowed angles turn into his own over the last half second, so
        // the camera does not snap when the real data starts.
        let angles = borrowed.map(|a| {
            let u = (1.0 - (first.svc_time - p.svc_time) / ANGLE_BLEND_SECS).clamp(0.0, 1.0);
            blend_angles(a, arrival_angles, u)
        });
        let mut d = arrival.clone();
        if let Some(a) = angles {
            for (i, key) in ANGLES.iter().enumerate() {
                let name = d.keys().find(|k| &***k == *key).cloned();
                if let Some(k) = name {
                    d.insert(k, f32_value(a[i]));
                }
            }
        }
        let names: Vec<FieldName> = d.keys().cloned().collect();
        for k in names {
            match trimmed(&k).as_str() {
                "origin[0]" => {
                    d.insert(k, f32_value(x));
                }
                "origin[1]" => {
                    d.insert(k, f32_value(y));
                }
                "origin[2]" => {
                    d.insert(k, f32_value(z));
                }
                "frame" => {
                    d.insert(k, f32_value(frame.round()));
                }
                _ => {}
            }
        }
        edits.push((p.entry, p.frame, p.message, d));
    }
    println!(
        "{} made-up packets from {:.3}; {} points with nobody's footprint near (kept the last height); frame rate {:.0}/s",
        edits.len(),
        start,
        misses,
        frame_rate
    );
    if let (Some((a, _)), Some((b, _))) = (path.first(), path.last()) {
        println!(
            "path {:.0} {:.0} {:.0} -> {:.0} {:.0} {:.0}",
            a[0], a[1], a[2], b[0], b[1], b[2]
        );
    }
    let mut done = 0;
    for (e, f, m, d) in edits {
        if let Some(em) = packet_mut(demo, e, f, m)
            && set_entity_fields(em, plan.entity, &d)
        {
            done += 1;
        }
    }
    // The first real packet: list everything he has there, so nothing the
    // made-up frames set survives into the real data.
    if let Some(em) = packet_mut(demo, first.entry, first.frame, first.message) {
        set_entity_fields(em, plan.entity, &arrival);
    }
    Ok(done)
}

fn arg<T: std::str::FromStr>(args: &[String], i: usize, what: &str) -> T {
    match args.get(i).and_then(|s| s.parse().ok()) {
        Some(v) => v,
        None => {
            eprintln!("missing or bad argument {i}: {what}");
            std::process::exit(2);
        }
    }
}

fn write_checked(demo: &mut Demo, source: &[u8], out: &str, entity: u16, from: f32, to: f32) {
    let fixed = restate_counts(demo);
    println!("entity counts restated: {fixed}");
    let bytes = demo.write_to_bytes();
    if let Err(e) = std::fs::write(out, &bytes) {
        eprintln!("write {out}: {e}");
        std::process::exit(1);
    }
    println!("bytes {} -> {}", source.len(), bytes.len());
    let again = match open_demo_from_bytes(&bytes) {
        Ok(d) => d,
        Err(e) => {
            println!("RE-PARSE FAILED: {e}");
            std::process::exit(1);
        }
    };
    let mut check = again;
    println!(
        "re-parse OK; counts still disagreeing after re-parse: {}",
        restate_counts(&mut check)
    );
    let packets = timeline(&check, entity);
    print_track(&packets, from, to);
    match analysis::Analysis::try_from_bytes(&bytes) {
        Ok(a) => println!("analysis OK: {} players", a.state.players.len()),
        Err(e) => println!("ANALYSIS FAILED: {e}"),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = args.first().map(String::as_str).unwrap_or("");
    let path: String = arg(&args, 1, "demo");
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("read {path}: {e}");
            std::process::exit(1);
        }
    };
    let mut demo = match open_demo_from_bytes(&bytes) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("parse {path}: {e}");
            std::process::exit(1);
        }
    };
    match mode {
        "roundtrip" => {
            let out = demo.write_to_bytes();
            let first_diff = out.iter().zip(&bytes).position(|(a, b)| a != b);
            let differing = out.iter().zip(&bytes).filter(|(a, b)| a != b).count();
            println!(
                "round trip: {} -> {} bytes, identical {}, first difference at {:?}, {} bytes differ",
                bytes.len(),
                out.len(),
                out == bytes,
                first_diff,
                differing
            );
        }
        "find" => {
            let recorder = recorder_entity(&demo);
            println!("recorder entity {recorder}");
            let packets = timeline(&demo, 0);
            find(&packets, recorder);
        }
        "track" => {
            let entity: u16 = arg(&args, 2, "entity");
            let packets = timeline(&demo, entity);
            print_track(&packets, arg(&args, 3, "from"), arg(&args, 4, "to"));
        }
        "deaths" => {
            let packets = timeline(&demo, 0);
            deaths(&demo, &packets, arg(&args, 2, "from"), arg(&args, 3, "to"));
        }
        "bend" => {
            let out: String = arg(&args, 2, "out");
            let entity: u16 = arg(&args, 3, "entity");
            let (from, to): (f32, f32) = (arg(&args, 4, "from"), arg(&args, 5, "to"));
            match bend(&mut demo, entity, from, to, arg(&args, 6, "units")) {
                Ok(n) => println!("bent {n} packets"),
                Err(e) => {
                    eprintln!("bend: {e}");
                    std::process::exit(1);
                }
            }
            write_checked(&mut demo, &bytes, &out, entity, from - 0.5, to + 0.5);
        }
        "fill" => {
            let out: String = arg(&args, 2, "out");
            let plan = FillPlan {
                entity: arg(&args, 3, "entity"),
                first: arg(&args, 4, "first"),
                secs: arg(&args, 5, "secs"),
                route: if args.get(6).map(String::as_str) == Some("borrow") {
                    Route::Borrow
                } else {
                    Route::Line {
                        walk_yaw: arg(&args, 6, "walk_yaw or borrow"),
                        speed: arg(&args, 7, "speed"),
                    }
                },
            };
            match fill(&mut demo, &plan) {
                Ok(n) => println!("filled {n} packets"),
                Err(e) => {
                    eprintln!("fill: {e}");
                    std::process::exit(1);
                }
            }
            write_checked(
                &mut demo,
                &bytes,
                &out,
                plan.entity,
                plan.first - plan.secs - 0.5,
                plan.first + 1.0,
            );
        }
        _ => {
            eprintln!("usage: entity_edit_probe find|track|deaths|bend|fill|roundtrip <demo> ...");
            std::process::exit(2);
        }
    }
}
