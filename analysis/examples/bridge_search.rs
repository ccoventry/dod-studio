//! Search for a working bridge across a demo's damage (#15).
//!
//! `bridge_skip_probe` takes the first place the frame layer walks cleanly again
//! and tries a few skips from there. That is enough for damage confined to one
//! frame; it fails where the wreckage is wider, because the first frame-plausible
//! offset is not necessarily a real frame boundary.
//!
//! This searches properly: candidate offsets are only considered when the walk
//! from them reaches the *end of the frame area*, which a false alignment does
//! not survive, and each candidate is then confirmed by actually parsing a
//! rebuilt demo. Parsing malformed input can abort the process outright (#225),
//! so progress is written to stderr and the caller is expected to restart this
//! from the last offset tried.
//!
//!     cargo run --release -p analysis --example bridge_search -- <demo> <damage> <from> <out.dem>

const DEMO_HEADER_SIZE: usize = 544;
const DIRECTORY_OFFSET_POS: usize = 540;
const FRAME_HEADER_SIZE: usize = 9;
const NETMSG_INFO_SIZE: usize = 464;
const NETWORK_HEADER_ALIGNMENT: usize = 468;
const MAX_PAYLOAD: usize = 2_097_152;

fn walk(bytes: &[u8], start: usize, end: usize, cap: usize) -> (usize, usize) {
    let mut pos = start; let mut n = 0usize;
    while n < cap {
        if pos + FRAME_HEADER_SIZE > end { break }
        let t = bytes[pos];
        let time = f32::from_le_bytes(bytes[pos+1..pos+5].try_into().unwrap());
        if (t > 9 && t != 255) || !time.is_finite() || time < 0.0 || time > 100_000.0 { break }
        pos += FRAME_HEADER_SIZE; n += 1;
        match t {
            5 | 2 | 255 => {}
            0 | 1 => {
                if pos + NETWORK_HEADER_ALIGNMENT > end { break }
                let l = i32::from_le_bytes(bytes[pos+NETMSG_INFO_SIZE..pos+NETWORK_HEADER_ALIGNMENT].try_into().unwrap());
                if l < 0 || l as usize > MAX_PAYLOAD { break }
                pos += NETWORK_HEADER_ALIGNMENT + l as usize;
            }
            3 => pos += 64, 4 => pos += 32, 6 => pos += 84, 7 => pos += 8,
            8 => { if pos+8 > end { break } let l = u32::from_le_bytes(bytes[pos+4..pos+8].try_into().unwrap()) as usize; pos += 24+l; }
            9 => { if pos+4 > end { break } let l = u32::from_le_bytes(bytes[pos..pos+4].try_into().unwrap()) as usize; pos += 4+l; }
            _ => break,
        }
        if pos > end { break }
    }
    (n, pos)
}

fn entry0_end(bytes: &[u8], end: usize) -> Option<usize> {
    let mut pos = DEMO_HEADER_SIZE;
    loop {
        if pos + FRAME_HEADER_SIZE > end { return None }
        let t = bytes[pos];
        pos += FRAME_HEADER_SIZE;
        match t {
            5 => return Some(pos),
            0 | 1 => { let l = i32::from_le_bytes(bytes[pos+NETMSG_INFO_SIZE..pos+NETWORK_HEADER_ALIGNMENT].try_into().unwrap()) as usize; pos += NETWORK_HEADER_ALIGNMENT + l; }
            2 | 255 => {}
            3 => pos += 64, 4 => pos += 32, 6 => pos += 84, 7 => pos += 8,
            8 => { let l = u32::from_le_bytes(bytes[pos+4..pos+8].try_into().unwrap()) as usize; pos += 24+l; }
            9 => { let l = u32::from_le_bytes(bytes[pos..pos+4].try_into().unwrap()) as usize; pos += 4+l; }
            _ => return None,
        }
        if pos > end { return None }
    }
}

