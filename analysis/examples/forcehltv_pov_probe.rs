//! What a POV demo loses when it plays with `dem_forcehltv 1` (#1452).
//!
//! Under `dem_forcehltv 1` the demo player's `Server::ProcessMessage` keeps
//! the network messages but drops every demo-only frame (console commands,
//! client data, the recorder's own events, weapon animations, client-side
//! sounds and the client.dll payload) and the per-frame `demo_info_t` (the
//! recorded view and view model), and it moves the local player to the last
//! slot, `maxclients - 1` (ReHLDS `HLTV/Core/src/Server.cpp`). This counts,
//! per demo, how much of each the file holds, so `docs/goldsrc_forcehltv_pov.md`
//! can say what goes missing and how often, instead of guessing from one
//! recording.
//!
//! Prints one TSV row per demo:
//!
//! - `slot`: the recorder's client slot (`svc_serverinfo` player index),
//!   `max`: the server's maxclients, and `last_slot_used`: whether some
//!   player entity has index `max` (slot `max - 1`, the one forcehltv takes).
//! - `cmd`, `cdata`, `ev`, `wanim`, `snd`, `buf`: demo-only frames of types
//!   3, 4, 6, 7, 8, 9. `ev_own`: of the events, how many name the recorder's
//!   own entity.
//! - `svc_ev`: events the server sent (kept under forcehltv); `svc_ev_own`:
//!   those naming the recorder (its entity in the event delta), `svc_te`:
//!   temp entities, `svc_wanim`: `svc_weaponanim` messages.
//! - `own_wm0` / `oth_wm0`: share of entity packets in which the recorder's
//!   own entity / every other player entity had `weaponmodel` 0 (the
//!   spectator view model comes from that field, so 0 means no gun).
//! - `secs`: the demo's length on the frame clock; `top_snd`: the commonest
//!   client-side sounds (type-8 frames), which forcehltv drops too.
//! - `bursts`: the recorder's own bursts of fire on the viewdemo clock.
//!
//! With `FH_TRACK=<entity>:<from>:<to>` it also prints how many entity
//! packets in that window (viewdemo clock) held that player, and when he was
//! missing: the stretches a forcehltv camera on him can show.
//!
//!     cargo run --release -p analysis --example forcehltv_pov_probe -- <demo> [<demo> ...]

use dem::open_demo_from_bytes;
use dem::types::{EngineMessage, FrameData, MessageData, NetMessage};
use std::collections::HashMap;

/// Delta field names arrive NUL-padded off the wire; trim before comparing
/// (the same trap `weapon_switch_probe` documents).
fn delta_u32(delta: &dem::types::Delta, key: &str) -> Option<u32> {
    delta
        .iter()
        .find(|(k, _)| k.trim_matches(|c: char| c == '\0' || c.is_whitespace()) == key)
        .and_then(|(_, v)| {
            let mut b = [0u8; 4];
            for (i, x) in v.iter().take(4).enumerate() {
                b[i] = *x;
            }
            (!v.is_empty()).then(|| u32::from_le_bytes(b))
        })
}

#[derive(Default)]
struct Counts {
    slot: Option<u8>,
    max: Option<u8>,
    last_slot_used: bool,
    cmd: u32,
    cdata: u32,
    ev: u32,
    ev_own: u32,
    wanim: u32,
    snd: u32,
    buf: u32,
    svc_ev: u32,
    svc_ev_own: u32,
    svc_te: u32,
    svc_wanim: u32,
    own_frames: u32,
    own_zero: u32,
    oth_frames: u32,
    oth_zero: u32,
    sounds: HashMap<String, u32>,
    secs: f32,
    /// The latest `svc_time`: the viewdemo bar's clock.
    svc_time: f32,
    /// `svc_time` of every event the recorder fired (their own shots).
    own_event_times: Vec<f32>,
    /// `FH_TRACK`: entity packets inside the window, how many held the
    /// tracked player, and the times of those that did not.
    track_frames: u32,
    track_seen: u32,
    track_gaps: Vec<f32>,
}

/// `FH_TRACK=<entity>:<from>:<to>` (viewdemo clock): is that player in the
/// recorder's entity list over the window? A forcehltv camera on him can
/// only move while he is.
struct Track {
    entity: u16,
    from: f32,
    to: f32,
}

