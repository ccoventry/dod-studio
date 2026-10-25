//! How often a POV recorder's own client failed to predict a shot the
//! server fired, per weapon, and how often that is right after a reload
//! (#1029, the empty-gun click on a real shot).
//!
//! A POV demo carries two records of each of the recorder's shots:
//!
//! - the **server's**: the active weapon's `m_iClip` in `weapon_data_t`
//!   (`svc_clientdata`) drops by one;
//! - the **client's**: a `Dem_Event` frame for the weapon's fire script, the
//!   event the recorder's own weapon prediction played.
//!
//! A server shot with no client event near it is one the recorder's client
//! did not predict, and playback has nothing to play for it. Lines:
//!
//! - `UNPRED`: each such shot, with the time since the weapon's last reload
//!   finished (`m_fInReload` 1 -> 0) and how much refire delay the server
//!   still had when the trigger went down (`early by`);
//! - `ARGS`: each paired client event's `iparam1`/`bparam1`/`bparam2` with
//!   the server's clip after the shot, and how long before the clip drop it
//!   came (`delay_ms`);
//! - `EMPTY`: attack presses while the server's clip is 0 and no reload runs
//!   (real empty clicks);
//! - `IDMAP`: which fire script each weapon id fired with;
//! - `SUMMARY` per demo and weapon, `TOTAL` per weapon.
//!
//!     cargo run --release -p analysis --example pov_dry_fire_survey -- <demo>...

use dem::bit::BitSliceCast;
use dem::open_demo_from_bytes;
use dem::types::{Delta, EngineMessage, FrameData, MessageData, NetMessage};
use std::collections::{BTreeMap, HashMap};

/// A client event this far before the server's clip drop, or this far
/// after, belongs to that shot.
const EVENT_BEFORE: f32 = 0.45;
const EVENT_AFTER: f32 = 0.05;
/// "Right after a reload": the attack press within this long of the reload
/// finishing on the server.
const AFTER_RELOAD: f32 = 0.5;

fn clean(s: &str) -> String {
    s.trim_matches(|c: char| c == '\0' || c.is_whitespace())
        .to_string()
}

fn field_i32(delta: &Delta, name: &str) -> Option<i32> {
    delta
        .iter()
        .find(|(k, _)| clean(k) == name)
        .map(|(_, v)| match v.len() {
            4 => i32::from_le_bytes([v[0], v[1], v[2], v[3]]),
            2 => i16::from_le_bytes([v[0], v[1]]) as i32,
            1 => v[0] as i8 as i32,
            _ => 0,
        })
}

/// Fire scripts of bullet weapons: the 29 scripts the game ships, less the
/// melee, rocket and mortar ones.
fn is_gun_script(name: &str) -> bool {
    name.strip_prefix("events/weapons/")
        .and_then(|s| s.strip_suffix(".sc"))
        .is_some_and(|stem| {
            !matches!(
                stem,
                "knife" | "melee" | "bazooka" | "pschreck" | "piat" | "mortar" | "gewehr"
            )
        })
}

#[derive(Default, Clone)]
struct Tally {
    shots: usize,
    unpredicted: usize,
    after_reload: usize,
    after_reload_unpredicted: usize,
    empty_clicks: usize,
}

struct Shot {
    t: f32,
    press: f32,
    since_reload: f32,
    /// The server's clip after the shot.
    clip_after: i32,
    /// Seconds of refire delay the server still had when the trigger went
    /// down: above 0 is a press made before the gun was ready.
    early_by: f32,
}

fn field_f32(delta: &Delta, name: &str) -> Option<f32> {
    delta
        .iter()
        .find(|(k, _)| clean(k) == name)
        .filter(|(_, v)| v.len() == 4)
        .map(|(_, v)| f32::from_le_bytes([v[0], v[1], v[2], v[3]]))
}

