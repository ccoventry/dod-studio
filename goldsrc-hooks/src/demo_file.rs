//! What a `.dem` file's own directory says: how long its playback is (#465).
//!
//! The header's last field (offset 540) points at the directory: an `i32`
//! count, then 92-byte `demoentry_t`s (`nEntryType`, a 64-byte description,
//! `nFlags`, `nCDTrack`, `fTrackTime`, `nFrames`, `nOffset`, `nFileLength`).
//! Entry 0 is the "LOADING" segment (type 0, no time); the rest are playback.
//!
//! The demo player's buffered span also counts a stretch of the LOADING
//! segment (#810): its last frame (the section's end) sits far later than
//! the frame before it, and the player's world clock moves by that gap
//! before the first playback frame. Measured on both builds: 350.3 s of
//! lead-in on a 1251 s POV demo whose LOADING gap is 350.26 s, 49.3 s
//! against 49.24 s, 32.0 s against 31.58 s, and none on an HLTV demo whose
//! LOADING frames all read 0. Left out, the load sat at 99% for the last
//! quarter of a demo.

// Only the 32-bit build has the window; a host check still compiles this.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

const DIRECTORY_OFFSET_POS: u64 = 540;
const ENTRY_SIZE: usize = 92;
const TRACK_TIME_AT: usize = 76;
const OFFSET_AT: usize = 84;
const LENGTH_AT: usize = 88;
/// More entries than this is not a directory.
const MAX_ENTRIES: i32 = 1024;
/// A LOADING segment is tens of KB; more than this is not one.
const MAX_LOADING_BYTES: usize = 4 << 20;

/// What the demo player will have buffered once a demo is read, in seconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DemoLength {
    /// Every non-loading entry's `fTrackTime`, summed.
    pub playback: f32,
    /// The world-clock stretch the LOADING segment adds in front of it.
    pub lead_in: f32,
}

/// The demo's playback length and lead-in, from its directory and its
/// LOADING segment's frames.
pub fn length(path: &Path) -> Option<DemoLength> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut magic = [0u8; 8];
    file.read_exact(&mut magic).ok()?;
    if &magic[..6] != b"HLDEMO" {
        return None;
    }
    file.seek(SeekFrom::Start(DIRECTORY_OFFSET_POS)).ok()?;
    let directory = read_i32(&mut file)?;
    file.seek(SeekFrom::Start(u64::try_from(directory).ok()?))
        .ok()?;
    let count = read_i32(&mut file)?;
    if !(1..=MAX_ENTRIES).contains(&count) {
        return None;
    }
    let mut entries = vec![0u8; count as usize * ENTRY_SIZE];
    file.read_exact(&mut entries).ok()?;
    let playback = sum_playback(&entries);
    let lead_in = loading_segment(&mut file, &entries)
        .map(|bytes| lead_in(&bytes))
        .unwrap_or(0.0);
    Some(DemoLength { playback, lead_in })
}

/// The first LOADING entry's bytes.
fn loading_segment(file: &mut std::fs::File, entries: &[u8]) -> Option<Vec<u8>> {
    let entry = entries
        .as_chunks::<ENTRY_SIZE>()
        .0
        .iter()
        .find(|entry| field_i32(&entry[..], 0) == 0)?;
    let offset = u64::try_from(field_i32(entry, OFFSET_AT)).ok()?;
    let length = usize::try_from(field_i32(entry, LENGTH_AT)).ok()?;
    if length > MAX_LOADING_BYTES {
        return None;
    }
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut bytes = vec![0u8; length];
    file.read_exact(&mut bytes).ok()?;
    Some(bytes)
}

fn field_i32(bytes: &[u8], at: usize) -> i32 {
    bytes
        .get(at..at + 4)
        .and_then(|b| b.try_into().ok())
        .map(i32::from_le_bytes)
        .unwrap_or(0)
}

/// The gap between the LOADING segment's last two frame times; 0 when the
/// frames can't be walked to the end.
fn lead_in(segment: &[u8]) -> f32 {
    let mut times = Vec::new();
    let mut at = 0usize;
    while at + 9 <= segment.len() {
        let kind = segment[at];
        let time = f32::from_le_bytes(segment[at + 1..at + 5].try_into().unwrap_or_default());
        times.push(time);
        let body = at + 9;
        let len_at = |pos: usize| usize::try_from(field_i32(segment, pos)).ok();
        let next = match kind {
            // Network message: 464 bytes of view and sequence info, then a sized message.
            0 | 1 => len_at(body + 464).map(|n| body + 468 + n),
            2 => Some(body),
            3 => Some(body + 64),
            4 => Some(body + 32),
            // NextSection ends the segment.
            5 => break,
            6 => Some(body + 84),
            7 => Some(body + 8),
            // Sound: channel, sized sample, then attenuation, volume, flags, pitch.
            8 => len_at(body + 4).map(|n| body + 8 + n + 16),
            9 => len_at(body).map(|n| body + 4 + n),
            _ => None,
        };
        match next {
            Some(next) if next > at => at = next,
            _ => return 0.0,
        }
    }
    match times.as_slice() {
        [.., before, last] if last.is_finite() && before.is_finite() && last > before => {
            last - before
        }
        _ => 0.0,
    }
}

fn read_i32(file: &mut std::fs::File) -> Option<i32> {
    let mut bytes = [0u8; 4];
    file.read_exact(&mut bytes).ok()?;
    Some(i32::from_le_bytes(bytes))
}

