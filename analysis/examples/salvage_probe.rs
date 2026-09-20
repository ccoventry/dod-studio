//! How much of a demo that DoD refuses to play is actually intact? (#15, #41)
//!
//! Four demos from one session fail to parse: one with
//! "delta_description_t entry missing name/bits/divisor/flags", three with
//! "message length too long". Those are *netmessage-level* failures. The frame
//! layer above them is length-prefixed and self-describing, so it can be walked
//! without decoding a single netmessage -- which locates where the file first
//! stops making sense, and how much sits in front of that point.
//!
//! If the damage is late in the file, everything before it is recoverable: the
//! init block lives in directory entry 0 and every delta refers backwards only
//! (#58), so a prefix is a valid demo in its own right.
//!
//!     cargo run --release -p analysis --example salvage_probe -- <demo>...

const DEMO_HEADER_SIZE: usize = 544;
const DIRECTORY_OFFSET_POS: usize = 540;
const FRAME_HEADER_SIZE: usize = 9;
const NETMSG_INFO_SIZE: usize = 464;
const NETWORK_HEADER_ALIGNMENT: usize = 468;
const MAX_PAYLOAD_LIMIT_BYTES: usize = 2_097_152;
const CMD_FRAME_SIZE: usize = 64;
const CLIENT_DATA_FRAME_SIZE: usize = 32;
const EVENT_FRAME_SIZE: usize = 84;

/// Walks the frame layer only. Returns (frames, last good time, byte offset,
/// why it stopped).
fn walk(bytes: &[u8], start: usize, end: usize) -> (usize, f32, usize, String) {
    let mut pos = start;
    let mut frames = 0usize;
    let mut last_time = 0.0f32;
    let mut sections = 0usize;
    loop {
        if pos + FRAME_HEADER_SIZE > end {
            return (frames, last_time, pos, "reached end of section".into());
        }
        let type_byte = bytes[pos];
        let time = f32::from_le_bytes(bytes[pos + 1..pos + 5].try_into().unwrap());
        if type_byte > 9 && type_byte != 255 {
            return (frames, last_time, pos, format!("impossible frame type {type_byte}"));
        }
        if !time.is_finite() {
            return (frames, last_time, pos, "frame time is not a number".into());
        }
        if type_byte != 255 {
            last_time = time;
        }
        pos += FRAME_HEADER_SIZE;
        frames += 1;
        match type_byte {
            5 => {
                // Entry 0 ends with one of these; the stream continues into the
                // playback section immediately after. Only the second one ends
                // the file.
                sections += 1;
                if sections >= 2 {
                    return (frames, last_time, pos, "final section boundary (clean end)".into());
                }
            }
            0 | 1 => {
                if pos + NETWORK_HEADER_ALIGNMENT > end {
                    return (frames, last_time, pos, "truncated inside a network frame".into());
                }
                let len = i32::from_le_bytes(
                    bytes[pos + NETMSG_INFO_SIZE..pos + NETWORK_HEADER_ALIGNMENT].try_into().unwrap(),
                ) as usize;
                if len > MAX_PAYLOAD_LIMIT_BYTES {
                    return (frames, last_time, pos, format!("impossible packet size {len}"));
                }
                pos += NETWORK_HEADER_ALIGNMENT + len;
            }
            2 | 255 => {}
            3 => pos += CMD_FRAME_SIZE,
            4 => pos += CLIENT_DATA_FRAME_SIZE,
            6 => pos += EVENT_FRAME_SIZE,
            7 => pos += 8,
            8 => {
                if pos + 8 > end { return (frames, last_time, pos, "truncated sound frame".into()); }
                let len = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()) as usize;
                pos += 24 + len;
            }
            9 => {
                if pos + 4 > end { return (frames, last_time, pos, "truncated buffer frame".into()); }
                let len = u32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap()) as usize;
                pos += 4 + len;
            }
            _ => return (frames, last_time, pos, format!("unhandled frame type {type_byte}")),
        }
        if pos > end {
            return (frames, last_time, pos, "a frame ran past the end of the section".into());
        }
    }
}

fn main() {
    for path in std::env::args().skip(1) {
        let Ok(bytes) = std::fs::read(&path) else { println!("{path}: unreadable"); continue };
        let name = std::path::Path::new(&path).file_name().unwrap().to_string_lossy();
        println!("\n=== {name}  ({:.1} MB) ===", bytes.len() as f64 / 1e6);

        match analysis::Analysis::try_from_bytes(&bytes) {
            Ok(_) => println!("  full parse: OK"),
            Err(e) => println!("  full parse: FAILS -- {e}"),
        }

        if bytes.len() < DEMO_HEADER_SIZE {
            println!("  shorter than a demo header");
            continue;
        }
        let dir_off = i32::from_le_bytes(
            bytes[DIRECTORY_OFFSET_POS..DEMO_HEADER_SIZE].try_into().unwrap(),
        ) as usize;
        println!("  directory offset: {dir_off} (file is {} bytes)", bytes.len());
        let end = if dir_off > 0 && dir_off <= bytes.len() { dir_off } else { bytes.len() };

        let (frames, last_time, stopped_at, why) = walk(&bytes, DEMO_HEADER_SIZE, end);
        println!("  frame walk: {frames} frames, last good time {last_time:.2}s");
        println!("  stopped at byte {stopped_at} ({:.1}% through the frame area) -- {why}",
            100.0 * (stopped_at - DEMO_HEADER_SIZE) as f64 / (end - DEMO_HEADER_SIZE).max(1) as f64);
    }
}
