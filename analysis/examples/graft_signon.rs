//! Repair a demo whose *signon* is damaged, using another demo of the same match (#15).
//!
//! `m3_h1-1` was written off as unrecoverable because its damage is not in the
//! playback stream but in directory entry 0: the parser stops with
//! `delta_description_t entry missing name/bits/divisor/flags`, so the delta
//! decoders never get built and nothing after it can be read. Bridging holes
//! cannot help, because the problem is upstream of every hole.
//!
//! But entry 0 is not unique to a recording. It is the server's signon: the
//! server info, the seven delta descriptions, the entity baselines, the resource
//! list and the user message table. Two demos of the same match on the same
//! server carry the same one. `m3_h1` (16.2 MB, parses cleanly) and `m3_h1-1`
//! (83.7 MB, does not) are the same map `dod_armory_b6` at the same BSP CRC
//! 0xe204275c with the same demo and network protocols -- so the intact signon
//! can stand in for the damaged one.
//!
//! This is only sound for demos of the *same session*: baselines and the
//! resource list are index-based, so a server restart between the two
//! recordings would silently renumber everything. The header check here is
//! necessary, not sufficient -- treat a graft across sessions as suspect even
//! when it parses.
//!
//!     cargo run --release -p analysis --example graft_signon -- <damaged.dem> <donor.dem> <out.dem>

const DEMO_HEADER_SIZE: usize = 544;
const DIRECTORY_OFFSET_POS: usize = 540;
const FRAME_HEADER_SIZE: usize = 9;
const NETMSG_INFO_SIZE: usize = 464;
const NETWORK_HEADER_ALIGNMENT: usize = 468;
const MAX_PAYLOAD: usize = 2_097_152;

/// Advances past one frame at `pos`, or returns None if it cannot be read.
fn step(bytes: &[u8], pos: usize, end: usize) -> Option<(u8, usize)> {
    if pos + FRAME_HEADER_SIZE > end { return None }
    let t = bytes[pos];
    let time = f32::from_le_bytes(bytes[pos + 1..pos + 5].try_into().ok()?);
    if (t > 9 && t != 255) || !time.is_finite() || time < 0.0 || time > 100_000.0 { return None }
    let mut p = pos + FRAME_HEADER_SIZE;
    match t {
        5 | 2 | 255 => {}
        0 | 1 => {
            if p + NETWORK_HEADER_ALIGNMENT > end { return None }
            let l = i32::from_le_bytes(bytes[p + NETMSG_INFO_SIZE..p + NETWORK_HEADER_ALIGNMENT].try_into().ok()?);
            if l < 0 || l as usize > MAX_PAYLOAD { return None }
            p += NETWORK_HEADER_ALIGNMENT + l as usize;
        }
        3 => p += 64,
        4 => p += 32,
        6 => p += 84,
        7 => p += 8,
        8 => { if p + 8 > end { return None } let l = u32::from_le_bytes(bytes[p + 4..p + 8].try_into().ok()?) as usize; p += 24 + l; }
        9 => { if p + 4 > end { return None } let l = u32::from_le_bytes(bytes[p..p + 4].try_into().ok()?) as usize; p += 4 + l; }
        _ => return None,
    }
    if p > end { return None }
    Some((t, p))
}

/// End of the signon by walking. Entry 0 finishes on a section-end frame, and
/// some recordings write two back to back -- a POV demo measured here does,
/// `m3_h2` does not -- so consume the whole run rather than the first one.
fn signon_end(bytes: &[u8], end: usize) -> Option<usize> {
    let mut pos = DEMO_HEADER_SIZE;
    loop {
        let (t, next) = step(bytes, pos, end)?;
        pos = next;
        if t == 5 {
            while let Some((t2, n2)) = step(bytes, pos, end) {
                if t2 != 5 { break }
                pos = n2;
            }
            return Some(pos);
        }
    }
}

fn put_i32(v: &mut Vec<u8>, x: i32) { v.extend_from_slice(&x.to_le_bytes()) }
fn put_f32(v: &mut Vec<u8>, x: f32) { v.extend_from_slice(&x.to_le_bytes()) }

