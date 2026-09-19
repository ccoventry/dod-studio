//! Why does a bridged demo parse far fewer frames than its bytes contain? (#15)
//!
//! `bridge_search` ranks candidate offsets by how many frames the rebuilt demo
//! parses to. For `m1_h1` that number (86,676) sits far below what the frame
//! layer walks through the same bytes, which was read as "the search settled for
//! a poor offset". This checks the competing explanation first: that the offset
//! is fine and the *parse* stops early, because a directory entry ends at the
//! first section-end frame it meets and the carried tail contains one.
//!
//! Distinguishing the two decides whether the work is a better search or a
//! different file layout, so measure before optimising.
//!
//!     cargo run --release -p analysis --example bridge_ceiling -- <demo.dem>

const DEMO_HEADER_SIZE: usize = 544;
const DIRECTORY_OFFSET_POS: usize = 540;
const FRAME_HEADER_SIZE: usize = 9;
const NETMSG_INFO_SIZE: usize = 464;
const NETWORK_HEADER_ALIGNMENT: usize = 468;
const MAX_PAYLOAD: usize = 2_097_152;

/// Walks frames from `start`, reporting every section-end (type 5) frame it
/// crosses rather than stopping at the first, plus where the walk finally dies.
fn walk_reporting_sections(bytes: &[u8], start: usize, end: usize) -> Walk {
    let mut pos = start;
    let mut frames = 0usize;
    let mut sections = Vec::new();
    let mut hist = [0usize; 11];
    let mut spans = [0usize; 11];
    let mut swallows: Vec<(usize, u8, usize, usize)> = Vec::new();
    let mut backwards = 0usize;
    let mut prev_time = -1.0f32;
    let mut first_time = f32::NAN;
    let mut last_time = 0.0f32;
    while pos + FRAME_HEADER_SIZE <= end {
        let t = bytes[pos];
        let time = f32::from_le_bytes(bytes[pos + 1..pos + 5].try_into().unwrap());
        if (t > 9 && t != 255) || !time.is_finite() || time < 0.0 || time > 100_000.0 { break }
        if t == 5 { sections.push((pos, frames, time)); }
        let slot = if t == 255 { 10 } else { t as usize };
        let frame_start = pos;
        if t != 255 && t != 5 {
            if first_time.is_nan() { first_time = time; }
            if time + 0.001 < prev_time { backwards += 1; }
            prev_time = time;
            last_time = time;
        }
        pos += FRAME_HEADER_SIZE;
        frames += 1;
        match t {
            5 | 2 | 255 => {}
            0 | 1 => {
                if pos + NETWORK_HEADER_ALIGNMENT > end { break }
                let l = i32::from_le_bytes(bytes[pos + NETMSG_INFO_SIZE..pos + NETWORK_HEADER_ALIGNMENT].try_into().unwrap());
                if l < 0 || l as usize > MAX_PAYLOAD { break }
                pos += NETWORK_HEADER_ALIGNMENT + l as usize;
            }
            3 => pos += 64,
            4 => pos += 32,
            6 => pos += 84,
            7 => pos += 8,
            8 => { if pos + 8 > end { break } let l = u32::from_le_bytes(bytes[pos+4..pos+8].try_into().unwrap()) as usize; pos += 24 + l; }
            9 => { if pos + 4 > end { break } let l = u32::from_le_bytes(bytes[pos..pos+4].try_into().unwrap()) as usize; pos += 4 + l; }
            _ => break,
        }
        if pos > end { break }
        hist[slot] += 1;
        spans[slot] += pos - frame_start;
        // A frame that consumes kilobytes is the walk skipping over rubble via a
        // garbage length field, not a frame. Where those cluster says whether the
        // damage is one hole or many.
        if pos - frame_start > 4096 && t != 0 && t != 1 {
            swallows.push((frame_start, t, pos - frame_start, frames));
        }
    }
    Walk { frames, sections, stop: pos, hist, spans, swallows, backwards, first_time, last_time }
}

