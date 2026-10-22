//! Does a demo carry *any* signal at the moment a player pulls a grenade pin?
//!
//! `grenade_timing_probe` established that the pin pull is played client-side
//! for the local player only: the third-person grenade models have one
//! sequence each, and `weapons/grenpinpull.wav` is in no demo's `svc_sound`
//! stream. That leaves one way a spectated player's pin pull could still be
//! driven -- some *other* replicated thing changing at that moment: a `body`
//! group, an `effects` bit, `fuser1`, an event, a new entity, anything. This
//! probe looks for one empirically instead of arguing from the SDK.
//!
//! On a POV demo the pin pull itself is known exactly: it is the `pinpull` (2)
//! or `exploding_pinpull` (7) viewmodel animation while a grenade viewmodel is
//! up. Around every one of them, in `[t-0.25s, t+0.35s]`, the probe records
//! every entity-state field of the local player's own entity that changes, and
//! every `svc_sound`, `svc_event`, `svc_temp_entity` and packet-entity
//! add/remove in the window -- the carriers an HLTV demo also has. The same
//! statistics over 200 random windows of the same length that hold no pin
//! pull and no grenade attack body sequence (82..=96) are the baseline, and a
//! second baseline is drawn only from windows where a grenade viewmodel was
//! up, to separate "the grenade is out" from "the pin was just pulled". A field
//! that changes in nearly every pull window and in few baseline windows is a
//! tell; one that changes in both at the same rate is just the player moving.
//!
//! The two animations are reported separately because they are not the same
//! moment: `pinpull` (2) is the pull, and is followed by `grenthrow.wav`;
//! `exploding_pinpull` (7) turns out to arrive a fixed ~0.6s *after* the
//! throw sound, with the throw body sequence already running, so it is the
//! post-throw animation of the next grenade and no pull at all. The per-pull
//! lines carry the delay since the previous throw sound so that reads off
//! directly.
//!
//! POV-only carriers (`clientdata_t`/`weapon_data_t` deltas, `Dem_Event` and
//! `Dem_Sound` frames) are recorded too, labelled as such, so the output also
//! shows what the recording player's *own* client saw. Nothing in that part of
//! the table reaches an HLTV demo.
//!
//! The local player's entity index is `ref_params_t::playernum + 1` from the
//! first network frame's demo info, cross-checked against every `svc_setview`
//! in the file; the output states both.
//!
//!     cargo run --release -p analysis --example grenade_pinpull_tell_probe -- <pov-demo>...

use dem::bit::BitSliceCast;
use dem::open_demo_from_bytes;
use dem::types::{Delta, EngineMessage, FrameData, MessageData, NetMessage};
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet};

/// The window around each pin pull, in seconds of demo time.
const BEFORE: f32 = 0.25;
const AFTER: f32 = 0.35;
/// Baseline windows per demo, and how many random draws to spend finding them.
const BASELINE_WINDOWS: usize = 200;
const BASELINE_DRAWS: usize = 200_000;
/// Two pin-pull animations closer than this are one pull carried twice.
const PULL_DEDUPE: f32 = 0.5;
/// How far around a pull to look for the body sequence and the throw sound.
const FOLLOW_UP: f32 = 15.0;
/// `stand_gren_shoot` .. `prone_mills_shoot` in DoD's player models; the
/// per-sequence list is in `grenade_timing_probe`.
const GRENADE_ATTACK_SEQUENCES: std::ops::RangeInclusive<u32> = 82..=96;
/// Counted, but never nominated as a tell: the player moving changes them.
const NOT_A_TELL: &[&str] = &[
    "origin",
    "angles",
    "velocity",
    "frame",
    "animtime",
    "gaitsequence",
];
/// A tell has to change in nearly every pull window and in few baseline ones.
const TELL_MIN_PULL_RATE: f64 = 0.8;
const TELL_MAX_BASELINE_RATE: f64 = 0.25;

fn clean(s: &str) -> String {
    s.trim_matches(|c: char| c == '\0' || c.is_whitespace())
        .to_string()
}

fn bytes_name(b: &[u8]) -> String {
    clean(&String::from_utf8_lossy(b))
}

fn value_u32(v: &[u8]) -> Option<u32> {
    (v.len() >= 4).then(|| u32::from_le_bytes([v[0], v[1], v[2], v[3]]))
}

/// A delta's fields with the trailing NUL the decoder table carries trimmed.
fn cleaned(delta: &Delta) -> BTreeMap<String, &[u8]> {
    delta
        .iter()
        .map(|(k, v)| (clean(k), v.as_slice()))
        .collect()
}

fn field_u32(delta: &Delta, field: &str) -> Option<u32> {
    delta
        .iter()
        .find(|(k, _)| clean(k) == field)
        .and_then(|(_, v)| value_u32(v))
}

fn is_grenade_viewmodel(name: &str) -> bool {
    name.contains("v_grenade") || name.contains("v_stick") || name.contains("v_mills")
}

fn is_excluded(field: &str) -> bool {
    NOT_A_TELL
        .iter()
        .any(|p| field == *p || field.starts_with(&format!("{p}[")))
}

fn anim_name(anim: i32) -> &'static str {
    match anim {
        2 => "pinpull",
        7 => "exploding_pinpull",
        _ => "?",
    }
}

/// Deterministic xorshift64, so two runs draw the same baseline windows.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }
}

struct Pull {
    time: f32,
    anim: i32,
    carrier: &'static str,
    viewmodel: String,
}

