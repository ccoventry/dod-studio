//! Compare a reset-aware kill count against the server's own frag counter.
//!
//! Runs the production `Analysis` pipeline rather than a naive pass, so
//! match-start scoreboard resets and reconnects are handled properly — the
//! difference between 15% and 75% exact agreement. Kill streaks are cleared
//! when a match goes live, so their summed length is the reset-aware derived
//! kill count; `stats.1` is what the server reported.
//!
//! Also compares three models of how teamkills relate to the frag counter.
//! `frags == kills` (teamkills excluded) wins at 75.3% exact.
//!
//!     cargo run --release -p analysis --example reconcile_probe -- demo.dem
//!     cargo run --release -p analysis --example reconcile_probe -- demo.dem -v

use analysis::Analysis;

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: reconcile_probe <demo>");
    let Ok(bytes) = std::fs::read(&path) else {
        println!("READFAIL\t{path}");
        return;
    };
    let a = match Analysis::try_from_bytes(&bytes) {
        Ok(a) => a,
        Err(e) => {
            println!("ANALYSISFAIL\t{path}\t{e}");
            return;
        }
    };
    drop(bytes);

    let file = path.rsplit(['/', '\\']).next().unwrap_or(&path);
    let verbose = std::env::args().any(|x| x == "-v");

    let mut checked = 0i64;
    let mut exact = 0i64;
    let mut within1 = 0i64;
    let mut within2 = 0i64;
    let mut exact_plus = 0i64;
    let mut exact_minus = 0i64;
    let mut plus_within1 = 0i64;
    let mut resid_sum = 0i64;
    let mut resid_abs = 0i64;

    if verbose {
        println!(
            "== {file} ==  clan_match={} start_witnessed={} started_late={}",
            a.state.clan_match_detected, a.state.match_start_witnessed, a.state.started_late
        );
        println!(
            "{:<26} {:>6} {:>6} {:>5} {:>5} {:>6}",
            "player", "svK", "derK", "TK", "suic", "resid"
        );
    }

    for p in &a.state.players {
        if p.name.is_empty() {
            continue;
        }
        let derived: i64 = p.kill_streaks.iter().map(|s| s.kills.len() as i64).sum();
        let tks: i64 = p.weapon_breakdown.values().map(|(_, t)| *t as i64).sum();
        let sv = p.stats.1 as i64;
        if sv == 0 && derived == 0 {
            continue;
        }
        // kill_streaks exclude teamkills. Three candidate models for how the
        // server's frag counter relates to them.
        let resid = sv - derived; // A: frags == kills
        let resid_plus = sv - (derived + tks); // B: frags counts teamkills too
        let resid_minus = sv - (derived - tks); // C: frags deducts teamkills
        checked += 1;
        resid_sum += resid;
        resid_abs += resid.abs();
        if resid == 0 {
            exact += 1;
        }
        if resid_plus == 0 {
            exact_plus += 1;
        }
        if resid_minus == 0 {
            exact_minus += 1;
        }
        if resid_plus.abs() <= 1 {
            plus_within1 += 1;
        }
        if resid.abs() <= 1 {
            within1 += 1;
        }
        if resid.abs() <= 2 {
            within2 += 1;
        }
        if verbose {
            println!(
                "{:<26} {:>6} {:>6} {:>5} {:>5} {:>6}",
                p.name.chars().take(26).collect::<String>(),
                sv,
                derived,
                tks,
                0,
                resid
            );
        }
    }

    if !verbose {
        println!(
            "OK\t{file}\t{ty}\t{checked}\t{exact}\t{within1}\t{within2}\t{resid_sum}\t{resid_abs}\t{live}\t{witnessed}\t{exact_plus}\t{exact_minus}\t{plus_within1}",
            ty = a.demo_info.demo_type,
            live = a.state.clan_match_detected,
            witnessed = a.state.match_start_witnessed,
        );
    }
}
