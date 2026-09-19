//! Find the *best* bridge across a demo's damage, and cut in the right place (#15).
//!
//! Two defects in `bridge_search` are fixed here, both found by `bridge_ceiling`
//! run over its own output:
//!
//! 1. **It cut at the wrong byte.** The salvage pass reports the offset where the
//!    frame walk gave up, which is *inside* the damaged frame -- past its 9-byte
//!    header. Splicing there leaves an orphan header whose length field is then
//!    read out of the first bytes of the tail. In `recovered_m1_h1.dem` that
//!    orphan declared 69,206,029 bytes and ate 84% of the file. Cut at the start
//!    of the damaged frame instead.
//!
//! 2. **Reaching the end of the file was too weak a test.** That orphan skip
//!    landed back on a frame boundary by luck, so the walk sailed to the end and
//!    the candidate scored as a win while carrying almost nothing. A healthy DoD
//!    demo runs ~165 bytes per frame with no frame consuming kilobytes, so a
//!    candidate that skips 4 KB+ anywhere, or steps backwards in time, is
//!    rejected however far it walks.
//!
//! Verification parses each surviving candidate in a *child process*, because
//! `dem` can abort outright on malformed input (#225) -- a crash then costs one
//! candidate instead of the whole search, which is what the sidecar-and-restart
//! scheme was working around.
//!
//!     cargo run --release -p analysis --example bridge_best -- <demo> <out.dem> [from]

const DEMO_HEADER_SIZE: usize = 544;
const DIRECTORY_OFFSET_POS: usize = 540;
const FRAME_HEADER_SIZE: usize = 9;
const NETMSG_INFO_SIZE: usize = 464;
const NETWORK_HEADER_ALIGNMENT: usize = 468;
const MAX_PAYLOAD: usize = 2_097_152;
/// No real frame in a healthy demo comes close to this; see the module note.
const SWALLOW: usize = 4096;
/// Healthy demos do step backwards in time, once or twice, at a section
/// boundary -- `m1_h2` does it twice in its intact prefix. Demanding a strictly
/// rising clock would reject a sound tail that happens to span one.
const BACKWARDS_OK: usize = 2;

struct Quality {
    frames: usize,
    stop: usize,
    swallows: usize,
    backwards: usize,
    /// Type 4 and type 9 frames, which a real recording emits in lockstep.
    t4: usize,
    t9: usize,
    bytes: usize,
    first_time: f32,
    last_time: f32,
}

impl Quality {
    /// Does this look like a recording, rather than a lucky path through rubble?
    /// Every healthy demo measured pairs type 4 with type 9 exactly (197642 /
    /// 197642, 191043 / 191043, 103747 / 103747) and runs 160-165 bytes per
    /// frame, where the bad `m1_h1` bridge ran 946. Judging that here rather
    /// than in the child saves an 82 MB re-read and a full parse per candidate,
    /// which is what the search actually spends its time on.
    fn plausible(&self) -> bool {
        if self.frames < 5000 { return false }
        let per = self.bytes as f64 / self.frames as f64;
        (100.0..=400.0).contains(&per) && self.t4.abs_diff(self.t9) <= 2
    }
}

/// Walks frames from `start`, judging the *shape* of what it finds rather than
/// only whether it can keep going.
fn walk(bytes: &[u8], start: usize, end: usize, cap: usize) -> Quality {
    let mut pos = start;
    let mut frames = 0usize;
    let mut swallows = 0usize;
    let mut backwards = 0usize;
    let mut t4 = 0usize;
    let mut t9 = 0usize;
    let mut prev_time = -1.0f32;
    let mut first_time = f32::NAN;
    let mut last_time = 0.0f32;
    while frames < cap && pos + FRAME_HEADER_SIZE <= end {
        let t = bytes[pos];
        let time = f32::from_le_bytes(bytes[pos + 1..pos + 5].try_into().unwrap());
        if (t > 9 && t != 255) || !time.is_finite() || time < 0.0 || time > 100_000.0 { break }
        let frame_start = pos;
        if t != 255 && t != 5 {
            if first_time.is_nan() { first_time = time }
            if time + 0.001 < prev_time { backwards += 1 }
            prev_time = time;
            last_time = time;
        }
        pos += FRAME_HEADER_SIZE;
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
            8 => { if pos + 8 > end { break } let l = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()) as usize; pos += 24 + l; }
            9 => { if pos + 4 > end { break } let l = u32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap()) as usize; pos += 4 + l; }
            _ => break,
        }
        if pos > end { break }
        frames += 1;
        if t == 4 { t4 += 1 }
        if t == 9 { t9 += 1 }
        if pos - frame_start > SWALLOW && t != 0 && t != 1 { swallows += 1 }
    }
    Quality { frames, stop: pos, swallows, backwards, t4, t9, bytes: pos - start, first_time, last_time }
}