struct Walk {
    frames: usize,
    sections: Vec<(usize, usize, f32)>,
    stop: usize,
    /// Frames seen per type; slot 10 holds type 255.
    hist: [usize; 11],
    /// Bytes those frames consumed, which is where a false alignment shows up.
    spans: [usize; 11],
    /// (byte offset, frame type, bytes consumed, frame index) for each skip over rubble.
    swallows: Vec<(usize, u8, usize, usize)>,
    backwards: usize,
    first_time: f32,
    last_time: f32,
}

fn main() {
    let path = std::env::args().nth(1).expect("usage: bridge_ceiling <demo.dem>");
    let bytes = std::fs::read(&path).expect("read");
    let dir_off = i32::from_le_bytes(bytes[DIRECTORY_OFFSET_POS..DEMO_HEADER_SIZE].try_into().unwrap()) as usize;
    let end = if dir_off > 0 && dir_off <= bytes.len() { dir_off } else { bytes.len() };
    println!("{} -- {:.1} MB, frame area {DEMO_HEADER_SIZE}..{end}", path, bytes.len() as f64 / 1e6);

    let w = walk_reporting_sections(&bytes, DEMO_HEADER_SIZE, end);
    let (frames, sections, stop) = (w.frames, &w.sections, w.stop);
    println!("\nframe walk (ignoring section ends): {frames} frames, stopped at byte {stop}");
    println!("  {} of the {} bytes in the frame area were walked", stop - DEMO_HEADER_SIZE, end - DEMO_HEADER_SIZE);
    println!("\nframe types (a healthy demo is almost all type 0/1 at ~165 bytes each):");
    for t in 0..11 {
        if w.hist[t] == 0 { continue }
        let name = if t == 10 { "255".to_string() } else { t.to_string() };
        let avg = w.spans[t] as f64 / w.hist[t] as f64;
        println!("  type {name:>3}: {:>8} frames, {:>12} bytes, {avg:>10.0} bytes/frame", w.hist[t], w.spans[t]);
    }
    println!("  overall: {:.0} bytes/frame", (stop - DEMO_HEADER_SIZE) as f64 / frames.max(1) as f64);
    println!("  time {:.2}s -> {:.2}s, {} frames stepped backwards in time", w.first_time, w.last_time, w.backwards);

    println!("\nframes that swallowed 4 KB+ (each is a skip over rubble): {}", w.swallows.len());
    let swallowed: usize = w.swallows.iter().map(|s| s.2).sum();
    if !w.swallows.is_empty() {
        println!("  {swallowed} bytes ({:.0}% of the frame area) vanished this way",
            100.0 * swallowed as f64 / (end - DEMO_HEADER_SIZE) as f64);
        for (off, t, n, idx) in w.swallows.iter().take(12) {
            println!("    byte {off:>10} type {t} swallowed {n:>9} bytes (at frame {idx})");
        }
        if w.swallows.len() > 12 { println!("    ... and {} more", w.swallows.len() - 12); }
    }

    println!("\nsection-end (type 5) frames crossed: {}", sections.len());
    for (off, n, t) in sections.iter().take(20) {
        println!("  byte {off}: after {n} frames, time {t:.2}s");
    }
    if sections.len() > 20 { println!("  ... and {} more", sections.len() - 20); }

    match dem::open_demo_from_bytes(&bytes) {
        Ok(d) => {
            let total: usize = d.directory.entries.iter().map(|e| e.frames.len()).sum();
            println!("\ndem parse: {total} frames across {} entries", d.directory.entries.len());
            for (i, e) in d.directory.entries.iter().enumerate() {
                println!("  entry {i} type {}: {} frames, frame_offset {}, length {}", e.type_, e.frames.len(), e.frame_offset, e.file_length);
            }
            println!("\nverdict: the walk sees {frames} frames, the parse keeps {total}.");
            if total + 2 < frames {
                println!("  => the bytes are there; the layout is throwing them away.");
            }
        }
        Err(e) => println!("\ndem parse FAILS: {e}"),
    }
}
