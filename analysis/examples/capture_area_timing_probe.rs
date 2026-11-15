//! The timings behind `analysis::capture_area` (#1132), per demo: how often
//! `ClientAreas` status 1 repeats while the recorder stands in an area (its
//! `STALE_AFTER`), how long after the recorder's death in an area the leave
//! message comes, the delay from each death to its team's next `PlayersIn`
//! drop, how often a drop within 1 s of the recorder's death agreed with
//! `ClientAreas` (tp/fp/fn: why drops don't tag kills), and which area indices
//! each message uses.
//!
//!     cargo run --release -p analysis --example capture_area_timing_probe -- demo.dem...

use dem::open_demo_from_bytes;
use dem::types::{EngineMessage, FrameData, MessageData, NetMessage};
use std::collections::{BTreeMap, BTreeSet, HashMap};

fn main() {
    for path in std::env::args().skip(1) {
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let Ok(demo) = open_demo_from_bytes(&bytes) else {
            continue;
        };
        let name = std::path::Path::new(&path)
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let mut time = 0.0f32;
        let mut recorder = None;
        let mut hltv = false;
        let mut last_one: HashMap<u8, f32> = HashMap::new();
        let mut gaps: Vec<f32> = Vec::new();
        let mut areas_ca = BTreeSet::new();
        let mut areas_pi = BTreeSet::new();
        let mut areas_so = BTreeSet::new();
        let mut areas_sp = BTreeSet::new();
        let mut in_set: BTreeSet<u8> = BTreeSet::new();
        let mut team_of: HashMap<u8, u8> = HashMap::new();
        let mut points: HashMap<u8, (u8, u8)> = HashMap::new();
        // recorder death time, in area?, got zero after?
        let mut deaths: Vec<(f32, bool)> = Vec::new();
        let mut zero_after_death: Vec<f32> = Vec::new();
        // drops: (time, team, point, new_count)
        let mut drops: Vec<(f32, u8, u8, u8)> = Vec::new();
        let mut all_deaths: Vec<(f32, u8, u8)> = Vec::new(); // time, victim, team
        let mut hist: BTreeMap<u8, u32> = BTreeMap::new();
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
                        NetMessage::EngineMessage(em) => match &**em {
                            EngineMessage::SvcTime(t) => time = t.time,
                            EngineMessage::SvcHltv(_) => hltv = true,
                            EngineMessage::SvcServerInfo(info) => {
                                recorder = Some(info.player_index + 1)
                            }
                            _ => {}
                        },
                        NetMessage::UserMessage(um) => {
                            let n = String::from_utf8_lossy(&um.name);
                            let d = &um.data[..];
                            match n.trim_end_matches('\0') {
                                "ClientAreas" if d.len() >= 2 => {
                                    *hist.entry(d[1]).or_default() += 1;
                                    areas_ca.insert((d[0], d[1].min(3)));
                                    match d[1] {
                                        1 => {
                                            if let Some(prev) = last_one.get(&d[0])
                                                && in_set.contains(&d[0])
                                            {
                                                gaps.push(time - prev);
                                            }
                                            last_one.insert(d[0], time);
                                            in_set.insert(d[0]);
                                        }
                                        0 => {
                                            if let Some((t, true)) = deaths.last()
                                                && time - t < 3.0
                                            {
                                                zero_after_death.push(time - t);
                                            }
                                            in_set.remove(&d[0]);
                                        }
                                        2 => in_set.clear(),
                                        _ => {}
                                    }
                                }
                                "PlayersIn" if d.len() == 4 => {
                                    areas_pi.insert(d[0]);
                                    if let Some((bt, bc)) = points.insert(d[0], (d[1], d[2]))
                                        && bt != 0
                                        && (d[1] != bt || d[2] < bc)
                                    {
                                        let nc = if d[1] == bt { d[2] } else { 0 };
                                        drops.push((time, bt, d[0], nc));
                                    }
                                }
                                "SetObj" if !d.is_empty() => {
                                    areas_so.insert(d[0]);
                                }
                                "StartProg" if !d.is_empty() => {
                                    areas_sp.insert(d[0]);
                                }
                                "PTeam" if d.len() >= 2 => {
                                    team_of.insert(d[0], d[1]);
                                }
                                "DeathMsg" if d.len() >= 3 => {
                                    let team = team_of.get(&d[1]).copied().unwrap_or(0);
                                    all_deaths.push((time, d[1], team));
                                    if !hltv && Some(d[1]) == recorder {
                                        deaths.push((time, !in_set.is_empty()));
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
        gaps.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let q = |p: f32| {
            gaps.get(((gaps.len() as f32 - 1.0) * p) as usize)
                .copied()
                .unwrap_or(-1.0)
        };
        // Drop delay after each death of the same team (closest following drop)
        let mut delays: Vec<f32> = Vec::new();
        for (t, _, team) in &all_deaths {
            if let Some(dr) = drops
                .iter()
                .find(|(dt, dteam, _, _)| dteam == team && *dt >= *t && *dt - *t <= 3.0)
            {
                delays.push(dr.0 - t);
            }
        }
        let mut delay_hist = [0u32; 7]; // 0-0.25 .. >1.5
        for d in &delays {
            let i = ((d / 0.25) as usize).min(6);
            delay_hist[i] += 1;
        }
        // Recorder truth vs drop attribution within 1.0 s
        let mut tp = 0;
        let mut fp = 0;
        let mut fn_ = 0;
        let rteam_deaths: Vec<_> = all_deaths
            .iter()
            .filter(|(_, v, _)| Some(*v) == recorder && !hltv)
            .collect();
        for ((t, _, team), (_, inarea)) in rteam_deaths.iter().zip(deaths.iter()) {
            let dropped = drops
                .iter()
                .any(|(dt, dteam, _, _)| dteam == team && *dt >= *t && *dt - *t <= 1.0);
            match (dropped, inarea) {
                (true, true) => tp += 1,
                (true, false) => fp += 1,
                (false, true) => fn_ += 1,
                _ => {}
            }
        }
        println!(
            "{name}\t{}\tgaps n={} p50={:.3} p90={:.3} p99={:.3} max={:.3}\trec deaths {} in-area {} zero-after {:?}\tdrop delays {:?}\trec drop tp {tp} fp {fp} fn {fn_}\tCA {:?} PI {:?} SO {:?} SP {:?} hist {:?}",
            if hltv { "HLTV" } else { "POV" },
            gaps.len(),
            q(0.5),
            q(0.9),
            q(0.99),
            q(1.0),
            deaths.len(),
            deaths.iter().filter(|d| d.1).count(),
            zero_after_death
                .iter()
                .map(|z| (z * 100.0).round() / 100.0)
                .collect::<Vec<_>>(),
            delay_hist,
            areas_ca,
            areas_pi,
            areas_so,
            areas_sp,
            hist,
        );
    }
}
