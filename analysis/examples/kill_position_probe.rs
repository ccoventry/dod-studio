//! Checks the entity replay behind kill positions, and prints what it gives
//! (#448).
//!
//! Three things:
//!
//! 1. The sequence-aware replay (`EntityReplay`) against the naive one --
//!    every delta applied over the latest state -- at each `DeathMsg`. They
//!    should agree almost everywhere; where they differ, the naive one is
//!    holding a position from before the player left the snapshot.
//! 2. How many packets the replay had to discard, and why.
//! 3. The analyzer's own `kill_positions`: coverage, per-weapon engagement
//!    distance, and how long the full analysis took.
//!
//! With a second argument, also writes every kill as CSV
//! (`time,weapon,victim_team,vx,vy,vz,kx,ky,kz,distance`) for plotting over
//! a map overview.
//!
//!     cargo run --release -p analysis --example kill_position_probe -- <demo.dem> [kills.csv]

use analysis::entity_replay::{ClientDataReplay, EntityReplay};
use analysis::{Analysis, PlayerPose};
use dem::open_demo_from_bytes;
use dem::types::{EngineMessage, FrameData, MessageData, NetMessage};
use std::collections::{BTreeMap, HashMap};
use std::time::Instant;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .expect("usage: kill_position_probe <demo.dem> [kills.csv]");
    let csv = args.next();
    let bytes = std::fs::read(&path).expect("read");

    // 1 + 2: replay by hand, next to the naive version.
    let demo = open_demo_from_bytes(&bytes).expect("parse");
    let mut replay: EntityReplay<PlayerPose> = EntityReplay::default();
    let mut naive: HashMap<u16, PlayerPose> = HashMap::new();
    let (mut compared, mut differ, mut naive_only, mut replay_only) = (0, 0, 0, 0);
    for entry in &demo.directory.entries {
        for f in &entry.frames {
            let FrameData::NetworkMessage(bt) = &f.frame_data else {
                continue;
            };
            replay.begin_frame(bt.1.sequence_info.incoming_sequence);
            let MessageData::Parsed(msgs) = &bt.1.messages else {
                continue;
            };
            for m in msgs {
                match m {
                    NetMessage::EngineMessage(em) => {
                        if let EngineMessage::SvcServerInfo(si) = &**em {
                            replay.reset(1..=u16::from(si.max_players));
                            naive.clear();
                        }
                        replay.apply(em);
                        if let EngineMessage::SvcDeltaPacketEntities(pe) = &**em {
                            for es in &pe.entity_states {
                                if es.remove_entity {
                                    naive.remove(&es.entity_index);
                                } else if let Some(d) = &es.delta {
                                    let mut p =
                                        naive.get(&es.entity_index).copied().unwrap_or_default();
                                    analysis::entity_replay::EntityFields::apply(&mut p, d);
                                    naive.insert(es.entity_index, p);
                                }
                            }
                        }
                    }
                    NetMessage::UserMessage(um) => {
                        if um.name.starts_with(b"DeathMsg") && um.data.len() >= 2 {
                            let victim = um.data[1] as u16;
                            let known =
                                |p: Option<&PlayerPose>| p.and_then(PlayerPose::known_origin);
                            match (known(replay.get(victim)), known(naive.get(&victim))) {
                                (Some(r), Some(n)) => {
                                    compared += 1;
                                    let d = (0..3).map(|i| (r[i] - n[i]).abs()).fold(0.0, f32::max);
                                    if d > 1.0 {
                                        differ += 1;
                                    }
                                }
                                (None, Some(_)) => naive_only += 1,
                                (Some(_), None) => replay_only += 1,
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
    }
    println!("{path}");
    println!("replay: {:?}", replay.counters);
    println!(
        "victim position at DeathMsg: {compared} compared, {differ} differ by >1 unit; \
         naive-only {naive_only}, replay-only {replay_only}"
    );

    // What the replay adds to an analyzer load: the same walk, replay only,
    // best of three.
    // The floor under it: visiting every player delta without applying one.
    // The analyzer's own loop never reaches into `entity_states`, so this
    // memory traffic is new too.
    let floor_ms = (0..3)
        .map(|_| {
            let t = Instant::now();
            let mut n = 0usize;
            for entry in &demo.directory.entries {
                for f in &entry.frames {
                    let FrameData::NetworkMessage(bt) = &f.frame_data else {
                        continue;
                    };
                    let MessageData::Parsed(msgs) = &bt.1.messages else {
                        continue;
                    };
                    for m in msgs {
                        if let NetMessage::EngineMessage(em) = m
                            && let EngineMessage::SvcDeltaPacketEntities(pe) = &**em
                        {
                            for es in &pe.entity_states {
                                if (1..=32).contains(&es.entity_index) {
                                    n += es.delta.as_ref().map_or(0, |d| d.len());
                                }
                            }
                        }
                    }
                }
            }
            std::hint::black_box(n);
            t.elapsed().as_secs_f64() * 1000.0
        })
        .fold(f64::MAX, f64::min);
    println!("visiting player deltas alone: {floor_ms:.1} ms (best of 3)");

    let replay_ms = (0..3)
        .map(|_| {
            let t = Instant::now();
            let mut replay: EntityReplay<PlayerPose> = EntityReplay::default();
            let mut recorder: ClientDataReplay<PlayerPose> = ClientDataReplay::default();
            for entry in &demo.directory.entries {
                for f in &entry.frames {
                    let FrameData::NetworkMessage(bt) = &f.frame_data else {
                        continue;
                    };
                    replay.begin_frame(bt.1.sequence_info.incoming_sequence);
                    recorder.begin_frame(bt.1.sequence_info.incoming_sequence);
                    let MessageData::Parsed(msgs) = &bt.1.messages else {
                        continue;
                    };
                    for m in msgs {
                        if let NetMessage::EngineMessage(em) = m {
                            if let EngineMessage::SvcServerInfo(si) = &**em {
                                replay.reset(1..=u16::from(si.max_players));
                            }
                            replay.apply(em);
                            recorder.apply(em);
                        }
                    }
                }
            }
            std::hint::black_box((replay.entities().len(), recorder.current().is_some()));
            t.elapsed().as_secs_f64() * 1000.0
        })
        .fold(f64::MAX, f64::min);
    println!("replay alone: {replay_ms:.1} ms (best of 3)");
    drop(demo);

    // 3: the analyzer's own output, timed.
    // Best of three, like the replay timing: a single run on a busy machine
    // swings by half a second.
    let mut took = std::time::Duration::MAX;
    let mut analysis = None;
    for _ in 0..3 {
        let t = Instant::now();
        let a = Analysis::try_from_bytes(&bytes).expect("analyze");
        took = took.min(t.elapsed());
        analysis = Some(a);
    }
    let analysis = analysis.expect("ran");
    let kills = &analysis.state.kill_positions;
    let with_distance = kills.iter().filter(|k| k.distance.is_some()).count();
    let with_victim = kills.iter().filter(|k| k.victim_origin.is_some()).count();
    println!(
        "analysis of {} ({}): {:.0} ms, {} deaths, victim position {with_victim}, distance {with_distance}",
        analysis.demo_info.map_name,
        analysis.demo_info.demo_type,
        took.as_secs_f64() * 1000.0,
        kills.len()
    );
    let mut per_weapon: BTreeMap<String, (usize, f32)> = BTreeMap::new();
    for k in kills.iter().filter(|k| !k.teamkill) {
        if let Some(d) = k.distance {
            let e = per_weapon.entry(format!("{:?}", k.weapon)).or_default();
            e.0 += 1;
            e.1 += d;
        }
    }
    for (w, (n, sum)) in &per_weapon {
        println!(
            "  {w:<20} {n:>4} kills  avg {:>6.0} units  ({:.1} m)",
            sum / *n as f32,
            sum / *n as f32 * 0.0254
        );
    }

    if let Some(csv) = csv {
        let fmt = |o: Option<[f32; 3]>| {
            o.map(|o| format!("{},{},{}", o[0], o[1], o[2]))
                .unwrap_or_else(|| ",,".into())
        };
        let mut out = String::from("time,weapon,victim_team,vx,vy,vz,kx,ky,kz,distance\n");
        for k in kills {
            out += &format!(
                "{:.2},{:?},{:?},{},{},{}\n",
                k.time.viewdemo_offset.as_secs_f32(),
                k.weapon,
                k.victim_team,
                fmt(k.victim_origin),
                fmt(k.killer_origin),
                k.distance.map(|d| d.to_string()).unwrap_or_default()
            );
        }
        std::fs::write(&csv, out).expect("write csv");
        println!("wrote {csv}");
    }
}
