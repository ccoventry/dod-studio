//! Rule checks for the batch planner: what every batch must hold, whatever
//! its highlights.
//!
//! The golden tests (`golden_tests.rs`) pin what a batch writes today, right
//! or wrong, so a bug present when they were written passes them: #1395 (a
//! merged take stopped before a nested highlight's last kill) did. These
//! checks run over the golden payload in every capture mode, over the
//! #1395 shapes, and over generated payloads with nested, overlapping,
//! touching and far-apart highlights across several demos and players. Each
//! rule names the doc comment that promises it.
//!
//! A failure prints the first failing case's highlights and settings, so the
//! input can be copied into a `blocks_tests.rs` example.

use super::golden_tests::{
    FPS, FRAMES, build_golden_batch, config, frame_times, highlight, payload,
};
use super::*;
use crate::patch::types::CaptureMode;
use crate::test_support::Scratch;
use std::sync::OnceLock;

/// How many payloads [`generated_cases`] makes. Each builds four times in the
/// order check, so this keeps the module to a few seconds.
const GENERATED_CASES: u64 = 120;

/// Half a frame at [`FPS`]: the slack for comparing frame times, which are
/// `f32`.
const TIME_SLACK_SECONDS: f32 = 0.5 / FPS;

/// The record settings a case's config is made with. Kept beside the case,
/// not the config, because a config points at a scratch folder that is gone
/// once the case is built; the order check makes a new one from these.
#[derive(Clone, Copy, Debug)]
struct Settings {
    mode: CaptureMode,
    record_start_lead: f32,
    record_stop_trail: f32,
    pre_roll_seconds: f32,
    post_roll_seconds: f32,
    /// Whether the batch records to a capture directory (the golden config's
    /// `takes` folder) or, with none, to the session folder. Generated cases
    /// use none: each capture directory costs a `mklink` process per build.
    capture_directory: bool,
}

/// The golden config's record settings.
const GOLDEN_SETTINGS: Settings = Settings {
    mode: CaptureMode::FrameSequence,
    record_start_lead: 1.0,
    record_stop_trail: 1.0,
    pre_roll_seconds: 5.0,
    post_roll_seconds: 1.0,
    capture_directory: true,
};

/// The golden config under `root` with `settings` applied.
fn make_config(root: &std::path::Path, settings: &Settings) -> PatcherConfig {
    let mut config = config(root, settings.mode);
    config.record_start_lead = settings.record_start_lead;
    config.record_stop_trail = settings.record_stop_trail;
    config.pre_roll_seconds = settings.pre_roll_seconds;
    config.post_roll_seconds = settings.post_roll_seconds;
    if !settings.capture_directory {
        config.capture_directories.clear();
    }
    config
}

/// One payload and the batch built from it.
struct Case {
    name: String,
    settings: Settings,
    highlights: Vec<CaptureStreak>,
    config: PatcherConfig,
    jobs: Vec<PatchJob>,
}

impl Case {
    /// Builds `highlights` with `settings` in a scratch folder of its own.
    fn build(name: String, settings: Settings, highlights: Vec<CaptureStreak>) -> Self {
        let root = Scratch::new(format!("planner_rules_{name}"));
        let config = make_config(&root, &settings);
        let (jobs, _headroom) = build_golden_batch(&highlights, &config);
        Case {
            name,
            settings,
            highlights,
            config,
            jobs,
        }
    }

    /// The case's highlights and settings, for a failure message.
    fn describe(&self) -> String {
        let mut out = format!("case {} with {:?}\n", self.name, self.settings);
        for (i, h) in self.highlights.iter().enumerate() {
            let kills: Vec<i32> = h.kills.iter().map(|k| k.0).collect();
            out.push_str(&format!(
                "  [{i}] {} {:?} kills {kills:?} range {}..={}\n",
                h.source_demo,
                h.target_player.as_deref().unwrap_or("-"),
                h.start_index,
                h.end_index
            ));
        }
        out
    }
}

