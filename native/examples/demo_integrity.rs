//! Demo integrity probe (#41): the library check
//! (`analysis::integrity::check_delta_continuity`, which `hl-demo-auditor
//! --integrity` runs) with every broken delta listed, plus `--skip A..B` to
//! drop frames A..B in memory first (nothing is written) and see what a copy
//! missing them would look like. Frame indices count every frame of every
//! directory entry from 0, as the report's do.
//!
//! The first version of this probe (#674) also checked the frame walk against
//! the directory, entity-count carry-forward and time/sequence hints; git
//! history has it.
//!
//!     cargo run --release -p native --example demo_integrity -- a.dem [b.dem ...]
//!     cargo run --release -p native --example demo_integrity -- --skip 200000..300000 a.dem

use analysis::integrity::check_delta_continuity;
use clap::Parser;
use std::ops::Range;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(about = "Lists every delta a GoldSrc demo builds on a frame it doesn't have (#41).")]
struct Args {
    /// Drop frames A..B before checking, simulating a copy that lost them. May
    /// be repeated.
    #[arg(long, value_parser = parse_range)]
    skip: Vec<Range<usize>>,

    /// One or more .dem files
    #[arg(required = true)]
    files: Vec<PathBuf>,
}

fn parse_range(s: &str) -> Result<Range<usize>, String> {
    let (a, b) = s.split_once("..").ok_or("expected A..B")?;
    let a = a.parse().map_err(|e| format!("{e}"))?;
    let b = b.parse().map_err(|e| format!("{e}"))?;
    Ok(a..b)
}

fn main() {
    let args = Args::parse();
    for path in &args.files {
        println!("== {}", path.display());
        let mut demo = match dem::open_demo(path) {
            Ok(demo) => demo,
            Err(e) => {
                println!("  cannot parse it: {e}\n");
                continue;
            }
        };
        if !args.skip.is_empty() {
            let mut index = 0;
            let mut dropped = 0;
            for entry in &mut demo.directory.entries {
                entry.frames.retain(|_| {
                    let keep = !args.skip.iter().any(|r| r.contains(&index));
                    index += 1;
                    dropped += usize::from(!keep);
                    keep
                });
            }
            println!("  dropped {dropped} frames");
        }
        let report = check_delta_continuity(&demo);
        println!(
            "  frames {}, deltas {}, broken {}",
            report.frames,
            report.delta_messages,
            report.broken.len()
        );
        for b in &report.broken {
            println!(
                "    frame #{} at {:.2}s: {:?} seq {} from ..{:02x}",
                b.frame_index, b.time, b.kind, b.incoming_seq, b.delta_seq
            );
        }
        println!("  VERDICT: {}\n", report.verdict());
    }
}
