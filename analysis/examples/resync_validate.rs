//! Find a resync point that is clean at the *message* layer, not just the frame
//! layer (#15).
//!
//! `resync_probe` finds where frame headers start making sense again, and that
//! is not sufficient: bridging there keeps the frame walk happy but netmessage
//! parsing still fails. Frame-level validation never inspects payload contents,
//! so an offset can be plausible frame-wise and still be wrong.
//!
//! This tries candidate offsets around it, and for each one builds a small demo
//! -- the original signon block plus a slice of tail -- and asks the real parser
//! whether it reads. The first offset that parses is a true frame boundary.
//!
//!     cargo run --release -p analysis --example resync_validate -- <demo> <approx-offset>

const DEMO_HEADER_SIZE: usize = 544;
const DIRECTORY_OFFSET_POS: usize = 540;
const FRAME_HEADER_SIZE: usize = 9;
const NETMSG_INFO_SIZE: usize = 464;
const NETWORK_HEADER_ALIGNMENT: usize = 468;

fn put_i32(v: &mut Vec<u8>, x: i32) { v.extend_from_slice(&x.to_le_bytes()); }
fn put_f32(v: &mut Vec<u8>, x: f32) { v.extend_from_slice(&x.to_le_bytes()); }
fn dir_entry(out: &mut Vec<u8>, type_: i32, desc: &str, tt: f32, fc: i32, fo: i32, fl: i32) {
    put_i32(out, type_);
    let mut d = [0u8; 64];
    for (i, b) in desc.bytes().take(63).enumerate() { d[i] = b; }
    out.extend_from_slice(&d);
    put_i32(out, 0); put_i32(out, -1); put_f32(out, tt);
    put_i32(out, fc); put_i32(out, fo); put_i32(out, fl);
}

/// End of entry 0: byte just past its terminating NextSection frame.
fn entry0_end(bytes: &[u8], end: usize) -> Option<usize> {
    let mut pos = DEMO_HEADER_SIZE;
    loop {
        if pos + FRAME_HEADER_SIZE > end { return None }
        let t = bytes[pos];
        pos += FRAME_HEADER_SIZE;
        match t {
            5 => return Some(pos),
            0 | 1 => {
                if pos + NETWORK_HEADER_ALIGNMENT > end { return None }
                let len = i32::from_le_bytes(bytes[pos+NETMSG_INFO_SIZE..pos+NETWORK_HEADER_ALIGNMENT].try_into().unwrap()) as usize;
                pos += NETWORK_HEADER_ALIGNMENT + len;
            }
            2 | 255 => {}
            3 => pos += 64, 4 => pos += 32, 6 => pos += 84, 7 => pos += 8,
            8 => { let l = u32::from_le_bytes(bytes[pos+4..pos+8].try_into().unwrap()) as usize; pos += 24 + l; }
            9 => { let l = u32::from_le_bytes(bytes[pos..pos+4].try_into().unwrap()) as usize; pos += 4 + l; }
            _ => return None,
        }
        if pos > end { return None }
    }
}

fn build(bytes: &[u8], e0_end: usize, tail_start: usize, tail_len: usize) -> Vec<u8> {
    let mut out = bytes[..e0_end].to_vec();
    let stop = (tail_start + tail_len).min(bytes.len());
    out.extend_from_slice(&bytes[tail_start..stop]);
    out.push(5); put_f32(&mut out, 0.0); put_i32(&mut out, 0);
    let dir = out.len();
    put_i32(&mut out, 2);
    dir_entry(&mut out, 0, "LOADING", 0.0, 0, DEMO_HEADER_SIZE as i32, (e0_end - DEMO_HEADER_SIZE) as i32);
    dir_entry(&mut out, 1, "Playback", 0.0, 0, e0_end as i32, (dir - e0_end) as i32);
    out[DIRECTORY_OFFSET_POS..DEMO_HEADER_SIZE].copy_from_slice(&(dir as i32).to_le_bytes());
    out
}

fn main() {
    let path = std::env::args().nth(1).expect("demo");
    let approx: usize = std::env::args().nth(2).expect("offset").parse().unwrap();
    let bytes = std::fs::read(&path).expect("read");
    let dir_off = i32::from_le_bytes(bytes[DIRECTORY_OFFSET_POS..DEMO_HEADER_SIZE].try_into().unwrap()) as usize;
    let end = if dir_off > 0 && dir_off <= bytes.len() { dir_off } else { bytes.len() };
    let e0 = entry0_end(&bytes, end).expect("entry0");
    println!("entry0 ends at {e0}; probing offsets around {approx} with a 4 MB tail slice\n");

    const SLICE: usize = 4 * 1024 * 1024;
    // The parser panics on some malformed input rather than returning Err, so
    // candidates have to be tried behind catch_unwind.
    std::panic::set_hook(Box::new(|_| {}));
    let mut hits = 0;
    for delta in -8i64..768 {
        let cand = (approx as i64 + delta) as usize;
        if cand >= end { break }
        let candidate = build(&bytes, e0, cand, SLICE);
        let res = std::panic::catch_unwind(|| {
            dem::open_demo_from_bytes(&candidate)
                .map(|d| d.directory.entries.iter().map(|e| e.frames.len()).sum::<usize>())
        });
        if let Ok(Ok(n)) = res {
            let _ = std::panic::take_hook();
            println!("  OFFSET {cand} (approx{delta:+}) PARSES -- {n} frames");
            std::panic::set_hook(Box::new(|_| {}));
            hits += 1;
            if hits >= 3 { break }
        }
    }
    if hits == 0 { println!("  no offset in the probed window parses at the message layer"); }
}
