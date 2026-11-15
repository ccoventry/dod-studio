//! How far a cancelled capture is from the death that cancelled it, and what
//! the analyzer credits as cap breaks (#192).
//!
//! `StartProg` and `CancelProg` name no player, so a break is attributed by
//! timing: the attacker who died just before the `CancelProg` was a capper,
//! and whoever killed them broke the capture. This measures that gap, so the
//! window in `objective::BREAK_WINDOW` comes from data:
//!
//! - for every cancelled attempt, the gap from the latest death of an
//!   attacking-team player at or before the cancel (and the earliest one
//!   after it, in case a cancel can arrive first);
//! - a baseline: the same lookup 5 s before each cancel, which is how often
//!   an unrelated attacker death lands in a window by chance;
//! - per demo, the analyzer's own `cap_breaks`, `blocks_unattributed` and
//!   `attempts_abandoned`.
//!
//!     cargo run --release -p analysis --example cap_break_probe -- <folder> --limit 20
//!
//! `--threads N` analyses N demos at once (default 4).

use analysis::{Analysis, AttemptOutcome, BREAK_WINDOW, Team};
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Upper bounds of the gap buckets, in seconds. A gap of exactly zero is the
/// same server tick.
const BUCKETS: [f64; 16] = [
    0.0, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0, 1.1, 1.2, 1.3, 1.5, 2.0,
];

#[derive(Default)]
struct Totals {
    demos: u32,
    cancelled: u32,
    // Gap from the latest attacker death at or before the cancel.
    before: [u32; BUCKETS.len() + 1],
    // Same, 5 s earlier: the chance rate.
    baseline: [u32; BUCKETS.len() + 1],
    // Earliest attacker death after the cancel, when none came before
    // within 2 s.
    after: [u32; BUCKETS.len() + 1],
    // Killer of the matched death (within BREAK_WINDOW): defender, teammate,
    // world.
    by_defender: u32,
    by_teammate: u32,
    by_world: u32,
    breaks: u32,
    unattributed: u32,
    abandoned: u32,
}

fn bucket(gap: f64) -> usize {
    BUCKETS
        .iter()
        .position(|upper| gap <= *upper + 1e-9)
        .unwrap_or(BUCKETS.len())
}

fn same_side(a: &Team, b: &Team) -> bool {
    let norm = |t: &Team| match t {
        Team::British => Team::Allies,
        other => other.clone(),
    };
    norm(a) == norm(b)
}

fn measure(analysis: &Analysis, totals: &mut Totals, file: &str) {
    let state = &analysis.state;
    let secs = |d: &std::time::Duration| d.as_secs_f64();
    totals.demos += 1;

    for attempt in &state.objectives.attempts {
        if attempt.outcome != AttemptOutcome::Cancelled {
            continue;
        }
        let Some(ended) = &attempt.ended else {
            continue;
        };
        totals.cancelled += 1;
        let cancel = secs(&ended.viewdemo_offset);
        let attacker_deaths = state.kill_positions.iter().filter(|k| {
            k.victim_team
                .as_ref()
                .is_some_and(|t| same_side(t, &attempt.team))
        });

        let latest_before = |at: f64| {
            attacker_deaths
                .clone()
                .filter(|k| secs(&k.time.viewdemo_offset) <= at + 1e-9)
                .max_by(|a, b| {
                    secs(&a.time.viewdemo_offset).total_cmp(&secs(&b.time.viewdemo_offset))
                })
        };
        // A death before the attempt started cannot be one of its cappers.
        let before = latest_before(cancel)
            .filter(|k| k.time.viewdemo_offset >= attempt.started.viewdemo_offset);
        let gap = before.map(|k| cancel - secs(&k.time.viewdemo_offset));
        totals.before[gap.map_or(BUCKETS.len(), bucket)] += 1;
        let base_gap =
            latest_before(cancel - 5.0).map(|k| cancel - 5.0 - secs(&k.time.viewdemo_offset));
        totals.baseline[base_gap.map_or(BUCKETS.len(), bucket)] += 1;

        if gap.is_none_or(|g| g > 2.0) {
            let after = attacker_deaths
                .clone()
                .filter(|k| secs(&k.time.viewdemo_offset) > cancel)
                .map(|k| secs(&k.time.viewdemo_offset) - cancel)
                .min_by(f64::total_cmp);
            totals.after[after.map_or(BUCKETS.len(), bucket)] += 1;
        }

        if let (Some(k), Some(g)) = (before, gap)
            && g <= BREAK_WINDOW.as_secs_f64() + 1e-9
        {
            if k.killer.is_none() {
                totals.by_world += 1;
            } else if k.teamkill {
                totals.by_teammate += 1;
            } else {
                totals.by_defender += 1;
            }
        }
    }

    let breaks: u32 = state.players.iter().map(|p| p.cap_breaks).sum();
    let objectives = &state.objectives;
    totals.breaks += breaks;
    totals.unattributed += objectives.blocks_unattributed;
    totals.abandoned += objectives.attempts_abandoned;
    println!(
        "{file}\tcancelled {}\tbreaks {breaks}\tunattributed {}\tabandoned {}",
        objectives
            .attempts
            .iter()
            .filter(|a| a.outcome == AttemptOutcome::Cancelled)
            .count(),
        objectives.blocks_unattributed,
        objectives.attempts_abandoned,
    );
}