/// Everything the window pass reads, stamped with `frame.time`.
#[derive(Default)]
struct Timeline {
    pulls: Vec<Pull>,
    pulls_by_carrier: BTreeMap<String, usize>,
    duplicate_pulls: usize,
    /// Local entity: field -> (time, changed since the last value seen).
    ent_fields: HashMap<String, Vec<(f32, bool)>>,
    ent_update_times: Vec<f32>,
    full_updates: usize,
    delta_updates: usize,
    /// Every `sequence` value the local entity was sent.
    local_seq: Vec<(f32, u32)>,
    /// `svc_sound` naming the local entity.
    sounds: Vec<(f32, String)>,
    /// `svc_sound` naming any entity.
    sounds_any: Vec<(f32, String)>,
    /// `svc_event`/`svc_event_reliable` whose `entindex` is the local entity.
    events: Vec<(f32, String)>,
    events_any: Vec<(f32, String)>,
    /// `svc_temp_entity` by TE type.
    tempents: Vec<(f32, String)>,
    /// Entities entering / leaving the packet-entity stream, by model name.
    added: Vec<(f32, String)>,
    removed: Vec<(f32, String)>,
    /// POV-only: `Dem_Event` frames naming the local entity.
    local_events: Vec<(f32, String)>,
    /// POV-only: `Dem_Sound` frames (client-side sounds).
    local_sounds: Vec<(f32, String)>,
    /// POV-only: `clientdata_t`/`weapon_data_t` field -> (time, changed).
    cd_fields: HashMap<String, Vec<(f32, bool)>>,
    /// User messages by name.
    usermsgs: Vec<(f32, String)>,
    /// User messages with their payload, for the closer look at lifted ones.
    usermsg_payloads: Vec<(f32, (String, Vec<u8>))>,
    /// Spans during which a grenade viewmodel was up.
    grenade_up: Vec<(f32, f32)>,
}

impl Timeline {
    fn note_pull(&mut self, t: f32, anim: i32, carrier: &'static str, vm: &str) {
        if !(anim == 2 || anim == 7) || !is_grenade_viewmodel(vm) {
            return;
        }
        *self
            .pulls_by_carrier
            .entry(format!("{carrier} anim {anim}"))
            .or_insert(0) += 1;
        if let Some(last) = self.pulls.last()
            && (t - last.time).abs() < PULL_DEDUPE
        {
            self.duplicate_pulls += 1;
            return;
        }
        self.pulls.push(Pull {
            time: t,
            anim,
            carrier,
            viewmodel: vm.to_string(),
        });
    }

    fn note_cd(
        &mut self,
        t: f32,
        prefix: &str,
        delta: &Delta,
        last: &mut HashMap<String, Vec<u8>>,
    ) {
        for (k, v) in cleaned(delta) {
            let key = if prefix.is_empty() {
                k
            } else {
                format!("{prefix}.{k}")
            };
            let changed = last.get(&key).map(|old| old.as_slice()) != Some(v);
            self.cd_fields
                .entry(key.clone())
                .or_default()
                .push((t, changed));
            last.insert(key, v.to_vec());
        }
    }

    fn sort(&mut self) {
        fn by_time<T>(v: &mut [(f32, T)]) {
            v.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(Ordering::Equal));
        }
        for v in self.ent_fields.values_mut() {
            by_time(v);
        }
        for v in self.cd_fields.values_mut() {
            by_time(v);
        }
        by_time(&mut self.local_seq);
        by_time(&mut self.sounds);
        by_time(&mut self.sounds_any);
        by_time(&mut self.events);
        by_time(&mut self.events_any);
        by_time(&mut self.tempents);
        by_time(&mut self.added);
        by_time(&mut self.removed);
        by_time(&mut self.local_events);
        by_time(&mut self.local_sounds);
        by_time(&mut self.usermsgs);
        by_time(&mut self.usermsg_payloads);
        self.ent_update_times
            .sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    }

    /// First and last local-entity update, once sorted.
    fn span(&self) -> Option<(f32, f32)> {
        Some((
            *self.ent_update_times.first()?,
            *self.ent_update_times.last()?,
        ))
    }

    /// The local entity's sequence in effect at `t`.
    fn seq_at(&self, t: f32) -> Option<u32> {
        let i = self.local_seq.partition_point(|(tt, _)| *tt <= t);
        (i > 0).then(|| self.local_seq[i - 1].1)
    }
}

/// Tracks the packet-entity stream: the local entity's last-seen values, so a
/// field's *change* can be told from its mere presence in a delta, plus which
/// entities are live, so arrivals and departures show up.
struct EntityTracker {
    local: u16,
    last: HashMap<String, Vec<u8>>,
    baseline: HashMap<String, Vec<u8>>,
    live: HashSet<u16>,
    models: HashMap<u16, u32>,
}

impl EntityTracker {
    fn model_name(&self, entity: u16, model_names: &HashMap<u32, String>) -> String {
        if entity == self.local {
            return "local player".to_string();
        }
        match self.models.get(&entity).and_then(|m| model_names.get(m)) {
            Some(name) => name.clone(),
            None if (1..=32).contains(&entity) => "player (no model seen)".to_string(),
            None => "(no model seen)".to_string(),
        }
    }

    fn arrive(
        &mut self,
        tl: &mut Timeline,
        t: f32,
        entity: u16,
        delta: &Delta,
        baselines: &HashMap<u16, HashMap<String, Vec<u8>>>,
        model_names: &HashMap<u32, String>,
    ) {
        let model = field_u32(delta, "modelindex").or_else(|| {
            baselines
                .get(&entity)
                .and_then(|b| b.get("modelindex"))
                .and_then(|v| value_u32(v))
        });
        if let Some(m) = model {
            self.models.insert(entity, m);
        }
        self.live.insert(entity);
        tl.added.push((t, self.model_name(entity, model_names)));
    }

    fn note_local(&mut self, tl: &mut Timeline, t: f32, delta: &Delta, full: bool) {
        let fields = cleaned(delta);
        for (k, v) in &fields {
            let changed = self.last.get(k).map(|old| old.as_slice()) != Some(*v);
            tl.ent_fields
                .entry(k.clone())
                .or_default()
                .push((t, changed));
            if k == "sequence"
                && let Some(s) = value_u32(v)
            {
                tl.local_seq.push((t, s));
            }
            self.last.insert(k.clone(), v.to_vec());
        }
        if full {
            // A full packet encodes against the baseline, so a field missing
            // from it has gone back to its baseline value -- a change a delta
            // cannot show, and one the tracker would otherwise miss.
            let reverted: Vec<(String, Vec<u8>)> = self
                .baseline
                .iter()
                .filter(|(k, bv)| {
                    !fields.contains_key(*k) && self.last.get(*k).is_some_and(|cur| cur != *bv)
                })
                .map(|(k, bv)| (k.clone(), bv.clone()))
                .collect();
            for (k, bv) in reverted {
                tl.ent_fields.entry(k.clone()).or_default().push((t, true));
                if k == "sequence"
                    && let Some(s) = value_u32(&bv)
                {
                    tl.local_seq.push((t, s));
                }
                self.last.insert(k, bv);
            }
            tl.full_updates += 1;
        } else {
            tl.delta_updates += 1;
        }
        tl.ent_update_times.push(t);
    }
}

