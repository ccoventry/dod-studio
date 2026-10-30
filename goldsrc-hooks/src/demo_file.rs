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
//!
//! The directory's time is only an estimate, though: the player's clock is
//! `svc_time`, and viewdemo stops loading where that clock turns back at a
//! map restart, so on such a demo the load ended at about half (#866).
//! [`loaded_span`] reads the `svc_time` span the player will hold, on a
//! background thread; the directory's figure stands until it answers.

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
    let entries = read_directory(&mut file)?;
    let playback = sum_playback(&entries);
    let lead_in = loading_segment(&mut file, &entries)
        .map(|bytes| lead_in(&bytes))
        .unwrap_or(0.0);
    Some(DemoLength { playback, lead_in })
}

/// The directory's entries, [`ENTRY_SIZE`] bytes each, in file order.
/// Leaves `reader` just after them. `None` when the file isn't a demo or its
/// directory doesn't read (a demo whose recording never finished has none).
pub(crate) fn read_directory<R: Read + Seek>(reader: &mut R) -> Option<Vec<u8>> {
    reader.seek(SeekFrom::Start(0)).ok()?;
    let mut magic = [0u8; 8];
    reader.read_exact(&mut magic).ok()?;
    if &magic[..6] != b"HLDEMO" {
        return None;
    }
    reader.seek(SeekFrom::Start(DIRECTORY_OFFSET_POS)).ok()?;
    let directory = read_i32_from(reader)?;
    reader
        .seek(SeekFrom::Start(u64::try_from(directory).ok()?))
        .ok()?;
    let count = read_i32_from(reader)?;
    if !(1..=MAX_ENTRIES).contains(&count) {
        return None;
    }
    let mut entries = vec![0u8; count as usize * ENTRY_SIZE];
    reader.read_exact(&mut entries).ok()?;
    Some(entries)
}

/// Where each of [`read_directory`]'s entries has its frames in the file:
/// (offset, length), in directory order, up to the first that can't be one.
pub(crate) fn segments(entries: &[u8]) -> impl Iterator<Item = (u64, u64)> + '_ {
    entries
        .as_chunks::<ENTRY_SIZE>()
        .0
        .iter()
        .map_while(|entry| {
            Some((
                u64::try_from(field_i32(entry, OFFSET_AT)).ok()?,
                u64::try_from(field_i32(entry, LENGTH_AT)).ok()?,
            ))
        })
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

/// How far a load is, 0 to 99. With `loaded` (the `svc_time` span the
/// player will hold, [`loaded_span`]): the player's span over it, the same
/// clock. Without it, the directory's estimate: the span less the lead-in
/// over the playback length, which runs short on a demo with a map restart
/// (#866). Never 100 while loading, so "done" is only ever the player's
/// own word.
pub fn load_percent(buffered: f64, length: DemoLength, loaded: Option<f32>) -> u32 {
    if !buffered.is_finite() {
        return 0;
    }
    if let Some(loaded) = loaded.filter(|s| s.is_finite() && *s > 0.0) {
        return (buffered / loaded as f64 * 100.0).clamp(0.0, 99.0) as u32;
    }
    if length.playback <= 0.0 {
        return 0;
    }
    let read = buffered - length.lead_in as f64;
    (read / length.playback as f64 * 100.0).clamp(0.0, 99.0) as u32
}

/// The buffered file the frame walk reads.
type Reader = std::io::BufReader<std::fs::File>;

/// `svc_time`, the server's clock, first in nearly every network frame's
/// message; the demo player's world clock is this one (#866).
const SVC_TIME: u8 = 7;