fn demo_paths(args: &[String]) -> (Vec<PathBuf>, usize, usize) {
    let mut limit = usize::MAX;
    let mut threads = 4;
    let mut paths = vec![];
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if arg == "--limit" {
            limit = args.next().and_then(|n| n.parse().ok()).unwrap_or(limit);
            continue;
        }
        if arg == "--threads" {
            threads = args.next().and_then(|n| n.parse().ok()).unwrap_or(threads);
            continue;
        }
        let path = PathBuf::from(arg);
        if path.is_dir() {
            let mut found: Vec<PathBuf> = std::fs::read_dir(&path)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("dem")))
                .collect();
            found.sort();
            paths.extend(found);
        } else {
            paths.push(path);
        }
    }
    paths.truncate(limit);
    (paths, threads.max(1), limit)
}

fn print_hist(label: &str, hist: &[u32]) {
    let total: u32 = hist.iter().sum();
    let mut cumulative = 0;
    println!("{label} (n = {total}):");
    for (i, n) in hist.iter().enumerate() {
        cumulative += n;
        let name = match i {
            0 => "same tick".to_string(),
            i if i < BUCKETS.len() => format!("<= {:.2} s", BUCKETS[i]),
            _ => format!("> {:.0} s or none", BUCKETS[BUCKETS.len() - 1]),
        };
        println!(
            "  {name:>16}  {n:6}  cumulative {:5.1}%",
            100.0 * cumulative as f64 / total.max(1) as f64
        );
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (paths, threads, _) = demo_paths(&args);
    let next = AtomicUsize::new(0);
    let totals = Mutex::new(Totals::default());

    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(path) = paths.get(i) else {
                        break;
                    };
                    let file = path.file_name().unwrap_or_default().to_string_lossy();
                    let Ok(bytes) = std::fs::read(path) else {
                        eprintln!("{file}: unreadable");
                        continue;
                    };
                    match Analysis::try_from_bytes(&bytes) {
                        Ok(analysis) => {
                            let mut totals = totals.lock().unwrap_or_else(|e| e.into_inner());
                            measure(&analysis, &mut totals, &file);
                        }
                        Err(e) => eprintln!("{file}: {e}"),
                    }
                }
            });
        }
    });

    let t = totals.into_inner().unwrap_or_else(|e| e.into_inner());
    println!("\n{} demos, {} cancelled attempts", t.demos, t.cancelled);
    print_hist(
        "Gap from the latest attacker death to the cancel",
        &t.before,
    );
    print_hist("Same, 5 s before the cancel (chance)", &t.baseline);
    print_hist(
        "No attacker death within 2 s before: earliest one after the cancel",
        &t.after,
    );
    println!(
        "Deaths within {:?}: by a defender {}, by a teammate {}, by the world or themself {}",
        BREAK_WINDOW, t.by_defender, t.by_teammate, t.by_world
    );
    println!(
        "Analyzer: cap_breaks {}, blocks_unattributed {}, attempts_abandoned {}",
        t.breaks, t.unattributed, t.abandoned
    );
}