fn range<T>(v: &[(f32, T)], a: f32, b: f32) -> &[(f32, T)] {
    let lo = v.partition_point(|(t, _)| *t < a);
    let hi = v.partition_point(|(t, _)| *t <= b);
    &v[lo..hi.max(lo)]
}

fn count_times(v: &[f32], a: f32, b: f32) -> usize {
    let lo = v.partition_point(|t| *t < a);
    let hi = v.partition_point(|t| *t <= b);
    hi.saturating_sub(lo)
}

#[derive(Default, Clone, Copy)]
struct Counts {
    changed: usize,
    present: usize,
}

/// Per-window presence counts: each key counts at most once per window.
#[derive(Default)]
struct WindowStats {
    n: usize,
    ent: BTreeMap<String, Counts>,
    cd: BTreeMap<String, Counts>,
    sounds: BTreeMap<String, usize>,
    sounds_any: BTreeMap<String, usize>,
    events: BTreeMap<String, usize>,
    events_any: BTreeMap<String, usize>,
    tempents: BTreeMap<String, usize>,
    added: BTreeMap<String, usize>,
    removed: BTreeMap<String, usize>,
    local_events: BTreeMap<String, usize>,
    local_sounds: BTreeMap<String, usize>,
    usermsgs: BTreeMap<String, usize>,
}

fn note_names(map: &mut BTreeMap<String, usize>, v: &[(f32, String)], a: f32, b: f32) {
    let mut seen = HashSet::new();
    for (_, name) in range(v, a, b) {
        if seen.insert(name) {
            *map.entry(name.clone()).or_insert(0) += 1;
        }
    }
}

fn note_fields(
    map: &mut BTreeMap<String, Counts>,
    fields: &HashMap<String, Vec<(f32, bool)>>,
    a: f32,
    b: f32,
) {
    for (field, v) in fields {
        let r = range(v, a, b);
        if r.is_empty() {
            continue;
        }
        let c = map.entry(field.clone()).or_default();
        c.present += 1;
        if r.iter().any(|(_, changed)| *changed) {
            c.changed += 1;
        }
    }
}

fn merge_counts(into: &mut BTreeMap<String, usize>, from: &BTreeMap<String, usize>) {
    for (k, v) in from {
        *into.entry(k.clone()).or_insert(0) += v;
    }
}

fn merge_fields(into: &mut BTreeMap<String, Counts>, from: &BTreeMap<String, Counts>) {
    for (k, v) in from {
        let c = into.entry(k.clone()).or_default();
        c.changed += v.changed;
        c.present += v.present;
    }
}

impl WindowStats {
    fn add(&mut self, tl: &Timeline, a: f32, b: f32) {
        self.n += 1;
        note_fields(&mut self.ent, &tl.ent_fields, a, b);
        note_fields(&mut self.cd, &tl.cd_fields, a, b);
        note_names(&mut self.sounds, &tl.sounds, a, b);
        note_names(&mut self.sounds_any, &tl.sounds_any, a, b);
        note_names(&mut self.events, &tl.events, a, b);
        note_names(&mut self.events_any, &tl.events_any, a, b);
        note_names(&mut self.tempents, &tl.tempents, a, b);
        note_names(&mut self.added, &tl.added, a, b);
        note_names(&mut self.removed, &tl.removed, a, b);
        note_names(&mut self.local_events, &tl.local_events, a, b);
        note_names(&mut self.local_sounds, &tl.local_sounds, a, b);
        note_names(&mut self.usermsgs, &tl.usermsgs, a, b);
    }

    fn merge(&mut self, o: &WindowStats) {
        self.n += o.n;
        merge_fields(&mut self.ent, &o.ent);
        merge_fields(&mut self.cd, &o.cd);
        merge_counts(&mut self.sounds, &o.sounds);
        merge_counts(&mut self.sounds_any, &o.sounds_any);
        merge_counts(&mut self.events, &o.events);
        merge_counts(&mut self.events_any, &o.events_any);
        merge_counts(&mut self.tempents, &o.tempents);
        merge_counts(&mut self.added, &o.added);
        merge_counts(&mut self.removed, &o.removed);
        merge_counts(&mut self.local_events, &o.local_events);
        merge_counts(&mut self.local_sounds, &o.local_sounds);
        merge_counts(&mut self.usermsgs, &o.usermsgs);
    }
}

/// The pull windows, together and split by which animation marked them.
#[derive(Default)]
struct PullSets {
    all: WindowStats,
    /// `pinpull` (2): the pull.
    pin: WindowStats,
    /// `exploding_pinpull` (7): see the module doc -- the post-throw animation.
    post: WindowStats,
}

impl PullSets {
    fn add(&mut self, anim: i32, one: &WindowStats) {
        self.all.merge(one);
        match anim {
            2 => self.pin.merge(one),
            7 => self.post.merge(one),
            _ => {}
        }
    }

    fn merge(&mut self, o: &PullSets) {
        self.all.merge(&o.all);
        self.pin.merge(&o.pin);
        self.post.merge(&o.post);
    }
}

#[derive(Default)]
struct Rejections {
    outside_grenade_up: usize,
    overlaps_pull: usize,
    attack_sequence: usize,
    entity_idle: usize,
}

/// Random windows of the pull-window length that hold no pin pull and no
/// grenade attack body sequence, and in which the local entity was being
/// updated at all (a dead or spectating player would otherwise dilute every
/// rate). `grenade_up` additionally requires a grenade viewmodel to be up for
/// the whole window.
fn sample_baseline(
    tl: &Timeline,
    rng: &mut Rng,
    grenade_up: bool,
) -> (WindowStats, Rejections, usize) {
    let mut stats = WindowStats::default();
    let mut rej = Rejections::default();
    let Some((t0, t1)) = tl.span() else {
        return (stats, rej, 0);
    };
    let len = BEFORE + AFTER;
    if t1 - t0 <= len {
        return (stats, rej, 0);
    }
    let mut draws = 0;
    while stats.n < BASELINE_WINDOWS && draws < BASELINE_DRAWS {
        draws += 1;
        let a = t0 + rng.unit() * (t1 - t0 - len);
        let b = a + len;
        if grenade_up && !tl.grenade_up.iter().any(|(s, e)| *s <= a && b <= *e) {
            rej.outside_grenade_up += 1;
            continue;
        }
        if tl
            .pulls
            .iter()
            .any(|p| p.time - BEFORE < b && p.time + AFTER > a)
        {
            rej.overlaps_pull += 1;
            continue;
        }
        let attack_in_effect = tl
            .seq_at(a)
            .is_some_and(|s| GRENADE_ATTACK_SEQUENCES.contains(&s));
        let attack_in_window = range(&tl.local_seq, a, b)
            .iter()
            .any(|(_, s)| GRENADE_ATTACK_SEQUENCES.contains(s));
        if attack_in_effect || attack_in_window {
            rej.attack_sequence += 1;
            continue;
        }
        if count_times(&tl.ent_update_times, a, b) == 0 {
            rej.entity_idle += 1;
            continue;
        }
        stats.add(tl, a, b);
    }
    (stats, rej, draws)
}

