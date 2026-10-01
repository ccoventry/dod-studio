//! Does an HLTV recording carry any trace of a shot whose fire event it
//! dropped?
//!
//! `hltv_shot_gap_probe` shows an HLTV demo losing a share of automatic fire
//! (gaps at exact multiples of the cyclic rate). The sound for a dropped round
//! can only be put back if something else in the stream still says the round
//! was fired. The candidate is the shooter's own replicated body state: the
//! server restarts the attack animation on every shot, so `frame` in
//! `entity_state_player_t` falls back to the start per round, and it is
//! carried in nearly every player update.
//!
//! So this pairs two things per player and compares them:
//!
//! - **fire events**, attributed through `packet_index` the way the client
//!   does it (the n-th entity of the frame's packet-entity list), or through
//!   the event's own `entindex` when it carries one;
//! - **restarts** of the body animation: `frame` going backwards, or the
//!   sequence changing.
//!
//! A restart in a bullet weapon's `*_shoot` sequence with no event beside it
//! is a round the recording dropped. The kill feed checks that: a kill by
//! gunfire is a shot that certainly happened.
//!
//! It also prints what a stand-in for a lost event would have to carry: the
//! integer and bool arguments recorded events have, per weapon, and whether
//! counting rounds since a Garand's reload says which one pings.
//!
//!     cargo run --release -p analysis --example hltv_shot_evidence_probe -- <demo>
//!     cargo run --release -p analysis --example hltv_shot_evidence_probe -- <demo> <from> <to> [entity]
//!
//!     cargo run --release -p analysis --example hltv_shot_evidence_probe -- <demo> --list
//!
//! The second form prints the raw timeline for a window instead; the third
//! lists every round with no event (time, player), to hold a session of the
//! hook's `dodstudio_hltv_play_missing_gunshots 2` log against.

use dem::bit::BitSliceCast;
use dem::open_demo_from_bytes;
use dem::types::{Delta, EngineMessage, FrameData, MessageData, NetMessage};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

const MAX_PLAYERS: u16 = 32;
/// The body-state fields worth a line in the timeline.
const WATCHED: &[&str] = &["sequence", "frame", "gaitsequence", "weaponmodel"];
/// An event and a restart this close together are the same shot. HLTV frames
/// are ~33ms apart, and the two ride in the same one.
const SAME_SHOT: f32 = 0.04;

fn clean(name: &str) -> &str {
    name.trim_matches(|c: char| c == '\0' || c.is_whitespace())
}

fn field<'a>(delta: &'a Delta, name: &str) -> Option<&'a [u8]> {
    delta
        .iter()
        .find(|(k, _)| clean(k) == name)
        .map(|(_, v)| v.as_slice())
        .filter(|v| v.len() >= 4)
}

fn field_u32(delta: &Delta, name: &str) -> Option<u32> {
    field(delta, name).map(|v| u32::from_le_bytes([v[0], v[1], v[2], v[3]]))
}

fn field_f32(delta: &Delta, name: &str) -> Option<f32> {
    field(delta, name).map(|v| f32::from_le_bytes([v[0], v[1], v[2], v[3]]))
}

/// A delta field as text: integers as-is, anything that reads as a sane float
/// as that, since the decoder hands back raw bytes for both.
fn show(value: &[u8]) -> String {
    if value.len() >= 4 {
        let raw = [value[0], value[1], value[2], value[3]];
        let (int, float) = (u32::from_le_bytes(raw), f32::from_le_bytes(raw));
        if float.is_finite() && float.abs() > 1e-3 && float.abs() < 1e7 {
            format!("{float:.3}")
        } else {
            format!("{int}")
        }
    } else {
        format!("{value:?}")
    }
}

/// One player's animation restarts and the fire events attributed to them.
#[derive(Default)]
struct Player {
    sequence: u32,
    frame: f32,
    /// (time, sequence the restart happened in, the sequence changed)
    restarts: Vec<(f32, u32, bool)>,
    /// (time, weapon)
    events: Vec<(f32, String)>,
    /// Rifle rounds since the body last started a Garand reload, or `None`
    /// before the first one seen.
    since_reload: Option<u32>,
    /// (time, which round of the clip) for each rifle round after a reload.
    clip_rounds: Vec<(f32, u32)>,
    /// (time, bparam1) of this player's Garand shots that have an event.
    garand_events: Vec<(f32, u32)>,
}

