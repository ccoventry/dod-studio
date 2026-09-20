//! Trim a demo to its first N seconds, producing a small but still-valid file (#58).
//!
//! The survey (`trim_survey_probe`) settled why this direction and no other:
//! a real demo carries its entire init state -- `SvcServerInfo`, seven
//! `SvcDeltaDescription` decoders, `SvcSpawnBaseline`, the resource list and 69
//! `SvcNewUserMsg` registrations -- in directory **entry 0**, and then exactly
//! **one** full `SvcPacketEntities` at the very start of entry 1 followed by
//! ~126,000 *delta* updates chained off it.
//!
//! So entity state has no mid-demo restart point. Cutting the front is not
//! possible without synthesising a fresh full snapshot (i.e. replaying every
//! delta and materialising entity state). Cutting the **end** is safe, because
//! a delta only ever refers backwards: keep entry 0 whole, keep entry 1 from
//! its start up to a chosen time, and every reference still resolves.
//!
//!     cargo run --release -p analysis --example trim_demo -- <in.dem> <seconds> <out.dem>
use dem::open_demo_from_bytes;

fn main() {
    let mut args = std::env::args().skip(1);
    let input = args.next().expect("usage: trim_demo <in.dem> <seconds> <out.dem>");
    let seconds: f32 = args.next().expect("seconds").parse().expect("seconds must be a number");
    let output = args.next().expect("out.dem");

    let bytes = std::fs::read(&input).expect("read input");
    let mut demo = open_demo_from_bytes(&bytes).expect("parse input");

    let before: usize = demo.directory.entries.iter().map(|e| e.frames.len()).sum();

    // Entry 0 is the signon block and is kept whole. Every later entry is
    // truncated at `seconds`, keeping any terminator frame that follows so the
    // stream still ends the way a reader expects.
    for (ei, entry) in demo.directory.entries.iter_mut().enumerate() {
        if ei == 0 {
            continue;
        }
        let cut = entry.frames.iter().position(|f| f.time > seconds).unwrap_or(entry.frames.len());
        let mut terminator = entry.frames.iter().skip(cut).find(|f| {
            matches!(f.frame_data, dem::types::FrameData::NextSection)
        }).cloned();
        entry.frames.truncate(cut);
        if let Some(t) = terminator.as_mut() {
            // Rebase it onto the cut: left at its original timestamp the file
            // ends with a frame minutes after the one before it.
            t.time = entry.frames.last().map(|f| f.time).unwrap_or(0.0);
            entry.frames.push(t.clone());
        }
        entry.frame_count = entry.frames.len() as i32;
        // track_time is the entry's duration, and readers (including this
        // repo's own analyzer) report it as the demo's length -- left alone,
        // a three-second file still claims to be twenty minutes long.
        entry.track_time = entry.frames.last().map(|f| f.time).unwrap_or(0.0);
    }

    let out = demo.write_to_bytes();
    std::fs::write(&output, &out).expect("write output");

    let after: usize = demo.directory.entries.iter().map(|e| e.frames.len()).sum();
    println!("frames {before} -> {after}");
    println!("bytes  {} -> {}  ({:.1}% of original)", bytes.len(), out.len(),
        100.0 * out.len() as f64 / bytes.len() as f64);

    // The only check that matters: does it read back?
    match open_demo_from_bytes(&out) {
        Ok(d) => {
            let n: usize = d.directory.entries.iter().map(|e| e.frames.len()).sum();
            let tmax = d.directory.entries.iter().flat_map(|e| e.frames.iter()).map(|f| f.time).fold(f32::MIN, f32::max);
            println!("re-parsed OK: {} entries, {n} frames, last frame t={tmax:.2}", d.directory.entries.len());
        }
        Err(e) => println!("RE-PARSE FAILED: {e}"),
    }

    // Stronger than re-parsing: the analyzer decodes the delta-compressed
    // stream end to end, so it fails loudly if the chain was broken.
    match analysis::Analysis::try_from_bytes(&out) {
        Ok(a) => println!(
            "analysis OK: map={:?} players={} duration={:?}",
            a.state.initial_map_name,
            a.state.players.len(),
            a.demo_info.playback_time
        ),
        Err(e) => println!("ANALYSIS FAILED: {e}"),
    }
}