impl Track {
    fn from_env() -> Option<Track> {
        let v = std::env::var("FH_TRACK").ok()?;
        let mut it = v.split(':');
        Some(Track {
            entity: it.next()?.parse().ok()?,
            from: it.next()?.parse().ok()?,
            to: it.next()?.parse().ok()?,
        })
    }
}

/// Gap times as `start-end` spans (packets under 0.2 s apart join).
fn spans(times: &[f32]) -> String {
    let mut out = Vec::new();
    let mut i = 0;
    while i < times.len() {
        let mut j = i;
        while j + 1 < times.len() && times[j + 1] - times[j] < 0.2 {
            j += 1;
        }
        out.push(format!("{:.2}-{:.2}", times[i], times[j]));
        i = j + 1;
    }
    out.join(" ")
}

/// The recorder's bursts of fire (3+ own events, gaps under a second), as
/// `start-end:count` on the viewdemo clock: moments worth a screenshot.
fn bursts(times: &[f32]) -> String {
    let mut out = Vec::new();
    let mut i = 0;
    while i < times.len() {
        let mut j = i;
        while j + 1 < times.len() && times[j + 1] - times[j] < 1.0 {
            j += 1;
        }
        if j - i + 1 >= 3 {
            out.push(format!("{:.1}-{:.1}:{}", times[i], times[j], j - i + 1));
        }
        i = j + 1;
    }
    out.join(" ")
}