fn dir_entry(o: &mut Vec<u8>, t: i32, d: &str, fo: i32, fl: i32) {
    put_i32(o, t);
    let mut b = [0u8; 64];
    for (i, c) in d.bytes().take(63).enumerate() { b[i] = c }
    o.extend_from_slice(&b);
    put_i32(o, 0);
    put_i32(o, -1);
    put_f32(o, 0.0);
    put_i32(o, 0);
    put_i32(o, fo);
    put_i32(o, fl);
}

fn frame_area(bytes: &[u8]) -> usize {
    let d = i32::from_le_bytes(bytes[DIRECTORY_OFFSET_POS..DEMO_HEADER_SIZE].try_into().unwrap()) as usize;
    if d > 0 && d <= bytes.len() { d } else { bytes.len() }
}

fn main() {
    let mut a = std::env::args().skip(1);
    let bad_path = a.next().expect("usage: graft_signon <damaged.dem> <donor.dem> <out.dem>");
    let donor_path = a.next().expect("donor.dem");
    let out_path = a.next().expect("out.dem");
    let bad = std::fs::read(&bad_path).expect("read damaged");
    let donor = std::fs::read(&donor_path).expect("read donor");

    // The signon only transfers between recordings of the same session. Map name,
    // BSP CRC and both protocol numbers must agree or the indices will not line up.
    let field = |b: &[u8], at: usize, len: usize| {
        let s = &b[at..at + len];
        String::from_utf8_lossy(&s[..s.iter().position(|c| *c == 0).unwrap_or(s.len())]).to_string()
    };
    let head = |b: &[u8]| (
        i32::from_le_bytes(b[8..12].try_into().unwrap()),
        i32::from_le_bytes(b[12..16].try_into().unwrap()),
        field(b, 16, 260),
        field(b, 276, 260),
        i32::from_le_bytes(b[536..540].try_into().unwrap()),
    );
    let (bd, bn, bmap, bdir, bcrc) = head(&bad);
    let (dd, dn, dmap, ddir, dcrc) = head(&donor);
    println!("damaged: {bmap} ({bdir}) crc 0x{:08x}, protocols {bd}/{bn}", bcrc as u32);
    println!("donor:   {dmap} ({ddir}) crc 0x{:08x}, protocols {dd}/{dn}", dcrc as u32);
    if (bd, bn, &bmap, &bdir, bcrc) != (dd, dn, &dmap, &ddir, dcrc) {
        println!("\nthese are not the same map and build -- refusing to graft");
        return;
    }
    println!("  same map, same BSP checksum, same protocols");

    let bad_end = frame_area(&bad);
    let donor_end = frame_area(&donor);
    let bad_signon = signon_end(&bad, bad_end).expect("damaged signon walk");
    let donor_signon = signon_end(&donor, donor_end).expect("donor signon walk");
    println!("  damaged signon is {} bytes, donor signon is {} bytes",
        bad_signon - DEMO_HEADER_SIZE, donor_signon - DEMO_HEADER_SIZE);

    // Damaged file's own header (identical but for the directory offset), the
    // donor's signon, then the damaged file's playback stream untouched.
    let mut out = bad[..DEMO_HEADER_SIZE].to_vec();
    out.extend_from_slice(&donor[DEMO_HEADER_SIZE..donor_signon]);
    let playback_at = out.len();
    out.extend_from_slice(&bad[bad_signon..bad_end]);
    out.push(5);
    put_f32(&mut out, 0.0);
    put_i32(&mut out, 0);
    let dir = out.len();
    put_i32(&mut out, 2);
    dir_entry(&mut out, 0, "LOADING", DEMO_HEADER_SIZE as i32, (playback_at - DEMO_HEADER_SIZE) as i32);
    dir_entry(&mut out, 1, "Playback", playback_at as i32, (dir - playback_at) as i32);
    out[DIRECTORY_OFFSET_POS..DEMO_HEADER_SIZE].copy_from_slice(&(dir as i32).to_le_bytes());
    std::fs::write(&out_path, &out).expect("write");
    println!("\nwrote {out_path} ({:.1} MB)", out.len() as f64 / 1e6);

    match dem::open_demo_from_bytes(&out) {
        Ok(d) => println!("  parse OK: {} frames across {} entries",
            d.directory.entries.iter().map(|e| e.frames.len()).sum::<usize>(),
            d.directory.entries.len()),
        Err(e) => println!("  parse FAILS: {e}"),
    }
}
