//! Rewrites an HLTV demo so no snapshot carries more packet entities than the
//! pre-Anniversary engine takes (#207).
//!
//! `dod_lennon2` HLTV demos carry 276-303 entities per snapshot, and playback on
//! the pre-Anniversary build dies on the first one: HLTV's `Core.dll`
//! (`Server::ParsePacketEntities: entnum>MAX_PACKET_ENTITIES`) refuses any
//! snapshot over 256. Trimming the map cannot help a recorded demo, since the
//! entities are baked into its packets, so this trims the packets instead.
//!
//! It drops a fixed set of entities from every snapshot in the file, chosen so
//! no snapshot ends up over the target. Only entities that are safe to lose are
//! eligible: present from the first snapshot, never updated and never removed
//! afterwards (static map props, so dropping one changes nothing else and its
//! index is never reused), and never a player. In order:
//!
//! - **A, invisible**: a render mode with `renderamt` 15 or less;
//! - **B, sprites**: static `.spr` entities (on `dod_lennon2`, its fog puffs);
//! - **C, non-solid brush props**: `*N` models with `solid` 0, so dropping one
//!   cannot change a trace, farthest from anywhere a player ever stood first
//!   (positions harvested from the demo itself; needs the map's `.bsp` for the
//!   props' bounds, otherwise index order).
//!
//! Dropping an entity from a delta-encoded stream means three things per packet:
//! leave its entries out, re-encode the next entry's index (indices are
//! differences from the previous listed entity), and restate `entity_count`,
//! which is the size of the snapshot that results from applying the packet, not
//! the number listed. The count is worked out by carrying every snapshot's
//! entity set forward from the one its delta names, the way the engine does:
//! `hw.dll`'s `CL_ParsePacketEntities` allocates the new frame from the declared
//! count and copies carried entities into it without a bounds check, so a count
//! that is too small overruns the heap (#15's `m1_h1` crash at `hw.dll+0x13453`).
//!
//! Writes a new file and never overwrites; then re-parses it and checks every
//! declared count against the carried-forward set and the target.
//!
//!     cargo run --release -p analysis --example packet_entity_trim -- <in.dem> <out.dem> [target] [map.bsp]

use dem::bit::BitSliceCast;
use dem::open_demo_from_bytes;
use dem::types::{Delta, Demo, EngineMessage, FrameData, MessageData, NetMessage};
use std::collections::{HashMap, HashSet};

#[path = "shared/entity_carry.rs"]
mod entity_carry;
use entity_carry::Carry;

/// Pre-Anniversary `MAX_PACKET_ENTITIES`.
const LEGACY_LIMIT: usize = 256;
/// Leaves a little room under the limit by default.
const DEFAULT_TARGET: usize = 246;
/// Player positions are deduplicated on this grid, in world units.
const GRID: f32 = 64.0;

fn field<'a>(delta: &'a Delta, name: &str) -> Option<&'a Vec<u8>> {
    delta
        .iter()
        .find(|(k, _)| k.trim_end_matches('\0') == name)
        .map(|(_, v)| v)
}

fn field_i32(delta: &Delta, name: &str) -> Option<i32> {
    field(delta, name)
        .filter(|v| v.len() >= 4)
        .map(|v| i32::from_le_bytes([v[0], v[1], v[2], v[3]]))
}

fn field_f32(delta: &Delta, name: &str) -> Option<f32> {
    field(delta, name)
        .filter(|v| v.len() >= 4)
        .map(|v| f32::from_le_bytes([v[0], v[1], v[2], v[3]]))
}

/// What the demo says about one entity: its spawn state merged with the first
/// snapshot, and whether anything about it ever changes afterwards.
#[derive(Default)]
struct Entity {
    fields: Delta,
    in_first: bool,
    updates: u32,
    removed: bool,
}

/// Everything the first pass learns.
struct Survey {
    models: HashMap<u32, String>,
    entities: HashMap<u16, Entity>,
    max_players: u16,
    /// Grid cells a player stood in.
    player_cells: HashSet<(i32, i32, i32)>,
}

fn network_messages(demo: &Demo) -> impl Iterator<Item = &NetMessage> {
    demo.directory
        .entries
        .iter()
        .flat_map(|e| e.frames.iter())
        .filter_map(|f| match &f.frame_data {
            FrameData::NetworkMessage(bt) => match &bt.1.messages {
                MessageData::Parsed(m) => Some(m.iter()),
                _ => None,
            },
            _ => None,
        })
        .flatten()
}

