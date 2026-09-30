//! Flag captures, cap credits and cap blocks as the analyzer now reports them
//! (#192), one TSV row per demo and a totals line.
//!
//! This runs the real `Analysis`, so every figure is post-match-start: warm-up
//! captures are wiped with the rest of the scoreboard. `capwindow_probe` is
//! the raw, whole-file measurement this is checked against.
//!
//!     cargo run --release -p analysis --example objective_probe -- demo.dem
//!     cargo run --release -p analysis --example objective_probe -- <folder> --limit 20
//!
//! `--limit N` stops after N demos, so a re-run does not have to walk a whole
//! library. The release profile aborts on a parser panic, so for a sweep
//! over an untrusted folder drive it one process per demo instead.

use analysis::{Analysis, AttemptOutcome};
use dem::open_demo_from_bytes;
use dem::types::{EngineMessage, FrameData, MessageData, NetMessage};
use std::path::PathBuf;

#[derive(Default)]
struct Row {
    demos: u32,
    captures: u32,
    // Captures by number of co-cappers: 0, 1, 2, 3 or more.
    co_hist: [u32; 4],
    credits: u32,
    player_credits: u32,
    no_area: u32,
    no_capper: u32,
    breaks: u32,
    attempts: u32,
    captured: u32,
    cancelled: u32,
    restarted: u32,
    round_ended: u32,
    in_progress: u32,
    obj_points: u32,
    teamkills: u32,
    suicides: u32,
}

impl Row {
    fn add(&mut self, other: &Row) {
        self.demos += other.demos;
        self.captures += other.captures;
        for (total, n) in self.co_hist.iter_mut().zip(other.co_hist) {
            *total += n;
        }
        self.credits += other.credits;
        self.player_credits += other.player_credits;
        self.no_area += other.no_area;
        self.no_capper += other.no_capper;
        self.breaks += other.breaks;
        self.attempts += other.attempts;
        self.captured += other.captured;
        self.cancelled += other.cancelled;
        self.restarted += other.restarted;
        self.round_ended += other.round_ended;
        self.in_progress += other.in_progress;
        self.obj_points += other.obj_points;
        self.teamkills += other.teamkills;
        self.suicides += other.suicides;
    }

    fn print(&self, label: &str, kind: &str) {
        println!(
            "{label}\t{kind}\t{}\t{}\t{:?}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            self.demos,
            self.captures,
            self.co_hist,
            self.credits,
            self.player_credits,
            self.no_area,
            self.no_capper,
            self.breaks,
            self.attempts,
            self.captured,
            self.cancelled,
            self.restarted,
            self.round_ended,
            self.in_progress,
            self.obj_points,
            self.teamkills,
            self.suicides,
        );
    }
}

/// `SvcHltv` is the reliable HLTV marker; `SvcDirector` also shows up in POV
/// demos whenever a caster is spectating.
fn is_hltv(bytes: &[u8]) -> Option<bool> {
    let demo = open_demo_from_bytes(bytes).ok()?;
    let found = demo
        .directory
        .entries
        .iter()
        .flat_map(|entry| entry.frames.iter())
        .any(|frame| {
            let FrameData::NetworkMessage(bt) = &frame.frame_data else {
                return false;
            };
            let MessageData::Parsed(msgs) = &bt.1.messages else {
                return false;
            };
            msgs.iter().any(|m| {
                matches!(m, NetMessage::EngineMessage(e) if matches!(**e, EngineMessage::SvcHltv(_)))
            })
        });
    Some(found)
}

fn measure(analysis: &Analysis) -> Row {
    let state = &analysis.state;
    let objectives = &state.objectives;
    let mut row = Row {
        demos: 1,
        captures: objectives.captures.len() as u32,
        ..Default::default()
    };
    for capture in &objectives.captures {
        row.co_hist[capture.co_cappers.len().min(3)] += 1;
        row.credits += capture.cappers().count() as u32;
        row.no_area += capture.area_index.is_none() as u32;
        row.no_capper += capture.capper.is_none() as u32;
        row.breaks += capture.is_break() as u32;
    }
    for attempt in &objectives.attempts {
        row.attempts += 1;
        match attempt.outcome {
            AttemptOutcome::Captured => row.captured += 1,
            AttemptOutcome::Cancelled => row.cancelled += 1,
            AttemptOutcome::Restarted => row.restarted += 1,
            AttemptOutcome::RoundEnded => row.round_ended += 1,
            AttemptOutcome::InProgress => row.in_progress += 1,
        }
    }
    for player in &state.players {
        row.player_credits += player.cap_credits;
        row.obj_points += player.obj_points;
        row.teamkills += player.teamkills;
        row.suicides += player.suicides;
    }
    row
}

fn demo_paths(args: &[String]) -> (Vec<PathBuf>, usize) {
    let mut limit = usize::MAX;
    let mut paths = vec![];
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if arg == "--limit" {
            limit = args.next().and_then(|n| n.parse().ok()).unwrap_or(limit);
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
    (paths, limit)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (paths, limit) = demo_paths(&args);

    println!(
        "file\ttype\tdemos\tcaptures\tco_capper_hist[0,1,2,3+]\tcredits\tplayer_credits\tno_area\tno_capper\tbreaks\tattempts\tcaptured\tcancelled\trestarted\tround_ended\tin_progress\tobj_points\tteamkills\tsuicides"
    );
    let mut totals = [Row::default(), Row::default()];
    for path in paths.iter().take(limit) {
        let file = path.file_name().unwrap_or_default().to_string_lossy();
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("{file}: unreadable");
            continue;
        };
        let Some(hltv) = is_hltv(&bytes) else {
            eprintln!("{file}: does not parse");
            continue;
        };
        let analysis = match Analysis::try_from_bytes(&bytes) {
            Ok(analysis) => analysis,
            Err(e) => {
                eprintln!("{file}: {e}");
                continue;
            }
        };
        let row = measure(&analysis);
        let kind = if hltv { "HLTV" } else { "POV" };
        row.print(&file, kind);
        totals[hltv as usize].add(&row);
    }

    for (total, kind) in totals.iter().zip(["POV", "HLTV"]) {
        if total.demos == 0 {
            continue;
        }
        total.print("TOTAL", kind);
        let pct = |n: u32, of: u32| 100.0 * n as f64 / of.max(1) as f64;
        let multi = total.captures - total.co_hist[0];
        println!(
            "  {kind}: {} captures, {:.1}% multi-capper ({:.1}% one co-capper, {:.1}% two, {:.1}% three+), {:.3} credits per capture; {:.1}% breaks",
            total.captures,
            pct(multi, total.captures),
            pct(total.co_hist[1], total.captures),
            pct(total.co_hist[2], total.captures),
            pct(total.co_hist[3], total.captures),
            total.credits as f64 / total.captures.max(1) as f64,
            pct(total.breaks, total.captures),
        );
        println!(
            "  {kind}: {} timed attempts, {} cancelled ({:.1}%), {} captured, {} restarted, {} ended by the round, {} still running",
            total.attempts,
            total.cancelled,
            pct(total.cancelled, total.attempts),
            total.captured,
            total.restarted,
            total.round_ended,
            total.in_progress,
        );
    }
}