/// Every case the rules run over, built once for the module.
fn cases() -> &'static [Case] {
    static CASES: OnceLock<Vec<Case>> = OnceLock::new();
    CASES.get_or_init(|| {
        let mut cases = Vec::new();
        for mode in [
            CaptureMode::FrameSequence,
            CaptureMode::DirectToVideo,
            CaptureMode::Obs,
            CaptureMode::Agr,
        ] {
            let settings = Settings {
                mode,
                ..GOLDEN_SETTINGS
            };
            cases.push(Case::build(format!("golden_{mode:?}"), settings, payload()));
        }
        cases.extend(nested_cases());
        cases.extend(generated_cases());
        cases
    })
}

/// The #1395 shapes: a highlight inside a longer one, after it in the input
/// and before it, and sharing its first kill.
fn nested_cases() -> Vec<Case> {
    let frames = frame_times();
    let demo = "D:/demos/nested_pov.dem";
    let player = ("m00cat", 3);
    let outer = highlight(demo, player, &[1000, 1200, 1400], false, &frames);
    let inner = highlight(demo, player, &[1100, 1150], false, &frames);
    let inner_same_start = highlight(demo, player, &[1000, 1150], false, &frames);
    let next = highlight(demo, player, &[1440, 1600], false, &frames);
    let settings = Settings {
        capture_directory: false,
        ..GOLDEN_SETTINGS
    };
    vec![
        Case::build(
            "nested_after".to_string(),
            settings,
            vec![outer.clone(), inner.clone(), next.clone()],
        ),
        Case::build(
            "nested_before".to_string(),
            settings,
            vec![inner, outer.clone(), next],
        ),
        Case::build(
            "nested_same_start".to_string(),
            settings,
            vec![outer, inner_same_start],
        ),
    ]
}

// ── Generated payloads ───────────────────────────────────────────────────────

/// SplitMix64: a small seeded generator, so a failing case is the same on
/// every run and machine.
struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number in `lo..=hi`.
    fn range(&mut self, lo: i32, hi: i32) -> i32 {
        if hi <= lo {
            return lo;
        }
        lo + (self.next_u64() % (hi - lo + 1) as u64) as i32
    }

    /// One of `items`.
    fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[(self.next_u64() % items.len() as u64) as usize]
    }

    /// True `percent` times in a hundred.
    fn chance(&mut self, percent: u64) -> bool {
        self.next_u64() % 100 < percent
    }

    /// `items` in a random order.
    fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = (self.next_u64() % (i as u64 + 1)) as usize;
            items.swap(i, j);
        }
    }
}

/// Where a generated highlight sits against one already in its group.
#[derive(Clone, Copy)]
enum Placement {
    /// Past the other's post-roll: its own take, with a fast-forward between.
    Far,
    /// Starts inside the other's kills or its take separation: one take.
    Overlapping,
    /// Every kill strictly between the other's first and last (#1395).
    Nested,
    /// On a merge or roll boundary, a tick either side of it or on it.
    Touching,
    /// Starts on the other's first kill.
    SameStart,
}

const PLACEMENTS: [Placement; 5] = [
    Placement::Far,
    Placement::Overlapping,
    Placement::Nested,
    Placement::Touching,
    Placement::SameStart,
];

/// The settings' reach in ticks, as `cut_blocks` works it out: how close two
/// highlights' kills come before they share a take (`merge`) and before
/// their rolls collide (`rolls`).
struct Reach {
    merge: i32,
    rolls: i32,
}

impl Reach {
    fn of(settings: &Settings) -> Self {
        let ticks = |seconds: f32| (seconds * FPS) as i32;
        let separation = take_separation_seconds(&PatcherConfig {
            capture_mode: settings.mode,
            ..PatcherConfig::default()
        });
        let merge = ticks(settings.record_start_lead)
            + ticks(settings.record_stop_trail)
            + ticks(separation);
        let rolls = ticks(settings.record_start_lead)
            + ticks(settings.pre_roll_seconds)
            + ticks(settings.record_stop_trail)
            + ticks(settings.post_roll_seconds);
        Reach { merge, rolls }
    }
}

