//! A dry run of the analyzer cache upgrades on a real cache, read-only: what
//! share of an older folder's entries upgrade in place, why the rest need a
//! re-parse, how long it takes, and whether an upgraded entry says what a
//! fresh analysis of its demo says.
//!
//! - Every entry of `<root>/v<from>` goes through the steps in memory
//!   (`analysis::cache::upgrade_entry_bytes`). Nothing is written anywhere.
//! - Where `<root>/v9` holds an entry for the same demo as it is now (written
//!   by a full parse with #744), the upgraded highlight slots are checked
//!   against it, for every such demo.
//! - For `--compare N` upgraded entries whose demo is still there unchanged
//!   (preferring ones with cancelled captures), the demo is analysed again
//!   and the highlight slots and cap breaks compared with the upgrade's.
//!
//!     cargo run --release -p analysis --example cache_migrate_probe -- --from 7 --compare 10
//!
//! `--root <folder>` defaults to `%APPDATA%\dod-studio\analyzer_cache`.

use analysis::cache::{Entry, upgrade_entry_bytes};
use analysis::highlights::highlights;
use analysis::{Analysis, AttemptOutcome};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

fn arg(name: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

/// Each highlight's player, slot and kill count, sorted.
fn slots(analysis: &Analysis) -> Vec<(String, Option<u8>, usize)> {
    let mut list: Vec<_> = highlights(analysis)
        .all
        .iter()
        .map(|h| (h.player_id.clone(), h.slot, h.kills.len()))
        .collect();
    list.sort();
    list
}

/// Per-player cap breaks (non-zero ones), the two per-demo totals, and every
/// attempt's cancel cause.
fn cap_breaks(analysis: &Analysis) -> (BTreeMap<String, u32>, u32, u32, Vec<String>) {
    let state = &analysis.state;
    let players = state
        .players
        .iter()
        .filter(|p| p.cap_breaks > 0)
        .map(|p| (p.id.to_string(), p.cap_breaks))
        .collect();
    let causes = state
        .objectives
        .attempts
        .iter()
        .map(|a| format!("{:?}", a.cancel_cause))
        .collect();
    (
        players,
        state.objectives.blocks_unattributed,
        state.objectives.attempts_abandoned,
        causes,
    )
}

fn entries(dir: &Path) -> Vec<PathBuf> {
    let mut list: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("could not list {}: {e}", dir.display()))
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with(".json") && n.matches('.').count() == 1)
        })
        .collect();
    list.sort();
    list
}

