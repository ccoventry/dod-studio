//! Survey of the position data in a set of demos (#448, step 1).
//!
//! For each demo: the demo type, parse cost (raw `open_demo_from_bytes` vs the
//! full `Analysis::try_from_bytes`), how often the server sends each field of
//! `entity_state_player_t` / `clientdata_t` / `weapon_data_t`, how often a
//! player's origin is refreshed, how big one origin step is, and how many
//! kills have both players' origins at the DeathMsg. Then the aggregate over
//! all demos, and the field tables the server sent (`svc_deltadescription`:
//! bits, divisor, flags).
//!
//!     cargo run --release -p analysis --example position_data_survey -- <demo.dem>...
//!
//! Origins here are a naive "latest value per axis" replay: good for rates and
//! step sizes, not for exact positions (that is `entity_replay`).

use analysis::Analysis;
use dem::open_demo_from_bytes;
use dem::types::{Delta, DeltaDecoder, EngineMessage, FrameData, MessageData, NetMessage};
use std::collections::{BTreeMap, HashMap};
use std::time::Instant;

fn trim(k: &str) -> &str {
    k.trim_end_matches('\0')
}

fn f32_of(v: &[u8]) -> Option<f32> {
    v.get(..4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

#[derive(Default)]
struct Tally {
    deltas: usize,
    fields: BTreeMap<String, usize>,
}

impl Tally {
    fn add(&mut self, d: &Delta) {
        self.deltas += 1;
        for k in d.keys() {
            *self.fields.entry(trim(k).to_string()).or_default() += 1;
        }
    }
    fn merge(&mut self, o: &Tally) {
        self.deltas += o.deltas;
        for (k, n) in &o.fields {
            *self.fields.entry(k.clone()).or_default() += n;
        }
    }
    fn print(&self, title: &str) {
        println!("\n{title}: {} deltas", self.deltas);
        let mut v: Vec<_> = self.fields.iter().collect();
        v.sort_by(|a, b| b.1.cmp(a.1));
        for (k, n) in v {
            println!(
                "  {k:<22} {n:>10}  {:>5.1}% of deltas",
                *n as f64 * 100.0 / self.deltas.max(1) as f64
            );
        }
    }
}

fn pct(v: &mut [f32], p: f32) -> f32 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v.get(((v.len() as f32 - 1.0) * p).max(0.0) as usize)
        .copied()
        .unwrap_or(f32::NAN)
}

#[derive(Default)]
struct Demo {
    player: Tally,
    client: Tally,
    weapon: Tally,
    /// Gaps in seconds between a player's origin refreshes, only while the
    /// player stays in the snapshot.
    gaps: Vec<f32>,
    /// Units moved per origin refresh.
    steps: Vec<f32>,
    /// Gaps between the recorder's `svc_clientdata` messages.
    client_gaps: Vec<f32>,
    client_origin_msgs: usize,
    secs: f32,
    delta_packets: usize,
    removals: usize,
    tables: BTreeMap<String, String>,
}

fn survey(bytes: &[u8]) -> Demo {
    let demo = open_demo_from_bytes(bytes).expect("parse");
    let mut d = Demo::default();
    let mut origin: HashMap<u16, [f32; 3]> = HashMap::new();
    let mut last_t: HashMap<u16, f32> = HashMap::new();
    let mut last_client_t: Option<f32> = None;
    for entry in &demo.directory.entries {
        for f in &entry.frames {
            d.secs = d.secs.max(f.time);
            let FrameData::NetworkMessage(bt) = &f.frame_data else {
                continue;
            };
            let MessageData::Parsed(msgs) = &bt.1.messages else {
                continue;
            };
            for m in msgs {
                let NetMessage::EngineMessage(em) = m else {
                    continue;
                };
                match &**em {
                    EngineMessage::SvcDeltaDescription(dd) => {
                        d.tables.insert(
                            trim(&String::from_utf8_lossy(&dd.name)).to_string(),
                            table(&dd.fields),
                        );
                    }
                    EngineMessage::SvcDeltaPacketEntities(pe) => {
                        d.delta_packets += 1;
                        for es in &pe.entity_states {
                            let idx = es.entity_index;
                            if !(1..=32).contains(&idx) {
                                continue;
                            }
                            if es.remove_entity {
                                d.removals += 1;
                                last_t.remove(&idx);
                                origin.remove(&idx);
                                continue;
                            }
                            let Some(delta) = &es.delta else { continue };
                            d.player.add(delta);
                            let mut o = origin.get(&idx).copied().unwrap_or([f32::NAN; 3]);
                            let mut moved = false;
                            for (k, v) in delta {
                                let axis = match trim(k) {
                                    "origin[0]" => 0,
                                    "origin[1]" => 1,
                                    "origin[2]" => 2,
                                    _ => continue,
                                };
                                if let Some(x) = f32_of(v) {
                                    o[axis] = x;
                                    moved = true;
                                }
                            }
                            if moved {
                                if let (Some(t), Some(prev)) = (last_t.get(&idx), origin.get(&idx))
                                {
                                    d.gaps.push(f.time - t);
                                    let s: f32 = (0..3)
                                        .map(|i| (o[i] - prev[i]).powi(2))
                                        .map(|x| if x.is_nan() { 0.0 } else { x })
                                        .sum();
                                    d.steps.push(s.sqrt());
                                }
                                last_t.insert(idx, f.time);
                                origin.insert(idx, o);
                            }
                        }
                    }
                    EngineMessage::SvcClientData(cd) => {
                        d.client.add(&cd.client_data);
                        if cd.client_data.keys().any(|k| trim(k).starts_with("origin")) {
                            d.client_origin_msgs += 1;
                        }
                        if let Some(t) = last_client_t {
                            d.client_gaps.push(f.time - t);
                        }
                        last_client_t = Some(f.time);
                        for w in cd.weapon_data.iter().flatten() {
                            d.weapon.add(&w.weapon_data);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    d
}

fn table(fields: &DeltaDecoder) -> String {
    let mut s = String::new();
    for f in fields {
        s.push_str(&format!(
            "  {:<22} bits {:>3}  divisor {:>8}  flags {:#06x}\n",
            trim(&String::from_utf8_lossy(&f.name)),
            f.bits,
            f.divisor,
            f.flags
        ));
    }
    s
}

fn main() {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    assert!(
        !paths.is_empty(),
        "usage: position_data_survey <demo.dem>..."
    );
    let mut all = Demo::default();
    let mut tables: BTreeMap<String, (String, usize)> = BTreeMap::new();
    let mut pov = (0usize, 0usize, 0usize, 0usize, 0usize);
    let mut hltv = pov;
    let (mut open_ms, mut full_ms) = (Vec::new(), Vec::new());
    println!(
        "{:<44} {:>4} {:>5} {:>5} | player gap ms p50/p90 | step u p50/p99 | client Hz | kills both-known | open/full ms",
        "demo", "type", "secs", "plyrs"
    );
    for p in &paths {
        let bytes = std::fs::read(p).expect("read");
        let t0 = Instant::now();
        let _ = open_demo_from_bytes(&bytes).expect("parse");
        let open = t0.elapsed().as_secs_f64() * 1e3;
        let t1 = Instant::now();
        let a = Analysis::try_from_bytes(&bytes).expect("analyze");
        let full = t1.elapsed().as_secs_f64() * 1e3;
        let mut d = survey(&bytes);
        let kills = &a.state.kill_positions;
        let by_player = kills.iter().filter(|k| k.killer.is_some()).count();
        let both = kills.iter().filter(|k| k.distance.is_some()).count();
        let is_hltv = a.demo_info.demo_type == "HLTV";
        let client_hz = d.client.deltas as f32 / d.secs.max(1.0);
        println!(
            "{:<44} {:>4} {:>5.0} {:>5} | {:>6.0}/{:<6.0}        | {:>5.1}/{:<6.1}  | {:>7.1}   | {:>4}/{:<4} ({:.0}%) | {:.0}/{:.0}",
            p.rsplit(['/', '\\']).next().unwrap(),
            a.demo_info.demo_type,
            d.secs,
            a.state.players.len(),
            pct(&mut d.gaps.clone(), 0.5) * 1e3,
            pct(&mut d.gaps.clone(), 0.9) * 1e3,
            pct(&mut d.steps.clone(), 0.5),
            pct(&mut d.steps.clone(), 0.99),
            client_hz,
            both,
            by_player,
            both as f64 * 100.0 / by_player.max(1) as f64,
            open,
            full
        );
        open_ms.push(open);
        full_ms.push(full);
        let bucket = if is_hltv { &mut hltv } else { &mut pov };
        bucket.0 += 1;
        bucket.1 += by_player;
        bucket.2 += both;
        bucket.3 += d.client.deltas;
        bucket.4 += d.client_origin_msgs;
        for (k, v) in &d.tables {
            tables.entry(k.clone()).or_insert((v.clone(), 0)).1 += 1;
            let _ = k;
        }
        all.player.merge(&d.player);
        all.client.merge(&d.client);
        all.weapon.merge(&d.weapon);
        all.gaps.append(&mut d.gaps);
        all.steps.append(&mut d.steps);
        all.client_gaps.append(&mut d.client_gaps);
        all.secs += d.secs;
        all.removals += d.removals;
        all.delta_packets += d.delta_packets;
    }
    println!("\nPOV demos {}, HLTV demos {}", pov.0, hltv.0);
    println!(
        "kills by a player with both origins: POV {}/{}, HLTV {}/{}",
        pov.2, pov.1, hltv.2, hltv.1
    );
    println!(
        "svc_clientdata messages: POV {} ({} with an origin field), HLTV {} ({} with one)",
        pov.3, pov.4, hltv.3, hltv.4
    );
    println!(
        "total demo seconds {:.0}, delta packets {}, player removals {}",
        all.secs, all.delta_packets, all.removals
    );
    all.player
        .print("entity_state_player_t (entities 1..=32), all demos");
    all.client.print("clientdata_t, all demos");
    all.weapon.print("weapon_data_t, all demos");
    println!(
        "\norigin refresh gap, all players: p50 {:.0} ms p90 {:.0} ms p99 {:.0} ms; step: p50 {:.1} p90 {:.1} p99 {:.1} max {:.0} units",
        pct(&mut all.gaps, 0.5) * 1e3,
        pct(&mut all.gaps, 0.9) * 1e3,
        pct(&mut all.gaps, 0.99) * 1e3,
        pct(&mut all.steps, 0.5),
        pct(&mut all.steps, 0.9),
        pct(&mut all.steps, 0.99),
        pct(&mut all.steps, 1.0)
    );
    println!(
        "recorder svc_clientdata gap: p50 {:.0} ms p90 {:.0} ms",
        pct(&mut all.client_gaps, 0.5) * 1e3,
        pct(&mut all.client_gaps, 0.9) * 1e3
    );
    println!(
        "parse ms mean: open {:.0}, full analysis {:.0}",
        open_ms.iter().sum::<f64>() / open_ms.len() as f64,
        full_ms.iter().sum::<f64>() / full_ms.len() as f64
    );
    for (name, (t, n)) in &tables {
        println!("\nfield table {name} (identical-by-name in {n} demos; last seen):\n{t}");
    }
}