fn probe(path: &str) -> Result<Counts, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read {path}: {e}"))?;
    let demo = open_demo_from_bytes(&bytes).map_err(|e| format!("parse {path}: {e:?}"))?;
    let mut c = Counts::default();
    let mut weapon: HashMap<u16, u32> = HashMap::new();
    // Entities the recorder's client holds right now (its PVS, roughly).
    let mut present: std::collections::HashSet<u16> = Default::default();
    let track = Track::from_env();
    for entry in &demo.directory.entries {
        for frame in &entry.frames {
            c.secs = c.secs.max(frame.time);
            // Entity index of the recorder, once the server info named it.
            let own = c.slot.map(|s| i32::from(s) + 1);
            match &frame.frame_data {
                FrameData::ConsoleCommand(_) => c.cmd += 1,
                FrameData::ClientData(_) => c.cdata += 1,
                FrameData::Event(e) => {
                    c.ev += 1;
                    if Some(e.args.entity_index) == own {
                        c.ev_own += 1;
                        c.own_event_times.push(c.svc_time);
                    }
                }
                FrameData::WeaponAnimation(_) => c.wanim += 1,
                FrameData::Sound(s) => {
                    c.snd += 1;
                    let name = String::from_utf8_lossy(&s.sample)
                        .trim_end_matches('\0')
                        .to_string();
                    *c.sounds.entry(name).or_default() += 1;
                }
                FrameData::DemoBuffer(_) => c.buf += 1,
                FrameData::NetworkMessage(bt) => {
                    let MessageData::Parsed(msgs) = &bt.1.messages else {
                        continue;
                    };
                    for m in msgs {
                        let NetMessage::EngineMessage(em) = m else {
                            continue;
                        };
                        let states: Vec<(u16, Option<u32>)> = match &**em {
                            EngineMessage::SvcTime(t) => {
                                c.svc_time = t.time;
                                continue;
                            }
                            EngineMessage::SvcServerInfo(si) => {
                                c.slot = Some(si.player_index);
                                c.max = Some(si.max_players);
                                continue;
                            }
                            EngineMessage::SvcEvent(ev) => {
                                for e in &ev.events {
                                    c.svc_ev += 1;
                                    let ent =
                                        e.delta.as_ref().and_then(|d| delta_u32(d, "entindex"));
                                    if ent.is_some() && ent.map(|x| x as i32) == own {
                                        c.svc_ev_own += 1;
                                    }
                                }
                                continue;
                            }
                            EngineMessage::SvcTempEntity(_) => {
                                c.svc_te += 1;
                                continue;
                            }
                            EngineMessage::SvcWeaponAnim(_) => {
                                c.svc_wanim += 1;
                                continue;
                            }
                            EngineMessage::SvcPacketEntities(pe) => {
                                present =
                                    pe.entity_states.iter().map(|es| es.entity_index).collect();
                                pe.entity_states
                                    .iter()
                                    .map(|es| {
                                        (es.entity_index, delta_u32(&es.delta, "weaponmodel"))
                                    })
                                    .collect()
                            }
                            EngineMessage::SvcDeltaPacketEntities(pe) => {
                                for es in &pe.entity_states {
                                    if es.remove_entity {
                                        present.remove(&es.entity_index);
                                    } else {
                                        present.insert(es.entity_index);
                                    }
                                }
                                pe.entity_states
                                    .iter()
                                    .map(|es| {
                                        (
                                            es.entity_index,
                                            es.delta
                                                .as_ref()
                                                .and_then(|d| delta_u32(d, "weaponmodel")),
                                        )
                                    })
                                    .collect()
                            }
                            _ => continue,
                        };
                        if let Some(t) = &track
                            && (t.from..=t.to).contains(&c.svc_time)
                        {
                            c.track_frames += 1;
                            if present.contains(&t.entity) {
                                c.track_seen += 1;
                            } else {
                                c.track_gaps.push(c.svc_time);
                            }
                        }
                        let any_players = states.iter().any(|(idx, _)| (1..=32).contains(idx));
                        for (idx, wm) in states {
                            if c.max.is_some_and(|m| idx == u16::from(m))
                                && own != Some(i32::from(idx))
                            {
                                c.last_slot_used = true;
                            }
                            if let (true, Some(w)) = ((1..=32).contains(&idx), wm) {
                                weapon.insert(idx, w);
                            }
                        }
                        // One sample per entity packet: the last weaponmodel
                        // each player entity was given, recorder vs the rest.
                        if any_players {
                            for (&idx, &w) in &weapon {
                                if own == Some(i32::from(idx)) {
                                    c.own_frames += 1;
                                    c.own_zero += u32::from(w == 0);
                                } else {
                                    c.oth_frames += 1;
                                    c.oth_zero += u32::from(w == 0);
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    Ok(c)
}

fn pct(part: u32, whole: u32) -> String {
    if whole == 0 {
        return "-".into();
    }
    format!("{:.0}%", 100.0 * f64::from(part) / f64::from(whole))
}

/// The five commonest client-side sound samples, `name:count` comma-joined.
fn top_sounds(sounds: &HashMap<String, u32>) -> String {
    let mut list: Vec<_> = sounds.iter().collect();
    list.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    list.iter()
        .take(5)
        .map(|(n, k)| format!("{n}:{k}"))
        .collect::<Vec<_>>()
        .join(",")
}

fn main() {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    if paths.is_empty() {
        eprintln!("usage: forcehltv_pov_probe <demo> [<demo> ...]");
        std::process::exit(2);
    }
    println!(
        "demo\tslot\tmax\tlast_slot_used\tcmd\tcdata\tev\tev_own\twanim\tsnd\tbuf\tsvc_ev\tsvc_ev_own\tsvc_te\tsvc_wanim\town_wm0\toth_wm0\tsecs\ttop_snd\tbursts"
    );
    for path in paths {
        let name = std::path::Path::new(&path)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.clone());
        match probe(&path) {
            Ok(c) => {
                println!(
                    "{name}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:.0}\t{}\t{}",
                    c.slot.map_or(-1, i32::from),
                    c.max.map_or(-1, i32::from),
                    c.last_slot_used,
                    c.cmd,
                    c.cdata,
                    c.ev,
                    c.ev_own,
                    c.wanim,
                    c.snd,
                    c.buf,
                    c.svc_ev,
                    c.svc_ev_own,
                    c.svc_te,
                    c.svc_wanim,
                    pct(c.own_zero, c.own_frames),
                    pct(c.oth_zero, c.oth_frames),
                    c.secs,
                    top_sounds(&c.sounds),
                    bursts(&c.own_event_times)
                );
                if c.track_frames > 0 {
                    println!(
                        "  tracked player in {} of {} entity packets ({}); missing at {}",
                        c.track_seen,
                        c.track_frames,
                        pct(c.track_seen, c.track_frames),
                        spans(&c.track_gaps)
                    );
                }
            }
            Err(e) => eprintln!("{name}: {e}"),
        }
    }
}