fn rate(n: usize, d: usize) -> f64 {
    if d == 0 { 0.0 } else { n as f64 / d as f64 }
}

fn cell(n: usize, d: usize) -> String {
    if d == 0 {
        format!("{:>9}", "-")
    } else {
        format!("{n:>3}/{d:<3}{:>3.0}%", 100.0 * rate(n, d))
    }
}

struct Row {
    name: String,
    all: usize,
    pin: usize,
    post: usize,
    base: usize,
    base_g: usize,
    present: Option<usize>,
}

fn qualifies(hits: usize, windows: usize, base_hits: usize, base_windows: usize) -> bool {
    windows >= 5
        && base_windows > 0
        && rate(hits, windows) >= TELL_MIN_PULL_RATE
        && rate(base_hits, base_windows) <= TELL_MAX_BASELINE_RATE
}

/// One table: name -> (pull windows it appeared in) vs the two baselines,
/// sorted by the `pinpull` (2) lift over the random baseline. Returns the tell
/// candidates.
fn print_table(
    title: &str,
    mut rows: Vec<Row>,
    sets: &PullSets,
    base: &WindowStats,
    base_g: &WindowStats,
    candidates: bool,
) -> Vec<String> {
    if rows.is_empty() {
        println!("\n{title}: nothing in any window");
        return Vec::new();
    }
    rows.sort_by(|x, y| {
        let lx = rate(x.pin, sets.pin.n) - rate(x.base, base.n);
        let ly = rate(y.pin, sets.pin.n) - rate(y.base, base.n);
        ly.partial_cmp(&lx)
            .unwrap_or(Ordering::Equal)
            .then_with(|| x.name.cmp(&y.name))
    });
    println!(
        "\n{title}\n{:<30} {:>9} {:>9} {:>9} {:>9} {:>9} {:>9}",
        "", "all pulls", "pinpull2", "expl7", "baseline", "base/gren", "present"
    );
    let mut tells = Vec::new();
    for row in &rows {
        let note = if !candidates {
            ""
        } else if is_excluded(&row.name) {
            "  (counted, not a candidate)"
        } else if qualifies(row.pin, sets.pin.n, row.base, base.n) {
            tells.push(row.name.clone());
            "  <-- TELL (pinpull)"
        } else if qualifies(row.all, sets.all.n, row.base, base.n) {
            tells.push(format!("{} (all pulls only)", row.name));
            "  <-- TELL (all pulls)"
        } else {
            ""
        };
        let present = match row.present {
            Some(p) => cell(p, sets.all.n),
            None => format!("{:>9}", ""),
        };
        println!(
            "{:<30} {} {} {} {} {} {}{note}",
            row.name,
            cell(row.all, sets.all.n),
            cell(row.pin, sets.pin.n),
            cell(row.post, sets.post.n),
            cell(row.base, base.n),
            cell(row.base_g, base_g.n),
            present,
        );
    }
    tells
}

fn field_rows(
    pick: fn(&WindowStats) -> &BTreeMap<String, Counts>,
    sets: &PullSets,
    base: &WindowStats,
    base_g: &WindowStats,
) -> Vec<Row> {
    let names: HashSet<&String> = pick(&sets.all)
        .keys()
        .chain(pick(base).keys())
        .chain(pick(base_g).keys())
        .collect();
    let changed =
        |s: &WindowStats, name: &String| pick(s).get(name).map(|c| c.changed).unwrap_or(0);
    names
        .into_iter()
        .map(|name| Row {
            name: name.clone(),
            all: changed(&sets.all, name),
            pin: changed(&sets.pin, name),
            post: changed(&sets.post, name),
            base: changed(base, name),
            base_g: changed(base_g, name),
            present: Some(pick(&sets.all).get(name).map(|c| c.present).unwrap_or(0)),
        })
        .collect()
}

fn name_rows(
    pick: fn(&WindowStats) -> &BTreeMap<String, usize>,
    sets: &PullSets,
    base: &WindowStats,
    base_g: &WindowStats,
) -> Vec<Row> {
    let names: HashSet<&String> = pick(&sets.all)
        .keys()
        .chain(pick(base).keys())
        .chain(pick(base_g).keys())
        .collect();
    let hits = |s: &WindowStats, name: &String| pick(s).get(name).copied().unwrap_or(0);
    names
        .into_iter()
        .map(|name| Row {
            name: name.clone(),
            all: hits(&sets.all, name),
            pin: hits(&sets.pin, name),
            post: hits(&sets.post, name),
            base: hits(base, name),
            base_g: hits(base_g, name),
            present: None,
        })
        .collect()
}