/// The `svc_time` span the demo player holds once it has read the demo: the
/// first network frame's `svc_time` to the last one before the clock jumps
/// back. A map restart inside the recording turns it back, and viewdemo
/// stops loading there (it only plays a demo's first map), so a whole-file
/// length runs long by everything after it. Reads every frame header, so
/// it's for a background thread, never a game frame; `None` when the file
/// isn't a demo or has no `svc_time`.
pub fn loaded_span(path: &Path) -> Option<f32> {
    let file = std::fs::File::open(path).ok()?;
    let mut reader = std::io::BufReader::with_capacity(1 << 20, file);
    let entries = read_directory(&mut reader)?;
    let mut clock = SvcClock::default();
    for entry in entries.as_chunks::<ENTRY_SIZE>().0 {
        let offset = u64::try_from(field_i32(entry, OFFSET_AT)).ok()?;
        let length = u64::try_from(field_i32(entry, LENGTH_AT)).ok()?;
        if walk_svc_times(&mut reader, offset, offset + length, &mut clock).is_none()
            || clock.turned_back
        {
            break;
        }
    }
    clock.span()
}

/// The `svc_time`s seen so far, up to the first that goes back.
#[derive(Default)]
struct SvcClock {
    first: Option<f32>,
    last: f32,
    turned_back: bool,
}

impl SvcClock {
    fn see(&mut self, t: f32) {
        if self.turned_back || !t.is_finite() {
            return;
        }
        match self.first {
            None => {
                self.first = Some(t);
                self.last = t;
            }
            Some(_) if t < self.last => self.turned_back = true,
            Some(_) => self.last = t,
        }
    }

    fn span(&self) -> Option<f32> {
        self.first
            .map(|first| self.last - first)
            .filter(|s| *s > 0.0)
    }
}

/// Feeds every network frame's `svc_time` between `from` and `to` to
/// `clock`, stopping once it turns back. `None` when a frame can't be read.
fn walk_svc_times(reader: &mut Reader, from: u64, to: u64, clock: &mut SvcClock) -> Option<()> {
    reader.seek(SeekFrom::Start(from)).ok()?;
    let mut at = from;
    while at + 9 <= to && !clock.turned_back {
        let mut head = [0u8; 9];
        reader.read_exact(&mut head).ok()?;
        let body: u64 = match head[0] {
            0 | 1 => {
                // 464 bytes of view and sequence info, the message's size,
                // then the message, which nearly always opens with svc_time.
                let mut info = [0u8; 468];
                reader.read_exact(&mut info).ok()?;
                let size = u32::try_from(field_i32(&info, 464)).ok()?;
                let mut opening = [0u8; 5];
                let peek = size.min(5) as usize;
                reader.read_exact(&mut opening[..peek]).ok()?;
                if peek == 5 && opening[0] == SVC_TIME {
                    clock.see(f32::from_le_bytes(
                        opening[1..5].try_into().unwrap_or_default(),
                    ));
                }
                reader.seek_relative(i64::from(size) - peek as i64).ok()?;
                468 + u64::from(size)
            }
            2 => 0,
            3 => skip(reader, 64)?,
            4 => skip(reader, 32)?,
            // NextSection ends the segment.
            5 => return Some(()),
            6 => skip(reader, 84)?,
            7 => skip(reader, 8)?,
            8 => {
                // Channel, sized sample, then attenuation, volume, flags, pitch.
                let mut lead = [0u8; 8];
                reader.read_exact(&mut lead).ok()?;
                let sample = u32::try_from(field_i32(&lead, 4)).ok()?;
                skip(reader, u64::from(sample) + 16)?;
                8 + u64::from(sample) + 16
            }
            9 => {
                let mut size = [0u8; 4];
                reader.read_exact(&mut size).ok()?;
                let n = u32::try_from(i32::from_le_bytes(size)).ok()?;
                skip(reader, u64::from(n))?;
                4 + u64::from(n)
            }
            _ => return None,
        };
        at += 9 + body;
    }
    Some(())
}

/// Skips `n` bytes; `n` itself, for the caller's running offset.
fn skip(reader: &mut Reader, n: u64) -> Option<u64> {
    // seek_relative keeps the buffer; seek() would drop and refill it for
    // every frame.
    reader.seek_relative(i64::try_from(n).ok()?).ok()?;
    Some(n)
}