fn sum_playback(entries: &[u8]) -> f32 {
    entries
        .as_chunks::<ENTRY_SIZE>()
        .0
        .iter()
        .filter(|entry| i32::from_le_bytes(entry[..4].try_into().unwrap_or_default()) != 0)
        .map(|entry| {
            f32::from_le_bytes(
                entry[TRACK_TIME_AT..TRACK_TIME_AT + 4]
                    .try_into()
                    .unwrap_or_default(),
            )
        })
        .filter(|t| t.is_finite() && *t > 0.0)
        .sum()
}

/// How far a load is, 0 to 99: the playback the demo player has buffered
/// (its span less the lead-in) over the playback length. Never 100 while
/// loading, so "done" is only ever the player's own word.
pub fn load_percent(buffered: f64, length: DemoLength) -> u32 {
    if length.playback <= 0.0 || !buffered.is_finite() {
        return 0;
    }
    let read = buffered - length.lead_in as f64;
    (read / length.playback as f64 * 100.0).clamp(0.0, 99.0) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(kind: i32, seconds: f32) -> Vec<u8> {
        let mut e = vec![0u8; ENTRY_SIZE];
        e[..4].copy_from_slice(&kind.to_le_bytes());
        e[TRACK_TIME_AT..TRACK_TIME_AT + 4].copy_from_slice(&seconds.to_le_bytes());
        e
    }

    #[test]
    fn playback_is_every_entry_but_the_loading_one() {
        let mut dir = entry(0, 5.0);
        dir.extend(entry(1, 1210.1));
        dir.extend(entry(1, 10.0));
        assert!((sum_playback(&dir) - 1220.1).abs() < 0.01);
    }

    #[test]
    fn a_real_demo_file_gives_its_length() {
        let dir = std::env::temp_dir().join(format!("demo_file_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("x.dem");
        let mut bytes = vec![0u8; 544];
        bytes[..8].copy_from_slice(b"HLDEMO\0\0");
        bytes[540..544].copy_from_slice(&544i32.to_le_bytes());
        bytes.extend(2i32.to_le_bytes());
        bytes.extend(entry(0, 0.0));
        bytes.extend(entry(1, 400.0));
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(
            length(&path),
            Some(DemoLength {
                playback: 400.0,
                lead_in: 0.0
            })
        );
        std::fs::write(&path, b"not a demo").unwrap();
        assert_eq!(length(&path), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn frame(kind: u8, time: f32, body: usize) -> Vec<u8> {
        let mut f = vec![kind];
        f.extend(time.to_le_bytes());
        f.extend(0i32.to_le_bytes());
        f.extend(vec![0u8; body]);
        f
    }

    #[test]
    fn the_lead_in_is_the_gap_before_the_loading_segments_end() {
        let mut segment = frame(2, 1271.9, 0);
        segment.extend(frame(3, 1272.5, 64));
        let mut net = frame(1, 1274.4, 464);
        net.extend(3i32.to_le_bytes());
        net.extend([1, 2, 3]);
        segment.extend(net);
        segment.extend(frame(5, 1624.6, 0));
        assert!((lead_in(&segment) - 350.2).abs() < 0.01);
        // A frame that can't be walked gives no lead-in rather than a guess.
        segment[0] = 42;
        assert_eq!(lead_in(&segment), 0.0);
        // An HLTV demo's LOADING frames all read 0.
        assert_eq!(lead_in(&[frame(2, 0.0, 0), frame(5, 0.0, 0)].concat()), 0.0);
    }

    #[test]
    fn a_demo_file_with_a_loading_segment_gives_its_lead_in() {
        let dir = std::env::temp_dir().join(format!("demo_file_lead_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("x.dem");
        let segment = [frame(2, 10.0, 0), frame(5, 60.0, 0)].concat();
        let mut bytes = vec![0u8; 544];
        bytes[..8].copy_from_slice(b"HLDEMO\0\0");
        let directory = 544 + segment.len() as i32;
        bytes[540..544].copy_from_slice(&directory.to_le_bytes());
        bytes.extend(&segment);
        bytes.extend(2i32.to_le_bytes());
        let mut loading = entry(0, 0.0);
        loading[OFFSET_AT..OFFSET_AT + 4].copy_from_slice(&544i32.to_le_bytes());
        loading[LENGTH_AT..LENGTH_AT + 4].copy_from_slice(&(segment.len() as i32).to_le_bytes());
        bytes.extend(loading);
        bytes.extend(entry(1, 400.0));
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(
            length(&path),
            Some(DemoLength {
                playback: 400.0,
                lead_in: 50.0
            })
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_load_stops_short_of_100() {
        let plain = DemoLength {
            playback: 400.0,
            lead_in: 0.0,
        };
        assert_eq!(load_percent(0.0, plain), 0);
        assert_eq!(load_percent(100.0, plain), 25);
        assert_eq!(load_percent(500.0, plain), 99);
        let empty = DemoLength {
            playback: 0.0,
            lead_in: 0.0,
        };
        assert_eq!(load_percent(10.0, empty), 0);
    }

    #[test]
    fn the_lead_in_is_not_counted_as_read() {
        // The 1251 s demo of #810: 350 s of lead-in, then playback.
        let length = DemoLength {
            playback: 1251.4,
            lead_in: 350.3,
        };
        assert_eq!(load_percent(350.3, length), 0);
        assert_eq!(load_percent(350.3 + 625.7, length), 50);
        assert_eq!(load_percent(1239.0, length), 71);
        assert_eq!(load_percent(1601.7, length), 99);
    }
}
