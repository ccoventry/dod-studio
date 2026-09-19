//! Bridge *every* hole in a damaged demo, not just the first one (#15).
//!
//! `bridge_best` requires a candidate resume point to walk to the end of the
//! frame area. That is a good false-alignment test, but it silently assumes the
//! recording has exactly one hole: if a second break lies further on, every
//! honest resume point before it fails the test, and the search can only ever
//! settle after the *last* break. On `m1_h1` nothing between byte 2.4M and 52.9M
//! reaches the end, so the recovery skipped 790 seconds of the match -- not
//! because those bytes are rubble, but because a later hole hid them.
//!
//! So walk, cut at the frame that breaks, find the next place the recording
//! resumes, and repeat until the walk reaches the end. Each resume point is
//! judged on the shape of the run it starts rather than on reaching the end:
//! ~160 bytes per frame, no frame swallowing 4 KB+, and type 4 and type 9 frames
//! paired, which they are exactly in every healthy demo measured. The whole
//! result is then parsed once as the real check.
//!
//!     cargo run --release -p analysis --example multi_bridge -- <demo> <out.dem>

const DEMO_HEADER_SIZE: usize = 544;
const DIRECTORY_OFFSET_POS: usize = 540;
const FRAME_HEADER_SIZE: usize = 9;
const NETMSG_INFO_SIZE: usize = 464;
const NETWORK_HEADER_ALIGNMENT: usize = 468;
const MAX_PAYLOAD: usize = 2_097_152;
const SWALLOW: usize = 4096;
/// How much clean recording a resume point must produce to be believed. Long
/// enough that a false alignment will not hold together, short enough that two
/// holes close to each other still leave a usable segment between them.
const MIN_RUN: usize = 5000;
/// Guard against a pathological file turning this into an infinite splice.
const MAX_HOLES: usize = 64;

#[derive(Default)]
struct Run {
    frames: usize,
    /// Start of the frame the walk could not read -- the byte to cut at.
    cut: usize,
    swallows: usize,
    backwards: usize,
    t4: usize,
    t9: usize,
    bytes: usize,
    first_time: f32,
    last_time: f32,
}

impl Run {
    fn plausible(&self) -> bool {
        if self.frames < MIN_RUN { return false }
        let per = self.bytes as f64 / self.frames as f64;
        self.swallows == 0
            && self.backwards <= 2
            && self.t4.abs_diff(self.t9) <= 2
            && (100.0..=400.0).contains(&per)
    }
}

/// Walks frames from `start`, stopping at the first one it cannot read. `cut` is
/// that frame's own start, never the offset part-way through it that the walk
/// happened to give up at -- splicing at the latter leaves an orphan header
/// whose length field is then read out of whatever follows.
fn walk(bytes: &[u8], start: usize, end: usize, cap: usize) -> Run {
    let mut r = Run { cut: start, first_time: f32::NAN, ..Default::default() };
    let mut pos = start;
    let mut prev_time = -1.0f32;
    while r.frames < cap && pos + FRAME_HEADER_SIZE <= end {
        let frame_start = pos;
        r.cut = frame_start;
        let t = bytes[pos];
        let time = f32::from_le_bytes(bytes[pos + 1..pos + 5].try_into().unwrap());
        if (t > 9 && t != 255) || !time.is_finite() || time < 0.0 || time > 100_000.0 { break }
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
        if t != 255 && t != 5 {
            if r.first_time.is_nan() { r.first_time = time }
            if time + 0.001 < prev_time { r.backwards += 1 }
            prev_time = time;
            r.last_time = time;
        }
        if t == 4 { r.t4 += 1 }
        if t == 9 { r.t9 += 1 }
        if pos - frame_start > SWALLOW && t != 0 && t != 1 { r.swallows += 1 }
        r.frames += 1;
        r.cut = pos;
    }
    r.bytes = r.cut.saturating_sub(start);
    r
}

