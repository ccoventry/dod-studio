//! R&D for #927: what the recorded player entity data does when a player's
//! model "twitches" in an HLTV demo.
//!
//! Timeline mode prints, for one entity, every update the demo carries: the
//! server clock (`svc_time`, the viewdemo clock), the origin, the z step from
//! the previous update, `gaitsequence`, `sequence` and `frame`.
//!
//! Survey mode (no entity) counts, per player, how often the leg state flips
//! between standing-moving gaits (9-12) and the crouch gaits (13, 14) and how
//! often the origin jumps 14+ units vertically in one update, against the
//! time that player spends moving.
//!
//!     cargo run --release -p analysis --example gait_twitch_probe -- <demo> [from to entity]

use dem::open_demo_from_bytes;
use dem::types::{Delta, EngineMessage, FrameData, MessageData, NetMessage};
use std::collections::BTreeMap;

fn clean(name: &str) -> &str {
    name.trim_matches(|c: char| c == '\0' || c.is_whitespace())
}

fn field(delta: &Delta, name: &str) -> Option<[u8; 4]> {
    delta
        .iter()
        .find(|(k, _)| clean(k) == name)
        .and_then(|(_, v)| (v.len() >= 4).then(|| [v[0], v[1], v[2], v[3]]))
}

#[derive(Default, Clone)]
struct State {
    origin: [f32; 3],
    yaw: f32,
    gait: i32,
    seq: i32,
    frame: f32,
    seen: bool,
}

#[derive(Default)]
struct Survey {
    name: String,
    last_time: f32,
    last_origin: [f32; 3],
    moving_s: f32,
    updates: usize,
    gait_flips: usize,
    short_crouches: usize,
    crouch_start: Option<f32>,
    z_jumps: usize,
    hops: usize,
    hop_pending: Option<f32>,
    gaps: Vec<f32>,
}

