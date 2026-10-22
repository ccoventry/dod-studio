//! Peak network-message bytes in any window of N seconds: an upper bound on
//! what one step of a stepped forward seek makes `DemoPlayer.dll` send in one
//! message (#596, `goldsrc-hooks/src/demo_seek.rs`'s `STEP_SECONDS`). The
//! player's stream holds 64 KB.
//!
//!     cargo run --release -p analysis --example seek_burst -- demo.dem [window_s]

use dem::open_demo_from_bytes;
use dem::types::FrameData;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("usage: seek_burst <demo> [window_s]");
    let window: f32 = args
        .next()
        .map(|w| w.parse().expect("window_s is a number of seconds"))
        .unwrap_or(5.0);
    let bytes = std::fs::read(&path).expect("read the demo");
    let demo = open_demo_from_bytes(&bytes).expect("parse the demo");
    let mut frames: Vec<(f32, u32)> = Vec::new();
    for entry in &demo.directory.entries {
        for f in &entry.frames {
            if let FrameData::NetworkMessage(bt) = &f.frame_data {
                frames.push((f.time, bt.1.message_length));
            }
        }
    }
    let total: u64 = frames.iter().map(|f| f.1 as u64).sum();
    let span = frames.last().map_or(0.0, |f| f.0) - frames.first().map_or(0.0, |f| f.0);
    let (mut lo, mut sum, mut peak, mut peak_at) = (0usize, 0u64, 0u64, 0f32);
    for hi in 0..frames.len() {
        sum += frames[hi].1 as u64;
        while frames[hi].0 - frames[lo].0 > window {
            sum -= frames[lo].1 as u64;
            lo += 1;
        }
        if sum > peak {
            peak = sum;
            peak_at = frames[lo].0;
        }
    }
    println!(
        "{path}: {span:.0} s, {} KB/s average, peak {} KB in {window} s (at {peak_at:.0} s)",
        total / 1024 / (span.max(1.0) as u64),
        peak / 1024,
    );
}
