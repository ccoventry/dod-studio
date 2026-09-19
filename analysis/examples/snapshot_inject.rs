//! Rebuild the client's entity table at a join by injecting a synthetic full
//! snapshot (#224, and the remaining jank in #15).
//!
//! A stitched demo plays, but the engine prints `WARNING: CL_FlushEntityPacket`
//! and world objects -- crates, doors, props -- go missing. The tail's deltas
//! describe changes to an entity table the client never built, so it discards
//! whole packets rather than applying them. Players keep moving because they are
//! re-sent constantly with absolute origins; a static crate gets one update,
//! dropped, and is never corrected.
//!
//! The fix is to give the client a complete table at the cut. This replays every
//! entity update in the prefix -- seeded from entry 0's `SvcSpawnBaseline`, so a
//! field set only at spawn and genuinely never resent survives too -- merging
//! each entity's fields into one accumulated state, and writes that out as a
//! full `SvcPacketEntities` at the join -- the message type the demo otherwise
//! only contains at t=0.
//!
//! **Two live-crash fixes are baked into how that packet is encoded**, both
//! found by testing in-engine rather than by offline checks (which all still
//! pass on the broken versions): entity 0 is dropped from the list entirely
//! rather than restated, and its entities are encoded with increment/difference
//! indexing rather than absolute, matching what real `SvcPacketEntities`
//! traffic actually does. See the comment just above where the snapshot is
//! built for the measurements behind both.
//!
//!     cargo run --release -p analysis --example snapshot_inject -- <in.dem> <out.dem>
use dem::bit::BitSliceCast;
use dem::open_demo_from_bytes;
use dem::types::{
    Delta, EngineMessage, EntityState, Frame, FrameData, MessageData, NetMessage,
    SvcClientData, SvcDeltaPacketEntities, SvcPacketEntities, SvcTime, UserMessage,
};
use std::collections::BTreeMap;

/// A join's `incoming_sequence` gap is smoothed into at most this many synthetic
/// no-op frames. Uncapped for `m3_h2` (gap 2000) and `m1_h1`/`m1_h2` (gaps
/// 582-2658) -- their whole point-to-point gap becomes single-sequence steps.
/// `m3_h1-1` carries two spurious back-to-back "joins" with gaps of 131,071 and
/// 135,094 (adjacent-frame artefacts of `multi_bridge`, not real holes); this
/// cap keeps those from turning into 100K+-frame files while still shrinking
/// their jump by two orders of magnitude.
const MAX_SMOOTH_FRAMES: usize = 3000;

#[derive(Clone, Default)]
struct EntAcc {
    fields: Delta,
    custom: bool,
}

/// The local viewing player's own clientdata, accumulated the same way `acc`
/// accumulates entity state -- merging every `SvcClientData` delta seen across
/// the prefix, so the join can start from "what the player's weapon/ammo/health
/// actually was" instead of nothing.
#[derive(Clone, Default)]
struct ClientAcc {
    fields: Delta,
    /// weapon_index -> accumulated per-weapon fields (ammo, clip, etc).
    weapons: BTreeMap<u32, Delta>,
    /// The most recent `CurWeapon` user message with `is_active=true` seen in
    /// the prefix, as (id, raw 3-byte payload) -- `id` is resolved per-file
    /// from this demo's own `SvcNewUserMsg` table, so it can't be hardcoded.
    /// `SvcClientData`'s `weapon_data` map (accumulated above) says what ammo
    /// each weapon the player has touched holds; it does not say which one is
    /// currently in hand -- that is this message alone. Replaying only the
    /// entity/clientdata state at a join left the engine using whatever
    /// weapon `CurWeapon` last named *before* the gap, playing that weapon's
    /// fire sound over the correct one until real gameplay happened to touch
    /// it again.
    cur_weapon: Option<(u8, Vec<u8>)>,
}