/// Kill ticks starting at `first`: one to four kills, mostly a few seconds
/// apart, sometimes over the golden config's 8 s kill gap.
fn kills_from(first: i32, rng: &mut Rng) -> Vec<i32> {
    let mut kills = vec![first];
    for _ in 0..rng.range(0, 3) {
        let gap = if rng.chance(15) {
            rng.range(900, 1300)
        } else {
            rng.range(15, 400)
        };
        kills.push(kills[kills.len() - 1] + gap);
    }
    kills
}

/// Kills for a new highlight placed against `other` (its kill ticks).
fn placed_kills(rng: &mut Rng, other: &[i32], reach: &Reach) -> Vec<i32> {
    let first = other[0];
    let last = other[other.len() - 1];
    match rng.pick(&PLACEMENTS) {
        Placement::Far => kills_from(last + reach.rolls + rng.range(50, 3000), rng),
        Placement::Overlapping => kills_from(rng.range(first + 1, last + reach.merge), rng),
        Placement::Nested if last - first >= 4 => {
            let start = rng.range(first + 1, last - 2);
            let end = rng.range(start + 1, last - 1);
            let mut kills = vec![start];
            if rng.chance(50) {
                kills.push(rng.range(start, end));
            }
            kills.push(end);
            kills.dedup();
            kills
        }
        Placement::Nested | Placement::SameStart => kills_from(first, rng),
        Placement::Touching => {
            let boundary = last + rng.pick(&[reach.merge, reach.rolls]);
            kills_from(boundary + rng.range(-1, 1), rng)
        }
    }
}

/// Where a group's first highlight starts: mostly mid-demo, sometimes in its
/// first second (the record start clamps at frame 0) or its last few (the
/// stop clamps at the exit frame).
fn group_start(rng: &mut Rng) -> i32 {
    match rng.range(0, 9) {
        0 => rng.range(10, 120),
        1 => rng.range(FRAMES - 400, FRAMES - 100),
        _ => rng.range(300, FRAMES / 2),
    }
}

/// A Kill Range edit in the game's Review: the range drops the first or the
/// last kill, `start_tick` and `end_tick` stay the whole highlight's.
fn edit_range(rng: &mut Rng, streak: &mut CaptureStreak) {
    if streak.kills.len() < 3 {
        return;
    }
    if rng.chance(50) {
        streak.start_index = 1;
    } else {
        streak.end_index = streak.kills.len() - 2;
    }
}

/// One generated payload: one to three demos, one or two players each (two
/// make it an HLTV demo), one to five highlights per player, placed against
/// each other, in a shuffled order.
fn generated_payload(rng: &mut Rng, settings: &Settings) -> Vec<CaptureStreak> {
    const PLAYERS: [&str; 4] = ["m00cat", "Nifty", "stealthlol", "k4b00m"];
    let frames = frame_times();
    let reach = Reach::of(settings);
    let mut highlights = Vec::new();
    let mut previous_group_start: Option<i32> = None;
    for demo_idx in 0..rng.range(1, 3) {
        let demo = format!("D:/demos/generated_{demo_idx}.dem");
        let players = rng.range(1, 2) as usize;
        for (slot, name) in PLAYERS.iter().enumerate().take(players) {
            let mut group: Vec<Vec<i32>> = Vec::new();
            for _ in 0..rng.range(1, 5) {
                let mut kills = match (group.is_empty(), previous_group_start) {
                    // Groups whose first highlights start on one tick (#1191).
                    (true, Some(start)) if rng.chance(25) => kills_from(start, rng),
                    (true, _) => kills_from(group_start(rng), rng),
                    (false, _) => {
                        let other = group[rng.range(0, group.len() as i32 - 1) as usize].clone();
                        placed_kills(rng, &other, &reach)
                    }
                };
                kills.retain(|&t| (1..=FRAMES - 10).contains(&t));
                if kills.is_empty() {
                    continue;
                }
                let mut streak = highlight(&demo, (name, slot), &kills, players > 1, &frames);
                if rng.chance(15) {
                    edit_range(rng, &mut streak);
                }
                if group.is_empty() {
                    previous_group_start = Some(kills[0]);
                }
                group.push(kills);
                highlights.push(streak);
            }
        }
    }
    rng.shuffle(&mut highlights);
    highlights
}