fn read_i32_from<R: Read>(reader: &mut R) -> Option<i32> {
    let mut bytes = [0u8; 4];
    reader.read_exact(&mut bytes).ok()?;
    Some(i32::from_le_bytes(bytes))
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
        assert_eq!(load_percent(0.0, plain, None), 0);
        assert_eq!(load_percent(100.0, plain, None), 25);
        assert_eq!(load_percent(500.0, plain, None), 99);
        let empty = DemoLength {
            playback: 0.0,
            lead_in: 0.0,
        };
        assert_eq!(load_percent(10.0, empty, None), 0);
    }

    #[test]
    fn the_lead_in_is_not_counted_as_read() {
        // The 1251 s demo of #810: 350 s of lead-in, then playback.
        let length = DemoLength {
            playback: 1251.4,
            lead_in: 350.3,
        };
        assert_eq!(load_percent(350.3, length, None), 0);
        assert_eq!(load_percent(350.3 + 625.7, length, None), 50);
        assert_eq!(load_percent(1239.0, length, None), 71);
        assert_eq!(load_percent(1601.7, length, None), 99);
    }

    #[test]
    fn with_the_loaded_span_the_percent_is_the_players_span_over_it() {
        let length = DemoLength {
            playback: 2996.1,
            lead_in: 349.5,
        };
        // solifotw-bewton (#866): the player holds 4.30 to 1956.38.
        assert_eq!(load_percent(976.0, length, Some(1952.1)), 49);
        assert_eq!(load_percent(1952.1, length, Some(1952.1)), 99);
        // An unusable span falls back to the directory's estimate.
        assert_eq!(load_percent(1952.1, length, Some(0.0)), 53);
    }

    /// A network frame whose message opens with `svc_time` = `svc`.
    fn net(svc: f32) -> Vec<u8> {
        let mut f = frame(1, 0.0, 464);
        f.extend(5i32.to_le_bytes());
        f.push(SVC_TIME);
        f.extend(svc.to_le_bytes());
        f
    }

    #[test]
    fn the_loaded_span_ends_where_the_server_clock_turns_back() {
        let dir = std::env::temp_dir().join(format!("demo_file_span_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("x.dem");
        let loading = [net(4.3), frame(5, 0.0, 0)].concat();
        let playback = [
            net(4.5),
            frame(3, 0.0, 64),
            net(600.0),
            net(1956.4),
            // A map restart: the clock starts again, and viewdemo stops here.
            net(4.4),
            net(1395.0),
            frame(5, 0.0, 0),
        ]
        .concat();
        let mut bytes = vec![0u8; 544];
        bytes[..8].copy_from_slice(b"HLDEMO\0\0");
        let loading_at = 544i32;
        let playback_at = loading_at + loading.len() as i32;
        let directory_at = playback_at + playback.len() as i32;
        bytes[540..544].copy_from_slice(&directory_at.to_le_bytes());
        bytes.extend(&loading);
        bytes.extend(&playback);
        bytes.extend(2i32.to_le_bytes());
        for (kind, at, len) in [
            (0, loading_at, loading.len()),
            (1, playback_at, playback.len()),
        ] {
            let mut e = entry(kind, 0.0);
            e[OFFSET_AT..OFFSET_AT + 4].copy_from_slice(&at.to_le_bytes());
            e[LENGTH_AT..LENGTH_AT + 4].copy_from_slice(&(len as i32).to_le_bytes());
            bytes.extend(e);
        }
        std::fs::write(&path, &bytes).unwrap();
        let span = loaded_span(&path).unwrap();
        assert!((span - (1956.4 - 4.3)).abs() < 0.01, "{span}");
        std::fs::write(&path, b"not a demo").unwrap();
        assert_eq!(loaded_span(&path), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