fn main() {
    let root = arg("--root").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(std::env::var_os("APPDATA").expect("APPDATA is not set"))
            .join("dod-studio")
            .join("analyzer_cache")
    });
    let from: u32 = arg("--from").and_then(|v| v.parse().ok()).unwrap_or(7);
    let compare: usize = arg("--compare").and_then(|v| v.parse().ok()).unwrap_or(10);
    let dir = root.join(format!("v{from}"));
    let files = entries(&dir);
    println!(
        "{}: {} entries, upgrading v{from} -> v{}, in memory only",
        dir.display(),
        files.len(),
        analysis::cache::SCHEMA_VERSION
    );

    let started = Instant::now();
    let mut bytes_read = 0u64;
    let mut upgraded: Vec<(PathBuf, Entry)> = Vec::new();
    let mut reasons: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut read_ms = 0f64;
    for file in &files {
        let read_at = Instant::now();
        let Ok(bytes) = std::fs::read(file) else {
            *reasons.entry("could not read the entry").or_default() += 1;
            continue;
        };
        read_ms += read_at.elapsed().as_secs_f64() * 1000.0;
        bytes_read += bytes.len() as u64;
        match upgrade_entry_bytes(from, &bytes) {
            Ok(entry) => upgraded.push((file.clone(), entry)),
            Err(why) => *reasons.entry(why).or_default() += 1,
        }
    }
    let took = started.elapsed();
    let reparse: usize = reasons.values().sum();
    println!(
        "upgraded {} / needs a re-parse {} ({:.1}% upgraded) in {:.1} s ({:.1} ms an entry, {:.1} s of it reading {} MB)",
        upgraded.len(),
        reparse,
        100.0 * upgraded.len() as f64 / files.len().max(1) as f64,
        took.as_secs_f64(),
        took.as_secs_f64() * 1000.0 / files.len().max(1) as f64,
        read_ms / 1000.0,
        bytes_read >> 20
    );
    for (why, n) in &reasons {
        println!("  re-parse: {n} x {why}");
    }

    // Against the v9 entries a full parse wrote for the same demos.
    let v9 = root.join("v9");
    let (mut checked, mut differ) = (0, 0);
    for (file, entry) in &upgraded {
        let Some(name) = file.file_name() else {
            continue;
        };
        let Ok(bytes) = std::fs::read(v9.join(name)) else {
            continue;
        };
        let Ok(theirs) = upgrade_entry_bytes(9, &bytes) else {
            continue;
        };
        if (theirs.size_bytes, theirs.modified_unix_secs)
            != (entry.size_bytes, entry.modified_unix_secs)
        {
            continue;
        }
        checked += 1;
        if slots(&theirs.analysis) != slots(&entry.analysis) {
            differ += 1;
            println!(
                "  v9 slots differ: {} ({})",
                entry.file_info.path,
                name.to_string_lossy()
            );
        }
    }
    println!("against v9's own entries for the same demo: {checked} checked, {differ} differ");

    // Against fresh analyses: demos still there unchanged, those with
    // cancelled captures first, spread over the list.
    let cancels = |e: &Entry| {
        e.analysis
            .state
            .objectives
            .attempts
            .iter()
            .filter(|a| a.outcome == AttemptOutcome::Cancelled)
            .count()
    };
    let mut candidates: Vec<&Entry> = upgraded
        .iter()
        .map(|(_, e)| e)
        .filter(|e| {
            analysis::cache::stamp(Path::new(&e.file_info.path))
                == Some((e.size_bytes, e.modified_unix_secs))
        })
        .collect();
    println!(
        "{} upgraded entries have their demo unchanged on disk",
        candidates.len()
    );
    candidates.sort_by_key(|e| std::cmp::Reverse(cancels(e).min(1)));
    let with_cancels = candidates.iter().filter(|e| cancels(e) > 0).count();
    let step = (with_cancels.max(compare) / compare.max(1)).max(1);
    let picked: Vec<&Entry> = candidates
        .iter()
        .step_by(step)
        .take(compare)
        .copied()
        .collect();
    let mut matched = 0;
    for entry in &picked {
        let path = Path::new(&entry.file_info.path);
        let at = Instant::now();
        let fresh = match std::fs::read(path)
            .map_err(|e| e.to_string())
            .and_then(|b| Analysis::try_from_bytes(&b))
        {
            Ok(a) => a,
            Err(e) => {
                println!("  {}: could not analyse: {e}", path.display());
                continue;
            }
        };
        let same_slots = slots(&fresh) == slots(&entry.analysis);
        let (mine, theirs) = (cap_breaks(&entry.analysis), cap_breaks(&fresh));
        let same_breaks = mine == theirs;
        if same_slots && same_breaks {
            matched += 1;
        }
        println!(
            "  {}: {} highlights, {} cancels, {} breaks, {} unattributed, {} abandoned -- slots {}, cap breaks {} ({:.1} s)",
            entry.file_info.name,
            slots(&entry.analysis).len(),
            cancels(entry),
            mine.0.values().sum::<u32>(),
            mine.1,
            mine.2,
            if same_slots { "match" } else { "DIFFER" },
            if same_breaks { "match" } else { "DIFFER" },
            at.elapsed().as_secs_f64()
        );
        if !same_breaks {
            println!("    upgraded {mine:?}\n    fresh    {theirs:?}");
        }
    }
    println!(
        "fresh analyses: {matched} of {} match on highlight slots and cap breaks",
        picked.len()
    );
}
