//! What the capture scanner would find in an HLTV demo, if it were allowed to
//! look.
//!
//! `native/src/patch/scanner.rs` refuses HLTV demos at the header:
//!
//! ```text
//!     match is_hltv_demo(path) {
//!         Ok(true) => return Err("Unsupported HLTV proxy demo format"),
//!         ...
//!     }
//! ```
//!
//! #247's first two questions are whether that guard still protects against
//! anything, and what would come out without it. Both are answerable offline,
//! against the real demo library, and that is what this does.
//!
//! It runs the scanner's own entry point first — so the guard's behaviour is
//! observed rather than described — and then runs the *same* analysis the
//! scanner would have run afterwards, reporting the streaks its per-player loop
//! would have iterated. Nothing here reimplements the scanner: the streak list
//! it walks is `analysis.state.players[..].kill_streaks`, read here directly.
//!
//!     cargo run --release -p native --example hltv_scan_probe -- <demo-or-dir>
//!
//! One line per demo, plus a summary. A demo that panics the parser takes the
//! process with it, which is itself the answer to "does it still break" — run
//! it per file if that matters.

use std::path::{Path, PathBuf};

use native::patch::{is_hltv_demo, scan_demo_for_highlights};

#[derive(Default)]
struct Totals {
    demos: usize,
    hltv: usize,
    refused: usize,
    scanned: usize,
    failed: usize,
    hltv_parsed: usize,
    hltv_streaks: usize,
    hltv_players_with_streaks: usize,
    /// Demos the *analysis* crate calls HLTV, by its own `SvcHltv` test -- the
    /// one `analysis/examples/README.md` calls reliable. The guard's header
    /// test and this disagreeing is the whole finding.
    not_pov: usize,
}

fn main() {
    let target = PathBuf::from(
        std::env::args()
            .nth(1)
            .expect("usage: hltv_scan_probe <demo-or-directory>"),
    );
    let mut demos: Vec<PathBuf> = Vec::new();
    if target.is_dir() {
        for entry in std::fs::read_dir(&target)
            .expect("read directory")
            .flatten()
        {
            let path = entry.path();
            if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("dem"))
            {
                demos.push(path);
            }
        }
        demos.sort();
    } else {
        demos.push(target);
    }

    let mut totals = Totals::default();
    for path in &demos {
        totals.demos += 1;
        report(path, &mut totals);
    }

    println!("\n== summary ==");
    println!(
        "  {} demo(s), {} of them HLTV by the guard's own header test",
        totals.demos, totals.hltv
    );
    println!(
        "  {} of them HLTV by the analysis crate's SvcHltv test",
        totals.not_pov
    );
    println!(
        "  scanner refused {}, scanned {}, failed for other reasons {}",
        totals.refused, totals.scanned, totals.failed
    );
    println!(
        "  of the refused: {} parsed cleanly, yielding {} streak(s) across {} player(s)",
        totals.hltv_parsed, totals.hltv_streaks, totals.hltv_players_with_streaks
    );
}

fn report(path: &Path, totals: &mut Totals) {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let hltv = match is_hltv_demo(path) {
        Ok(v) => v,
        Err(e) => {
            println!("{name}: header unreadable -- {e}");
            totals.failed += 1;
            return;
        }
    };
    if hltv {
        totals.hltv += 1;
    }

    // The scanner's own entry point, so the guard is observed rather than
    // described.
    match scan_demo_for_highlights(path) {
        Ok((_tick, streaks, is_pov, local, frames, _start, _times)) => {
            totals.scanned += 1;
            if !is_pov {
                totals.not_pov += 1;
            }
            println!(
                "{name}: scanner OK -- {} streak(s), demo_type {}, pov_player_index {:?}, {frames} frames",
                streaks.len(),
                if is_pov {
                    "POV"
                } else {
                    "NOT POV (analysis says HLTV)"
                },
                local
            );
            return;
        }
        Err(why) if why.contains("HLTV") => {
            totals.refused += 1;
        }
        Err(why) => {
            totals.failed += 1;
            println!("{name}: scanner failed -- {why}");
            return;
        }
    }

    // Refused. Run what the scanner would have run next.
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            println!("{name}: refused, and unreadable -- {e}");
            return;
        }
    };
    let analysis = match analysis::Analysis::try_from_bytes(&bytes) {
        Ok(a) => a,
        Err(e) => {
            println!("{name}: refused, and the analysis crate cannot parse it either -- {e}");
            return;
        }
    };
    totals.hltv_parsed += 1;

    // Exactly the list the scanner's per-player loop walks: connected players,
    // and their `kill_streaks`.
    let mut streaks = 0usize;
    let mut players = 0usize;
    let mut biggest = 0usize;
    for player in &analysis.state.players {
        if !matches!(player.connection, analysis::Connection::Connected { .. }) {
            continue;
        }
        let with_kills = player
            .kill_streaks
            .iter()
            .filter(|s| !s.kills.is_empty())
            .count();
        if with_kills > 0 {
            players += 1;
            streaks += with_kills;
            biggest = biggest.max(
                player
                    .kill_streaks
                    .iter()
                    .map(|s| s.kills.len())
                    .max()
                    .unwrap_or(0),
            );
        }
    }
    totals.hltv_streaks += streaks;
    totals.hltv_players_with_streaks += players;

    println!(
        "{name}: REFUSED by the guard -- parses fine: {} streak(s) across {} player(s), longest {biggest} kills, demo_type {:?}, pov_player_index {:?}",
        streaks, players, analysis.demo_info.demo_type, analysis.state.pov_player_index
    );
}