/// Every table for one set of windows, HLTV-visible carriers first. Returns
/// the HLTV-visible tell candidates.
fn report(sets: &PullSets, base: &WindowStats, base_g: &WindowStats) -> Vec<String> {
    let mut tells = Vec::new();
    println!(
        "\n--- HLTV-visible carriers ({} pull windows: {} pinpull, {} exploding_pinpull; {} baseline, {} baseline with a grenade up) ---",
        sets.all.n, sets.pin.n, sets.post.n, base.n, base_g.n
    );
    tells.extend(print_table(
        "entity_state_player_t fields of the local entity (changed in window)",
        field_rows(|s| &s.ent, sets, base, base_g),
        sets,
        base,
        base_g,
        true,
    ));
    tells.extend(
        print_table(
            "svc_sound naming the local entity",
            name_rows(|s| &s.sounds, sets, base, base_g),
            sets,
            base,
            base_g,
            true,
        )
        .into_iter()
        .map(|n| format!("svc_sound {n}")),
    );
    tells.extend(
        print_table(
            "svc_event / svc_event_reliable with entindex = local entity",
            name_rows(|s| &s.events, sets, base, base_g),
            sets,
            base,
            base_g,
            true,
        )
        .into_iter()
        .map(|n| format!("svc_event {n}")),
    );
    tells.extend(
        print_table(
            "entities arriving in the packet-entity stream (by model)",
            name_rows(|s| &s.added, sets, base, base_g),
            sets,
            base,
            base_g,
            true,
        )
        .into_iter()
        .map(|n| format!("entity added: {n}")),
    );
    print_table(
        "entities leaving the packet-entity stream (by model)",
        name_rows(|s| &s.removed, sets, base, base_g),
        sets,
        base,
        base_g,
        false,
    );
    tells.extend(
        print_table(
            "svc_temp_entity by type",
            name_rows(|s| &s.tempents, sets, base, base_g),
            sets,
            base,
            base_g,
            true,
        )
        .into_iter()
        .map(|n| format!("temp entity {n}")),
    );
    tells.extend(
        print_table(
            "svc_sound naming any entity",
            name_rows(|s| &s.sounds_any, sets, base, base_g),
            sets,
            base,
            base_g,
            true,
        )
        .into_iter()
        .map(|n| format!("svc_sound (any entity) {n}")),
    );
    tells.extend(
        print_table(
            "svc_event with any entindex",
            name_rows(|s| &s.events_any, sets, base, base_g),
            sets,
            base,
            base_g,
            true,
        )
        .into_iter()
        .map(|n| format!("svc_event (any entity) {n}")),
    );
    print_table(
        "user messages by name (HLTV carries the broadcast ones only)",
        name_rows(|s| &s.usermsgs, sets, base, base_g),
        sets,
        base,
        base_g,
        false,
    );

    println!("\n--- POV-only carriers (never in an HLTV demo) ---");
    print_table(
        "clientdata_t / weapon_data_t fields (changed in window)",
        field_rows(|s| &s.cd, sets, base, base_g),
        sets,
        base,
        base_g,
        false,
    );
    print_table(
        "Dem_Sound frames (client-side sounds)",
        name_rows(|s| &s.local_sounds, sets, base, base_g),
        sets,
        base,
        base_g,
        false,
    );
    print_table(
        "Dem_Event frames naming the local entity (client-predicted events)",
        name_rows(|s| &s.local_events, sets, base, base_g),
        sets,
        base,
        base_g,
        false,
    );
    tells
}

fn quantiles(label: &str, v: &mut [f32], missing: usize) {
    if v.is_empty() {
        println!("{label}: none found ({missing} pulls)");
        return;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    let pick = |f: f32| v[((v.len() - 1) as f32 * f) as usize];
    println!(
        "{label}: {} of {} pulls; min {:.3}s  p25 {:.3}s  median {:.3}s  p75 {:.3}s  max {:.3}s",
        v.len(),
        v.len() + missing,
        v[0],
        pick(0.25),
        pick(0.50),
        pick(0.75),
        v[v.len() - 1]
    );
}

/// The three delays measured per pull, collected per animation.
#[derive(Default)]
struct Delays {
    to_attack: Vec<f32>,
    no_attack: usize,
    to_throw: Vec<f32>,
    no_throw: usize,
    since_throw: Vec<f32>,
    no_prev_throw: usize,
    /// `grenthrow.wav` minus the attack body sequence, where both followed.
    lead: Vec<f32>,
}

impl Delays {
    fn print(&mut self, anim: i32) {
        let name = anim_name(anim);
        println!();
        quantiles(
            &format!("{name} ({anim}) -> next grenade attack body sequence (82..=96)"),
            &mut self.to_attack,
            self.no_attack,
        );
        quantiles(
            &format!("{name} ({anim}) -> next weapons/grenthrow.wav svc_sound on the local entity"),
            &mut self.to_throw,
            self.no_throw,
        );
        quantiles(
            &format!("{name} ({anim}) since the previous grenthrow.wav on the local entity"),
            &mut self.since_throw,
            self.no_prev_throw,
        );
        quantiles(
            &format!("{name} ({anim}): attack body sequence leads grenthrow.wav by"),
            &mut self.lead,
            0,
        );
    }
}

fn main() {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    if paths.is_empty() {
        eprintln!("usage: grenade_pinpull_tell_probe <pov-demo>...");
        std::process::exit(2);
    }
    let mut pooled = PullSets::default();
    let mut pooled_base = WindowStats::default();
    let mut pooled_base_g = WindowStats::default();
    let mut pooled_tells: BTreeMap<String, usize> = BTreeMap::new();
    let mut demos_done = 0;

    for path in &paths {
        println!("\n==================== {path} ====================");
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                println!("read failed: {e}");
                continue;
            }
        };
        let demo = match open_demo_from_bytes(&bytes) {
            Ok(d) => d,
            Err(e) => {
                println!("parse failed: {e}");
                continue;
            }
        };
        let Some((sets, base, base_g, tells)) = probe(&demo) else {
            continue;
        };
        demos_done += 1;
        pooled.merge(&sets);
        pooled_base.merge(&base);
        pooled_base_g.merge(&base_g);
        for t in tells {
            *pooled_tells.entry(t).or_insert(0) += 1;
        }
    }

    if demos_done > 1 {
        println!("\n==================== ALL {demos_done} DEMOS POOLED ====================");
        let tells = report(&pooled, &pooled_base, &pooled_base_g);
        verdict(&tells, &pooled);
        if !pooled_tells.is_empty() {
            println!("per-demo tells (name -> demos it was a tell in):");
            for (name, n) in &pooled_tells {
                println!("  {name}: {n} of {demos_done}");
            }
        }
    }
}

fn verdict(tells: &[String], sets: &PullSets) {
    println!();
    if tells.is_empty() {
        println!(
            "VERDICT: no HLTV-visible tell. Nothing an HLTV demo carries changed in >= {:.0}% of the {} pinpull \
             windows (or of all {} windows) while staying under {:.0}% of baseline windows.",
            100.0 * TELL_MIN_PULL_RATE,
            sets.pin.n,
            sets.all.n,
            100.0 * TELL_MAX_BASELINE_RATE
        );
    } else {
        println!("VERDICT: HLTV-visible tell candidate(s) at pin-pull time:");
        for t in tells {
            println!("  {t}");
        }
    }
}

