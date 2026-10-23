//! Times a Duplicates audit over the folders given, as Studio runs it (#888):
//! the parallel walk, then the key pass with the key cache. Read-only for
//! the demos: it lists folders and reads files, and writes only the cache
//! file and the `--out` list it is given.
//!
//!     cargo run --release -p hl-demo-auditor --example audit_timing -- \
//!         --cache <scratch>\audit_keys.json --out <scratch>\groups.txt C:\ D:\
//!
//! Without `--cache` every run keys from scratch. `--out` writes every group
//! (sorted paths, one group per line), so two runs can be diffed.

use hl_demo_auditor::{AuditProgress, KeyCache, find_duplicates_cached, scan_roots};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

fn main() {
    let mut roots: Vec<PathBuf> = Vec::new();
    let mut out: Option<PathBuf> = None;
    let mut cache_file: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" => out = args.next().map(PathBuf::from),
            "--cache" => cache_file = args.next().map(PathBuf::from),
            _ => roots.push(PathBuf::from(arg)),
        }
    }
    let cancel = AtomicBool::new(false);
    let (tx, rx) = std::sync::mpsc::channel();
    let counter = std::thread::spawn(move || {
        let mut folders = 0usize;
        for msg in rx {
            if let AuditProgress::Scanning(_) = msg {
                folders += 1;
            }
        }
        folders
    });

    let started = Instant::now();
    let files = scan_roots(&roots, &cancel, &Some(tx.clone()));
    let walk = started.elapsed();
    let demos = files.len();
    let t = Instant::now();
    let mut cache = cache_file
        .as_deref()
        .map(KeyCache::load)
        .unwrap_or_default();
    let audit = find_duplicates_cached(files, &cancel, &Some(tx), &mut cache);
    cache.prune_unseen_under(&roots);
    if let Some(file) = &cache_file {
        cache.save(file).expect("save the key cache");
    }
    let key = t.elapsed();
    let folders = counter.join().unwrap_or(0);
    println!(
        "folders {folders}, demos {demos}, groups {}, copies {}, wasted {:.2} GB",
        audit.groups.len(),
        audit.copies,
        audit.wasted_bytes as f64 / 1e9
    );
    println!(
        "reads: {} heads, {} tails, {} whole files",
        audit.reads.heads, audit.reads.tails, audit.reads.fulls
    );
    println!(
        "walk {:.2} s, key {:.2} s, total {:.2} s",
        walk.as_secs_f64(),
        key.as_secs_f64(),
        started.elapsed().as_secs_f64()
    );
    if let Some(out) = out {
        let mut lines: Vec<String> = audit
            .groups
            .iter()
            .map(|g| {
                let mut files: Vec<String> =
                    g.files.iter().map(|p| p.display().to_string()).collect();
                files.sort();
                files.join(" | ")
            })
            .collect();
        lines.sort();
        std::fs::write(&out, lines.join("\n")).expect("write --out");
    }
}