impl Player {
    fn update(&mut self, delta: &Delta, time: f32) {
        let new_sequence = field_u32(delta, "sequence");
        let new_frame = field_f32(delta, "frame");
        let changed = new_sequence.is_some_and(|s| s != self.sequence);
        // Any step backwards. A looping sequence wraps on its own, but those
        // are not firing sequences and are filtered out below by never
        // coinciding with a fire event; a firing sequence does not loop, so
        // it only goes backwards when the server restarts it.
        let rewound = new_frame.is_some_and(|f| f < self.frame);
        if let Some(s) = new_sequence {
            self.sequence = s;
        }
        if let Some(f) = new_frame {
            self.frame = f;
        }
        if changed || rewound {
            self.restarts.push((time, self.sequence, changed));
            // stand/crouch/prone_garand_reload, or a death (19-29, 341-344:
            // the respawn brings a full clip); then the rifle `*_shoot` five.
            if changed && matches!(self.sequence, 239 | 266 | 315 | 19..=29 | 341..=344) {
                self.since_reload = Some(0);
            } else if matches!(self.sequence, 30..=32 | 36..=37)
                && let Some(n) = &mut self.since_reload
            {
                *n += 1;
                self.clip_rounds.push((time, *n));
            }
        }
    }
}

/// The `*_shoot` body sequences of the bullet weapons, by index, read out of
/// `models/player/us-inf/us-inf.mdl` (345 sequences; the five player models
/// share the numbering): rifle 30-32 and 36-37, then bolt, tommy, fcarb, mp40,
/// stg44, sten, grease, pistol, mg, bren and 30cal as 41-81. 33-35 are the
/// rifle-butt swing and 38-40 the bayonet stab; 82 onwards are grenades,
/// blades and rockets, whose events this does not try to account for.
struct Bullets;

impl Bullets {
    fn contains(&self, sequence: &u32) -> bool {
        matches!(sequence, 30..=32 | 36..=37 | 41..=81)
    }
}

/// The event scripts that go with those sequences: every weapon script but
/// these.
const NOT_BULLETS: &[&str] = &["melee", "knife", "bazooka", "pschreck", "piat", "mortar"];