/// The first offset at or after `from` where the recording plausibly resumes.
///
/// Shape alone is not enough to accept one: on `m3_h2` the first shape-plausible
/// offset (42,043,622) is one the message parser rejects outright, and only the
/// third (42,073,853) is real. So each candidate is confirmed by actually
/// parsing a bounded probe demo built from it -- entry 0 plus the run it starts
/// -- in a child process, since malformed input can abort the parser (#225) and
/// a crash should cost one candidate rather than the whole pass.
fn next_resume(bytes: &[u8], e0: usize, from: usize, end: usize, after_time: f32) -> Option<usize> {
    let exe = std::env::current_exe().expect("exe");
    let mut scan = from;
    let mut tried = 0usize;
    while scan + FRAME_HEADER_SIZE < end {
        let t = bytes[scan];
        if t > 9 && t != 255 { scan += 1; continue }
        let time = f32::from_le_bytes(bytes[scan + 1..scan + 5].try_into().unwrap());
        if !time.is_finite() || time < after_time || time > 100_000.0 { scan += 1; continue }
        let quick = walk(bytes, scan, end, 400);
        if quick.frames < 400 || quick.swallows > 0 || quick.backwards > 2 { scan += 1; continue }
        let run = walk(bytes, scan, end, MIN_RUN * 4);
        if !run.plausible() { scan += 1; continue }

        tried += 1;
        if verify(&exe, &probe_demo(bytes, e0, scan, run.cut)) {
            if tried > 1 { println!("    ({tried} candidates tried; the earlier ones did not parse)") }
            return Some(scan);
        }
        scan += 1;
    }
    None
}

/// Child mode: parse a probe demo handed over on stdin, so an abort is
/// contained. The probe arrives down the pipe rather than being rebuilt from the
/// source file, because a search can spawn tens of thousands of these and having
/// each one re-read an 84 MB demo is most of the runtime.
fn verify_child() -> ! {
    use std::io::Read;
    let mut probe = Vec::new();
    if std::io::stdin().read_to_end(&mut probe).is_err() { std::process::exit(2) }
    match dem::open_demo_from_bytes(&probe) {
        Ok(_) => std::process::exit(0),
        Err(_) => std::process::exit(1),
    }
}

/// Parse `probe` in a child process. Returns false for a rejection *or* a crash.
fn verify(exe: &std::path::Path, probe: &[u8]) -> bool {
    use std::io::Write;
    use std::process::Stdio;
    let Ok(mut child) = std::process::Command::new(exe)
        .arg("--verify")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn() else { return false };
    if let Some(mut si) = child.stdin.take() {
        if si.write_all(probe).is_err() { /* child died early; the wait below reports it */ }
    }
    child.wait().map(|s| s.success()).unwrap_or(false)
}

/// A probe demo for one segment: entry 0, then the run. The first segment
/// already contains entry 0; every later one needs it prepended, or there are no
/// delta descriptions to decode against.
fn probe_demo(bytes: &[u8], e0: usize, start: usize, stop: usize) -> Vec<u8> {
    if start == DEMO_HEADER_SIZE {
        assemble(bytes, e0, &[(start, stop)])
    } else {
        assemble(bytes, e0, &[(DEMO_HEADER_SIZE, e0), (start, stop)])
    }
}

/// Walks like `walk`, and keeps every frame's start offset so a segment can be
/// trimmed back frame by frame.
fn walk_collect(bytes: &[u8], start: usize, end: usize) -> (Run, Vec<usize>) {
    let mut starts = Vec::new();
    let mut pos = start;
    loop {
        let step = walk(bytes, pos, end, 1);
        if step.frames == 0 { break }
        starts.push(pos);
        pos = step.cut;
    }
    (walk(bytes, start, end, usize::MAX), starts)
}

/// Trim frames off the end of a segment until it parses.
///
/// The frame layer walking cleanly up to a hole does not mean the frames right
/// before it are intact -- their headers survive while their message payloads do
/// not, which is why a run of segments can walk perfectly and still fail to
/// parse. Back off a frame at a time (doubling) until the parser accepts it.
fn trim_to_parse(bytes: &[u8], e0: usize, seg: (usize, usize), starts: &[usize]) -> Option<(usize, usize)> {
    let exe = std::env::current_exe().expect("exe");
    let mut k = 0usize;
    loop {
        if k >= starts.len() { return None }
        let stop = if k == 0 { seg.1 } else { starts[starts.len() - k] };
        if verify(&exe, &probe_demo(bytes, e0, seg.0, stop)) {
            if k > 0 { println!("    trimmed {k} damaged frames off the end of this segment") }
            return Some((seg.0, stop));
        }
        k = if k == 0 { 1 } else { k * 2 };
        if k > 4096 { return None }
    }
}