fn main() {
    let mut total: BTreeMap<String, Tally> = BTreeMap::new();
    let mut recorders: BTreeMap<String, usize> = BTreeMap::new();
    for path in std::env::args().skip(1) {
        let Ok(bytes) = std::fs::read(&path) else {
            eprintln!("{path}: can't read");
            continue;
        };
        let Ok(demo) = open_demo_from_bytes(&bytes) else {
            eprintln!("{path}: can't parse");
            continue;
        };
        let short = path.rsplit(['/', '\\']).next().unwrap_or(&path).to_string();

        let mut event_names: HashMap<u32, String> = HashMap::new();
        let mut names: HashMap<u8, String> = HashMap::new();
        let mut svc_time = 0f32;
        // Per weapon id: server clip, in reload, when the reload last ended.
        let mut clip: HashMap<u32, i32> = HashMap::new();
        let mut reloading: HashMap<u32, bool> = HashMap::new();
        let mut reload_end: HashMap<u32, f32> = HashMap::new();
        let mut cur_id = 0u32;
        let mut id_since = 0f32;
        let mut last_press = -10f32;
        let mut attack = false;
        // (time, script, entity) of every client fire event.
        let mut events: Vec<(f32, String, i32)> = Vec::new();
        let mut shots: Vec<(u32, Shot)> = Vec::new();
        let mut empty: HashMap<u32, usize> = HashMap::new();
        let mut next_primary: HashMap<u32, f32> = HashMap::new();
        let mut press_delay = 0f32;
        // Full args of each client event, by index into `events`.
        let mut event_args: Vec<(i32, i32, i32)> = Vec::new();

        for entry in &demo.directory.entries {
            for frame in &entry.frames {
                match &frame.frame_data {
                    FrameData::Event(ev) => {
                        if let Some(name) = event_names.get(&(ev.index as u32))
                            && is_gun_script(name)
                        {
                            events.push((svc_time, name.clone(), ev.args.entity_index));
                            event_args.push((ev.args.iparam1, ev.args.bparam1, ev.args.bparam2));
                        }
                    }
                    FrameData::NetworkMessage(bt) => {
                        let pressed = bt.1.info.usercmd.buttons & 1 != 0;
                        if pressed && !attack {
                            last_press = svc_time;
                            press_delay = next_primary.get(&cur_id).copied().unwrap_or(0.0);
                            let c = clip.get(&cur_id).copied().unwrap_or(-1);
                            if c == 0
                                && !reloading.get(&cur_id).copied().unwrap_or(false)
                                && svc_time - id_since > 1.0
                            {
                                *empty.entry(cur_id).or_default() += 1;
                                println!("EMPTY\t{short}\tid{cur_id}\tpress {svc_time:.3}");
                            }
                        }
                        attack = pressed;
                        let MessageData::Parsed(msgs) = &bt.1.messages else {
                            continue;
                        };
                        for m in msgs {
                            let NetMessage::EngineMessage(em) = m else {
                                continue;
                            };
                            match &**em {
                                EngineMessage::SvcTime(st) => svc_time = st.time,
                                EngineMessage::SvcResourceList(rl) => {
                                    for r in &rl.resources {
                                        if r.type_.to_u8() == 5 {
                                            event_names.insert(
                                                r.index.to_u32(),
                                                clean(&r.name.get_string()),
                                            );
                                        }
                                    }
                                }
                                EngineMessage::SvcUpdateUserInfo(u) => {
                                    let info = String::from_utf8_lossy(&u.user_info.0).to_string();
                                    let mut it = info.split('\\').skip(1);
                                    while let (Some(k), Some(v)) = (it.next(), it.next()) {
                                        if k == "name" {
                                            names.insert(u.index, clean(v));
                                        }
                                    }
                                }
                                EngineMessage::SvcClientData(cd) => {
                                    if let Some(id) = field_i32(&cd.client_data, "m_iId") {
                                        let id = id as u32;
                                        if id != cur_id {
                                            cur_id = id;
                                            id_since = svc_time;
                                        }
                                    }
                                    for w in cd.weapon_data.iter().flatten() {
                                        let slot = w.weapon_index.to_u32();
                                        if let Some(r) = field_i32(&w.weapon_data, "m_fInReload") {
                                            let was =
                                                reloading.insert(slot, r != 0).unwrap_or(false);
                                            if was && r == 0 {
                                                reload_end.insert(slot, svc_time);
                                            }
                                        }
                                        if let Some(n) =
                                            field_f32(&w.weapon_data, "m_flNextPrimaryAttack")
                                        {
                                            next_primary.insert(slot, n);
                                        }
                                        if let Some(c) = field_i32(&w.weapon_data, "m_iClip") {
                                            let old = clip.insert(slot, c);
                                            if old == Some(c + 1) && slot == cur_id {
                                                let press = if svc_time - last_press < 0.6 {
                                                    last_press
                                                } else {
                                                    svc_time
                                                };
                                                let since = reload_end
                                                    .get(&slot)
                                                    .map_or(f32::INFINITY, |e| press - e);
                                                shots.push((
                                                    slot,
                                                    Shot {
                                                        t: svc_time,
                                                        press,
                                                        since_reload: since,
                                                        clip_after: c,
                                                        early_by: if press < svc_time {
                                                            press_delay
                                                        } else {
                                                            0.0
                                                        },
                                                    },
                                                ));
                                            }
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // The recorder: the entity most client events carry.
        let mut by_ent: HashMap<i32, usize> = HashMap::new();
        for (_, _, e) in &events {
            *by_ent.entry(*e).or_default() += 1;
        }
        let Some((&recorder, _)) = by_ent.iter().max_by_key(|(_, n)| **n) else {
            eprintln!("{short}: no client fire events (HLTV?), skipped");
            continue;
        };
        let name = names
            .get(&((recorder - 1) as u8))
            .cloned()
            .unwrap_or_else(|| format!("ent {recorder}"));
        *recorders.entry(name.clone()).or_default() += 1;

        // Weapon id -> script, from the events nearest its shots.
        let mut used = vec![false; events.len()];
        let mut per: BTreeMap<String, Tally> = BTreeMap::new();
        let mut script_of: HashMap<u32, HashMap<String, usize>> = HashMap::new();
        let mut matched: Vec<(u32, &Shot, bool)> = Vec::new();
        for (id, s) in &shots {
            let mut best = None;
            for (i, (t, _, e)) in events.iter().enumerate() {
                if used[i] || *e != recorder {
                    continue;
                }
                if *t >= s.t - EVENT_BEFORE && *t <= s.t + EVENT_AFTER {
                    best = Some(i);
                    break;
                }
            }
            if let Some(i) = best {
                used[i] = true;
                let (i1, b1, b2) = event_args[i];
                println!(
                    "ARGS\t{}\tclip_after {}\ti1 {i1}\tb1 {b1}\tb2 {b2}\tdelay_ms {:.0}",
                    events[i]
                        .1
                        .trim_start_matches("events/weapons/")
                        .trim_end_matches(".sc"),
                    s.clip_after,
                    (s.t - events[i].0) * 1000.0
                );
                *script_of
                    .entry(*id)
                    .or_default()
                    .entry(events[i].1.clone())
                    .or_default() += 1;
            }
            matched.push((*id, s, best.is_some()));
        }
        // Which fire script each weapon id fired with, for the hook's table.
        for (id, scripts) in &script_of {
            for (script, n) in scripts {
                println!("IDMAP\t{id}\t{script}\t{n}");
            }
        }
        let weapon = |id: u32| -> String {
            script_of
                .get(&id)
                .and_then(|m| m.iter().max_by_key(|(_, n)| **n).map(|(s, _)| s.clone()))
                .map(|s| {
                    s.trim_start_matches("events/weapons/")
                        .trim_end_matches(".sc")
                        .to_string()
                })
                .unwrap_or_else(|| format!("id{id}"))
        };
        for (id, s, predicted) in &matched {
            // No fire script matched: a grenade or another thrown weapon.
            if weapon(*id).starts_with("id") {
                continue;
            }
            let t = per.entry(weapon(*id)).or_default();
            t.shots += 1;
            let after = s.since_reload >= 0.0 && s.since_reload <= AFTER_RELOAD;
            if after {
                t.after_reload += 1;
            }
            if !predicted {
                t.unpredicted += 1;
                if after {
                    t.after_reload_unpredicted += 1;
                }
                println!(
                    "UNPRED\t{short}\t{name}\t{}\tpress {:.3}\tserver {:.3}\tsince reload {:.2}\tearly by {:.3}",
                    weapon(*id),
                    s.press,
                    s.t,
                    s.since_reload,
                    s.early_by
                );
            }
        }
        for (id, n) in &empty {
            if weapon(*id).starts_with("id") {
                continue;
            }
            per.entry(weapon(*id)).or_default().empty_clicks += n;
        }
        for (w, t) in &per {
            println!(
                "SUMMARY\t{short}\t{name}\t{w}\tshots {}\tunpredicted {}\tafter-reload {}\tafter-reload unpredicted {}\tempty presses {}",
                t.shots, t.unpredicted, t.after_reload, t.after_reload_unpredicted, t.empty_clicks
            );
            let g = total.entry(w.clone()).or_default();
            g.shots += t.shots;
            g.unpredicted += t.unpredicted;
            g.after_reload += t.after_reload;
            g.after_reload_unpredicted += t.after_reload_unpredicted;
            g.empty_clicks += t.empty_clicks;
        }
    }
    println!(
        "TOTAL by weapon (shots / unpredicted / after-reload / after-reload unpredicted / empty presses)"
    );
    for (w, t) in &total {
        println!(
            "TOTAL\t{w}\t{}\t{}\t{}\t{}\t{}",
            t.shots, t.unpredicted, t.after_reload, t.after_reload_unpredicted, t.empty_clicks
        );
    }
    println!("RECORDERS {} {:?}", recorders.len(), recorders);
}