fn short_weapon(event_name: &str) -> String {
    event_name
        .trim_start_matches("events/weapons/")
        .trim_end_matches(".sc")
        .to_string()
}

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .expect("usage: hltv_shot_evidence_probe <demo> [from to [entity]]");
    let list = std::env::args().any(|a| a == "--list");
    let window: Option<(f32, f32)> = args
        .next()
        .and_then(|a| a.parse().ok())
        .zip(args.next().and_then(|a| a.parse().ok()));
    let only: Option<u16> = args.next().and_then(|a| a.parse().ok());
    let in_window = |time: f32| window.is_some_and(|(from, to)| time >= from && time <= to);
    let bytes = std::fs::read(&path).expect("read demo");
    let demo = open_demo_from_bytes(&bytes).expect("parse demo");

    let mut event_names: HashMap<u32, String> = HashMap::new();
    // What the frame's packet-entity list holds, in entity order: an event's
    // `packet_index` counts into it.
    let mut in_packet: BTreeSet<u16> = BTreeSet::new();
    let mut players: BTreeMap<u16, Player> = BTreeMap::new();
    let (mut by_index, mut by_entindex, mut unresolved) = (0usize, 0usize, 0usize);

    // (time, killer's entity index): a kill by gunfire needs a shot just before it.
    let mut kills: Vec<(f32, u16)> = Vec::new();
    // weapon -> (iparam1, iparam2, bparam1, bparam2) -> how many events: what
    // a stand-in for a lost event has to pass to look like a recorded one.
    let mut event_args: BTreeMap<String, BTreeMap<[u32; 4], usize>> = BTreeMap::new();

    let note = |entity: u16, delta: &Delta, time: f32, players: &mut BTreeMap<u16, Player>| {
        if entity == 0 || entity > MAX_PLAYERS {
            return;
        }
        players.entry(entity).or_default().update(delta, time);
        if !in_window(time) || only.is_some_and(|e| e != entity) {
            return;
        }
        let mut shown: Vec<String> = delta
            .iter()
            .filter(|(k, _)| WATCHED.contains(&clean(k)))
            .map(|(k, v)| format!("{}={}", clean(k), show(v)))
            .collect();
        if !shown.is_empty() {
            shown.sort();
            println!("{time:9.3}  entity {entity:2}  {}", shown.join("  "));
        }
    };

    for entry in &demo.directory.entries {
        for frame in &entry.frames {
            let FrameData::NetworkMessage(bt) = &frame.frame_data else {
                continue;
            };
            let MessageData::Parsed(msgs) = &bt.1.messages else {
                continue;
            };
            for m in msgs {
                let em = match m {
                    NetMessage::EngineMessage(em) => em,
                    NetMessage::UserMessage(um) => {
                        // DeathMsg: u8 killer, u8 victim, u8 weapon.
                        if um.name.starts_with(b"DeathMsg")
                            && let [killer, victim, ..] = um.data[..]
                            && killer != 0
                            && killer != victim
                        {
                            kills.push((frame.time, killer as u16));
                        }
                        continue;
                    }
                };
                match &**em {
                    EngineMessage::SvcResourceList(rl) => {
                        for r in &rl.resources {
                            if r.type_.to_u8() == 5 {
                                event_names.insert(
                                    r.index.to_u32(),
                                    clean(&r.name.get_string()).to_string(),
                                );
                            }
                        }
                    }
                    EngineMessage::SvcPacketEntities(pe) => {
                        in_packet.clear();
                        for es in &pe.entity_states {
                            in_packet.insert(es.entity_index);
                            note(es.entity_index, &es.delta, frame.time, &mut players);
                        }
                    }
                    EngineMessage::SvcDeltaPacketEntities(pe) => {
                        for es in &pe.entity_states {
                            if es.remove_entity {
                                in_packet.remove(&es.entity_index);
                                continue;
                            }
                            in_packet.insert(es.entity_index);
                            if let Some(delta) = &es.delta {
                                note(es.entity_index, delta, frame.time, &mut players);
                            }
                        }
                    }
                    EngineMessage::SvcEvent(ev) => {
                        for e in &ev.events {
                            let Some(name) = event_names.get(&e.event_index.to_u32()) else {
                                continue;
                            };
                            if !name.contains("events/weapons/") {
                                continue;
                            }
                            // The event's own entindex wins; it is only sent
                            // when the shooter is not in the packet list.
                            let named = e
                                .delta
                                .as_ref()
                                .and_then(|d| field_u32(d, "entindex"))
                                .filter(|v| *v > 0 && *v <= MAX_PLAYERS as u32)
                                .map(|v| v as u16);
                            let indexed = e
                                .packet_index
                                .as_ref()
                                .and_then(|b| in_packet.iter().nth(b.to_u32() as usize).copied());
                            let shooter = match (named, indexed) {
                                (Some(s), _) => {
                                    by_entindex += 1;
                                    Some(s)
                                }
                                (None, Some(s)) => {
                                    by_index += 1;
                                    Some(s)
                                }
                                (None, None) => {
                                    unresolved += 1;
                                    None
                                }
                            };
                            if in_window(frame.time) {
                                println!(
                                    "{:9.3}  EVENT {}  packet_index {:?} -> entity {shooter:?}",
                                    frame.time,
                                    short_weapon(name),
                                    e.packet_index.as_ref().map(|b| b.to_u32()),
                                );
                            }
                            let small = |field_name: &str| {
                                e.delta
                                    .as_ref()
                                    .and_then(|d| d.iter().find(|(k, _)| clean(k) == field_name))
                                    .map(|(_, v)| {
                                        v.iter()
                                            .take(4)
                                            .enumerate()
                                            .fold(0u32, |n, (i, b)| n | (*b as u32) << (8 * i))
                                    })
                                    .unwrap_or(0)
                            };
                            *event_args
                                .entry(short_weapon(name))
                                .or_default()
                                .entry([
                                    small("iparam1"),
                                    small("iparam2"),
                                    small("bparam1"),
                                    small("bparam2"),
                                ])
                                .or_insert(0) += 1;
                            if let Some(s) = shooter.filter(|s| *s <= MAX_PLAYERS)
                                && short_weapon(name) == "garand"
                                && small("iparam1") == 0
                            {
                                players
                                    .entry(s)
                                    .or_default()
                                    .garand_events
                                    .push((frame.time, small("bparam1")));
                            }
                            if let Some(s) = shooter.filter(|s| *s <= MAX_PLAYERS) {
                                players
                                    .entry(s)
                                    .or_default()
                                    .events
                                    .push((frame.time, short_weapon(name)));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    if window.is_some() {
        return;
    }

    println!("=== {path} ===");
    println!(
        "fire events: {by_index} attributed by packet_index, {by_entindex} by their own entindex, {unresolved} unresolved\n"
    );

    // A firing sequence is one whose restarts coincide with fire events. Count
    // both ways per sequence first, then keep the ones that clearly are.
    let near = |events: &[(f32, String)], time: f32| -> Option<usize> {
        events
            .iter()
            .position(|(t, _)| (t - time).abs() <= SAME_SHOT)
    };
    let mut per_sequence: HashMap<u32, (usize, usize)> = HashMap::new();
    for p in players.values() {
        for (time, sequence, _) in &p.restarts {
            let row = per_sequence.entry(*sequence).or_default();
            row.0 += 1;
            if near(&p.events, *time).is_some() {
                row.1 += 1;
            }
        }
    }
    // Learned as a check on the fixed list below: a sequence fires if a real
    // share of its restarts carry an event -- the rest being exactly the
    // drops this is looking for, the bar cannot be high.
    let learned: HashSet<u32> = per_sequence
        .iter()
        .filter(|(_, (restarts, with_event))| *with_event >= 5 && *with_event * 3 >= *restarts)
        .map(|(s, _)| *s)
        .collect();
    let firing = Bullets;

    #[derive(Default)]
    struct Row {
        events: usize,
        events_with_restart: usize,
        silent: usize,
    }
    let mut rows: BTreeMap<String, Row> = BTreeMap::new();
    let mut silent_times: Vec<(f32, u16, String)> = Vec::new();
    let mut orphans: Vec<(f32, u16, String)> = Vec::new();
    for (entity, p) in &players {
        for (time, weapon) in &p.events {
            if NOT_BULLETS.contains(&weapon.as_str()) {
                continue;
            }
            let row = rows.entry(weapon.clone()).or_default();
            row.events += 1;
            if p.restarts
                .iter()
                .any(|(t, _, _)| (t - time).abs() <= SAME_SHOT)
            {
                row.events_with_restart += 1;
            } else {
                orphans.push((*time, *entity, weapon.clone()));
            }
        }
        for (time, sequence, _) in &p.restarts {
            if !firing.contains(sequence) || near(&p.events, *time).is_some() {
                continue;
            }
            // Which weapon: the one this player last fired, or fires next.
            let weapon = p
                .events
                .iter()
                .filter(|(t, _)| (t - time).abs() < 5.0)
                .min_by(|a, b| (a.0 - time).abs().total_cmp(&(b.0 - time).abs()))
                .map(|(_, w)| w.clone())
                .unwrap_or_else(|| "(no event within 5s)".to_string());
            rows.entry(weapon.clone()).or_default().silent += 1;
            silent_times.push((*time, *entity, weapon));
        }
    }

    println!(
        "{:<22} {:>7} {:>13} {:>16} {:>7}",
        "weapon", "events", "with restart", "silent restarts", "loss"
    );
    let (mut events, mut matched, mut silent) = (0usize, 0usize, 0usize);
    let mut sorted: Vec<(&String, &Row)> = rows.iter().collect();
    sorted.sort_by_key(|(_, r)| std::cmp::Reverse(r.events));
    for (weapon, r) in sorted {
        println!(
            "{weapon:<22} {:>7} {:>13} {:>16} {:>6.1}%",
            r.events,
            r.events_with_restart,
            r.silent,
            100.0 * r.silent as f32 / (r.events + r.silent).max(1) as f32
        );
        events += r.events;
        matched += r.events_with_restart;
        silent += r.silent;
    }
    println!(
        "\ntotal: {events} events, {matched} with a restart beside them ({:.1}%); {silent} restarts in a firing sequence with no event ({:.1}% of all rounds)",
        100.0 * matched as f32 / events.max(1) as f32,
        100.0 * silent as f32 / (events + silent).max(1) as f32
    );
    // Both kinds of restart should lose their event equally often if both are
    // rounds: what drops an event has nothing to do with how the body shows it.
    let mut kinds = [(0usize, 0usize); 2];
    for p in players.values() {
        for (time, sequence, changed) in &p.restarts {
            if firing.contains(sequence) {
                let kind = &mut kinds[usize::from(*changed)];
                kind.0 += 1;
                if near(&p.events, *time).is_some() {
                    kind.1 += 1;
                }
            }
        }
    }
    println!(
        "restarts by kind: frame stepped back {} ({} with an event), sequence changed {} ({} with an event)",
        kinds[0].0, kinds[0].1, kinds[1].0, kinds[1].1
    );
    println!("sequences whose restarts coincide with events: {}", {
        let mut list: Vec<u32> = learned.iter().copied().collect();
        list.sort();
        format!("{list:?}")
    });

    // The check on the method: a kill is a shot that certainly happened. If
    // restarts are real rounds, a kill with no event should still have one.
    let (mut both, mut event_only, mut restart_only, mut neither) =
        (0usize, 0usize, 0usize, 0usize);
    for (time, killer) in &kills {
        let before = |t: f32| t >= time - 0.25 && t <= time + 0.05;
        let Some(p) = players.get(killer) else {
            neither += 1;
            continue;
        };
        let event = p.events.iter().any(|(t, _)| before(*t));
        let restart = p
            .restarts
            .iter()
            .any(|(t, s, _)| before(*t) && firing.contains(s));
        match (event, restart) {
            (true, true) => both += 1,
            (true, false) => event_only += 1,
            (false, true) => restart_only += 1,
            (false, false) => neither += 1,
        }
    }
    println!(
        "\nkills ({}): the killer's shot in the 0.25s before -- event and restart {both}, event only {event_only}, restart only {restart_only}, neither {neither} (grenades, and anything missed)",
        kills.len()
    );

    println!("\nevent arguments by weapon (iparam1, iparam2, bparam1, bparam2): events");
    for (weapon, combos) in &event_args {
        let shown: Vec<String> = combos
            .iter()
            .map(|(k, n)| format!("({}, {}, {}, {}): {n}", k[0], k[1], k[2], k[3]))
            .collect();
        println!("  {weapon:<14} {}", shown.join("   "));
    }

    // Can a Garand's last round be told without its event? bparam1 is 0 on
    // the round that pings and 1 on the rest; if the clip is always eight
    // after a reload, counting rounds since the body's reload says which.
    let mut by_round: BTreeMap<u32, [usize; 2]> = BTreeMap::new();
    for p in players.values() {
        for (time, bparam1) in &p.garand_events {
            if let Some((_, round)) = p
                .clip_rounds
                .iter()
                .find(|(t, _)| (t - time).abs() <= SAME_SHOT)
            {
                by_round.entry(*round).or_default()[usize::from(*bparam1 != 0)] += 1;
            }
        }
    }
    println!("\nGarand events by round since the reload: round -> pings (bparam1 0) / no ping");
    for (round, [ping, quiet]) in &by_round {
        println!("  round {round:>2}: {ping:>4} / {quiet}");
    }

    orphans.sort_by(|a, b| a.0.total_cmp(&b.0));
    println!("\nevents with no restart beside them: {}", orphans.len());
    for (time, entity, weapon) in orphans.iter().take(12) {
        println!("  {time:9.3}s  entity {entity:2}  {weapon}");
    }

    silent_times.sort_by(|a, b| a.0.total_cmp(&b.0));
    println!("\nsilent restarts -- rounds with no event, to check in a window:");
    let shown = if list { silent_times.len() } else { 15 };
    for (time, entity, weapon) in silent_times.iter().take(shown) {
        println!("  {time:9.3}s  entity {entity:2}  {weapon}");
    }
}
