//! Is the material *after* a demo's corruption still intact? (#15)
//!
//! `salvage_demo` recovers the prefix before the damage. That says nothing
//! about the remainder: a frame walk cannot continue because it no longer knows
//! where the next frame starts, not because the bytes are gone. This scans
//! forward from the damage looking for a position where the frame layer walks
//! cleanly again for a long run, which distinguishes "the rest of the file is
//! rubble" from "one bad spot, then the recording carries on".
//!
//!     cargo run --release -p analysis --example resync_probe -- <demo>

const DEMO_HEADER_SIZE: usize = 544;
const DIRECTORY_OFFSET_POS: usize = 544 - 4;
const FRAME_HEADER_SIZE: usize = 9;
const NETMSG_INFO_SIZE: usize = 464;
const NETWORK_HEADER_ALIGNMENT: usize = 468;
const MAX_PAYLOAD_LIMIT_BYTES: usize = 2_097_152;

/// Walks from `start`, returning how many frames it managed before something
/// stopped making sense (and the last time value it saw).
fn run_length(bytes: &[u8], start: usize, end: usize, cap: usize) -> (usize, f32) {
    let mut pos = start;
    let mut frames = 0usize;
    let mut last_time = 0.0f32;
    while frames < cap {
        if pos + FRAME_HEADER_SIZE > end { break }
        let t = bytes[pos];
        let time = f32::from_le_bytes(bytes[pos + 1..pos + 5].try_into().unwrap());
        if (t > 9 && t != 255) || !time.is_finite() || time < 0.0 || time > 100_000.0 { break }
        if t != 255 && time > 0.0 { last_time = time; }
        pos += FRAME_HEADER_SIZE;
        frames += 1;
        match t {
            5 => {}
            0 | 1 => {
                if pos + NETWORK_HEADER_ALIGNMENT > end { break }
                let len = i32::from_le_bytes(bytes[pos + NETMSG_INFO_SIZE..pos + NETWORK_HEADER_ALIGNMENT].try_into().unwrap());
                if len < 0 || len as usize > MAX_PAYLOAD_LIMIT_BYTES { break }
                pos += NETWORK_HEADER_ALIGNMENT + len as usize;
            }
            2 | 255 => {}
            3 => pos += 64,
            4 => pos += 32,
            6 => pos += 84,
            7 => pos += 8,
            8 => { if pos + 8 > end { break } let l = u32::from_le_bytes(bytes[pos+4..pos+8].try_into().unwrap()) as usize; pos += 24 + l; }
            9 => { if pos + 4 > end { break } let l = u32::from_le_bytes(bytes[pos..pos+4].try_into().unwrap()) as usize; pos += 4 + l; }
            _ => break,
        }
        if pos > end { break }
    }
    (frames, last_time)
}

fn main() {
    let path = std::env::args().nth(1).expect("usage: resync_probe <demo>");
    let damage: usize = std::env::args().nth(2).expect("damage offset").parse().unwrap();
    let bytes = std::fs::read(&path).expect("read");
    let dir_off = i32::from_le_bytes(bytes[DIRECTORY_OFFSET_POS..DEMO_HEADER_SIZE].try_into().unwrap()) as usize;
    let end = if dir_off > 0 && dir_off <= bytes.len() { dir_off } else { bytes.len() };
    println!("file {:.1} MB, frame area ends at {end}, damage at {damage}", bytes.len() as f64/1e6);
    println!("searching forward for a position that walks cleanly for 5000+ frames...\n");

    const GOOD_RUN: usize = 5000;
    let mut found = 0;
    let mut scan = damage;
    while scan < end && found < 6 {
        let (n, t) = run_length(&bytes, scan, end, GOOD_RUN + 1);
        if n > GOOD_RUN {
            let pct = 100.0 * (scan - DEMO_HEADER_SIZE) as f64 / (end - DEMO_HEADER_SIZE) as f64;
            // how far does it actually go?
            let (full, ft) = run_length(&bytes, scan, end, usize::MAX);
            println!("  resync at byte {scan} ({pct:.1}% in, {} bytes past the damage)", scan - damage);
            println!("     walks {full} frames, last time {ft:.2}s (started around {t:.2}s)");
            found += 1;
            // jump past this run to look for the next independent one
            let (_, _) = (full, ft);
            break;
        }
        scan += 1;
    }
    if found == 0 { println!("  no clean resync point found in the remainder"); }
}