/// Where the intact prefix ends. The walk's break position sits *inside* the
/// broken frame, so track frame starts and hand back the last clean one.
fn prefix(bytes: &[u8], end: usize) -> (usize, usize, usize, f32) {
    let mut pos = DEMO_HEADER_SIZE;
    let mut frames = 0usize;
    let mut cut = DEMO_HEADER_SIZE;
    let mut last_time = 0.0f32;
    loop {
        let step = walk(bytes, pos, end, 1);
        if step.frames == 0 { return (pos, frames, cut, last_time) }
        if step.last_time > 0.0 { last_time = step.last_time }
        cut = step.stop;
        pos = step.stop;
        frames += 1;
    }
}

fn entry0_end(bytes: &[u8], end: usize) -> Option<usize> {
    let mut pos = DEMO_HEADER_SIZE;
    loop {
        if pos + FRAME_HEADER_SIZE > end { return None }
        let t = bytes[pos];
        let step = walk(bytes, pos, end, 1);
        if step.frames == 0 { return None }
        pos = step.stop;
        if t == 5 { return Some(pos) }
    }
}

fn put_i32(v: &mut Vec<u8>, x: i32) { v.extend_from_slice(&x.to_le_bytes()) }
fn put_f32(v: &mut Vec<u8>, x: f32) { v.extend_from_slice(&x.to_le_bytes()) }

fn dir_entry(o: &mut Vec<u8>, t: i32, d: &str, tt: f32, fc: i32, fo: i32, fl: i32) {
    put_i32(o, t);
    let mut b = [0u8; 64];
    for (i, c) in d.bytes().take(63).enumerate() { b[i] = c }
    o.extend_from_slice(&b);
    put_i32(o, 0);
    put_i32(o, -1);
    put_f32(o, tt);
    put_i32(o, fc);
    put_i32(o, fo);
    put_i32(o, fl);
}

fn build(bytes: &[u8], e0: usize, cut: usize, from: usize, end: usize) -> Vec<u8> {
    let mut out = bytes[..cut].to_vec();
    out.extend_from_slice(&bytes[from..end]);
    out.push(5);
    put_f32(&mut out, 0.0);
    put_i32(&mut out, 0);
    let dir = out.len();
    put_i32(&mut out, 2);
    dir_entry(&mut out, 0, "LOADING", 0.0, 0, DEMO_HEADER_SIZE as i32, (e0 - DEMO_HEADER_SIZE) as i32);
    dir_entry(&mut out, 1, "Playback", 0.0, 0, e0 as i32, (dir - e0) as i32);
    out[DIRECTORY_OFFSET_POS..DEMO_HEADER_SIZE].copy_from_slice(&(dir as i32).to_le_bytes());
    out
}

/// Child mode: rebuild one candidate and parse it, reporting the frame count.
/// Runs in its own process so that an abort costs one candidate, not the search.
fn verify_child(args: &[String]) -> ! {
    let bytes = std::fs::read(&args[0]).expect("read");
    let cut: usize = args[1].parse().unwrap();
    let from: usize = args[2].parse().unwrap();
    let e0: usize = args[3].parse().unwrap();
    let end: usize = args[4].parse().unwrap();
    let cand = build(&bytes, e0, cut, from, end);
    match dem::open_demo_from_bytes(&cand) {
        Ok(d) => {
            let frames: usize = d.directory.entries.iter().map(|e| e.frames.len()).sum();
            println!("{frames}");
            std::process::exit(0)
        }
        Err(_) => std::process::exit(1),
    }
}

