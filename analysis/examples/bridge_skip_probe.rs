//! Bridge a demo's damage by resuming a little *after* the resync point (#15).
//!
//! Stitching two distant slices of a healthy demo works (`stitch_probe`), so the
//! discontinuity itself is not fatal. That points at the earlier byte-level
//! bridge failing for a narrower reason: the frames immediately after the hole
//! are themselves damaged, even though the frame layer walks. This copies whole
//! frames -- so framing is exact by construction -- and tries resuming several
//! frames later, to step past the wreckage at the edge of the hole.
//!
//!     cargo run --release -p analysis --example bridge_skip_probe -- <demo> <damage-off> <resync-off>

const DEMO_HEADER_SIZE: usize = 544;
const DIRECTORY_OFFSET_POS: usize = 540;
const FRAME_HEADER_SIZE: usize = 9;
const NETMSG_INFO_SIZE: usize = 464;
const NETWORK_HEADER_ALIGNMENT: usize = 468;
const MAX_PAYLOAD: usize = 2_097_152;

/// Byte offsets of each frame start from `start`, until the walk breaks.
fn frame_offsets(bytes: &[u8], start: usize, end: usize, cap: usize) -> Vec<usize> {
    let mut pos = start; let mut out = Vec::new();
    while out.len() < cap {
        if pos + FRAME_HEADER_SIZE > end { break }
        let t = bytes[pos];
        let time = f32::from_le_bytes(bytes[pos+1..pos+5].try_into().unwrap());
        if (t > 9 && t != 255) || !time.is_finite() || time < 0.0 || time > 100_000.0 { break }
        out.push(pos);
        pos += FRAME_HEADER_SIZE;
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
    out
}

fn entry0_end(bytes: &[u8], end: usize) -> Option<usize> {
    let offs = frame_offsets(bytes, DEMO_HEADER_SIZE, end, usize::MAX);
    for (i, &o) in offs.iter().enumerate() {
        if bytes[o] == 5 { return Some(offs.get(i+1).copied().unwrap_or(o + FRAME_HEADER_SIZE)) }
    }
    None
}

fn put_i32(v: &mut Vec<u8>, x: i32) { v.extend_from_slice(&x.to_le_bytes()); }
fn put_f32(v: &mut Vec<u8>, x: f32) { v.extend_from_slice(&x.to_le_bytes()); }
fn dir_entry(o: &mut Vec<u8>, t: i32, d: &str, tt: f32, fc: i32, fo: i32, fl: i32) {
    put_i32(o, t); let mut b=[0u8;64];
    for (i,c) in d.bytes().take(63).enumerate() { b[i]=c }
    o.extend_from_slice(&b); put_i32(o,0); put_i32(o,-1); put_f32(o,tt); put_i32(o,fc); put_i32(o,fo); put_i32(o,fl);
}

fn main() {
    let p = std::env::args().nth(1).expect("demo");
    let damage: usize = std::env::args().nth(2).expect("damage").parse().unwrap();
    let resync: usize = std::env::args().nth(3).expect("resync").parse().unwrap();
    let bytes = std::fs::read(&p).expect("read");
    let dir_off = i32::from_le_bytes(bytes[DIRECTORY_OFFSET_POS..DEMO_HEADER_SIZE].try_into().unwrap()) as usize;
    let end = if dir_off>0 && dir_off<=bytes.len() { dir_off } else { bytes.len() };
    let e0 = entry0_end(&bytes, end).expect("entry0");
    let tail = frame_offsets(&bytes, resync, end, 40_000);
    println!("entry0 ends {e0}; prefix ends {damage}; tail has {} frame starts mapped", tail.len());

    let out_path = std::env::args().nth(4);
    std::panic::set_hook(Box::new(|_| {}));
    let mut best: Option<(usize, Vec<u8>)> = None;
    for &skip in &[0usize, 1, 2, 3, 5, 10, 20, 50, 100] {
        let Some(&from) = tail.get(skip) else { continue };
        let mut out = bytes[..damage].to_vec();
        out.extend_from_slice(&bytes[from..end]);
        out.push(5); put_f32(&mut out, 0.0); put_i32(&mut out, 0);
        let dir = out.len();
        put_i32(&mut out, 2);
        dir_entry(&mut out, 0, "LOADING", 0.0, 0, DEMO_HEADER_SIZE as i32, (e0-DEMO_HEADER_SIZE) as i32);
        dir_entry(&mut out, 1, "Playback", 0.0, 0, e0 as i32, (dir-e0) as i32);
        out[DIRECTORY_OFFSET_POS..DEMO_HEADER_SIZE].copy_from_slice(&(dir as i32).to_le_bytes());
        let r = std::panic::catch_unwind(|| dem::open_demo_from_bytes(&out)
            .map(|d| d.directory.entries.iter().map(|e| e.frames.len()).sum::<usize>()));
        let verdict = match r {
            Ok(Ok(n)) => format!("PARSES -- {n} frames"),
            Ok(Err(_)) => "parse error".into(),
            Err(_) => "PANIC".into(),
        };
        let _ = std::panic::take_hook();
        println!("  skip {skip:>5} frames (byte {from}): {verdict}");
        if verdict.starts_with("PARSES") && best.is_none() { best = Some((skip, out)); }
        std::panic::set_hook(Box::new(|_| {}));
        if best.is_some() { break }
    }
    let _ = std::panic::take_hook();

    if let (Some((skip, out)), Some(path)) = (best, out_path) {
        // Reparse and restate the entry durations: assembled byte-wise, the
        // directory's track_time is a placeholder, and readers (including this
        // repo's analyzer) report it as the demo's length.
        let out = match dem::open_demo_from_bytes(&out) {
            Ok(mut d) => {
                for e in d.directory.entries.iter_mut() {
                    e.frame_count = e.frames.len() as i32;
                    e.track_time = e.frames.iter().map(|f| f.time).fold(0.0f32, f32::max);
                }
                d.write_to_bytes()
            }
            Err(_) => out,
        };
        std::fs::write(&path, &out).expect("write");
        println!("
  recovered with skip={skip}: {} ({:.1} MB, {:.1}% of original)",
            path, out.len() as f64/1e6, 100.0*out.len() as f64/bytes.len() as f64);
        match analysis::Analysis::try_from_bytes(&out) {
            Ok(a) => println!("  ANALYSIS OK: map={:?} players={} duration={:.1}s",
                a.state.initial_map_name, a.state.players.len(), a.demo_info.playback_time),
            Err(e) => println!("  analysis failed: {e}"),
        }
    }
}