fn survey(demo: &Demo) -> Survey {
    let mut s = Survey {
        models: HashMap::new(),
        entities: HashMap::new(),
        max_players: 32,
        player_cells: HashSet::new(),
    };
    let mut first_seen = false;
    let mut origins: HashMap<u16, [f32; 3]> = HashMap::new();
    for m in network_messages(demo) {
        let NetMessage::EngineMessage(em) = m else {
            continue;
        };
        match &**em {
            EngineMessage::SvcServerInfo(si) => s.max_players = si.max_players as u16,
            EngineMessage::SvcResourceList(rl) => {
                for r in rl.resources.iter().filter(|r| r.type_.to_u8() == 2) {
                    let name = r.name.get_string();
                    s.models
                        .insert(r.index.to_u32(), name.trim_matches('\0').to_string());
                }
            }
            EngineMessage::SvcSpawnBaseline(sb) => {
                for e in &sb.entities {
                    let ent = s.entities.entry(e.entity_index).or_default();
                    ent.fields.extend(e.delta.clone());
                }
            }
            EngineMessage::SvcPacketEntities(pe) if !first_seen => {
                first_seen = true;
                for e in &pe.entity_states {
                    let ent = s.entities.entry(e.entity_index).or_default();
                    ent.fields.extend(e.delta.clone());
                    ent.in_first = true;
                }
            }
            EngineMessage::SvcPacketEntities(pe) => {
                for e in &pe.entity_states {
                    s.entities.entry(e.entity_index).or_default().updates += 1;
                }
            }
            EngineMessage::SvcDeltaPacketEntities(dpe) => {
                for e in &dpe.entity_states {
                    let ent = s.entities.entry(e.entity_index).or_default();
                    if e.remove_entity {
                        ent.removed = true;
                        continue;
                    }
                    ent.updates += 1;
                    let is_player = e.entity_index >= 1 && e.entity_index <= s.max_players;
                    if let (true, Some(delta)) = (is_player, &e.delta) {
                        let o = origins.entry(e.entity_index).or_insert([0.0; 3]);
                        let mut moved = false;
                        for (axis, slot) in o.iter_mut().enumerate() {
                            if let Some(v) = field_f32(delta, &format!("origin[{axis}]")) {
                                *slot = v;
                                moved = true;
                            }
                        }
                        if moved {
                            s.player_cells.insert((
                                (o[0] / GRID).round() as i32,
                                (o[1] / GRID).round() as i32,
                                (o[2] / GRID).round() as i32,
                            ));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    s
}

/// What the map's `.bsp` says about its brush models (`*N`): where each sits,
/// and the class of the entity that uses it. The demo can't say either: brush
/// entities are networked at origin 0, and `solid` is 0 on the wire for all of
/// them (measured on `wsod25_grp3_h1_hltv.dem`), the solid ones included.
#[derive(Default)]
struct MapInfo {
    centres: Vec<[f32; 3]>,
    classes: HashMap<usize, String>,
}

/// Reads a GoldSrc BSP (version 30): lump 0 (entities) and lump 14 (models).
fn read_map(path: &str) -> MapInfo {
    let Ok(b) = std::fs::read(path) else {
        eprintln!("could not read {path}; tier C is left out");
        return MapInfo::default();
    };
    let rd_i32 = |o: usize| i32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
    let rd_f32 = |o: usize| f32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
    if b.len() < 4 + 15 * 8 || rd_i32(0) != 30 {
        eprintln!("{path} is not a version-30 BSP; tier C is left out");
        return MapInfo::default();
    }
    let lump = |n: usize| {
        let (off, len) = (rd_i32(4 + n * 8) as usize, rd_i32(4 + n * 8 + 4) as usize);
        (off + len <= b.len()).then_some(off..off + len)
    };
    let mut map = MapInfo::default();
    if let Some(r) = lump(14) {
        map.centres = (0..r.len() / 64)
            .map(|i| {
                let m = r.start + i * 64;
                let c = |a: usize| (rd_f32(m + a * 4) + rd_f32(m + 12 + a * 4)) / 2.0;
                [c(0), c(1), c(2)]
            })
            .collect();
    }
    if let Some(r) = lump(0) {
        let text = String::from_utf8_lossy(&b[r]);
        for block in text.split('}') {
            let quoted: Vec<&str> = block.split('"').skip(1).step_by(2).collect();
            let pairs: HashMap<&str, &str> = quoted
                .chunks(2)
                .filter(|p| p.len() == 2)
                .map(|p| (p[0], p[1]))
                .collect();
            if let (Some(model), Some(class)) = (pairs.get("model"), pairs.get("classname"))
                && let Some(n) = model.strip_prefix('*').and_then(|n| n.parse().ok())
            {
                map.classes.insert(n, class.to_string());
            }
        }
    }
    map
}

fn model_name<'a>(s: &'a Survey, e: &Entity) -> &'a str {
    field_i32(&e.fields, "modelindex")
        .and_then(|mi| s.models.get(&(mi as u32)))
        .map(String::as_str)
        .unwrap_or("")
}

/// The tier an entity falls in, or `None` when it must stay.
fn tier(s: &Survey, map: &MapInfo, index: u16, e: &Entity) -> Option<char> {
    if !e.in_first || e.removed || e.updates > 0 || index <= s.max_players || index == 0 {
        return None;
    }
    let model = model_name(s, e);
    let rendermode = field_i32(&e.fields, "rendermode").unwrap_or(0);
    let renderamt = field_i32(&e.fields, "renderamt").unwrap_or(0);
    let class = model
        .strip_prefix('*')
        .and_then(|n| n.parse::<usize>().ok())
        .and_then(|n| map.classes.get(&n))
        .map(String::as_str);
    if rendermode != 0 && renderamt <= 15 {
        Some('A')
    } else if model.ends_with(".spr") {
        Some('B')
    } else if class == Some("func_illusionary") {
        // SOLID_NOT by definition: nothing could have collided with it.
        Some('C')
    } else {
        None
    }
}

/// How far an entity sits from the nearest place a player stood, or 0 when
/// that can't be told.
fn distance_from_players(s: &Survey, map: &MapInfo, e: &Entity) -> f32 {
    let model = model_name(s, e);
    let origin = [0, 1, 2].map(|a| field_f32(&e.fields, &format!("origin[{a}]")).unwrap_or(0.0));
    let at = match model
        .strip_prefix('*')
        .and_then(|n| n.parse::<usize>().ok())
    {
        Some(n) if n < map.centres.len() => [0, 1, 2].map(|a| map.centres[n][a] + origin[a]),
        Some(_) => return 0.0,
        None => origin,
    };
    s.player_cells
        .iter()
        .map(|&(x, y, z)| {
            let d = [
                x as f32 * GRID - at[0],
                y as f32 * GRID - at[1],
                z as f32 * GRID - at[2],
            ];
            (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
        })
        .fold(f32::INFINITY, f32::min)
}

/// Picks the entities to drop: tier by tier, farthest from the players first,
/// until `need` are chosen. Returns them with their tier and distance.
fn choose(s: &Survey, map: &MapInfo, need: usize) -> Vec<(u16, char, f32)> {
    let mut pool: Vec<(u16, char, f32)> = s
        .entities
        .iter()
        .filter_map(|(&i, e)| tier(s, map, i, e).map(|t| (i, t, distance_from_players(s, map, e))))
        .collect();
    pool.sort_by(|a, b| a.1.cmp(&b.1).then(b.2.total_cmp(&a.2)).then(a.0.cmp(&b.0)));
    pool.truncate(need);
    pool
}

#[derive(Default)]
struct Tally {
    packets: usize,
    count_mismatch: usize,
    peak_before: usize,
    peak_after: usize,
    entries_dropped: usize,
}

/// Index encoding for a full snapshot entry: +1 by a flag, a 6-bit difference,
/// or an 11-bit absolute index, as the engine writes them.
fn encode_full(e: &mut dem::types::EntityState, prev: u16) {
    let diff = e.entity_index.wrapping_sub(prev);
    e.increment_entity_number = diff == 1;
    if diff == 1 {
        e.is_absolute_entity_index = None;
        e.absolute_entity_index = None;
        e.entity_index_difference = None;
    } else if diff < 64 {
        e.is_absolute_entity_index = Some(false);
        e.absolute_entity_index = None;
        e.entity_index_difference = Some(dem::nbit_num!(diff, 6));
    } else {
        e.is_absolute_entity_index = Some(true);
        e.absolute_entity_index = Some(dem::nbit_num!(e.entity_index, 11));
        e.entity_index_difference = None;
    }
}

/// Index encoding for a delta entry: a 6-bit difference or an 11-bit absolute.
fn encode_delta(e: &mut dem::types::EntityStateDelta, prev: u16) {
    let diff = e.entity_index.wrapping_sub(prev);
    if diff < 64 {
        e.is_absolute_entity_index = false;
        e.absolute_entity_index = None;
        e.entity_index_difference = Some(dem::nbit_num!(diff, 6));
    } else {
        e.is_absolute_entity_index = true;
        e.absolute_entity_index = Some(dem::nbit_num!(e.entity_index, 11));
        e.entity_index_difference = None;
    }
}

fn has_packet_entities(msgs: &[NetMessage]) -> bool {
    msgs.iter().any(|m| {
        matches!(m, NetMessage::EngineMessage(em) if matches!(&**em,
            EngineMessage::SvcPacketEntities(_) | EngineMessage::SvcDeltaPacketEntities(_)))
    })
}

/// Rewrites one packet-entities message without the dropped entities. Returns
/// (declared count before, carried count before, count after, entries dropped).
fn trim_message(
    em: &mut EngineMessage,
    seq: i32,
    drop: &HashSet<u16>,
    carry: &mut Carry,
) -> Option<(usize, usize, usize, usize)> {
    match em {
        EngineMessage::SvcPacketEntities(pe) => {
            let declared = pe.entity_count.to_u32() as usize;
            let set = carry.full(seq, pe.entity_states.iter().map(|e| e.entity_index));
            let carried = set.len();
            // The declared count less the dropped entities in this snapshot:
            // equal to the carried set's size wherever the original agreed
            // with it, and leaves any original disagreement as it was.
            let after = declared.saturating_sub(set.iter().filter(|i| drop.contains(i)).count());
            let before_len = pe.entity_states.len();
            pe.entity_states.retain(|e| !drop.contains(&e.entity_index));
            let dropped = before_len - pe.entity_states.len();
            if dropped > 0 {
                let mut prev = 0u16;
                for e in &mut pe.entity_states {
                    encode_full(e, prev);
                    prev = e.entity_index;
                }
            }
            if !drop.is_empty() && after != declared {
                pe.entity_count = dem::nbit_num!(after as u32, 16);
            }
            Some((declared, carried, after, dropped))
        }
        EngineMessage::SvcDeltaPacketEntities(dpe) => {
            let declared = dpe.entity_count.to_u32() as usize;
            let from = dpe.delta_sequence.to_u8();
            let entries: Vec<(u16, bool)> = dpe
                .entity_states
                .iter()
                .map(|e| (e.entity_index, e.remove_entity))
                .collect();
            let (carried, present) = match carry.delta(seq, from, &entries) {
                Some(set) => (set.len(), set.iter().filter(|i| drop.contains(i)).count()),
                // The engine throws this packet away, so its count is never used.
                None => (declared, 0),
            };
            // The declared count less the dropped entities in this snapshot:
            // equal to the carried set's size wherever the original agreed
            // with it, and leaves any original disagreement as it was.
            let after = declared.saturating_sub(present);
            let before_len = dpe.entity_states.len();
            dpe.entity_states
                .retain(|e| !drop.contains(&e.entity_index));
            let dropped = before_len - dpe.entity_states.len();
            if dropped > 0 {
                let mut prev = 0u16;
                for e in &mut dpe.entity_states {
                    encode_delta(e, prev);
                    prev = e.entity_index;
                }
            }
            if !drop.is_empty() && after != declared {
                dpe.entity_count = dem::nbit_num!(after as u32, 16);
            }
            Some((declared, carried, after, dropped))
        }
        _ => None,
    }
}

/// Walks every packet-entities message, trimming when `drop` is non-empty.
/// With an empty set it only measures, which is how the output is checked.
fn walk(demo: &mut Demo, drop: &HashSet<u16>) -> (Tally, Carry) {
    let mut tally = Tally::default();
    let mut carry = Carry::default();
    for entry in demo.directory.entries.iter_mut() {
        for frame in entry.frames.iter_mut() {
            let FrameData::NetworkMessage(bt) = &mut frame.frame_data else {
                continue;
            };
            let seq = bt.1.sequence_info.incoming_sequence;
            let touched =
                matches!(&bt.1.messages, MessageData::Parsed(m) if has_packet_entities(m));
            if !touched {
                continue;
            }
            let msgs = if drop.is_empty() {
                &mut bt.1.messages
            } else {
                bt.1.messages_mut()
            };
            let MessageData::Parsed(msgs) = msgs else {
                continue;
            };
            for m in msgs.iter_mut() {
                let NetMessage::EngineMessage(em) = m else {
                    continue;
                };
                if let Some((declared, carried, after, dropped)) =
                    trim_message(em, seq, drop, &mut carry)
                {
                    tally.packets += 1;
                    tally.count_mismatch += usize::from(declared != carried);
                    tally.peak_before = tally.peak_before.max(declared);
                    tally.peak_after = tally.peak_after.max(after);
                    tally.entries_dropped += dropped;
                }
            }
        }
    }
    (tally, carry)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let usage = "usage: packet_entity_trim <in.dem> <out.dem> [target] [map.bsp]";
    let input = args.next().expect(usage);
    let output = args.next().expect(usage);
    let target: usize = args
        .next()
        .map(|t| t.parse().expect("target is a number"))
        .unwrap_or(DEFAULT_TARGET);
    let bsp = args.next();
    if std::path::Path::new(&output).exists() {
        eprintln!("{output} already exists; this never overwrites");
        std::process::exit(2);
    }

    let bytes = std::fs::read(&input).expect("read demo");
    let mut demo = open_demo_from_bytes(&bytes).expect("parse demo");
    let s = survey(&demo);
    let (measure, carry) = walk(&mut demo, &HashSet::new());
    println!("input: {input}");
    println!(
        "  {} packet-entity messages, peak declared {} (limit {LEGACY_LIMIT}, target {target}); declared != carried in {}; delta base missing {}",
        measure.packets, measure.peak_before, measure.count_mismatch, carry.missing_base
    );
    println!(
        "  {} player positions harvested ({}-unit cells)",
        s.player_cells.len(),
        GRID
    );

    let need = measure.peak_before.saturating_sub(target);
    let map = bsp.as_deref().map(read_map).unwrap_or_default();
    let chosen = choose(&s, &map, need);
    let mut by_tier: HashMap<char, usize> = HashMap::new();
    for (_, t, _) in &chosen {
        *by_tier.entry(*t).or_default() += 1;
    }
    println!(
        "  need to drop {need}; chose {} (A invisible {}, B sprites {}, C func_illusionary {})",
        chosen.len(),
        by_tier.get(&'A').unwrap_or(&0),
        by_tier.get(&'B').unwrap_or(&0),
        by_tier.get(&'C').unwrap_or(&0)
    );
    for (i, t, d) in &chosen {
        let e = &s.entities[i];
        let model = model_name(&s, e);
        let class = model
            .strip_prefix('*')
            .and_then(|n| n.parse::<usize>().ok())
            .and_then(|n| map.classes.get(&n))
            .map(String::as_str)
            .unwrap_or("");
        println!(
            "    drop ent {i:>4} tier {t} {model:<24} {class:<18} rendermode {} renderamt {:<3}  {:.0} units from the nearest player position",
            field_i32(&e.fields, "rendermode").unwrap_or(0),
            field_i32(&e.fields, "renderamt").unwrap_or(0),
            d
        );
    }
    if chosen.len() < need {
        eprintln!("not enough safe entities to reach {target}; nothing written");
        std::process::exit(1);
    }

    let drop: HashSet<u16> = chosen.iter().map(|c| c.0).collect();
    let (trim, _) = walk(&mut demo, &drop);
    println!(
        "  trimmed: {} entries dropped, peak {} -> {}",
        trim.entries_dropped, trim.peak_before, trim.peak_after
    );
    let out = demo.write_to_bytes();
    std::fs::write(&output, &out).expect("write output");
    println!("wrote {output} ({} -> {} bytes)", bytes.len(), out.len());

    // Check the file as written, from scratch.
    let mut again = open_demo_from_bytes(&out).expect("output re-parses");
    let (check, carry) = walk(&mut again, &HashSet::new());
    let leaked = network_messages(&again)
        .filter_map(|m| match m {
            NetMessage::EngineMessage(em) => Some(em),
            _ => None,
        })
        .map(|em| match &**em {
            EngineMessage::SvcPacketEntities(pe) => pe
                .entity_states
                .iter()
                .filter(|e| drop.contains(&e.entity_index))
                .count(),
            EngineMessage::SvcDeltaPacketEntities(d) => d
                .entity_states
                .iter()
                .filter(|e| drop.contains(&e.entity_index))
                .count(),
            _ => 0,
        })
        .sum::<usize>();
    println!(
        "check: {} packets, peak declared {}, declared != carried in {}, delta base missing {}, dropped entities still listed {}",
        check.packets, check.peak_before, check.count_mismatch, carry.missing_base, leaked
    );
    let ok = check.peak_before <= LEGACY_LIMIT
        && check.count_mismatch <= measure.count_mismatch
        && leaked == 0;
    println!("{}", if ok { "OK" } else { "CHECK FAILED" });
    std::process::exit(if ok { 0 } else { 1 });
}