fn main() {
    let argv: Vec<String> = std::env::args().collect();
    if argv.len() > 1 && argv[1] == "--verify" { verify_child(&argv[2..]) }

    let path = argv.get(1).expect("usage: bridge_best <demo> <out.dem> [from]").clone();
    let out_path = argv.get(2).expect("usage: bridge_best <demo> <out.dem> [from]").clone();
    let bytes = std::fs::read(&path).expect("read");
    let dir_off = i32::from_le_bytes(bytes[DIRECTORY_OFFSET_POS..DEMO_HEADER_SIZE].try_into().unwrap()) as usize;
    let end = if dir_off > 0 && dir_off <= bytes.len() { dir_off } else { bytes.len() };
    let e0 = entry0_end(&bytes, end).expect("entry0");

    let (broke_at, prefix_frames, cut, prefix_time) = prefix(&bytes, end);
    println!("{path}");
    println!("  frame area {DEMO_HEADER_SIZE}..{end} ({:.1} MB)", (end - DEMO_HEADER_SIZE) as f64 / 1e6);
    println!("  prefix: {prefix_frames} frames to {prefix_time:.2}s");
    println!("  walk broke at byte {broke_at}; damaged frame starts at {cut} -- cutting there");

    let start_scan: usize = argv.get(3).and_then(|s| s.parse().ok()).unwrap_or(cut + 1);
    let exe = std::env::current_exe().expect("exe");
    let mut best = 0usize;
    let mut best_from = 0usize;
    let mut considered = 0usize;
    let mut deep = 0usize;
    let mut verified = 0usize;
    let mut crashed = 0usize;
    // Every valid resume point later in the file carries strictly *less* of the
    // tail, so the earliest one that survives verification is the best one --
    // there is no better candidate further on to hold out for. Scanning a window
    // past the first hit is a check on that, not a search: a later candidate
    // that beats it would mean the first was a false alignment, so the window
    // restarts from any improvement.
    const WINDOW: usize = 1 << 20;
    let mut hit: Option<usize> = None;

    // The scan can run for tens of minutes over an 80 MB tail, so say where it
    // has got to; a silent search is indistinguishable from a hung one.
    let mut next_report = start_scan + (1 << 22);

    let mut scan = start_scan.max(cut + 1);
    while scan + FRAME_HEADER_SIZE < end && hit.map_or(true, |h| scan < h + WINDOW) {
        if scan >= next_report {
            eprintln!("  ... at byte {scan} ({:.0}% of the way to the end), {considered} candidates seen so far",
                100.0 * (scan - cut) as f64 / (end - cut) as f64);
            next_report = scan + (1 << 22);
        }
        // Cheap reject on the frame header alone: a byte that cannot be a frame
        // type, or a time from before the prefix ended, is not a resume point.
        let t = bytes[scan];
        if t > 9 && t != 255 {
            scan += 1;
            continue;
        }
        let time = f32::from_le_bytes(bytes[scan + 1..scan + 5].try_into().unwrap());
        if !time.is_finite() || time < prefix_time || time > 100_000.0 {
            scan += 1;
            continue;
        }

        let quick = walk(&bytes, scan, end, 400);
        if quick.frames < 400 || quick.swallows > 0 || quick.backwards > BACKWARDS_OK {
            scan += 1;
            continue;
        }
        considered += 1;

        let full = walk(&bytes, scan, end, usize::MAX);
        if full.stop < end.saturating_sub(64) || full.swallows > 0 || full.backwards > BACKWARDS_OK
            || full.frames <= best || !full.plausible()
        {
            scan += 1;
            continue;
        }
        deep += 1;
        eprintln!("  verifying offset {scan}: {} frames, {:.0} bytes/frame, t4/t9 {}/{}",
            full.frames, full.bytes as f64 / full.frames as f64, full.t4, full.t9);

        let child = std::process::Command::new(&exe)
            .args(["--verify", &path, &cut.to_string(), &scan.to_string(), &e0.to_string(), &end.to_string()])
            .output();
        match child {
            Ok(o) if o.status.success() => {
                verified += 1;
                let frames: usize = String::from_utf8_lossy(&o.stdout).trim().parse().unwrap_or(0);
                if frames > best {
                    if hit.is_some() {
                        println!("  (this beats the earlier hit, so that one was a false alignment)");
                    }
                    best = frames;
                    best_from = scan;
                    hit = Some(scan);
                    let cand = build(&bytes, e0, cut, scan, end);
                    std::fs::write(&out_path, &cand).expect("write");
                    println!("  BEST offset={scan} frames={frames} ({:.2}s -> {:.2}s), {} bytes",
                        full.first_time, full.last_time, cand.len());
                }
            }
            Ok(o) if o.status.code().is_none() => crashed += 1,
            _ => {}
        }
        scan += 1;
    }

    println!("\n{considered} offsets passed the quick shape test, {deep} of those walked cleanly to the end, {verified} parsed, {crashed} crashed the parser");
    if best > 0 {
        println!("best: offset {best_from}, {best} frames -> {out_path}");
    } else {
        println!("no candidate survived");
    }
}