/// End of the signon. Entry 0 finishes on a section-end frame, and some
/// recordings write two back to back -- `m3_h1-1` does, `m3_h2` does not -- so
/// consume the whole run. Stopping after the first leaves the second as entry
/// 1's opening frame, which ends entry 1 immediately: the result parses, and
/// yields 32 frames out of half a million.
fn entry0_end(bytes: &[u8], end: usize) -> Option<usize> {
    let mut pos = DEMO_HEADER_SIZE;
    loop {
        if pos + FRAME_HEADER_SIZE > end { return None }
        let t = bytes[pos];
        let step = walk(bytes, pos, end, 1);
        if step.frames == 0 { return None }
        pos = step.cut;
        if t == 5 {
            while pos + FRAME_HEADER_SIZE <= end && bytes[pos] == 5 {
                let s = walk(bytes, pos, end, 1);
                if s.frames == 0 { break }
                pos = s.cut;
            }
            return Some(pos);
        }
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

/// Splice `segments` together behind the original file header and wrap the
/// result in a two-entry directory. Segment 0 is always entry 0 (the signon).
fn assemble(bytes: &[u8], e0: usize, segments: &[(usize, usize)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&bytes[..DEMO_HEADER_SIZE]);
    for (s, e) in segments { out.extend_from_slice(&bytes[*s..*e]) }
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

fn main() {
    let argv: Vec<String> = std::env::args().collect();
    if argv.len() > 1 && argv[1] == "--verify" { verify_child() }

    let path = std::env::args().nth(1).expect("usage: multi_bridge <demo> <out.dem>");
    let out_path = std::env::args().nth(2).expect("out.dem");
    let bytes = std::fs::read(&path).expect("read");
    let dir_off = i32::from_le_bytes(bytes[DIRECTORY_OFFSET_POS..DEMO_HEADER_SIZE].try_into().unwrap()) as usize;
    let end = if dir_off > 0 && dir_off <= bytes.len() { dir_off } else { bytes.len() };
    let e0 = entry0_end(&bytes, end).expect("entry0");
    println!("{path}");
    println!("  frame area {DEMO_HEADER_SIZE}..{end} ({:.1} MB), entry 0 ends at {e0}",
        (end - DEMO_HEADER_SIZE) as f64 / 1e6);

    // Walk, cut, resume, repeat.
    let mut segments: Vec<(usize, usize)> = Vec::new();
    let mut frames_kept = 0usize;
    let mut dropped_bytes = 0usize;
    let mut pos = DEMO_HEADER_SIZE;
    let mut last_time = 0.0f32;

    for hole in 0..=MAX_HOLES {
        let (run, starts) = walk_collect(&bytes, pos, end);
        if run.frames > 0 {
            match trim_to_parse(&bytes, e0, (pos, run.cut), &starts) {
                Some(seg) => {
                    let kept = starts.iter().filter(|s| **s < seg.1).count();
                    segments.push(seg);
                    frames_kept += kept;
                    if run.last_time > 0.0 { last_time = run.last_time }
                }
                None => println!("    this segment never parses; dropping it"),
            }
        }
        if run.cut >= end.saturating_sub(64) {
            println!("  segment {}: {} frames to t={:.2}s -- reached the end",
                segments.len(), run.frames, run.last_time);
            break;
        }
        println!("  segment {}: {} frames, t={:.2}s -> {:.2}s, breaks at byte {}",
            segments.len(), run.frames, run.first_time, run.last_time, run.cut);
        if hole == MAX_HOLES {
            println!("  giving up after {MAX_HOLES} holes");
            break;
        }
        match next_resume(&bytes, e0, run.cut + 1, end, last_time) {
            Some(next) => {
                dropped_bytes += next - run.cut;
                println!("    resumes at byte {next} ({} bytes of rubble skipped)", next - run.cut);
                pos = next;
            }
            None => {
                println!("    nothing after this break resumes cleanly -- stopping");
                break;
            }
        }
    }

    let out = assemble(&bytes, e0, &segments);
    std::fs::write(&out_path, &out).expect("write");

    println!("\n  {} segments, {frames_kept} frames walked, {dropped_bytes} bytes of rubble dropped",
        segments.len());
    println!("  wrote {out_path} ({:.1} MB)", out.len() as f64 / 1e6);
    match dem::open_demo_from_bytes(&out) {
        Ok(d) => {
            let total: usize = d.directory.entries.iter().map(|e| e.frames.len()).sum();
            println!("  parse OK: {total} frames across {} entries", d.directory.entries.len());
            // Parsing is not the same as carrying the recording. A directory
            // entry ends at the first section-end frame inside it, so a wrong
            // entry 0 boundary yields a demo that parses perfectly and holds 32
            // frames out of half a million. Say so rather than leave it to be
            // noticed.
            if total * 10 < frames_kept * 9 {
                println!("  WRONG: the walk kept {frames_kept} frames but only {total} survive the directory");
                println!("         -- an entry is ending early, most likely at a section-end frame");
            }
        }
        Err(e) => println!("  parse FAILS: {e}"),
    }
}