fn quantile_line(v: &mut [f32]) -> String {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    let pick = |f: f32| v[((v.len() - 1) as f32 * f) as usize];
    format!(
        "min {:+.3}s  p25 {:+.3}s  median {:+.3}s  p75 {:+.3}s  max {:+.3}s",
        v[0],
        pick(0.25),
        pick(0.50),
        pick(0.75),
        v[v.len() - 1]
    )
}

/// A user message that is lifted in the pinpull windows deserves more than a
/// rate: when it lands relative to the pull, and with what payload. Only for
/// the demo at hand -- the pooled pass has no timeline. Whether it reaches an
/// HLTV demo at all is a separate question (`msg_probe` on an HLTV demo).
fn detail_lifted_usermsgs(tl: &Timeline, sets: &PullSets, base: &WindowStats) {
    let pulls: Vec<f32> = tl
        .pulls
        .iter()
        .filter(|p| p.anim == 2)
        .map(|p| p.time)
        .collect();
    if pulls.is_empty() {
        return;
    }
    let mut lifted: Vec<&String> = sets
        .pin
        .usermsgs
        .iter()
        .filter(|(name, hits)| {
            rate(**hits, sets.pin.n) >= 0.25
                && rate(base.usermsgs.get(*name).copied().unwrap_or(0), base.n) <= 0.05
        })
        .map(|(name, _)| name)
        .collect();
    lifted.sort();
    if lifted.is_empty() {
        return;
    }
    println!(
        "\n--- user messages lifted in pinpull windows (>= 25% of pulls, <= 5% of baseline) ---"
    );
    for name in lifted {
        let instances: Vec<&(f32, (String, Vec<u8>))> = tl
            .usermsg_payloads
            .iter()
            .filter(|(_, (n, _))| n == name)
            .collect();
        let mut in_window = 0usize;
        let mut offsets: Vec<f32> = Vec::new();
        let mut far = 0usize;
        let mut payloads: BTreeMap<String, usize> = BTreeMap::new();
        for (t, (_, data)) in &instances {
            // Signed offset to the nearest pinpull: positive = after it.
            let nearest = pulls
                .iter()
                .map(|p| t - p)
                .min_by(|a, b| a.abs().partial_cmp(&b.abs()).unwrap_or(Ordering::Equal));
            match nearest {
                Some(d) if d.abs() <= 5.0 => {
                    offsets.push(d);
                    if (-BEFORE..=AFTER).contains(&d) {
                        in_window += 1;
                    }
                }
                _ => far += 1,
            }
            let shown: Vec<String> = data.iter().take(8).map(|b| format!("{b:02x}")).collect();
            *payloads.entry(shown.join(" ")).or_insert(0) += 1;
        }
        println!(
            "{name}: {} in the demo, {in_window} inside a pinpull window, {far} more than 5s from any pinpull",
            instances.len()
        );
        if !offsets.is_empty() {
            println!(
                "  offset from the nearest pinpull (+ = after), {} within 5s: {}",
                offsets.len(),
                quantile_line(&mut offsets)
            );
        }
        let mut by_count: Vec<(String, usize)> = payloads.into_iter().collect();
        by_count.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let shown: Vec<String> = by_count
            .iter()
            .take(8)
            .map(|(p, n)| format!("[{p}] x{n}"))
            .collect();
        println!("  payloads (hex, first 8 bytes): {}", shown.join("  "));
    }
}

type ProbeResult = (PullSets, WindowStats, WindowStats, Vec<String>);

