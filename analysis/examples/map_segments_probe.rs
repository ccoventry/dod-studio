//! What does each map of a multi-map demo analyse to? (#217)
//!
//! A demo that kept recording through a level change holds one map segment per
//! signon (`DemoInfo::map_segments`). This lists them, then prints the default
//! analysis and each segment's own analysis side by side: players with team,
//! class and K/D, team scores, rounds and the durations the Summary tab shows.
//!
//!     cargo run --release -p analysis --example map_segments_probe -- <demo>...
//!
//! `--fingerprint` prints only a hash of the default analysis and how long it
//! took, one line per demo, for checking that a change leaves ordinary demos
//! alone. The hash is over the analysis's sorted `{:#?}` lines (hash maps print
//! in a random order), without the fields #217 added.
use analysis::{Analysis, AnalysisOptions, AnalyzerState, Round, Team};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let fingerprint = args.iter().any(|a| a == "--fingerprint");
    for path in args.iter().filter(|a| !a.starts_with("--")) {
        let Ok(bytes) = std::fs::read(path) else {
            println!("{path}: unreadable");
            continue;
        };
        let started = Instant::now();
        let analysis = match Analysis::try_from_bytes(&bytes) {
            Ok(a) => a,
            Err(e) => {
                println!("{path}: {e}");
                continue;
            }
        };
        let elapsed = started.elapsed();
        let name = std::path::Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        if fingerprint {
            println!(
                "{name}\t{:016x}\t{} ms\t{} segment(s)",
                state_hash(&analysis.state),
                elapsed.as_millis(),
                analysis.demo_info.map_segments.len()
            );
            continue;
        }

        println!(
            "\n{name}  ({:.1} MB, {} ms)",
            bytes.len() as f64 / 1e6,
            elapsed.as_millis()
        );
        for (i, s) in analysis.demo_info.map_segments.iter().enumerate() {
            println!(
                "  segment {i}: {} {}-{}  (frame {})",
                s.map_name,
                mmss(s.start_secs),
                mmss(s.end_secs),
                s.start_frame
            );
        }
        println!(
            "\n  == default (analyses segment {})",
            analysis.state.map_segment
        );
        summarise(&analysis.state);

        if analysis.demo_info.map_segments.len() > 1 {
            for i in 0..analysis.demo_info.map_segments.len() {
                let started = Instant::now();
                let options = AnalysisOptions { segment: Some(i) };
                match Analysis::try_from_bytes_with_options(&bytes, options, |_, _| {}) {
                    Ok(a) => {
                        println!("\n  == segment {i} ({} ms)", started.elapsed().as_millis());
                        summarise(&a.state);
                    }
                    Err(e) => println!("\n  == segment {i}: {e}"),
                }
            }
        }
    }
}

fn mmss(secs: f32) -> String {
    let s = secs.max(0.0) as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

fn summarise(st: &AnalyzerState) {
    let match_secs = match (st.rounds.first(), st.rounds.last()) {
        (Some(first), Some(last)) => {
            let start = match first {
                Round::Active { start_time, .. } | Round::Completed { start_time, .. } => {
                    start_time.viewdemo_offset
                }
            };
            let end = match last {
                Round::Completed { end_time, .. } => end_time.viewdemo_offset,
                Round::Active { .. } => st.current_time.viewdemo_offset,
            };
            end.saturating_sub(start).as_secs_f32()
        }
        _ => 0.0,
    };
    println!(
        "  map {:?}, demo duration {}, match duration {}, rounds {}, clan match {}, map_changed {}",
        st.initial_map_name,
        mmss(st.current_time.viewdemo_offset.as_secs_f32()),
        mmss(match_secs),
        st.rounds.len(),
        st.clan_match_detected,
        st.map_changed
    );
    let allies = if st.allies_are_british {
        Team::British
    } else {
        Team::Allies
    };
    println!(
        "  score: {:?} {} - Axis {}",
        allies,
        st.team_scores.get_team_score(allies.clone()),
        st.team_scores.get_team_score(Team::Axis)
    );
    let mut players: Vec<_> = st.players.iter().collect();
    players.sort_by_key(|p| (format!("{:?}", p.team), -p.stats.1));
    for p in players {
        println!(
            "    {:<28} {:<12} {:<22} K {:>3}  D {:>3}  streak kills {}",
            p.name,
            p.team
                .as_ref()
                .map(|t| format!("{t:?}"))
                .unwrap_or("Unassigned".into()),
            p.class
                .as_ref()
                .map(|c| format!("{c:?}"))
                .unwrap_or("Unassigned".into()),
            p.stats.1,
            p.stats.2,
            p.kill_streaks.iter().map(|k| k.kills.len()).sum::<usize>()
        );
    }
}

fn state_hash(st: &AnalyzerState) -> u64 {
    let dump = format!("{st:#?}");
    let mut lines: Vec<&str> = dump
        .lines()
        .filter(|l| !l.trim_start().starts_with("map_segment:"))
        .collect();
    lines.sort_unstable();
    let mut h = DefaultHasher::new();
    lines.hash(&mut h);
    h.finish()
}
