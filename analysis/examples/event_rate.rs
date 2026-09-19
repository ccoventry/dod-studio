//! Are events spread evenly through a recovered demo, or missing from part of it? (#15)
//!
//! A recovered demo can parse, predict no discarded packets and still be quietly
//! wrong -- a segment that decodes structurally but whose user messages are
//! being dropped looks identical from the outside. `m1_h1` recovers 96.7% of its
//! bytes yet carries ~24% fewer kill events than the HLTV recording of the same
//! match, which is more than its 42 seconds of holes explain.
//!
//! Counting death messages per minute separates the two explanations. Holes show
//! up as a couple of empty buckets in an otherwise steady rate; a segment whose
//! messages are being lost shows up as a long stretch running near zero.
//!
//!     cargo run --release -p analysis --example event_rate -- <demo.dem>

use dem::open_demo_from_bytes;
use dem::types::{FrameData, MessageData, NetMessage};
use dod::UserMessage;

const BUCKET: f32 = 60.0;

fn main() {
    let path = std::env::args().nth(1).expect("usage: event_rate <demo.dem>");
    let bytes = std::fs::read(&path).expect("read");
    let demo = open_demo_from_bytes(&bytes).expect("parse");

    println!("{path}");

    let mut buckets: Vec<usize> = Vec::new();
    let mut frames_per_bucket: Vec<usize> = Vec::new();
    let mut total = 0usize;
    for entry in demo.directory.entries.iter().skip(1) {
        for f in &entry.frames {
            let b = (f.time / BUCKET).max(0.0) as usize;
            if b > 100_000 { continue }
            while buckets.len() <= b { buckets.push(0); frames_per_bucket.push(0) }
            frames_per_bucket[b] += 1;
            let FrameData::NetworkMessage(bt) = &f.frame_data else { continue };
            let MessageData::Parsed(msgs) = &bt.1.messages else { continue };
            for m in msgs {
                if let NetMessage::UserMessage(um) = m {
                    if matches!(UserMessage::new(&um.name, &um.data), Ok(UserMessage::DeathMsg(_))) {
                        buckets[b] += 1;
                        total += 1;
                    }
                }
            }
        }
    }

    println!("  {total} death messages over {} minutes\n", buckets.len());
    println!("  min   frames   deaths");
    for (i, (d, fr)) in buckets.iter().zip(&frames_per_bucket).enumerate() {
        let bar = "#".repeat((*d).min(60));
        let flag = if *fr == 0 { "  <- no frames at all (hole)" }
            else if *d == 0 { "  <- frames but no deaths" } else { "" };
        println!("  {i:>3} {fr:>8} {d:>8} {bar}{flag}");
    }
}