fn crouch(g: i32) -> bool {
    matches!(g, 13 | 14)
}
fn upright_moving(g: i32) -> bool {
    matches!(g, 9..=12)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .expect("usage: gait_twitch_probe <demo> [from to entity]");
    let from: Option<f32> = args.next().and_then(|a| a.parse().ok());
    let to: Option<f32> = args.next().and_then(|a| a.parse().ok());
    let only: Option<u16> = args.next().and_then(|a| a.parse().ok());
    let bytes = std::fs::read(&path).expect("read demo");
    let demo = open_demo_from_bytes(&bytes).expect("parse demo");

    let mut states: BTreeMap<u16, State> = BTreeMap::new();
    let mut survey: BTreeMap<u16, Survey> = BTreeMap::new();
    let mut names: BTreeMap<u16, String> = BTreeMap::new();
    let mut svc_time = 0f32;

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
                let mut updates: Vec<(u16, &Delta)> = Vec::new();
                match &**em {
                    EngineMessage::SvcTime(t) => svc_time = t.time,
                    EngineMessage::SvcUpdateUserInfo(u) => {
                        let info = String::from_utf8_lossy(&u.user_info.0).to_string();
                        let mut parts = info.trim_start_matches('\\').split('\\');
                        while let Some(k) = parts.next() {
                            let v = parts.next().unwrap_or("");
                            if k == "name" {
                                names.insert(u.index as u16 + 1, v.trim_matches('\0').to_string());
                            }
                        }
                    }
                    EngineMessage::SvcPacketEntities(pe) => {
                        for es in &pe.entity_states {
                            updates.push((es.entity_index, &es.delta));
                        }
                    }
                    EngineMessage::SvcDeltaPacketEntities(pe) => {
                        for es in &pe.entity_states {
                            if let Some(d) = &es.delta {
                                updates.push((es.entity_index, d));
                            }
                        }
                    }
                    _ => {}
                }
                for (ent, delta) in updates {
                    if !(1..=32).contains(&ent) {
                        continue;
                    }
                    let st = states.entry(ent).or_default();
                    let prev = st.clone();
                    for (i, k) in ["origin[0]", "origin[1]", "origin[2]"].iter().enumerate() {
                        if let Some(b) = field(delta, k) {
                            st.origin[i] = f32::from_le_bytes(b);
                        }
                    }
                    if let Some(b) = field(delta, "angles[1]") {
                        st.yaw = f32::from_le_bytes(b);
                    }
                    if let Some(b) = field(delta, "gaitsequence") {
                        st.gait = i32::from_le_bytes(b);
                    }
                    if let Some(b) = field(delta, "sequence") {
                        st.seq = i32::from_le_bytes(b);
                    }
                    if let Some(b) = field(delta, "frame") {
                        st.frame = f32::from_le_bytes(b);
                    }
                    let first = !st.seen;
                    st.seen = true;
                    let t = svc_time;
                    if let (Some(a), Some(b), Some(e)) = (from, to, only)
                        && e == ent
                        && t >= a
                        && t <= b
                    {
                        let dz = st.origin[2] - prev.origin[2];
                        let dx = st.origin[0] - prev.origin[0];
                        let dy = st.origin[1] - prev.origin[1];
                        println!(
                            "{t:8.3} demo {:8.3} org {:8.2} {:8.2} {:8.2} d {:6.2} {:6.2} dz {:6.2} yaw {:6.1} gait {:2} seq {:3} frame {:6.1}",
                            frame.time,
                            st.origin[0],
                            st.origin[1],
                            st.origin[2],
                            dx,
                            dy,
                            dz,
                            st.yaw,
                            st.gait,
                            st.seq,
                            st.frame
                        );
                    }
                    let sv = survey.entry(ent).or_default();
                    if !first && t > sv.last_time {
                        let dt = t - sv.last_time;
                        let dxy = ((st.origin[0] - sv.last_origin[0]).powi(2)
                            + (st.origin[1] - sv.last_origin[1]).powi(2))
                        .sqrt();
                        let moving = dt < 0.2 && dxy / dt > 60.0;
                        if moving {
                            sv.moving_s += dt;
                            sv.gaps.push(dt);
                        }
                        sv.updates += 1;
                        let dz = st.origin[2] - sv.last_origin[2];
                        if moving && dz.abs() >= 14.0 && dz.abs() <= 20.0 {
                            sv.z_jumps += 1;
                        }
                        // A duck-tap: the origin snaps up a duck hull's worth
                        // while the legs still show a standing gait, and the
                        // crouch gait follows within a quarter second (a stair
                        // step snaps up too, but stays upright).
                        if moving && (15.0..=19.0).contains(&dz) && !crouch(prev.gait) {
                            sv.hop_pending = Some(t);
                        }
                        if let Some(h) = sv.hop_pending {
                            if crouch(st.gait) && t - h <= 0.25 {
                                sv.hops += 1;
                                sv.hop_pending = None;
                            } else if t - h > 0.25 {
                                sv.hop_pending = None;
                            }
                        }
                        if moving
                            && ((crouch(prev.gait) && upright_moving(st.gait))
                                || (upright_moving(prev.gait) && crouch(st.gait)))
                        {
                            sv.gait_flips += 1;
                        }
                    }
                    if crouch(st.gait) && !crouch(prev.gait) {
                        sv.crouch_start = Some(t);
                    }
                    if !crouch(st.gait)
                        && crouch(prev.gait)
                        && let Some(s) = sv.crouch_start.take()
                        && t - s < 0.35
                        && upright_moving(st.gait)
                    {
                        sv.short_crouches += 1;
                    }
                    sv.last_time = t;
                    sv.last_origin = st.origin;
                }
            }
        }
    }

    if only.is_some() {
        return;
    }
    println!("\n{path}");
    println!(
        "  ent name                   moving_s  gait_flips/min  short_crouches/min  z_jumps/min  hops/min  median_gap_ms"
    );
    for (ent, sv) in &mut survey {
        if sv.moving_s < 20.0 {
            continue;
        }
        sv.gaps.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let med = sv.gaps.get(sv.gaps.len() / 2).copied().unwrap_or(0.0);
        let per_min = |n: usize| n as f32 * 60.0 / sv.moving_s;
        let name = names.get(ent).cloned().unwrap_or_default();
        sv.name = name;
        println!(
            "  {:3} {:<22} {:8.0}  {:14.1}  {:18.1}  {:11.1}  {:8.1}  {:13.1}",
            ent,
            sv.name.chars().take(22).collect::<String>(),
            sv.moving_s,
            per_min(sv.gait_flips),
            per_min(sv.short_crouches),
            per_min(sv.z_jumps),
            per_min(sv.hops),
            med * 1000.0
        );
    }
}
