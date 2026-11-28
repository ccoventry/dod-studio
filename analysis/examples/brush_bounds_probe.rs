//! Prints every brush entity's spawn baseline in an HLTV demo as TSV: its
//! `*N` model, bounds, origin and render settings, so the demo can be matched
//! piece by piece against a map file on disk. Used to tell whether a demo was
//! recorded on a different build of the map, since HLTV demos zero the header's
//! map checksum.
//!
//! With a map file as well, prints what the hook DLL's `ds_map_fix_old_build`
//! does with each entity on that map instead (`dod::brush_pieces`, #1540):
//! the piece it fits, the action, and a summary line.
//!
//!     cargo run --release -p analysis --example brush_bounds_probe -- <demo> [<bsp>]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "dev-only probe run by hand: a panic on bad input is its error report (#1179)"
)]

use dem::bit::BitSliceCast;
use dem::open_demo_from_bytes;
use dem::types::{EngineMessage, FrameData, MessageData, NetMessage};
use dod::brush_pieces::{self, Action, Bounds, Fit, Piece, Spawned};
use std::collections::{BTreeMap, HashMap};

/// The BSP models lump's index, and one `dmodel_t`'s size on disk.
const LUMP_MODELS: usize = 14;
const DMODEL_SIZE: usize = 64;

fn field<'a>(delta: &'a dem::types::Delta, name: &str) -> Option<&'a [u8]> {
    delta
        .iter()
        .find(|(k, _)| k.trim_matches(|c: char| c == '\0' || c.is_whitespace()) == name)
        .map(|(_, v)| v.as_slice())
}

fn f32_of(delta: &dem::types::Delta, name: &str) -> f32 {
    field(delta, name)
        .filter(|v| v.len() >= 4)
        .map(|v| f32::from_le_bytes([v[0], v[1], v[2], v[3]]))
        .unwrap_or(0.0)
}

fn i32_of(delta: &dem::types::Delta, name: &str) -> i32 {
    field(delta, name)
        .map(|v| match v.len() {
            1 => i32::from(v[0]),
            2 | 3 => i32::from(i16::from_le_bytes([v[0], v[1]])),
            0 => 0,
            _ => i32::from_le_bytes([v[0], v[1], v[2], v[3]]),
        })
        .unwrap_or(0)
}

/// One brush entity's spawn baseline.
struct Row {
    entity: u16,
    spawned: Spawned,
    rendermode: i32,
    renderamt: i32,
    renderfx: i32,
    skin: i32,
    solid: i32,
    effects: i32,
    /// How many packets carried the entity: one the server never sends
    /// (EF_NODRAW triggers) is never drawn, whatever its baseline says.
    packets: u32,
}

fn vec3(v: [f32; 3]) -> String {
    format!("{} {} {}", v[0], v[1], v[2])
}