fn probe(demo: &dem::types::Demo) -> Option<ProbeResult> {
    let mut model_names: HashMap<u32, String> = HashMap::new();
    let mut sound_names: HashMap<u32, String> = HashMap::new();
    let mut event_names: HashMap<u32, String> = HashMap::new();
    let mut baselines: HashMap<u16, HashMap<String, Vec<u8>>> = HashMap::new();
    let mut tl = Timeline::default();
    let mut cd_last: HashMap<String, Vec<u8>> = HashMap::new();

    // Local entity: ref_params_t::playernum + 1 from the first network frame,
    // cross-checked against svc_setview and ref_params_t::viewentity.
    let mut tracker: Option<EntityTracker> = None;
    let mut local_from_refparams: Option<u16> = None;
    let mut setviews: Vec<i16> = Vec::new();
    let mut frames = 0usize;
    let mut view_agrees = 0usize;
    let mut max_clients = 0;

    // Viewmodel, from clientdata (the server's word) with the frame's demo
    // info as a cross-check.
    let mut vm = String::new();
    let mut grenade_since: Option<f32> = None;
    let mut demoinfo_vm_frames = 0usize;
    let mut demoinfo_vm_agrees = 0usize;
    let mut last_time = 0f32;

    for entry in &demo.directory.entries {
        for frame in &entry.frames {
            let t = frame.time;
            last_time = last_time.max(t);
            match &frame.frame_data {
                FrameData::WeaponAnimation(wa) => {
                    tl.note_pull(t, wa.anim, "Dem_WeaponAnim", &vm);
                    continue;
                }
                FrameData::Event(ev) => {
                    if let Some(tr) = &tracker
                        && ev.args.entity_index == tr.local as i32
                    {
                        let name = event_names
                            .get(&(ev.index as u32))
                            .cloned()
                            .unwrap_or_else(|| format!("event#{}", ev.index));
                        tl.local_events.push((t, name));
                    }
                    continue;
                }
                FrameData::Sound(s) => {
                    tl.local_sounds.push((t, bytes_name(&s.sample)));
                    continue;
                }
                FrameData::NetworkMessage(_) => {}
                _ => continue,
            }
            let FrameData::NetworkMessage(bt) = &frame.frame_data else {
                continue;
            };
            let info = &bt.1.info;
            frames += 1;
            if info.refparams.view_entity == info.refparams.player_num + 1 {
                view_agrees += 1;
            }
            if local_from_refparams.is_none() && info.refparams.player_num >= 0 {
                local_from_refparams = Some(info.refparams.player_num as u16 + 1);
                max_clients = info.refparams.max_clients;
            }
            if let Some(name) = model_names.get(&(info.viewmodel as u32)) {
                demoinfo_vm_frames += 1;
                if clean(name) == vm {
                    demoinfo_vm_agrees += 1;
                }
            }
            let MessageData::Parsed(msgs) = &bt.1.messages else {
                continue;
            };
            // A server-sent weapon animation lands in the same packet as, and
            // ahead of, the clientdata that names the viewmodel it is for --
            // at a weapon switch it would be judged against the *old*
            // viewmodel. Hold it until the packet is done.
            let mut server_anims: Vec<i32> = Vec::new();
            for m in msgs {
                let em = match m {
                    NetMessage::UserMessage(um) => {
                        let name = bytes_name(&um.name);
                        tl.usermsg_payloads
                            .push((t, (name.clone(), um.data.clone())));
                        tl.usermsgs.push((t, name));
                        continue;
                    }
                    NetMessage::EngineMessage(em) => em,
                };
                match &**em {
                    EngineMessage::SvcResourceList(rl) => {
                        for r in &rl.resources {
                            let name = clean(&r.name.get_string());
                            let idx = r.index.to_u32();
                            match r.type_.to_u8() {
                                0 => {
                                    sound_names.insert(idx, name);
                                }
                                2 => {
                                    model_names.insert(idx, name);
                                }
                                5 => {
                                    event_names.insert(idx, name);
                                }
                                _ => {}
                            }
                        }
                    }
                    EngineMessage::SvcSetView(sv) => {
                        if !setviews.contains(&sv.entity_index) {
                            setviews.push(sv.entity_index);
                        }
                    }
                    EngineMessage::SvcSpawnBaseline(sb) => {
                        for e in &sb.entities {
                            let owned = cleaned(&e.delta)
                                .into_iter()
                                .map(|(k, v)| (k, v.to_vec()))
                                .collect();
                            baselines.insert(e.entity_index, owned);
                        }
                    }
                    EngineMessage::SvcClientData(cd) => {
                        tl.note_cd(t, "", &cd.client_data, &mut cd_last);
                        if let Some(wd) = &cd.weapon_data {
                            for w in wd {
                                let prefix = format!("weapon_data[{}]", w.weapon_index.to_u32());
                                tl.note_cd(t, &prefix, &w.weapon_data, &mut cd_last);
                            }
                        }
                        if let Some(idx) = field_u32(&cd.client_data, "viewmodel")
                            && let Some(name) = model_names.get(&idx)
                        {
                            let name = clean(name);
                            if name != vm {
                                let was = is_grenade_viewmodel(&vm);
                                let now = is_grenade_viewmodel(&name);
                                if !was && now {
                                    grenade_since = Some(t);
                                }
                                if was
                                    && !now
                                    && let Some(s) = grenade_since.take()
                                {
                                    tl.grenade_up.push((s, t));
                                }
                                vm = name;
                            }
                        }
                    }
                    EngineMessage::SvcPacketEntities(pe) => {
                        let Some(local) = local_from_refparams else {
                            continue;
                        };
                        let tr = tracker.get_or_insert_with(|| EntityTracker {
                            local,
                            last: HashMap::new(),
                            baseline: baselines.get(&local).cloned().unwrap_or_default(),
                            live: HashSet::new(),
                            models: HashMap::new(),
                        });
                        let mut seen: HashSet<u16> = HashSet::new();
                        for es in &pe.entity_states {
                            seen.insert(es.entity_index);
                            if !tr.live.contains(&es.entity_index) {
                                tr.arrive(
                                    &mut tl,
                                    t,
                                    es.entity_index,
                                    &es.delta,
                                    &baselines,
                                    &model_names,
                                );
                            } else if let Some(m) = field_u32(&es.delta, "modelindex") {
                                tr.models.insert(es.entity_index, m);
                            }
                            if es.entity_index == tr.local {
                                tr.note_local(&mut tl, t, &es.delta, true);
                            }
                        }
                        let gone: Vec<u16> = tr.live.difference(&seen).copied().collect();
                        for e in gone {
                            tl.removed.push((t, tr.model_name(e, &model_names)));
                        }
                        tr.live = seen;
                    }
                    EngineMessage::SvcDeltaPacketEntities(pe) => {
                        let Some(tr) = tracker.as_mut() else {
                            continue;
                        };
                        for es in &pe.entity_states {
                            if es.remove_entity {
                                if tr.live.remove(&es.entity_index) {
                                    tl.removed
                                        .push((t, tr.model_name(es.entity_index, &model_names)));
                                }
                                continue;
                            }
                            let Some(delta) = &es.delta else {
                                continue;
                            };
                            if !tr.live.contains(&es.entity_index) {
                                tr.arrive(
                                    &mut tl,
                                    t,
                                    es.entity_index,
                                    delta,
                                    &baselines,
                                    &model_names,
                                );
                            } else if let Some(m) = field_u32(delta, "modelindex") {
                                tr.models.insert(es.entity_index, m);
                            }
                            if es.entity_index == tr.local {
                                tr.note_local(&mut tl, t, delta, false);
                            }
                        }
                    }
                    EngineMessage::SvcSound(s) => {
                        let index = s
                            .sound_index_long
                            .as_ref()
                            .map(|b| b.to_u32())
                            .or_else(|| s.sound_index_short.as_ref().map(|b| b.to_u32()));
                        let name = index
                            .and_then(|i| sound_names.get(&i).cloned())
                            .unwrap_or_else(|| format!("sound#{index:?}"));
                        let entity = s.entity_index.to_u32() as u16;
                        tl.sounds_any.push((t, name.clone()));
                        if tracker.as_ref().is_some_and(|tr| tr.local == entity) {
                            tl.sounds.push((t, name));
                        }
                    }
                    EngineMessage::SvcEvent(e) => {
                        for ev in &e.events {
                            let idx = ev.event_index.to_u32();
                            let name = event_names
                                .get(&idx)
                                .cloned()
                                .unwrap_or_else(|| format!("event#{idx}"));
                            let ent = ev.delta.as_ref().and_then(|d| field_u32(d, "entindex"));
                            tl.events_any.push((t, name.clone()));
                            if tracker
                                .as_ref()
                                .is_some_and(|tr| ent == Some(tr.local as u32))
                            {
                                tl.events.push((t, name));
                            }
                        }
                    }
                    EngineMessage::SvcEventReliable(e) => {
                        let idx = e.event_index.to_u32();
                        let name = event_names
                            .get(&idx)
                            .map(|n| format!("{n} (reliable)"))
                            .unwrap_or_else(|| format!("event#{idx} (reliable)"));
                        let ent = field_u32(&e.event_args, "entindex");
                        tl.events_any.push((t, name.clone()));
                        if tracker
                            .as_ref()
                            .is_some_and(|tr| ent == Some(tr.local as u32))
                        {
                            tl.events.push((t, name));
                        }
                    }
                    EngineMessage::SvcTempEntity(te) => {
                        tl.tempents.push((t, format!("TE#{}", te.entity_type)));
                    }
                    EngineMessage::SvcWeaponAnim(wa) => {
                        server_anims.push(wa.sequence_number as i32);
                    }
                    _ => {}
                }
            }
            for anim in server_anims {
                tl.note_pull(t, anim, "svc_weaponanim", &vm);
            }
        }
    }
    if let Some(s) = grenade_since.take() {
        tl.grenade_up.push((s, last_time));
    }
    tl.sort();

    let Some(tr) = &tracker else {
        println!("no packet entities seen; not a playable demo");
        return None;
    };
    println!(
        "local entity: {} (ref_params playernum {} + 1, max_clients {}); svc_setview said {:?}; \
         ref_params viewentity == playernum+1 in {view_agrees} of {frames} network frames",
        tr.local,
        tr.local as i32 - 1,
        max_clients,
        setviews
    );
    if setviews.iter().any(|s| *s as u16 != tr.local) {
        println!("WARNING: svc_setview disagrees with ref_params; the local entity may be wrong");
    }
    let (t0, t1) = tl.span().unwrap_or((0.0, 0.0));
    println!(
        "local entity updates: {} ({} in full packets, {} in delta packets) over {t0:.1}s..{t1:.1}s, {} fields seen, baseline has {} fields",
        tl.ent_update_times.len(),
        tl.full_updates,
        tl.delta_updates,
        tl.ent_fields.len(),
        tr.baseline.len()
    );
    println!(
        "demo info viewmodel agreed with clientdata viewmodel in {demoinfo_vm_agrees} of {demoinfo_vm_frames} frames; \
         grenade viewmodel up for {:.1}s in {} spans",
        tl.grenade_up.iter().map(|(s, e)| e - s).sum::<f32>(),
        tl.grenade_up.len()
    );
    println!(
        "pin pulls: {} ({} more were the same pull within {PULL_DEDUPE}s), by carrier and animation {:?}",
        tl.pulls.len(),
        tl.duplicate_pulls,
        tl.pulls_by_carrier
    );
    if tl.pulls.is_empty() {
        println!("no pin pulls in this demo; nothing to measure");
        return None;
    }

    // Per pull: the window's stats, and the delays to the body sequence and
    // the throw sound.
    let mut sets = PullSets::default();
    let mut delays: BTreeMap<i32, Delays> = BTreeMap::new();
    println!(
        "\nper pull (demo seconds; 'changed' lists the non-excluded entity fields that changed in the window):"
    );
    for (i, p) in tl.pulls.iter().enumerate() {
        let (a, b) = (p.time - BEFORE, p.time + AFTER);
        let mut one = WindowStats::default();
        one.add(&tl, a, b);
        let changed: Vec<&String> = one
            .ent
            .iter()
            .filter(|(k, c)| c.changed > 0 && !is_excluded(k))
            .map(|(k, _)| k)
            .collect();
        let attack = range(&tl.local_seq, p.time, p.time + FOLLOW_UP)
            .iter()
            .find(|(_, s)| GRENADE_ATTACK_SEQUENCES.contains(s))
            .map(|(tt, s)| (tt - p.time, *s));
        let throw = range(&tl.sounds, p.time, p.time + FOLLOW_UP)
            .iter()
            .find(|(_, n)| n.contains("grenthrow"))
            .map(|(tt, _)| tt - p.time);
        let prev_throw = range(&tl.sounds, p.time - FOLLOW_UP, p.time)
            .iter()
            .rev()
            .find(|(_, n)| n.contains("grenthrow"))
            .map(|(tt, _)| p.time - tt);
        let d = delays.entry(p.anim).or_default();
        match attack {
            Some((x, _)) => d.to_attack.push(x),
            None => d.no_attack += 1,
        }
        match throw {
            Some(x) => d.to_throw.push(x),
            None => d.no_throw += 1,
        }
        match prev_throw {
            Some(x) => d.since_throw.push(x),
            None => d.no_prev_throw += 1,
        }
        if let (Some((x, _)), Some(y)) = (attack, throw)
            && x <= y
        {
            d.lead.push(y - x);
        }
        println!(
            "  #{:<3} t={:>9.3}  {} ({}) via {:<14} {:<22} seq@pull={:<4} ->attack seq {:<16} ->grenthrow {:<9} since prev grenthrow {:<9} changed: {}",
            i + 1,
            p.time,
            anim_name(p.anim),
            p.anim,
            p.carrier,
            p.viewmodel,
            tl.seq_at(p.time)
                .map(|s| s.to_string())
                .unwrap_or_else(|| "?".into()),
            attack
                .map(|(x, s)| format!("+{x:.3}s (seq {s})"))
                .unwrap_or_else(|| "none".into()),
            throw
                .map(|x| format!("+{x:.3}s"))
                .unwrap_or_else(|| "none".into()),
            prev_throw
                .map(|x| format!("-{x:.3}s"))
                .unwrap_or_else(|| "none".into()),
            if changed.is_empty() {
                "-".to_string()
            } else {
                changed
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        );
        sets.add(p.anim, &one);
    }

    for (anim, d) in delays.iter_mut() {
        d.print(*anim);
    }

    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let (base, rej, draws) = sample_baseline(&tl, &mut rng, false);
    println!(
        "\nbaseline (random): {} windows from {draws} draws; rejected {} overlapping a pull, {} with a grenade attack sequence, {} with no local update",
        base.n, rej.overlaps_pull, rej.attack_sequence, rej.entity_idle
    );
    let (base_g, rej_g, draws_g) = sample_baseline(&tl, &mut rng, true);
    println!(
        "baseline (grenade up): {} windows from {draws_g} draws; rejected {} outside a grenade span, {} overlapping a pull, {} with a grenade attack sequence, {} with no local update",
        base_g.n,
        rej_g.outside_grenade_up,
        rej_g.overlaps_pull,
        rej_g.attack_sequence,
        rej_g.entity_idle
    );

    let tells = report(&sets, &base, &base_g);
    detail_lifted_usermsgs(&tl, &sets, &base);
    verdict(&tells, &sets);
    Some((sets, base, base_g, tells))
}