/// Record settings for a generated payload, every lead and trail above zero.
fn generated_settings(rng: &mut Rng) -> Settings {
    Settings {
        mode: rng.pick(&[CaptureMode::FrameSequence, CaptureMode::Obs]),
        record_start_lead: rng.pick(&[0.5, 1.0, 2.0]),
        record_stop_trail: rng.pick(&[0.5, 1.0, 2.0]),
        pre_roll_seconds: rng.pick(&[1.0, 2.0, 5.0]),
        post_roll_seconds: rng.pick(&[0.5, 1.0, 3.0]),
        capture_directory: false,
    }
}

fn generated_cases() -> Vec<Case> {
    (0..GENERATED_CASES)
        .map(|seed| {
            let mut rng = Rng(seed);
            let settings = generated_settings(&mut rng);
            let highlights = generated_payload(&mut rng, &settings);
            Case::build(format!("generated_{seed}"), settings, highlights)
        })
        .filter(|case| !case.highlights.is_empty())
        .collect()
}

// ── Rules ────────────────────────────────────────────────────────────────────

/// Runs `rule` over every case and fails with every violation found, and the
/// first failing case's input.
fn check_every_case(rule: impl Fn(&Case) -> Vec<String>) {
    let mut failures = Vec::new();
    let mut first_failing = None;
    for case in cases() {
        let violations = rule(case);
        if !violations.is_empty() && first_failing.is_none() {
            first_failing = Some(case.describe());
        }
        failures.extend(
            violations
                .into_iter()
                .map(|v| format!("{}: {v}", case.name)),
        );
    }
    if let Some(input) = first_failing {
        let shown: Vec<&String> = failures.iter().take(8).collect();
        panic!(
            "{} violation(s), first {}:\n{}\nfirst failing input:\n{input}",
            failures.len(),
            shown.len(),
            shown
                .iter()
                .map(|s| format!("  {s}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
}

/// The frame time of `tick` in `highlight`'s demo, `None` past its end.
fn time_of(highlight: &CaptureStreak, tick: i32) -> Option<f32> {
    usize::try_from(tick)
        .ok()
        .and_then(|i| highlight.frame_times.get(i).copied())
}

/// The frame `record_stop_tick` is clamped to at the demo's end
/// (`schedule::clip_window`).
fn exit_frame(highlight: &CaptureStreak) -> i32 {
    highlight
        .total_demo_frames
        .max(highlight.frame_times.len() as i32)
        .saturating_sub(5)
}

/// `CaptureBlock::record_start_tick` is the first frame in the take and
/// `record_stop_tick` the frame recording stops on (`types.rs`): a take
/// starts before it stops.
#[test]
fn every_take_starts_before_it_stops() {
    check_every_case(|case| {
        case.jobs
            .iter()
            .flat_map(|job| &job.blocks)
            .filter(|b| b.record_start_tick >= b.record_stop_tick)
            .map(|b| {
                format!(
                    "{} block {} records {}..{}",
                    b.demo_name, b.block_index, b.record_start_tick, b.record_stop_tick
                )
            })
            .collect()
    });
}

/// Every kill in a highlight's Kill Range lies in its take, with the start
/// lead before the first and the stop trail after the last
/// (`schedule::clip_window`: "the record bounds a start lead before its
/// first kill and a stop trail after its last"), except where the demo's
/// start or end clamps them.
#[test]
fn every_highlights_kills_lead_in_and_trail_lie_inside_its_take() {
    check_every_case(|case| {
        let lead = case.config.record_start_lead;
        let trail = case.config.record_stop_trail;
        let mut violations = Vec::new();
        for block in case.jobs.iter().flat_map(|job| &job.blocks) {
            let (start, stop) = (block.record_start_tick, block.record_stop_tick);
            for &idx in &block.source_streak_indices {
                let h = &case.highlights[idx];
                let where_ = format!(
                    "highlight {idx} in {} block {}",
                    block.demo_name, block.block_index
                );
                if let Some(kill) = block_kill_frames(h)
                    .into_iter()
                    .find(|k| !(start..=stop).contains(k))
                {
                    violations.push(format!("{where_}: kill {kill} outside {start}..={stop}"));
                    continue;
                }
                let (first, last) = (first_kill_frame(h), last_kill_frame(h));
                let lead_in = time_of(h, start).zip(time_of(h, first));
                if start > 0 && lead_in.is_some_and(|(s, f)| s > f - lead + TIME_SLACK_SECONDS) {
                    violations.push(format!(
                        "{where_}: record start {start} is less than {lead} s before kill {first}"
                    ));
                }
                let trail_out = time_of(h, stop).zip(time_of(h, last));
                if stop < exit_frame(h)
                    && trail_out.is_some_and(|(s, l)| s < l + trail - TIME_SLACK_SECONDS)
                {
                    violations.push(format!(
                        "{where_}: record stop {stop} is less than {trail} s after kill {last}"
                    ));
                }
            }
        }
        violations
    });
}

/// `CaptureBlock::source_streak_indices` carries which dispatched highlights
/// a take records (`types.rs`): each is recorded once, by one take.
#[test]
fn every_highlight_is_in_exactly_one_take() {
    check_every_case(|case| {
        let mut takes = vec![0usize; case.highlights.len()];
        for block in case.jobs.iter().flat_map(|job| &job.blocks) {
            for &idx in &block.source_streak_indices {
                match takes.get_mut(idx) {
                    Some(count) => *count += 1,
                    None => {
                        return vec![format!("block points at highlight {idx}, past the payload")];
                    }
                }
            }
        }
        takes
            .iter()
            .enumerate()
            .filter(|(_, count)| **count != 1)
            .map(|(idx, count)| format!("highlight {idx} is in {count} takes"))
            .collect()
    });
}

/// One chain job per demo and player (`blocks.rs`, `passes.rs`), holding
/// only that demo and player's highlights, in the order their first
/// highlights start (`group_highlights`).
#[test]
fn one_job_per_demo_and_player_in_start_order() {
    check_every_case(|case| {
        let mut violations = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut previous_start = i32::MIN;
        for job in &case.jobs {
            let key = (job.source_demo.as_str(), job.target_player.as_deref());
            if !seen.insert(key) {
                violations.push(format!("a second job for {key:?}"));
            }
            let indices: Vec<usize> = job
                .blocks
                .iter()
                .flat_map(|b| b.source_streak_indices.iter().copied())
                .collect();
            for &idx in &indices {
                let h = &case.highlights[idx];
                if (h.source_demo.as_str(), h.target_player.as_deref()) != key {
                    violations.push(format!("highlight {idx} is in the job for {key:?}"));
                }
            }
            let start = indices
                .iter()
                .map(|&i| case.highlights[i].start_tick)
                .min()
                .unwrap_or(i32::MIN);
            if start < previous_start {
                violations.push(format!(
                    "the job for {key:?} starts at {start}, before the job ahead of it ({previous_start})"
                ));
            }
            previous_start = start;
        }
        violations
    });
}

/// Two takes of one job never overlap, and the gap between them is at least
/// the mode's take separation (`cut_blocks`: recordings that overlap must be
/// one take, "you can't run two mirv_recordmovie sessions at once"; and the
/// separation is what a stop/start cycle needs, #9).
#[test]
fn a_jobs_takes_are_apart_by_the_take_separation() {
    check_every_case(|case| {
        let separation = take_separation_seconds(&case.config);
        let mut violations = Vec::new();
        for job in &case.jobs {
            for pair in job.blocks.windows(2) {
                let (before, after) = (&pair[0], &pair[1]);
                let Some(&idx) = after.source_streak_indices.first() else {
                    continue;
                };
                let h = &case.highlights[idx];
                let gap = time_of(h, after.record_start_tick)
                    .zip(time_of(h, before.record_stop_tick))
                    .map(|(start, stop)| start - stop);
                if after.record_start_tick <= before.record_stop_tick
                    || gap.is_some_and(|gap| gap < separation - TIME_SLACK_SECONDS)
                {
                    violations.push(format!(
                        "{} blocks {} and {}: {}..{} then {}..{}",
                        job.source_demo,
                        before.block_index,
                        after.block_index,
                        before.record_start_tick,
                        before.record_stop_tick,
                        after.record_start_tick,
                        after.record_stop_tick
                    ));
                }
            }
        }
        violations
    });
}

/// A take's plan holds every kill the take records, the merged highlights'
/// too, each inside the take (`take_plan` in `mod.rs`: "so the check finds
/// any of them outside the video"). Every case's highlights carry `viewdemo`
/// times, so every block has a plan.
#[test]
fn every_takes_plan_holds_each_of_its_kills_inside_it() {
    check_every_case(|case| {
        let mut violations = Vec::new();
        for block in case.jobs.iter().flat_map(|job| &job.blocks) {
            let where_ = format!("{} block {}", block.demo_name, block.block_index);
            let Some(plan) = &block.plan else {
                violations.push(format!("{where_} has no plan"));
                continue;
            };
            let mut kills: Vec<i32> = block
                .source_streak_indices
                .iter()
                .flat_map(|&i| block_kill_frames(&case.highlights[i]))
                .collect();
            kills.sort_unstable();
            kills.dedup();
            if plan.kill_offsets_secs.len() != kills.len() {
                violations.push(format!(
                    "{where_} plans {} kills, its highlights have {}",
                    plan.kill_offsets_secs.len(),
                    kills.len()
                ));
            }
            let slack = f64::from(TIME_SLACK_SECONDS);
            if let Some(at) = plan
                .kill_offsets_secs
                .iter()
                .find(|&&at| at < -slack || at > plan.length_secs + slack)
            {
                violations.push(format!(
                    "{where_} plans a kill at {at:.2} s of a {:.2} s take",
                    plan.length_secs
                ));
            }
        }
        violations
    });
}

/// The take cleanup's disk estimate and the batch read one plan (`passes.rs`,
/// #1085): one figure per block the batch records, in play order, each the
/// sum of its highlights' own estimates.
#[test]
fn the_disk_estimate_bills_the_takes_the_batch_records() {
    check_every_case(|case| {
        let billed: Vec<u64> = case
            .jobs
            .iter()
            .flat_map(|job| &job.blocks)
            .map(|block| {
                block
                    .source_streak_indices
                    .iter()
                    .map(|&i| block_byte_estimate(&case.config, &case.highlights[i], FPS))
                    .fold(0u64, u64::saturating_add)
            })
            .collect();
        let estimate = estimate_batch_capture_blocks(&case.highlights, &case.config);
        if estimate == billed {
            Vec::new()
        } else {
            vec![format!("estimate {estimate:?}, batch {billed:?}")]
        }
    });
}

// ── Same batch in any order (#1222) ──────────────────────────────────────────

/// A highlight as the order check tells it apart: its demo, player, kills
/// and Kill Range. Indices into the payload change with its order.
fn identity(highlight: &CaptureStreak) -> String {
    let kills: Vec<i32> = highlight.kills.iter().map(|k| k.0).collect();
    format!(
        "{} {:?} {kills:?} {}..={}",
        highlight.source_demo, highlight.target_player, highlight.start_index, highlight.end_index
    )
}

/// What a batch hands the game and the take checks, bar the director
/// events ([`director_view`]), with each block's highlights named by
/// [`identity`] rather than payload index.
fn batch_view(jobs: &[PatchJob], highlights: &[CaptureStreak]) -> Vec<String> {
    let mut lines = Vec::new();
    for job in jobs {
        lines.push(format!(
            "job {} {:?} -> {}",
            job.source_demo,
            job.target_player,
            job.output_demo.display()
        ));
        lines.extend(job.init_commands.iter().map(|c| format!("  init {c}")));
        lines.extend(
            job.scheduled_commands
                .iter()
                .map(|(tick, c)| format!("  at {tick} {c}")),
        );
        lines.extend(job.block_routes.iter().map(|r| format!("  route {r:?}")));
        for block in &job.blocks {
            let mut sources: Vec<String> = block
                .source_streak_indices
                .iter()
                .map(|&i| identity(&highlights[i]))
                .collect();
            sources.sort();
            let mut block = block.clone();
            block.source_streak_indices.clear();
            lines.push(format!(
                "  block {} of {sources:?}",
                serde_json::to_string(&block).expect("a block serialises")
            ));
        }
    }
    lines
}

/// The orders the check rebuilds a payload in besides its own: reversed,
/// rotated by one, and shuffled.
fn reorderings(highlights: &[CaptureStreak], seed: u64) -> Vec<Vec<CaptureStreak>> {
    let mut reversed = highlights.to_vec();
    reversed.reverse();
    let mut rotated = highlights.to_vec();
    rotated.rotate_left(1);
    let mut shuffled = highlights.to_vec();
    Rng(seed ^ 0x5EED).shuffle(&mut shuffled);
    vec![reversed, rotated, shuffled]
}

/// Each job's director events: the `viewdemo` Event List's entries.
fn director_view(jobs: &[PatchJob], _highlights: &[CaptureStreak]) -> Vec<String> {
    jobs.iter()
        .flat_map(|job| {
            job.director_events
                .iter()
                .map(move |(tick, label)| format!("{} director {tick} {label}", job.source_demo))
        })
        .collect()
}

/// Rebuilds every case in each of [`reorderings`] and fails when `view`
/// of the batch differs from the case's own order. `tag` names the scratch
/// folder, one per test.
fn check_any_order(tag: &str, view: fn(&[PatchJob], &[CaptureStreak]) -> Vec<String>) {
    let root = Scratch::new(format!("planner_rules_{tag}"));
    check_every_case(|case| {
        let config = make_config(&root, &case.settings);
        let (jobs, _) = build_golden_batch(&case.highlights, &config);
        let expected = view(&jobs, &case.highlights);
        let seed = case
            .name
            .bytes()
            .fold(0u64, |h, b| h.wrapping_mul(31).wrapping_add(u64::from(b)));
        reorderings(&case.highlights, seed)
            .into_iter()
            .enumerate()
            .filter_map(|(n, reordered)| {
                let (jobs, _) = build_golden_batch(&reordered, &config);
                let actual = view(&jobs, &reordered);
                (actual != expected).then(|| {
                    let difference = actual
                        .iter()
                        .zip(&expected)
                        .find(|(a, e)| a != e)
                        .map(|(a, e)| format!("got `{a}`, expected `{e}`"))
                        .unwrap_or_else(|| "one batch is longer".to_string());
                    format!("reordering {n}: {difference}")
                })
            })
            .collect()
    });
}

/// The same highlights in any order make the same batch (#1222): the same
/// jobs, commands, routes and takes, each take recording the same
/// highlights.
#[test]
fn the_same_highlights_in_any_order_make_the_same_batch() {
    check_any_order("order", batch_view);
}

/// The same highlights in any order label the same director events (#1222).
#[test]
#[ignore = "#1417: highlights of one player that share a first kill are numbered in the order they were sent"]
fn the_same_highlights_in_any_order_make_the_same_director_events() {
    check_any_order("order_director", director_view);
}

/// The generator makes every placement the rules are meant to cover, so a
/// change to it can't quietly stop testing one.
#[test]
fn the_generated_payloads_cover_nested_touching_and_multi_demo_shapes() {
    let generated: Vec<&Case> = cases()
        .iter()
        .filter(|c| c.name.starts_with("generated_"))
        .collect();
    let merged_takes = generated
        .iter()
        .flat_map(|c| c.jobs.iter().flat_map(|j| &j.blocks))
        .filter(|b| b.source_streak_indices.len() > 1)
        .count();
    let nested = generated
        .iter()
        .filter(|c| {
            c.highlights.iter().any(|outer| {
                c.highlights.iter().any(|inner| {
                    inner.source_demo == outer.source_demo
                        && inner.target_player == outer.target_player
                        && first_kill_frame(inner) > first_kill_frame(outer)
                        && last_kill_frame(inner) < last_kill_frame(outer)
                })
            })
        })
        .count();
    let multi_demo = generated.iter().filter(|c| c.jobs.len() > 1).count();
    assert!(
        generated.len() >= 100,
        "{} generated cases",
        generated.len()
    );
    assert!(merged_takes >= 50, "{merged_takes} merged takes");
    assert!(nested >= 10, "{nested} cases with a nested highlight");
    assert!(multi_demo >= 50, "{multi_demo} cases with several jobs");
}
