//! Does dem's writer reproduce GoldSrc's real bytes, or only its own reader's
//! expectations? (#224)
//!
//! A demo that parses via `dem::open_demo_from_bytes` after `write_to_bytes`
//! only proves the writer and reader agree with EACH OTHER -- not that either
//! agrees with the real GoldSrc wire format. `snapshot_inject` hand-constructs
//! a brand new `SvcPacketEntities` (absolute indexing on every entity, fields
//! assembled from scratch) rather than editing an existing message's fields in
//! place, which is a code path nothing has verified against real bytes.
//!
//! This does the cleanest possible test: parse a healthy, untouched demo,
//! re-serialize the WHOLE thing via `write_to_bytes` with zero modifications,
//! and diff against the original byte-for-byte. Any mismatch is dem's writer
//! disagreeing with the real GoldSrc server that produced the original bytes.
//!
//!     cargo run --release -p analysis --example writer_fidelity -- <healthy.dem>

use dem::open_demo_from_bytes;

fn main() {
    let path = std::env::args().nth(1).expect("usage: writer_fidelity <healthy.dem>");
    let original = std::fs::read(&path).expect("read");
    let demo = open_demo_from_bytes(&original).expect("parse");
    let rewritten = demo.write_to_bytes();

    println!("{path}");
    println!("  original:  {} bytes", original.len());
    println!("  rewritten: {} bytes", rewritten.len());

    if original == rewritten {
        println!("  IDENTICAL -- dem's writer reproduces the real bytes exactly");
        return;
    }

    let min_len = original.len().min(rewritten.len());
    let first_diff = (0..min_len).find(|&i| original[i] != rewritten[i]);
    match first_diff {
        Some(i) => {
            println!("  DIFFERS at byte {i}");
            let start = i.saturating_sub(16);
            let end = (i + 32).min(min_len);
            println!("  original:  {:02x?}", &original[start..end]);
            println!("  rewritten: {:02x?}", &rewritten[start..end]);
            println!("  (mismatch marked at relative offset {})", i - start);
        }
        None => println!("  same up to the shorter length, then a size difference"),
    }

    // Count how many bytes actually differ, capped, to gauge how localized
    // the discrepancy is vs. a full desync of everything downstream.
    let mut diffs = 0usize;
    for i in 0..min_len {
        if original[i] != rewritten[i] { diffs += 1 }
    }
    println!("  {diffs} of {min_len} compared bytes differ ({:.2}%)",
        100.0 * diffs as f64 / min_len as f64);
}