/// Every brush entity's first spawn baseline in the demo at `path`.
fn read_rows(path: &str) -> Vec<Row> {
    let bytes = std::fs::read(path).expect("read demo");
    let demo = open_demo_from_bytes(&bytes).expect("parse demo");

    let mut models: HashMap<u32, String> = HashMap::new();
    let mut rows: BTreeMap<u16, Row> = BTreeMap::new();
    let mut seen: HashMap<u16, u32> = HashMap::new();
    let mut have_baseline = false;
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
                match &**em {
                    EngineMessage::SvcResourceList(rl) => {
                        for r in &rl.resources {
                            if r.type_.to_u8() == 2 {
                                models.insert(
                                    r.index.to_u32(),
                                    r.name.get_string().trim_matches('\0').to_string(),
                                );
                            }
                        }
                    }
                    EngineMessage::SvcPacketEntities(pe) => {
                        for s in &pe.entity_states {
                            *seen.entry(s.entity_index).or_default() += 1;
                        }
                    }
                    EngineMessage::SvcDeltaPacketEntities(pe) => {
                        for s in &pe.entity_states {
                            if !s.remove_entity {
                                *seen.entry(s.entity_index).or_default() += 1;
                            }
                        }
                    }
                    EngineMessage::SvcSpawnBaseline(sb) if !have_baseline => {
                        have_baseline = true;
                        for e in &sb.entities {
                            let d = &e.delta;
                            let mi = i32_of(d, "modelindex");
                            let Some(name) = models.get(&(mi as u32)) else {
                                continue;
                            };
                            let Some(n) = name.strip_prefix('*') else {
                                continue;
                            };
                            let xyz = |k: &str| [0, 1, 2].map(|i| f32_of(d, &format!("{k}[{i}]")));
                            rows.insert(
                                e.entity_index,
                                Row {
                                    entity: e.entity_index,
                                    spawned: Spawned {
                                        named: n.parse().expect("an inline model number"),
                                        bounds: Bounds {
                                            mins: xyz("mins"),
                                            maxs: xyz("maxs"),
                                        },
                                        origin: xyz("origin"),
                                    },
                                    rendermode: i32_of(d, "rendermode"),
                                    renderamt: i32_of(d, "renderamt"),
                                    renderfx: i32_of(d, "renderfx"),
                                    skin: i32_of(d, "skin"),
                                    solid: i32_of(d, "solid"),
                                    effects: i32_of(d, "effects"),
                                    packets: 0,
                                },
                            );
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    for (entity, row) in &mut rows {
        row.packets = seen.get(entity).copied().unwrap_or(0);
    }
    rows.into_values().collect()
}

/// The map's pieces as the engine loads them: the models lump's bounds
/// padded by one unit (`Mod_LoadSubmodels`), each with its entity's origin.
fn read_pieces(path: &str) -> Vec<Piece> {
    let bsp = std::fs::read(path).expect("read map");
    let (offset, length) = dod::map_entities::lump_span(&bsp, LUMP_MODELS).expect("models lump");
    let lump = &bsp[offset..offset + length];
    let entities = dod::map_entities::parse_entities(&bsp).expect("entity lump");
    let (models, _) = lump.as_chunks::<DMODEL_SIZE>();
    let origins = brush_pieces::piece_origins(&entities, models.len());
    models
        .iter()
        .zip(origins)
        .map(|(model, origin)| {
            let f = |i: usize| f32::from_le_bytes(model[i * 4..i * 4 + 4].try_into().unwrap());
            Piece {
                bounds: Bounds {
                    mins: [f(0) - 1.0, f(1) - 1.0, f(2) - 1.0],
                    maxs: [f(3) + 1.0, f(4) + 1.0, f(5) + 1.0],
                },
                origin: Some(origin),
            }
        })
        .collect()
}

fn print_baselines(rows: &[Row]) {
    println!(
        "ent\tmodel\tmins\tmaxs\torigin\trendermode\trenderamt\trenderfx\tskin\tsolid\teffects\tpackets"
    );
    for r in rows {
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            r.entity,
            r.spawned.named,
            vec3(r.spawned.bounds.mins),
            vec3(r.spawned.bounds.maxs),
            vec3(r.spawned.origin),
            r.rendermode,
            r.renderamt,
            r.renderfx,
            r.skin,
            r.solid,
            r.effects,
            r.packets
        );
    }
}

fn print_plan(rows: &[Row], pieces: &[Piece]) {
    let spawned: Vec<Spawned> = rows.iter().map(|r| r.spawned).collect();
    let plan = brush_pieces::plan(&spawned, pieces);
    println!("ent\tnamed\tfit\taction\trendermode\trenderamt\tpackets");
    for ((r, action), s) in rows.iter().zip(&plan.actions).zip(&spawned) {
        let fit = match brush_pieces::fit(s, pieces) {
            Fit::Named => "named".to_string(),
            Fit::Other(n) => format!("*{n}"),
            Fit::Several => "several".to_string(),
            Fit::Nothing => "nothing".to_string(),
            Fit::NoBounds => "no bounds".to_string(),
        };
        let action = match action {
            Action::Keep => "keep".to_string(),
            Action::Draw(n) => format!("draw *{n}"),
            Action::Hide => "hide".to_string(),
        };
        println!(
            "{}\t*{}\t{fit}\t{action}\t{}\t{}\t{}",
            r.entity, s.named, r.rendermode, r.renderamt, r.packets
        );
    }
    let c = plan.counts;
    println!(
        "# {} pieces in the map (with the world); {} brush entities: {} fit their own piece, \
         {} another, {} several, {} none, {} carry no bounds; other build: {}",
        pieces.len(),
        rows.len(),
        c.named,
        c.other,
        c.several,
        c.nothing,
        c.no_bounds,
        c.is_other_build()
    );
}

fn main() {
    let mut args = std::env::args().skip(1);
    let demo = args
        .next()
        .expect("usage: brush_bounds_probe <demo> [<bsp>]");
    let rows = read_rows(&demo);
    match args.next() {
        None => print_baselines(&rows),
        Some(bsp) => print_plan(&rows, &read_pieces(&bsp)),
    }
}