fn main() {
    let mut a = std::env::args().skip(1);
    let input = a.next().expect("usage: snapshot_inject <in.dem> <out.dem>");
    let output = a.next().expect("out.dem");
    let bytes = std::fs::read(&input).expect("read");
    let mut demo = open_demo_from_bytes(&bytes).expect("parse");

    // Find the join by the jump in `incoming_sequence`, not in frame time.
    // Frame time is not monotonic across a whole recording -- `m1_h2` resets to
    // 0.00s inside its intact prefix -- and the largest forward time jump then
    // lands at the start of playback rather than at the cut, which replays no
    // entities at all and injects an empty snapshot over a healthy packet.
    // `incoming_sequence` advances by one per network frame and jumps by
    // thousands at a splice, so it names the join whatever the clock does.
    // And find *every* join, not just the largest: `multi_bridge` splices a demo
    // once per hole, so `m1_h1` carries two and `m1_h2` three. A snapshot at only
    // one of them leaves the others resolving deltas against entity state the
    // client never built.
    let mut joins: Vec<(usize, f32, i64)> = Vec::new();
    // Parallel to `joins`: the last real (incoming_sequence, delta_sequence)
    // seen strictly before each join, so the smoothing ramp can start exactly
    // where the prefix's own numbering left off rather than guessing at it.
    let mut join_prev_seq: Vec<i32> = Vec::new();
    let mut join_prev_delta_seq: Vec<u32> = Vec::new();
    {
        let mut idx = 0usize;
        let mut prev: Option<(i32, f32)> = None;
        let mut last_delta_seq = 0u32;
        for entry in demo.directory.entries.iter().skip(1) {
            for f in &entry.frames {
                if let FrameData::NetworkMessage(bt) = &f.frame_data {
                    let seq = bt.1.sequence_info.incoming_sequence;
                    if let Some((p, pt)) = prev {
                        // A network frame advances the sequence by one; a splice
                        // moves it by thousands. Anything past a handful is a cut.
                        let jump = (seq as i64 - p as i64).abs();
                        if jump > 100 {
                            joins.push((idx, pt, jump));
                            join_prev_seq.push(p);
                            join_prev_delta_seq.push(last_delta_seq);
                        }
                    }
                    prev = Some((seq, f.time));
                    if let MessageData::Parsed(msgs) = &bt.1.messages {
                        for m in msgs {
                            if let NetMessage::EngineMessage(em) = m {
                                if let EngineMessage::SvcDeltaPacketEntities(pe) = &**em {
                                    last_delta_seq = pe.delta_sequence.to_u32();
                                }
                            }
                        }
                    }
                }
                idx += 1;
            }
        }
    }
    if joins.is_empty() { println!("no join found -- nothing to inject"); return }
    println!("{} join(s) found:", joins.len());
    for (i, t, j) in &joins { println!("  frame {i} (t={t:.2}s), incoming_sequence jumps by {j}") }
    // Replay the whole demo once, merging every entity's fields into one state,
    // and take a copy of that state as each join goes past. Replaying up to each
    // join separately would be the same work over again per join.
    //
    // Seeded from entry 0's SvcSpawnBaseline first, not left empty. A field set
    // only at spawn and genuinely never resent -- worldspawn's modelindex is the
    // case that surfaced this: entity 0 moves and changes for nobody, so no
    // delta in the whole recording ever touches it again -- would otherwise be
    // absent from every injected snapshot. The client does not re-seed a full
    // update from the baseline on its own; it instantiates the entity from
    // exactly the fields the packet lists. Missing modelindex on entity 0 is
    // `SV_LinkEdict`'s "Tried to link edict 0 without model", spammed once per
    // tick from the join onward -- measured live: 1,658 occurrences, then the
    // process gone with no crash dialog.
    let mut acc: BTreeMap<u16, EntAcc> = BTreeMap::new();
    if let Some(entry0) = demo.directory.entries.first() {
        for f in &entry0.frames {
            let FrameData::NetworkMessage(bt) = &f.frame_data else { continue };
            let MessageData::Parsed(msgs) = &bt.1.messages else { continue };
            for m in msgs {
                let NetMessage::EngineMessage(em) = m else { continue };
                let EngineMessage::SvcSpawnBaseline(sb) = &**em else { continue };
                for es in &sb.entities {
                    let e = acc.entry(es.entity_index).or_default();
                    for (k, v) in es.delta.iter() { e.fields.insert(k.clone(), v.clone()); }
                }
            }
        }
    }
    let baseline_seeded = acc.len();
    let mut at_join: Vec<BTreeMap<u16, EntAcc>> = Vec::new();
    // Mirrors `acc`/`at_join`, but for the local player's own clientdata rather
    // than entity state -- the wrong-weapon-sound bug found live: for several
    // seconds after a join, the game predicted gunfire for a weapon the player
    // was no longer holding. The entity snapshot reconstructs the world; this
    // reconstructs the ONE thing it does not cover, the viewing player's own
    // weapon/ammo/health state, which the smoothing ramp otherwise leaves
    // exactly where the prefix left it -- correct until the real weapon change
    // that happened inside the skipped hole finally gets communicated by
    // whatever real delta downstream happens to touch it.
    let mut client_acc = ClientAcc::default();
    let mut client_at_join: Vec<ClientAcc> = Vec::new();
    let mut next_join = 0usize;
    let mut updates = 0usize;
    let mut idx = 0usize;
    'replay: for entry in demo.directory.entries.iter().skip(1) {
        for f in &entry.frames {
            let here = idx;
            idx += 1;
            while next_join < joins.len() && here > joins[next_join].0 {
                at_join.push(acc.clone());
                client_at_join.push(client_acc.clone());
                next_join += 1;
            }
            if next_join >= joins.len() { break 'replay }
            let FrameData::NetworkMessage(bt) = &f.frame_data else { continue };
            let MessageData::Parsed(msgs) = &bt.1.messages else { continue };
            for m in msgs {
                let NetMessage::EngineMessage(em) = m else {
                    // `CurWeapon` payload is `[is_active: u8, weapon: u8, clip_ammo: u8]`
                    // (dod::lib.rs `cur_weapon`). Only the is_active=true half of a
                    // switch pair says what's actually in hand right now.
                    if let NetMessage::UserMessage(um) = m {
                        if um.name.starts_with(b"CurWeapon") && um.data.len() == 3 && um.data[0] != 0 {
                            client_acc.cur_weapon = Some((um.id, um.data.clone()));
                        }
                    }
                    continue;
                };
                match &**em {
                    EngineMessage::SvcClientData(cd) => {
                        for (k, v) in cd.client_data.iter() { client_acc.fields.insert(k.clone(), v.clone()); }
                        if let Some(weapons) = &cd.weapon_data {
                            for w in weapons {
                                let idx = w.weapon_index.to_u32();
                                let slot = client_acc.weapons.entry(idx).or_default();
                                for (k, v) in w.weapon_data.iter() { slot.insert(k.clone(), v.clone()); }
                            }
                        }
                    }
                    EngineMessage::SvcPacketEntities(pe) => {
                        for es in &pe.entity_states {
                            let e = acc.entry(es.entity_index).or_default();
                            e.custom = es.has_custom_delta;
                            for (k, v) in es.delta.iter() { e.fields.insert(k.clone(), v.clone()); }
                            updates += 1;
                        }
                    }
                    EngineMessage::SvcDeltaPacketEntities(pe) => {
                        for es in &pe.entity_states {
                            if es.remove_entity { acc.remove(&es.entity_index); continue }
                            let Some(d) = &es.delta else { continue };
                            let e = acc.entry(es.entity_index).or_default();
                            if let Some(c) = es.has_custom_delta { e.custom = c; }
                            for (k, v) in d.iter() { e.fields.insert(k.clone(), v.clone()); }
                            updates += 1;
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    while at_join.len() < joins.len() { at_join.push(acc.clone()) }
    while client_at_join.len() < joins.len() { client_at_join.push(client_acc.clone()) }
    println!("baseline seeded {baseline_seeded} entities; replayed {updates} entity updates on top");

    // Encode each join's state as a full snapshot. Absolute indices throughout:
    // unambiguous, and it avoids depending on the incremental index arithmetic
    // being right.
    // Two encoding choices below came from a live svc_bad crash (playdemo and
    // viewdemo both, same underlying cause) and are backed by measurement, not
    // guesswork -- checked across 3 healthy demos, ~1.65M real entity
    // observations combined:
    //
    // - entity_index 0 (world) appears in ZERO of them, full snapshots or
    //   deltas. The client keeps its signon baseline for it permanently; no
    //   real server ever restates it. So it is dropped here rather than
    //   encoded, matching real traffic exactly rather than inventing a
    //   representation nothing has ever validated.
    // - `is_absolute_entity_index = true` appears in 0 of 2,337 entities across
    //   every real *full* `SvcPacketEntities` sampled -- yet in ~4% of entities
    //   in real *delta* packets. So the 11-bit absolute-index path is a
    //   genuinely exercised, presumably-correct part of the wire format, but
    //   specifically for the message type we are NOT constructing here. The
    //   previous version of this code used absolute indexing for every single
    //   entity in the injected full snapshot, exercising a path with zero
    //   precedent for that message type -- and does not reach the parser via a
    //   normal read either, only the injector's own writer. Increment/
    //   difference encoding, computed against a running index starting at 0
    //   (dem-patch's own decoder, `packet_entities.rs`), is used instead,
    //   falling back to absolute only for a gap too wide for 6 bits (>63) --
    //   never observed in real full snapshots, but not structurally
    //   impossible for a set reconstructed from scratch rather than walked
    //   live by a server.
    let snapshots: Vec<SvcPacketEntities> = at_join.iter().enumerate().map(|(n, state)| {
        let live: Vec<(&u16, &EntAcc)> = state.iter().filter(|(idx, _)| **idx != 0).collect();
        let players = live.iter().filter(|(i, _)| **i >= 1 && **i <= 32).count();
        println!("  join {}: {} live entities ({players} player-slot, {} world, entity 0 excluded)",
            n + 1, live.len(), live.len() - players);

        let mut running: u32 = 0;
        let mut states: Vec<EntityState> = Vec::with_capacity(live.len());
        for (idx, e) in live {
            let idx32 = *idx as u32;
            let gap = idx32 - running; // ascending BTreeMap keys, entity 0 excluded: always > 0
            let (increment_entity_number, is_absolute_entity_index, absolute_entity_index, entity_index_difference) =
                if gap == 1 {
                    (true, None, None, None)
                } else if gap <= 63 {
                    (false, Some(false), None, Some(dem::nbit_num!(gap, 6)))
                } else {
                    (false, Some(true), Some(dem::nbit_num!(idx32, 11)), None)
                };
            running = idx32;
            states.push(EntityState {
                entity_index: *idx,
                increment_entity_number,
                is_absolute_entity_index,
                absolute_entity_index,
                entity_index_difference,
                has_custom_delta: e.custom,
                has_baseline_index: false,
                baseline_index: None,
                delta: e.fields.clone(),
            });
        }
        SvcPacketEntities {
            entity_count: dem::nbit_num!(states.len() as u32, 16),
            entity_states: states,
        }
    }).collect();
    let live_counts: Vec<u32> = at_join.iter()
        .map(|s| s.keys().filter(|i| **i != 0).count() as u32)
        .collect();

    // A live test found the snapshot lands cleanly but the client still pays a
    // real cost right there: `SV_LinkEdict`'s "Tried to link edict %i without
    // model" spammed ~1,657 times, and a ~30s stall, both at the join, and
    // identically whether or not the injected packet even lists entity 0 --
    // ruling out packet *content* as the cause. What every join has in common
    // is an instantaneous jump in both `incoming_sequence` (thousands) and
    // frame time (tens of real seconds), which nothing in a normal capture
    // ever produces -- the working theory is the client's local
    // physics/prediction system paying to catch up the elapsed time in one
    // burst. This inserts a ramp of ordinary-looking no-op frames spanning the
    // gap -- entity_count unchanged, zero entities listed, meaning "nothing
    // changed" -- so `incoming_sequence`, `delta_sequence` and frame time all
    // advance one ordinary step at a time instead of jumping, the same shape
    // a real recording never interrupts.
    //
    // `delta_sequence` is stepped in lockstep with `incoming_sequence` (not by
    // a fixed +1) so the gap `flush_predict`/CL_ParsePacketEntities checks
    // stays at whatever it already was in the prefix, not growing across the
    // ramp -- reopening the very flush condition this whole tool exists to
    // avoid would be a strange way to fix a different bug.
    fn build_ramp(
        template: &Frame,
        start_seq: i32, start_time: f32, start_delta_seq: u32,
        end_seq: i32, entity_count: u32,
        client_state: &ClientAcc,
    ) -> Vec<Frame> {
        let FrameData::NetworkMessage(tbt) = &template.frame_data else { return Vec::new() };
        let seq_gap = (end_seq as i64 - start_seq as i64).max(0);
        let n_steps = (seq_gap.saturating_sub(1) as usize).min(MAX_SMOOTH_FRAMES);
        if n_steps == 0 { return Vec::new() }
        // A no-op template for svc_clientdata (structure cloned, content zeroed).
        // The first live test crashed with "World::ParseClientData: couldn't
        // uncompress delta frame %i" (Core.dll) -- a *separate*, per-player
        // history ring from the entity one, keyed by frame number, that
        // `svc_clientdata` deltas against. Omitting it from every ramp frame
        // starved that ring for the whole gap; the real frame that finally
        // followed then tried to delta against a reference no longer in it.
        // Same failure mode as the original entity-flush bug, one layer over.
        let clientdata_template: Option<SvcClientData> =
            if let MessageData::Parsed(msgs) = &tbt.1.messages {
                msgs.iter().find_map(|m| match m {
                    NetMessage::EngineMessage(em) => match &**em {
                        EngineMessage::SvcClientData(cd) => Some(cd.clone()),
                        _ => None,
                    },
                    _ => None,
                })
            } else { None };
        // The no-op structure feeds the history ring, but empty content on
        // *every* frame leaves the client predicting whatever weapon/ammo it
        // had before the join, right through the ramp -- a live test caught
        // wrong gunfire sounds for several seconds, self-correcting once real
        // downstream content happened to touch weapon state. So the FIRST ramp
        // frame carries the real, replayed client state as a full refresh
        // (mirroring the one-time entity snapshot); every frame after it goes
        // back to empty, since the state is now actually established rather
        // than assumed.
        let full_client_data = client_state.fields.clone();
        let full_weapon_data: Option<Vec<dem::types::ClientDataWeaponData>> = if client_state.weapons.is_empty() {
            None
        } else {
            Some(client_state.weapons.iter().map(|(idx, fields)| dem::types::ClientDataWeaponData {
                weapon_index: dem::nbit_num!(*idx, 6),
                weapon_data: fields.clone(),
            }).collect())
        };
        // NOT `tbt.1.info.timestamp` -- that field is 0.0 on every frame checked
        // in this capture (confirmed directly, not assumed), unrelated to demo
        // playback time. The real per-frame time is the outer `Frame.time`.
        // Using the wrong one interpolated every ramp frame toward t=0 instead
        // of toward the join's real end time -- caught by `bridge_ceiling`
        // flagging exactly `n_steps` frames as stepping backwards in time.
        let end_time = template.time;
        (1..=n_steps).map(|i| {
            let frac = i as f64 / (n_steps + 1) as f64;
            let seq_i = start_seq as i64 + (frac * seq_gap as f64).round() as i64;
            let time_i = start_time + (end_time - start_time) * frac as f32;
            let delta_seq_i = (start_delta_seq as i64 + (frac * seq_gap as f64).round() as i64) as u32 & 0xff;
            // `info.timestamp` is left as the template's own value (0.0 in
            // every real frame checked) rather than set to `time_i` -- matching
            // real captures exactly rather than inventing a value nothing else
            // in the file has.
            let info = tbt.1.info.clone();
            let mut sequence_info = tbt.1.sequence_info.clone();
            sequence_info.incoming_sequence = seq_i as i32;
            let no_op = SvcDeltaPacketEntities {
                entity_count: dem::nbit_num!(entity_count, 16),
                delta_sequence: dem::nbit_num!(delta_seq_i, 8),
                entity_states: Vec::new(),
            };
            let mut msgs = vec![
                NetMessage::EngineMessage(Box::new(EngineMessage::SvcTime(SvcTime { time: time_i }))),
            ];
            if let Some(cd) = &clientdata_template {
                // Structural fields (has_delta_update_mask/delta_update_mask)
                // cloned as-is throughout. Content: the real accumulated state
                // on the first step only (a one-time refresh, same shape as the
                // entity snapshot), empty afterward -- "no further change from
                // what step 1 just established".
                let (client_data, weapon_data) = if i == 1 {
                    (full_client_data.clone(), full_weapon_data.clone())
                } else {
                    (Delta::new(), None)
                };
                msgs.push(NetMessage::EngineMessage(Box::new(EngineMessage::SvcClientData(SvcClientData {
                    has_delta_update_mask: cd.has_delta_update_mask,
                    delta_update_mask: cd.delta_update_mask.clone(),
                    client_data,
                    weapon_data,
                }))));
            }
            // Same one-time-refresh shape as the clientdata above: `CurWeapon`
            // is what actually drives which weapon's fire sound the client
            // predicts, and it is not part of `SvcClientData` at all -- a
            // separate user message. Replaying the last real one here, once,
            // stops the join from starting the client on a stale weapon.
            if i == 1 {
                if let Some((id, data)) = &client_state.cur_weapon {
                    msgs.push(NetMessage::UserMessage(UserMessage {
                        id: *id,
                        name: b"CurWeapon".to_vec(),
                        data: data.clone(),
                    }));
                }
            }
            msgs.push(NetMessage::EngineMessage(Box::new(EngineMessage::SvcDeltaPacketEntities(no_op))));
            Frame {
                time: time_i,
                // Unique and monotonic, continuing from the template's own
                // ordinal -- not `template.frame` repeated on every ramp frame.
                // `Core.dll`'s per-player clientdata history ring is keyed by
                // this number; ~2000 frames sharing one value is exactly the
                // kind of collision "couldn't uncompress delta frame %i" reports.
                frame: template.frame + i as i32,
                frame_data: FrameData::NetworkMessage(Box::new((
                    tbt.0.clone(),
                    dem::types::NetworkMessage { info, sequence_info, message_length: 0, messages: MessageData::Parsed(msgs) },
                ))),
            }
        }).collect()
    }

    // *Replace* the first post-join delta packet rather than sitting in front of
    // it. Both messages would share one frame, so they share one
    // incoming_sequence and one slot in the client's frame ring -- whichever is
    // parsed second is the state that survives. Prepending the snapshot means
    // the delta still lands on top of it, resolved against a ring slot that
    // holds a frame from the *prefix*, which is the state this whole probe
    // exists to stop the client from using.
    let mut injected = 0usize;
    let mut ramped = 0usize;
    let mut idx = 0usize;
    let mut ramp_frames_total = 0usize;
    for entry in demo.directory.entries.iter_mut().skip(1) {
        let mut new_frames: Vec<Frame> = Vec::with_capacity(entry.frames.len() + ramp_frames_total.max(64));
        for mut f in entry.frames.drain(..) {
            let here = idx;
            idx += 1;

            if ramped < joins.len() && here == joins[ramped].0 {
                let ramp = build_ramp(
                    &f,
                    join_prev_seq[ramped], joins[ramped].1, join_prev_delta_seq[ramped],
                    if let FrameData::NetworkMessage(bt) = &f.frame_data { bt.1.sequence_info.incoming_sequence } else { join_prev_seq[ramped] },
                    live_counts[ramped],
                    &client_at_join[ramped],
                );
                println!("  join {}: smoothed with {} synthetic no-op frames (gap was {})",
                    ramped + 1, ramp.len(), joins[ramped].2);
                ramp_frames_total += ramp.len();
                new_frames.extend(ramp);
                ramped += 1;
            }

            if injected < joins.len() && here > joins[injected].0 {
                if let FrameData::NetworkMessage(bt) = &mut f.frame_data {
                    if let MessageData::Parsed(msgs) = &mut bt.1.messages {
                        if let Some(at) = msgs.iter().position(|m| matches!(m, NetMessage::EngineMessage(em)
                            if matches!(**em, EngineMessage::SvcDeltaPacketEntities(_))))
                        {
                            msgs[at] = NetMessage::EngineMessage(Box::new(
                                EngineMessage::SvcPacketEntities(snapshots[injected].clone())));
                            println!("  join {}: replaced the first delta packet after it with its snapshot, at t={:.2}s",
                                injected + 1, f.time);
                            injected += 1;
                        }
                    }
                }
            }

            new_frames.push(f);
        }
        entry.frames = new_frames;
    }
    if injected < joins.len() {
        println!("only {injected} of {} joins got a snapshot -- no frame carrying entity data followed the rest",
            joins.len());
    }
    if injected == 0 { return }

    let out = demo.write_to_bytes();
    std::fs::write(&output, &out).expect("write");
    println!("wrote {output} ({:.1} MB)", out.len() as f64 / 1e6);
    match open_demo_from_bytes(&out) {
        Ok(d) => {
            let fulls: usize = d.directory.entries.iter().flat_map(|e| e.frames.iter())
                .filter_map(|f| if let FrameData::NetworkMessage(bt) = &f.frame_data {
                    if let MessageData::Parsed(ms) = &bt.1.messages { Some(ms) } else { None } } else { None })
                .flatten()
                .filter(|m| matches!(m, NetMessage::EngineMessage(em)
                    if matches!(**em, EngineMessage::SvcPacketEntities(_)))).count();
            println!("re-parse OK: {} frames, {fulls} full snapshots now present",
                d.directory.entries.iter().map(|e| e.frames.len()).sum::<usize>());
        }
        Err(e) => println!("re-parse FAILED: {e}"),
    }
    match analysis::Analysis::try_from_bytes(&out) {
        Ok(an) => println!("analysis OK: map={:?} players={}", an.state.initial_map_name, an.state.players.len()),
        Err(e) => println!("analysis failed: {e}"),
    }
}