fn put_i32(v: &mut Vec<u8>, x: i32) { v.extend_from_slice(&x.to_le_bytes()); }
fn put_f32(v: &mut Vec<u8>, x: f32) { v.extend_from_slice(&x.to_le_bytes()); }
fn dir_entry(o: &mut Vec<u8>, t: i32, d: &str, tt: f32, fc: i32, fo: i32, fl: i32) {
    put_i32(o, t); let mut b=[0u8;64];
    for (i,c) in d.bytes().take(63).enumerate() { b[i]=c }
    o.extend_from_slice(&b); put_i32(o,0); put_i32(o,-1); put_f32(o,tt); put_i32(o,fc); put_i32(o,fo); put_i32(o,fl);
}

fn build(bytes: &[u8], e0: usize, damage: usize, from: usize, end: usize) -> Vec<u8> {
    let mut out = bytes[..damage].to_vec();
    out.extend_from_slice(&bytes[from..end]);
    out.push(5); put_f32(&mut out, 0.0); put_i32(&mut out, 0);
    let dir = out.len();
    put_i32(&mut out, 2);
    dir_entry(&mut out, 0, "LOADING", 0.0, 0, DEMO_HEADER_SIZE as i32, (e0-DEMO_HEADER_SIZE) as i32);
    dir_entry(&mut out, 1, "Playback", 0.0, 0, e0 as i32, (dir-e0) as i32);
    out[DIRECTORY_OFFSET_POS..DEMO_HEADER_SIZE].copy_from_slice(&(dir as i32).to_le_bytes());
    out
}

fn main() {
    use std::io::Write;
    let mut a = std::env::args().skip(1);
    let path = a.next().expect("demo");
    let damage: usize = a.next().expect("damage").parse().unwrap();
    let from: usize = a.next().expect("from").parse().unwrap();
    let out_path = a.next().expect("out.dem");
    let bytes = std::fs::read(&path).expect("read");
    let dir_off = i32::from_le_bytes(bytes[DIRECTORY_OFFSET_POS..DEMO_HEADER_SIZE].try_into().unwrap()) as usize;
    let end = if dir_off>0 && dir_off<=bytes.len() { dir_off } else { bytes.len() };
    let e0 = entry0_end(&bytes, end).expect("entry0");

    // Parsing alone is too weak a test: an offset can parse and contribute
    // almost nothing, because the entry ends at the first section boundary the
    // tail happens to contain. Demand that the result actually carries the tail.
    let (prefix_frames, _) = walk(&bytes, DEMO_HEADER_SIZE, damage, usize::MAX);
    let sidecar = format!("{out_path}.best");
    let mut best: usize = std::fs::read_to_string(&sidecar).ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(prefix_frames + 10_000);
    let required = best;
    eprintln!("prefix walks {prefix_frames} frames; a real bridge must exceed {required}");

    let mut scan = from.max(damage);
    while scan < end {
        // Only consider offsets whose walk reaches the very end of the frame
        // area -- a false alignment does not survive that far.
        // Cheap reject first: almost every offset dies within a few frames, and
        // walking each one to the end of an 80 MB file would dominate runtime.
        let (quick, _) = walk(&bytes, scan, end, 200);
        if quick < 200 { scan += 1; continue }
        let (n, stop) = walk(&bytes, scan, end, usize::MAX);
        if n > 5000 && stop >= end.saturating_sub(64) {
            eprintln!("TRYING {scan}");
            let _ = std::io::stderr().flush();
            let cand = build(&bytes, e0, damage, scan, end);
            if let Ok(d) = dem::open_demo_from_bytes(&cand) {
                let frames: usize = d.directory.entries.iter().map(|e| e.frames.len()).sum();
                if frames <= best {
                    eprintln!("  offset {scan} parses but yields only {frames} frames -- keeping best {best}");
                    scan += 1;
                    continue;
                }
                best = frames;
                std::fs::write(&sidecar, best.to_string()).ok();
                std::fs::write(&out_path, &cand).expect("write");
                println!("BETTER offset={scan} frames={frames} bytes={}", cand.len());
                let _ = std::io::stdout().flush();
            }
            scan += 1;
        } else {
            scan += 1;
        }
    }
    println!("EXHAUSTED best={best}");
}
