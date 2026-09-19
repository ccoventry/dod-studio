//! Can two distant slices of a *healthy* demo be stitched into one file?
//!
//! Isolates the question that #15's repair and #224's splicing both depend on,
//! with corruption removed as a variable: take a demo that parses perfectly,
//! keep its first N seconds and its last M seconds, drop everything between,
//! and see whether the result still reads.
//!
//! Directory entry 0 (the signon block) is kept whole, so decoders and
//! baselines are all present. What the joint cannot carry over is the entity
//! table the tail's deltas were encoded against.
//!
//!     cargo run --release -p analysis --example stitch_probe -- <in.dem> <head-secs> <tail-secs> <out.dem>
use dem::open_demo_from_bytes;
use dem::types::FrameData;

fn main() {
    let mut a = std::env::args().skip(1);
    let input = a.next().expect("usage: stitch_probe <in.dem> <head-secs> <tail-secs> <out.dem>");
    let head: f32 = a.next().expect("head secs").parse().unwrap();
    let tail: f32 = a.next().expect("tail secs").parse().unwrap();
    let output = a.next().expect("out.dem");

    let bytes = std::fs::read(&input).expect("read");
    let mut demo = open_demo_from_bytes(&bytes).expect("parse (input must be a healthy demo)");

    let tmax = demo.directory.entries.iter().skip(1)
        .flat_map(|e| e.frames.iter()).map(|f| f.time).fold(f32::MIN, f32::max);
    let tail_from = tmax - tail;
    println!("input spans 0..{tmax:.1}s; keeping 0..{head:.1}s and {tail_from:.1}..{tmax:.1}s");

    for (ei, entry) in demo.directory.entries.iter_mut().enumerate() {
        if ei == 0 { continue }
        let before = entry.frames.len();
        let mut kept: Vec<_> = entry.frames.iter()
            .filter(|f| f.time <= head || f.time >= tail_from
                || matches!(f.frame_data, FrameData::DemoStart))
            .cloned().collect();
        // Close the section cleanly.
        if !matches!(kept.last().map(|f| &f.frame_data), Some(FrameData::NextSection)) {
            if let Some(term) = entry.frames.iter().rev()
                .find(|f| matches!(f.frame_data, FrameData::NextSection)).cloned() {
                kept.push(term);
            }
        }
        println!("  entry {ei}: {before} -> {} frames", kept.len());
        entry.frames = kept;
        entry.frame_count = entry.frames.len() as i32;
        entry.track_time = entry.frames.last().map(|f| f.time).unwrap_or(0.0);
    }

    let out = demo.write_to_bytes();
    std::fs::write(&output, &out).expect("write");
    println!("  wrote {:.1} MB ({:.1}% of original)", out.len() as f64/1e6, 100.0*out.len() as f64/bytes.len() as f64);

    std::panic::set_hook(Box::new(|_| {}));
    let parsed = std::panic::catch_unwind(|| open_demo_from_bytes(&out).map(|d|
        d.directory.entries.iter().map(|e| e.frames.len()).sum::<usize>()));
    let _ = std::panic::take_hook();
    match parsed {
        Ok(Ok(n)) => println!("  RE-PARSE OK: {n} frames"),
        Ok(Err(e)) => println!("  re-parse failed: {e}"),
        Err(_) => println!("  re-parse PANICKED"),
    }
    std::panic::set_hook(Box::new(|_| {}));
    let an = std::panic::catch_unwind(|| analysis::Analysis::try_from_bytes(&out)
        .map(|a| (a.state.initial_map_name.clone(), a.state.players.len(), a.demo_info.playback_time)));
    let _ = std::panic::take_hook();
    match an {
        Ok(Ok((m, p, d))) => println!("  ANALYSIS OK: map={m:?} players={p} duration={d:.1}s"),
        Ok(Err(e)) => println!("  analysis failed: {e}"),
        Err(_) => println!("  analysis PANICKED"),
    }
}
