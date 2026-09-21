//! Recover the intact prefix of a demo that DoD refuses to play (#15).
//!
//! The four demos from one bad session fail at the *netmessage* layer
//! ("delta_description_t entry missing ...", "message length too long"), which
//! takes the whole file down even though the damage is often far into it. The
//! frame layer above is length-prefixed and self-describing, so it can be walked
//! without decoding a netmessage, and the walk stops exactly where the file
//! first stops making sense.
//!
//! Everything before that point is a complete demo in its own right: the init
//! block lives in directory entry 0 and every entity delta refers backwards only
//! (#58). So salvage is: keep the good prefix, terminate it, and write a fresh
//! directory describing it.
//!
//!     cargo run --release -p analysis --example salvage_demo -- <in.dem> <out.dem>

const DEMO_HEADER_SIZE: usize = 544;
const DIRECTORY_OFFSET_POS: usize = 540;
const FRAME_HEADER_SIZE: usize = 9;
const NETMSG_INFO_SIZE: usize = 464;
const NETWORK_HEADER_ALIGNMENT: usize = 468;
const MAX_PAYLOAD_LIMIT_BYTES: usize = 2_097_152;

struct Walk {
    /// Byte offset just past entry 0's terminating NextSection.
    entry0_end: Option<usize>,
    /// Start of the first frame that did not make sense, or the clean end.
    good_end: usize,
    frames: usize,
    last_time: f32,
    reason: String,
}

fn walk(bytes: &[u8], end: usize) -> Walk {
    let mut pos = DEMO_HEADER_SIZE;
    let mut w = Walk { entry0_end: None, good_end: pos, frames: 0, last_time: 0.0, reason: String::new() };
    loop {
        let frame_start = pos;
        if pos + FRAME_HEADER_SIZE > end {
            w.good_end = frame_start; w.reason = "reached end of file".into(); return w;
        }
        let type_byte = bytes[pos];
        let time = f32::from_le_bytes(bytes[pos + 1..pos + 5].try_into().unwrap());
        if (type_byte > 9 && type_byte != 255) || !time.is_finite() {
            w.good_end = frame_start;
            w.reason = format!("frame type {type_byte} / time {time}");
            return w;
        }
        if type_byte != 255 && time > 0.0 { w.last_time = time; }
        pos += FRAME_HEADER_SIZE;
        w.frames += 1;
        match type_byte {
            5 => {
                if w.entry0_end.is_none() {
                    w.entry0_end = Some(pos);
                } else {
                    w.good_end = pos; w.reason = "clean end".into(); return w;
                }
            }
            0 | 1 => {
                if pos + NETWORK_HEADER_ALIGNMENT > end {
                    w.good_end = frame_start; w.reason = "truncated network frame".into(); return w;
                }
                let len = i32::from_le_bytes(
                    bytes[pos + NETMSG_INFO_SIZE..pos + NETWORK_HEADER_ALIGNMENT].try_into().unwrap()) as usize;
                if len > MAX_PAYLOAD_LIMIT_BYTES {
                    w.good_end = frame_start; w.reason = format!("impossible packet size {len}"); return w;
                }
                pos += NETWORK_HEADER_ALIGNMENT + len;
            }
            2 | 255 => {}
            3 => pos += 64,
            4 => pos += 32,
            6 => pos += 84,
            7 => pos += 8,
            8 => {
                if pos + 8 > end { w.good_end = frame_start; w.reason = "truncated sound".into(); return w; }
                let len = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()) as usize;
                pos += 24 + len;
            }
            9 => {
                if pos + 4 > end { w.good_end = frame_start; w.reason = "truncated buffer".into(); return w; }
                let len = u32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap()) as usize;
                pos += 4 + len;
            }
            _ => { w.good_end = frame_start; w.reason = format!("unhandled type {type_byte}"); return w; }
        }
        if pos > end { w.good_end = frame_start; w.reason = "frame ran past end".into(); return w; }
        w.good_end = pos;
    }
}

fn put_i32(v: &mut Vec<u8>, x: i32) { v.extend_from_slice(&x.to_le_bytes()); }
fn put_f32(v: &mut Vec<u8>, x: f32) { v.extend_from_slice(&x.to_le_bytes()); }

fn dir_entry(out: &mut Vec<u8>, type_: i32, desc: &str, track_time: f32, frame_count: i32, frame_offset: i32, file_length: i32) {
    put_i32(out, type_);
    let mut d = [0u8; 64];
    for (i, b) in desc.bytes().take(63).enumerate() { d[i] = b; }
    out.extend_from_slice(&d);
    put_i32(out, 0);            // flags
    put_i32(out, -1);           // cd_track
    put_f32(out, track_time);
    put_i32(out, frame_count);
    put_i32(out, frame_offset);
    put_i32(out, file_length);
}

fn main() {
    let mut args = std::env::args().skip(1);
    let input = args.next().expect("usage: salvage_demo <in.dem> <out.dem>");
    let output = args.next().expect("out.dem");
    let bytes = std::fs::read(&input).expect("read");

    let dir_off = i32::from_le_bytes(bytes[DIRECTORY_OFFSET_POS..DEMO_HEADER_SIZE].try_into().unwrap()) as usize;
    let end = if dir_off > 0 && dir_off <= bytes.len() { dir_off } else { bytes.len() };
    let w = walk(&bytes, end);

    println!("walk: {} frames, last good time {:.2}s", w.frames, w.last_time);
    println!("  stopped at byte {} ({:.1}% of frame area) -- {}", w.good_end,
        100.0 * (w.good_end - DEMO_HEADER_SIZE) as f64 / (end - DEMO_HEADER_SIZE).max(1) as f64, w.reason);
    let Some(entry0_end) = w.entry0_end else { println!("  no entry-0 boundary found; nothing to salvage"); return };

    // Good prefix, then a NextSection to close the playback section.
    let mut out = bytes[..w.good_end].to_vec();
    out.push(5);
    put_f32(&mut out, w.last_time);
    put_i32(&mut out, 0);

    let new_dir_off = out.len();
    let e0_frames = 0i32; // engine tolerates a nominal count; offsets are what it seeks by
    let e1_frames = w.frames as i32;
    put_i32(&mut out, 2);
    dir_entry(&mut out, 0, "LOADING", 0.0, e0_frames, DEMO_HEADER_SIZE as i32, (entry0_end - DEMO_HEADER_SIZE) as i32);
    let e1_len = (new_dir_off - entry0_end) as i32;
    dir_entry(&mut out, 1, "Playback", w.last_time, e1_frames, entry0_end as i32, e1_len);
    out[DIRECTORY_OFFSET_POS..DEMO_HEADER_SIZE].copy_from_slice(&(new_dir_off as i32).to_le_bytes());

    std::fs::write(&output, &out).expect("write");
    println!("  wrote {} ({:.1} MB, {:.1}% of original)", output, out.len() as f64 / 1e6,
        100.0 * out.len() as f64 / bytes.len() as f64);

    match dem::open_demo_from_bytes(&out) {
        Ok(d) => {
            let n: usize = d.directory.entries.iter().map(|e| e.frames.len()).sum();
            println!("  RE-PARSE OK: {} entries, {n} frames", d.directory.entries.len());
        }
        Err(e) => println!("  re-parse failed: {e}"),
    }
    match analysis::Analysis::try_from_bytes(&out) {
        Ok(a) => println!("  ANALYSIS OK: map={:?} players={} duration={:.1}s",
            a.state.initial_map_name, a.state.players.len(), a.demo_info.playback_time),
        Err(e) => println!("  analysis failed: {e}"),
    }
}
