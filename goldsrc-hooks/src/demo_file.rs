//! What a `.dem` file's own directory says: how long its playback is (#465).
//!
//! The header's last field (offset 540) points at the directory: an `i32`
//! count, then 92-byte `demoentry_t`s (`nEntryType`, a 64-byte description,
//! `nFlags`, `nCDTrack`, `fTrackTime`, `nFrames`, `nOffset`, `nFileLength`).
//! Entry 0 is the "LOADING" segment (type 0, no time); the rest are playback.

// Only the 32-bit build has the window; a host check still compiles this.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

const DIRECTORY_OFFSET_POS: u64 = 540;
const ENTRY_SIZE: usize = 92;
const TRACK_TIME_AT: usize = 76;
/// More entries than this is not a directory.
const MAX_ENTRIES: i32 = 1024;

/// The demo's playback length in seconds: every non-loading entry's
/// `fTrackTime`, summed.
pub fn playback_seconds(path: &Path) -> Option<f32> {
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
    Some(sum_playback(&entries))
}

fn read_i32(file: &mut std::fs::File) -> Option<i32> {
    let mut bytes = [0u8; 4];
    file.read_exact(&mut bytes).ok()?;
    Some(i32::from_le_bytes(bytes))
}

fn sum_playback(entries: &[u8]) -> f32 {
    entries
        .chunks_exact(ENTRY_SIZE)
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

/// How far a load is, 0 to 99: the span the demo player has buffered over
/// the playback length. Never 100 while loading, so "done" is only ever the
/// player's own word.
pub fn load_percent(buffered: f64, total: f32) -> u32 {
    if total <= 0.0 || !buffered.is_finite() {
        return 0;
    }
    (buffered / total as f64 * 100.0).clamp(0.0, 99.0) as u32
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
        assert_eq!(playback_seconds(&path), Some(400.0));
        std::fs::write(&path, b"not a demo").unwrap();
        assert_eq!(playback_seconds(&path), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_load_stops_short_of_100() {
        assert_eq!(load_percent(0.0, 400.0), 0);
        assert_eq!(load_percent(100.0, 400.0), 25);
        assert_eq!(load_percent(500.0, 400.0), 99);
        assert_eq!(load_percent(10.0, 0.0), 0);
    }
}
