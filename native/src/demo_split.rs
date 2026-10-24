//! Split a demo that recorded more than one map into one demo per map (#624).
//!
//! ## Why a split is clean
//!
//! A level change inside a recording re-sends the whole signon --
//! `SvcServerInfo`, the delta descriptions, user messages, resources,
//! baselines -- and then full entity snapshots. Nothing after it refers to
//! anything before it (#217's survey). So each map is a demo of its own,
//! laid out the way the engine records a fresh one:
//!
//! - **Directory entry 0 ("LOADING")**: the map's signon, from its
//!   `SvcServerInfo` up to and including the full `SvcPacketEntities` that
//!   follow the baseline, as "start" network frames, then a `NextSection`.
//! - **Directory entry 1 ("Playback")**: a `DemoStart`, then the rest of the
//!   map's frames with their clock and frame numbers rebased to start at 0
//!   (a recording's playback section starts at 0), then a `NextSection`.
//! - **Header**: the map's own name and checksum, from its `SvcServerInfo`.
//!
//! The first map is simpler: the original demo cut off just before the
//! second signon (the end-trim of #58).
//!
//! Frames are copied, not re-encoded: the writer reuses each frame's source
//! bytes.
//!
//! ## Finding demos with more than one map, quickly
//!
//! [`quick_maps`] scans the file's bytes for `SvcServerInfo` (the message id,
//! then the network protocol, and the game folder, host name and
//! `maps/<name>.bsp` strings where they belong) without parsing the demo.

use dem::types::{
    Demo, DirectoryEntry, EngineMessage, Frame, FrameData, MessageData, NetMessage,
    NetworkMessageType,
};

/// `svc_serverinfo`.
const SVC_SERVERINFO: u8 = 11;
/// From the message id to the game folder string: protocol, spawn count,
/// map checksum (4 each), client.dll hash (16), max players, player index,
/// deathmatch (1 each).
const TO_GAME_DIR: usize = 1 + 4 + 4 + 4 + 16 + 3;

/// One map in a demo.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MapSegment {
    /// 0 for the first map.
    pub index: usize,
    /// `dod_anzio`.
    pub map: String,
    pub map_checksum: u32,
    /// When it starts and ends on the demo's playback clock (the first map's
    /// signon is before 0), in seconds.
    pub start_seconds: f32,
    pub end_seconds: f32,
    pub frames: usize,
}

impl MapSegment {
    pub fn seconds(&self) -> f32 {
        (self.end_seconds - self.start_seconds).max(0.0)
    }
}

/// `maps/dod_anzio.bsp` -> `dod_anzio`.
pub fn map_stem(file: &str) -> String {
    let base = file.rsplit(['/', '\\']).next().unwrap_or(file);
    base.trim_end_matches('\0')
        .strip_suffix(".bsp")
        .unwrap_or(base)
        .to_string()
}

fn c_string(bytes: &[u8], at: usize, max: usize) -> Option<(&[u8], usize)> {
    let rest = bytes.get(at..)?;
    let end = rest.iter().take(max + 1).position(|&b| b == 0)?;
    Some((&rest[..end], at + end + 1))
}

/// The maps a demo's bytes signon to, in order, without parsing it.
///
/// Each `SvcServerInfo` is recognised by its id and the network protocol,
/// then confirmed by a printable game folder, a host name and a
/// `maps/*.bsp` file name where they belong; a stray match fails those.
pub fn quick_maps(bytes: &[u8], network_protocol: i32) -> Vec<String> {
    let needle = {
        let mut n = vec![SVC_SERVERINFO];
        n.extend_from_slice(&network_protocol.to_le_bytes());
        n
    };
    let printable = |s: &[u8]| s.iter().all(|&b| (0x20..0x7f).contains(&b));
    let mut maps = Vec::new();
    let mut at = 0;
    while let Some(found) = memchr_seq(&bytes[at..], &needle) {
        let start = at + found;
        at = start + 1;
        let Some((game_dir, next)) = c_string(bytes, start + TO_GAME_DIR, 32) else {
            continue;
        };
        if game_dir.is_empty() || !printable(game_dir) {
            continue;
        }
        let Some((_host, next)) = c_string(bytes, next, 256) else {
            continue;
        };
        let Some((map_file, _)) = c_string(bytes, next, 128) else {
            continue;
        };
        let map_file = String::from_utf8_lossy(map_file);
        if map_file.starts_with("maps/") && map_file.ends_with(".bsp") {
            maps.push(map_stem(&map_file));
        }
    }
    maps
}

fn memchr_seq(hay: &[u8], needle: &[u8]) -> Option<usize> {
    let first = needle[0];
    let mut from = 0;
    while from + needle.len() <= hay.len() {
        let i = hay[from..].iter().position(|&b| b == first)? + from;
        if hay.get(i..i + needle.len()) == Some(needle) {
            return Some(i);
        }
        from = i + 1;
    }
    None
}

/// The demo's network protocol, from its header (48 for DoD 1.3), without
/// parsing the rest.
pub fn header_protocol(bytes: &[u8]) -> Option<i32> {
    if bytes.get(..8)? != b"HLDEMO\0\0" {
        return None;
    }
    Some(i32::from_le_bytes(bytes.get(12..16)?.try_into().ok()?))
}

fn server_info(frame: &Frame) -> Option<&dem::types::SvcServerInfo> {
    let FrameData::NetworkMessage(b) = &frame.frame_data else {
        return None;
    };
    let MessageData::Parsed(msgs) = &b.1.messages else {
        return None;
    };
    msgs.iter().find_map(|m| match m {
        NetMessage::EngineMessage(e) => match &**e {
            EngineMessage::SvcServerInfo(info) => Some(info),
            _ => None,
        },
        _ => None,
    })
}

fn has_full_snapshot(frame: &Frame) -> bool {
    let FrameData::NetworkMessage(b) = &frame.frame_data else {
        return false;
    };
    let MessageData::Parsed(msgs) = &b.1.messages else {
        return false;
    };
    msgs.iter().any(|m| {
        matches!(m, NetMessage::EngineMessage(e) if matches!(**e, EngineMessage::SvcPacketEntities(_)))
    })
}

/// Where each map starts in a parsed demo: (entry, frame) of its
/// `SvcServerInfo`. The first map starts at entry 0, frame 0.
fn starts(demo: &Demo) -> Vec<(usize, usize)> {
    let mut out = vec![(0, 0)];
    for (ei, e) in demo.directory.entries.iter().enumerate().skip(1) {
        for (fi, f) in e.frames.iter().enumerate() {
            if server_info(f).is_some() {
                out.push((ei, fi));
            }
        }
    }
    out
}

/// The maps of a parsed demo.
pub fn segments(demo: &Demo) -> Vec<MapSegment> {
    let starts = starts(demo);
    let header_map = String::from_utf8_lossy(&demo.header.map_name.0)
        .trim_end_matches('\0')
        .to_string();
    let last = demo.directory.entries.len().saturating_sub(1);
    let end_of = |i: usize| -> (usize, usize) {
        starts.get(i + 1).copied().unwrap_or((
            last,
            demo.directory
                .entries
                .get(last)
                .map_or(0, |e| e.frames.len()),
        ))
    };
    let frame_time = |(ei, fi): (usize, usize)| {
        demo.directory
            .entries
            .get(ei)
            .and_then(|e| e.frames.get(fi).or_else(|| e.frames.last()).map(|f| f.time))
    };
    starts
        .iter()
        .enumerate()
        .map(|(i, &(ei, fi))| {
            let (map, crc) = if i == 0 {
                (header_map.clone(), demo.header.map_checksum)
            } else {
                let info = server_info(&demo.directory.entries[ei].frames[fi]).expect("a start");
                (
                    map_stem(&String::from_utf8_lossy(&info.map_file_name)),
                    info.map_checksum as u32,
                )
            };
            let end = end_of(i);
            let frames = if i == 0 {
                demo.directory.entries[0].frames.len() + if end.0 >= 1 { end.1 } else { 0 }
            } else {
                end.1.saturating_sub(fi)
            };
            MapSegment {
                index: i,
                map,
                map_checksum: crc,
                start_seconds: if i == 0 {
                    0.0
                } else {
                    frame_time((ei, fi)).unwrap_or(0.0)
                },
                end_seconds: frame_time((end.0, end.1.saturating_sub(1))).unwrap_or(0.0),
                frames,
            }
        })
        .collect()
}

fn next_section(time: f32, frame: i32) -> Frame {
    Frame {
        time,
        frame,
        frame_data: FrameData::NextSection,
    }
}

/// `template` (entry 0 or 1 of the source) holding `frames`. The playback
/// entry's length is its last frame's time; the loading entry keeps the
/// source's (0 in a recording: its frames are on the server's clock).
fn entry_like(template: &DirectoryEntry, frames: Vec<Frame>, playback: bool) -> DirectoryEntry {
    let mut e = template.clone();
    e.frame_count = frames.len() as i32;
    if playback {
        e.track_time = frames.last().map_or(0.0, |f| f.time);
    }
    e.frames = frames;
    e
}

/// The demo holding only map `index` of `demo`, or `None` when there is no
/// such map.
///
/// The header's map checksum is the BSP's (`bsp::map_checksum`), a different
/// number from the one in `SvcServerInfo`. A later map that is the header's
/// own map keeps the header's; another map takes what `bsp_checksum` finds
/// for it, or 0, which is what an HLTV demo's header carries and plays with.
pub fn extract(
    demo: &Demo,
    index: usize,
    bsp_checksum: &dyn Fn(&str) -> Option<u32>,
) -> Option<Demo> {
    let starts = starts(demo);
    let &(ei, fi) = starts.get(index)?;
    let playback = demo.directory.entries.get(1)?;
    let mut out = demo.clone();
    if index == 0 {
        // The original, cut just before the second signon.
        if let Some(&(next_e, next_f)) = starts.get(1) {
            out.directory.entries.truncate(next_e + 1);
            let e = &mut out.directory.entries[next_e];
            e.frames.truncate(next_f);
            let last = e.frames.last().map_or((0.0, 0), |f| (f.time, f.frame));
            e.frames.push(next_section(last.0, last.1));
            e.frame_count = e.frames.len() as i32;
            e.track_time = last.0;
        }
        return Some(out);
    }

    let source = &demo.directory.entries[ei].frames;
    let end = starts
        .get(index + 1)
        .filter(|(e, _)| *e == ei)
        .map_or(source.len(), |(_, f)| *f);
    let map_frames = &source[fi..end];
    // The signon runs to the last full snapshot of the run that follows the
    // baseline.
    let first_full = map_frames.iter().position(has_full_snapshot)?;
    let mut signon_end = first_full;
    while map_frames
        .get(signon_end + 1)
        .is_some_and(has_full_snapshot)
    {
        signon_end += 1;
    }

    let mut loading: Vec<Frame> = map_frames[..=signon_end]
        .iter()
        .cloned()
        .map(|mut f| {
            if let FrameData::NetworkMessage(b) = &mut f.frame_data {
                b.0 = NetworkMessageType::Start;
            }
            f
        })
        .collect();
    let last = loading.last().map_or((0.0, 0), |f| (f.time, f.frame));
    loading.push(next_section(last.0, last.1));

    let rest: Vec<&Frame> = map_frames[signon_end + 1..]
        .iter()
        .filter(|f| !matches!(f.frame_data, FrameData::NextSection))
        .collect();
    let (t0, n0) = rest.first().map_or((0.0, 0), |f| (f.time, f.frame));
    let demo_start = playback
        .frames
        .iter()
        .find(|f| matches!(f.frame_data, FrameData::DemoStart))
        .cloned()
        .map(|mut f| {
            f.time = 0.0;
            f.frame = 0;
            f
        })
        .unwrap_or(Frame {
            time: 0.0,
            frame: 0,
            frame_data: FrameData::DemoStart,
        });
    let mut frames = vec![demo_start];
    frames.extend(rest.into_iter().cloned().map(|mut f| {
        f.time = (f.time - t0).max(0.0);
        f.frame = (f.frame - n0).max(0);
        f
    }));
    let last = frames.last().map_or((0.0, 0), |f| (f.time, f.frame));
    frames.push(next_section(last.0, last.1));

    let info = server_info(&map_frames[0])?;
    let map = map_stem(&String::from_utf8_lossy(&info.map_file_name));
    let mut name = map.into_bytes();
    name.resize(demo.header.map_name.0.len().max(260), 0);
    out.header.map_name.0 = name;
    let header_map = String::from_utf8_lossy(&demo.header.map_name.0)
        .trim_end_matches('\0')
        .to_string();
    out.header.map_checksum =
        if map_stem(&String::from_utf8_lossy(&info.map_file_name)) == header_map {
            demo.header.map_checksum
        } else {
            bsp_checksum(&map_stem(&String::from_utf8_lossy(&info.map_file_name))).unwrap_or(0)
        };
    out.directory.entries = vec![
        entry_like(&demo.directory.entries[0], loading, false),
        entry_like(playback, frames, true),
    ];
    Some(out)
}

/// The file name for map `segment` of `demo_stem`:
/// `<demo>_split<N>_<map>`, `N` the map's place in the original (from 1)
/// so the pieces sort in playing order, and the map without its `dod_`.
pub fn split_name(demo_stem: &str, segments: &[MapSegment], segment: &MapSegment) -> String {
    let maps: Vec<&str> = segments.iter().map(|s| s.map.as_str()).collect();
    split_name_at(demo_stem, &maps, segment.index)
}

/// [`split_name`] from the demo's map names alone (`maps[index]` is the
/// piece's map), for knowing a piece's file before the demo is parsed.
pub fn split_name_at(demo_stem: &str, maps: &[&str], index: usize) -> String {
    let map = maps.get(index).copied().unwrap_or_default();
    let short = map.strip_prefix("dod_").unwrap_or(map);
    format!("{demo_stem}_split{}_{short}", index + 1)
}

/// The name splits used before `_split<N>` (#844): `<demo>_<map>`, with the
/// map's place added when the same map appears twice. Only for finding
/// pieces a split already wrote.
pub fn old_split_name_at(demo_stem: &str, maps: &[&str], index: usize) -> String {
    let map = maps.get(index).copied().unwrap_or_default();
    let repeats = maps.iter().filter(|m| **m == map).count() > 1;
    if repeats {
        format!("{demo_stem}_{map}_{}", index + 1)
    } else {
        format!("{demo_stem}_{map}")
    }
}

/// The maps of the demo at `path`, without a full parse when it can: the
/// frame headers and the signons are enough for [`segments`]' answer, in
/// well under a second where a full parse takes 5-10 s (#844). Falls back to
/// the full parse when the walk can't read a frame.
pub fn quick_demo_segments(path: &std::path::Path) -> Result<Vec<MapSegment>, String> {
    let bytes =
        std::fs::read(path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
    match quick_segments(&bytes) {
        Some(segs) => Ok(segs),
        None => {
            let demo = dem::open_demo_from_bytes(&bytes)
                .map_err(|e| format!("could not read the demo: {e}"))?;
            Ok(segments(&demo))
        }
    }
}

/// One frame as [`quick_segments`] sees it: its time, and the map it signs
/// on to when it carries a `SvcServerInfo`.
struct QuickFrame {
    time: f32,
    signon: Option<(String, u32)>,
}

/// [`segments`] from the file's bytes: each directory entry's frames walked
/// by their headers to the `NextSection`, a network frame's message checked
/// for a `SvcServerInfo` as [`quick_maps`] recognises one. `None` when a
/// frame can't be walked.
pub fn quick_segments(bytes: &[u8]) -> Option<Vec<MapSegment>> {
    let i32_at = |at: usize| -> Option<i32> {
        Some(i32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
    };
    let protocol = header_protocol(bytes)?;
    // As `segments` reads the parsed header's map_name.
    let header_map = String::from_utf8_lossy(bytes.get(16..276)?)
        .trim_end_matches('\0')
        .to_string();
    let header_crc = i32_at(536)? as u32;
    let directory = usize::try_from(i32_at(540)?).ok()?;
    let count = usize::try_from(i32_at(directory)?).ok()?;
    if count == 0 || count > 1024 {
        return None;
    }
    let mut needle = vec![SVC_SERVERINFO];
    needle.extend_from_slice(&protocol.to_le_bytes());
    let mut entries: Vec<Vec<QuickFrame>> = Vec::with_capacity(count);
    for e in 0..count {
        let entry = directory + 4 + e * 92;
        let mut at = usize::try_from(i32_at(entry + 84)?).ok()?;
        let mut frames = Vec::new();
        loop {
            let kind = *bytes.get(at)?;
            let time = f32::from_le_bytes(bytes.get(at + 1..at + 5)?.try_into().ok()?);
            let body = at + 9;
            let (next, signon) = match kind {
                0 | 1 => {
                    let size = usize::try_from(i32_at(body + 464)?).ok()?;
                    let msg = bytes.get(body + 468..body + 468 + size)?;
                    (body + 468 + size, signon_in(msg, &needle))
                }
                2 => (body, None),
                3 => (body + 64, None),
                4 => (body + 32, None),
                5 => (body, None),
                6 => (body + 84, None),
                7 => (body + 8, None),
                8 => {
                    let sample = usize::try_from(i32_at(body + 4)?).ok()?;
                    (body + 8 + sample + 16, None)
                }
                9 => (body + 4 + usize::try_from(i32_at(body)?).ok()?, None),
                _ => return None,
            };
            frames.push(QuickFrame { time, signon });
            at = next;
            if kind == 5 {
                break;
            }
        }
        entries.push(frames);
    }
    Some(segments_from_frames(&entries, header_map, header_crc))
}

/// The map a network message signs on to, when it carries a `SvcServerInfo`
/// that passes [`quick_maps`]' checks: (map, the checksum it carries).
fn signon_in(msg: &[u8], needle: &[u8]) -> Option<(String, u32)> {
    let printable = |s: &[u8]| s.iter().all(|&b| (0x20..0x7f).contains(&b));
    let mut at = 0;
    while let Some(found) = memchr_seq(msg.get(at..)?, needle) {
        let start = at + found;
        at = start + 1;
        let Some((game_dir, next)) = c_string(msg, start + TO_GAME_DIR, 32) else {
            continue;
        };
        if game_dir.is_empty() || !printable(game_dir) {
            continue;
        }
        let Some((_host, next)) = c_string(msg, next, 256) else {
            continue;
        };
        let Some((map_file, _)) = c_string(msg, next, 128) else {
            continue;
        };
        let map_file = String::from_utf8_lossy(map_file);
        if map_file.starts_with("maps/") && map_file.ends_with(".bsp") {
            let crc = u32::from_le_bytes(msg.get(start + 9..start + 13)?.try_into().ok()?);
            return Some((map_stem(&map_file), crc));
        }
    }
    None
}

/// [`segments`]' rules over walked frames: the first map starts at entry 0,
/// frame 0 and is the header's; a later one at each signon past entry 0.
fn segments_from_frames(
    entries: &[Vec<QuickFrame>],
    header_map: String,
    header_crc: u32,
) -> Vec<MapSegment> {
    let mut starts = vec![(0usize, 0usize)];
    for (ei, frames) in entries.iter().enumerate().skip(1) {
        for (fi, f) in frames.iter().enumerate() {
            if f.signon.is_some() {
                starts.push((ei, fi));
            }
        }
    }
    let last = entries.len().saturating_sub(1);
    let end_of = |i: usize| -> (usize, usize) {
        starts
            .get(i + 1)
            .copied()
            .unwrap_or((last, entries.get(last).map_or(0, Vec::len)))
    };
    let frame_time = |(ei, fi): (usize, usize)| {
        entries
            .get(ei)
            .and_then(|e| e.get(fi).or_else(|| e.last()).map(|f| f.time))
    };
    starts
        .iter()
        .enumerate()
        .map(|(i, &(ei, fi))| {
            let (map, crc) = if i == 0 {
                (header_map.clone(), header_crc)
            } else {
                entries[ei][fi].signon.clone().unwrap_or_default()
            };
            let end = end_of(i);
            let frames = if i == 0 {
                entries.first().map_or(0, Vec::len) + if end.0 >= 1 { end.1 } else { 0 }
            } else {
                end.1.saturating_sub(fi)
            };
            MapSegment {
                index: i,
                map,
                map_checksum: crc,
                start_seconds: if i == 0 {
                    0.0
                } else {
                    frame_time((ei, fi)).unwrap_or(0.0)
                },
                end_seconds: frame_time((end.0, end.1.saturating_sub(1))).unwrap_or(0.0),
                frames,
            }
        })
        .collect()
}

/// The maps of the demo at `path`, from a full parse.
pub fn demo_segments(path: &std::path::Path) -> Result<Vec<MapSegment>, String> {
    let bytes =
        std::fs::read(path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
    let demo =
        dem::open_demo_from_bytes(&bytes).map_err(|e| format!("could not read the demo: {e}"))?;
    Ok(segments(&demo))
}

/// One demo a split wrote.
#[derive(Clone, Debug, serde::Serialize)]
pub struct Written {
    /// Which map of the original this is ([`MapSegment::index`], 0 for the
    /// first). The original's own analysis covers map 0 only
    /// (`analysis::use_segment_boundary`), so the queue carries a highlight's
    /// work over to that piece alone (#217).
    pub index: usize,
    pub path: String,
    pub map: String,
    pub seconds: f32,
    pub size_bytes: u64,
}

/// Writes maps `keep` of the demo at `path` as demos of their own, next to
/// it, and checks each one reads back as a demo with exactly that one map.
/// The original is never changed. A map's BSP checksum, for a header that
/// needs one, is looked up in the demo's own `maps` folder and then in
/// `map_dirs`.
pub fn split_file(
    path: &std::path::Path,
    keep: &[usize],
    map_dirs: &[std::path::PathBuf],
) -> Result<Vec<Written>, String> {
    split_file_with_progress(path, Keep::These(keep), map_dirs, &mut |_| {})
}

/// Which maps a split writes.
pub enum Keep<'a> {
    /// These, by [`MapSegment::index`].
    These(&'a [usize]),
    /// Every map at least this many seconds long ([`keep_at_least`]).
    AtLeast(f32),
}

/// Every map at least `min_seconds` long. A shorter one is almost always the
/// next map loading as the recording stopped, so Split Maps leaves it
/// unticked too. When that leaves nothing, every map.
pub fn keep_at_least(segs: &[MapSegment], min_seconds: f32) -> Vec<usize> {
    let long: Vec<usize> = segs
        .iter()
        .filter(|s| s.seconds() >= min_seconds)
        .map(|s| s.index)
        .collect();
    if long.is_empty() {
        segs.iter().map(|s| s.index).collect()
    } else {
        long
    }
}

/// How far a split has got, for a progress bar and each map's row (#876).
#[derive(Clone, Debug, serde::Serialize)]
pub struct SplitProgress {
    /// 0.0 to 1.0 over the whole split.
    pub fraction: f32,
    /// "reading" the demo; then, for one map, "waiting" its turn, "writing"
    /// it, "checking" it read back, and "finished", "failed" or "cancelled".
    pub stage: &'static str,
    /// The map this is about; empty while reading.
    pub map: String,
    /// Which of `parts` (1-based); 0 while reading.
    pub part: usize,
    pub parts: usize,
    /// The map's [`MapSegment::index`]; `None` while reading.
    pub index: Option<usize>,
    /// 0.0 to 1.0 through this map's own write and check (the write is the
    /// first half); through the read itself while reading.
    pub map_fraction: f32,
    /// How many maps have finished, failed or been cancelled.
    pub done: usize,
    /// What a finished map wrote.
    pub file: Option<Written>,
    /// Why a failed map failed.
    pub error: Option<String>,
}

/// A map a split couldn't write.
#[derive(Clone, Debug, serde::Serialize)]
pub struct MapFailure {
    pub index: usize,
    pub map: String,
    pub error: String,
}

/// What a split did with each map it was asked for (#876): one map failing
/// no longer stops the others.
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct SplitReport {
    /// In the order the maps were asked for.
    pub written: Vec<Written>,
    pub failed: Vec<MapFailure>,
    /// Cancel stopped at least one map.
    pub cancelled: bool,
}

impl SplitReport {
    /// The written maps and whether Cancel stopped it, or an error naming
    /// every map that failed, for callers that treat any failure as the
    /// split's. The other maps still run, so the error also names the files
    /// they wrote: the caller drops them (the queue keeps the demo), and they
    /// would otherwise sit next to it unmentioned.
    pub fn into_written(self) -> Result<(Vec<Written>, bool), String> {
        if self.failed.is_empty() {
            return Ok((self.written, self.cancelled));
        }
        let mut error = self
            .failed
            .iter()
            .map(|f| f.error.as_str())
            .collect::<Vec<_>>()
            .join("; ");
        if !self.written.is_empty() {
            let names: Vec<String> = self
                .written
                .iter()
                .map(|w| {
                    let path = std::path::Path::new(&w.path);
                    path.file_name()
                        .map_or_else(|| w.path.clone(), |n| n.to_string_lossy().to_string())
                })
                .collect();
            error.push_str(&format!(" (still written: {})", names.join(", ")));
        }
        Err(error)
    }
}

/// At most this many maps are checked at once. Each check is a full parse of
/// the written map, a few hundred MB for a long one, on top of the source
/// demo's own parse.
const MAX_CHECKS_AT_ONCE: usize = 4;

/// Runs `work` on each job of `jobs`, up to `max` at once, each on a thread
/// of its own. The next job is pulled, on this thread, only once one of the
/// `max` is free, so `jobs` can do work of its own (here: write the map out
/// of a demo that can't leave this thread) without running ahead. Results
/// come back in the jobs' order.
fn run_bounded<J: Send, R: Send>(
    max: usize,
    jobs: impl Iterator<Item = J>,
    work: &(dyn Fn(J) -> R + Sync),
) -> Vec<R> {
    let max = max.max(1);
    let mut results = Vec::new();
    std::thread::scope(|s| {
        let mut running = std::collections::VecDeque::new();
        let join = |h: std::thread::ScopedJoinHandle<'_, R>| match h.join() {
            Ok(r) => r,
            Err(panic) => std::panic::resume_unwind(panic),
        };
        for job in jobs {
            running.push_back(s.spawn(move || work(job)));
            while running.len() >= max {
                results.push(join(running.pop_front().expect("one running")));
            }
        }
        while let Some(h) = running.pop_front() {
            results.push(join(h));
        }
    });
    results
}

/// Makes `dir/name.dem`, or `name (2).dem` and so on when that exists, and
/// writes `bytes` to it. Creating the file is what claims the name, so two
/// maps saved at once can never pick the same one.
fn save_new(dir: &std::path::Path, name: &str, bytes: &[u8]) -> Result<std::path::PathBuf, String> {
    use std::io::Write as _;
    for n in 1.. {
        let path = if n == 1 {
            dir.join(format!("{name}.dem"))
        } else {
            dir.join(format!("{name} ({n}).dem"))
        };
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => {
                return match file.write_all(bytes) {
                    Ok(()) => Ok(path),
                    Err(e) => {
                        drop(file);
                        let _ = std::fs::remove_file(&path);
                        Err(format!("could not write {}: {e}", path.display()))
                    }
                };
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("could not write {}: {e}", path.display())),
        }
    }
    unreachable!("an unused name")
}

/// What happened to one map.
enum MapOutcome {
    Written(Written),
    Failed(String),
    Cancelled,
}

/// Tells `progress` how each map is doing, from whichever thread is on it.
struct Reporter<'a> {
    progress: &'a mut (dyn FnMut(SplitProgress) + Send),
    /// Each map's own 0.0-1.0, by its place in the maps asked for.
    fractions: Vec<f32>,
    done: usize,
}

impl Reporter<'_> {
    /// Map `index` (`map`), the `k`th asked for, is at `stage`.
    fn map(
        &mut self,
        k: usize,
        index: usize,
        map: &str,
        stage: &'static str,
        map_fraction: f32,
        outcome: Option<&MapOutcome>,
    ) {
        if let Some(f) = self.fractions.get_mut(k) {
            *f = map_fraction;
        }
        if outcome.is_some() {
            self.done += 1;
        }
        let parts = self.fractions.len().max(1);
        let sum: f32 = self.fractions.iter().sum();
        (self.progress)(SplitProgress {
            fraction: READ_SHARE + (1.0 - READ_SHARE) * sum / parts as f32,
            stage,
            map: map.to_string(),
            part: k + 1,
            parts,
            index: Some(index),
            map_fraction,
            done: self.done,
            file: match outcome {
                Some(MapOutcome::Written(w)) => Some(w.clone()),
                _ => None,
            },
            error: match outcome {
                Some(MapOutcome::Failed(e)) => Some(e.clone()),
                _ => None,
            },
        });
    }
}

/// Reading the demo is this much of a split; the rest is shared between the
/// maps written, half writing and half reading each one back.
const READ_SHARE: f32 = 0.4;

/// [`split_file`], parsing the demo once even when the maps to keep depend on
/// their lengths, and calling `progress` as it goes: about every 1% of a
/// parse, and at each map.
pub fn split_file_with_progress(
    path: &std::path::Path,
    keep: Keep,
    map_dirs: &[std::path::PathBuf],
    progress: &mut (dyn FnMut(SplitProgress) + Send),
) -> Result<Vec<Written>, String> {
    split_file_cancellable(path, keep, map_dirs, progress, &|| false).map(|(written, _)| written)
}

/// [`split_file_report`] for callers that treat any map failing as the
/// split failing. Returns what was written and whether Cancel stopped it.
pub fn split_file_cancellable(
    path: &std::path::Path,
    keep: Keep,
    map_dirs: &[std::path::PathBuf],
    progress: &mut (dyn FnMut(SplitProgress) + Send),
    cancel: &(dyn Fn() -> bool + Sync),
) -> Result<(Vec<Written>, bool), String> {
    split_file_report(path, keep, map_dirs, progress, cancel)?.into_written()
}

/// Writes maps `keep` of the demo at `path` as demos of their own, reading
/// the demo once (#876). The maps are cut out and written one at a time on
/// this thread (the parsed demo stays on it), and each is checked, read
/// back as a demo with exactly that one map, and saved on a thread of its
/// own while the next is written: up to [`MAX_CHECKS_AT_ONCE`] at once.
///
/// One map failing doesn't stop the others. `cancel` stops it after the
/// demo is read, before a map starts, while one is written, and before one
/// is saved; a map already saved stays. Every map asked for ends with a
/// "finished", "failed" or "cancelled" `progress` call. `Err` only when the
/// demo can't be read at all.
pub fn split_file_report(
    path: &std::path::Path,
    keep: Keep,
    map_dirs: &[std::path::PathBuf],
    progress: &mut (dyn FnMut(SplitProgress) + Send),
    cancel: &(dyn Fn() -> bool + Sync),
) -> Result<SplitReport, String> {
    let bytes =
        std::fs::read(path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
    let demo = dem::open_demo_from_bytes_with_progress(&bytes, &mut |read, total| {
        progress(SplitProgress {
            fraction: READ_SHARE * read as f32 / total.max(1) as f32,
            stage: "reading",
            map: String::new(),
            part: 0,
            parts: 0,
            index: None,
            // How far through the read itself.
            map_fraction: read as f32 / total.max(1) as f32,
            done: 0,
            file: None,
            error: None,
        })
    })
    .map_err(|e| format!("could not read the demo: {e}"))?;
    let segs = segments(&demo);
    let mut keep: Vec<usize> = match keep {
        Keep::These(k) => k.to_vec(),
        Keep::AtLeast(min) => keep_at_least(&segs, min),
    };
    // A map asked for twice is written once.
    let mut seen = std::collections::HashSet::new();
    keep.retain(|i| seen.insert(*i));
    let dir = path.parent().ok_or("the demo has no folder")?;
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut dirs = vec![dir.join("maps")];
    dirs.extend(map_dirs.iter().cloned());
    let find_bsp = |map: &str| {
        dirs.iter().find_map(|d| {
            let bytes = std::fs::read(d.join(format!("{map}.bsp"))).ok()?;
            crate::patch::bsp::map_checksum(&bytes).ok()
        })
    };
    let map_name = |i: usize| segs.get(i).map_or_else(String::new, |s| s.map.clone());

    let reporter = std::sync::Mutex::new(Reporter {
        progress,
        fractions: vec![0.0; keep.len()],
        done: 0,
    });
    let report = |k: usize, i: usize, stage, fraction, outcome: Option<&MapOutcome>| {
        reporter.lock().unwrap_or_else(|p| p.into_inner()).map(
            k,
            i,
            &map_name(i),
            stage,
            fraction,
            outcome,
        );
    };
    for (k, &i) in keep.iter().enumerate() {
        report(k, i, "waiting", 0.0, None);
    }

    /// A map on its way: settled on this thread, or written and to be checked.
    enum Job {
        Settled(usize, usize, MapOutcome),
        Check(usize, usize, Vec<u8>),
    }
    let jobs = keep.iter().enumerate().map(|(k, &i)| {
        if cancel() {
            return Job::Settled(k, i, MapOutcome::Cancelled);
        }
        let Some(seg) = segs.get(i) else {
            return Job::Settled(
                k,
                i,
                MapOutcome::Failed(format!("the demo has no map {}", i + 1)),
            );
        };
        report(k, i, "writing", 0.0, None);
        let Some(part) = extract(&demo, i, &find_bsp) else {
            return Job::Settled(
                k,
                i,
                MapOutcome::Failed(format!("map {} ({}) could not be cut out", i + 1, seg.map)),
            );
        };
        match part.write_to_bytes_reusing_source_cancellable(&bytes, cancel) {
            Some(out) => Job::Check(k, i, out),
            None => Job::Settled(k, i, MapOutcome::Cancelled),
        }
    });
    let check = |k: usize, i: usize, out: Vec<u8>| -> MapOutcome {
        let map = map_name(i);
        report(k, i, "checking", 0.5, None);
        let back = match dem::open_demo_from_bytes_with_progress(&out, &mut |read, total| {
            report(
                k,
                i,
                "checking",
                0.5 + 0.5 * read as f32 / total.max(1) as f32,
                None,
            )
        }) {
            Ok(back) => back,
            Err(e) => {
                return MapOutcome::Failed(format!("map {} ({map}) did not read back: {e}", i + 1));
            }
        };
        let back_segs = segments(&back);
        if back_segs.len() != 1 || back_segs[0].map != map {
            return MapOutcome::Failed(format!(
                "map {} ({map}) read back as {} map(s)",
                i + 1,
                back_segs.len()
            ));
        }
        if cancel() {
            return MapOutcome::Cancelled;
        }
        let name = split_name_at(
            &stem,
            &segs.iter().map(|s| s.map.as_str()).collect::<Vec<_>>(),
            i,
        );
        match save_new(dir, &name, &out) {
            Ok(target) => MapOutcome::Written(Written {
                index: i,
                path: target.to_string_lossy().to_string(),
                map,
                seconds: back_segs[0].seconds(),
                size_bytes: out.len() as u64,
            }),
            Err(e) => MapOutcome::Failed(e),
        }
    };
    let work = |job: Job| -> (usize, MapOutcome) {
        let (k, i, outcome) = match job {
            Job::Settled(k, i, outcome) => (k, i, outcome),
            Job::Check(k, i, out) => (k, i, check(k, i, out)),
        };
        let stage = match &outcome {
            MapOutcome::Written(_) => "finished",
            MapOutcome::Failed(_) => "failed",
            MapOutcome::Cancelled => "cancelled",
        };
        report(k, i, stage, 1.0, Some(&outcome));
        (i, outcome)
    };
    let workers = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(MAX_CHECKS_AT_ONCE);

    let mut out = SplitReport::default();
    for (i, outcome) in run_bounded(workers, jobs, &work) {
        match outcome {
            MapOutcome::Written(w) => out.written.push(w),
            MapOutcome::Failed(error) => out.failed.push(MapFailure {
                index: i,
                map: map_name(i),
                error,
            }),
            MapOutcome::Cancelled => out.cancelled = true,
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ci_fixture() -> Vec<u8> {
        std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../test-fixtures/ci_fixture.dem"
        ))
        .expect("test-fixtures/ci_fixture.dem")
    }

    #[test]
    fn the_quick_walk_gives_what_the_full_parse_gives() {
        let bytes = ci_fixture();
        let full = segments(&dem::open_demo_from_bytes(&bytes).unwrap());
        assert_eq!(quick_segments(&bytes), Some(full));
        // A file it can't walk gives no answer rather than a wrong one.
        assert_eq!(quick_segments(&bytes[..bytes.len() / 2]), None);
    }

    /// Every multi-map demo in `DOD_SPLIT_DEMOS` (a folder), or the PRE
    /// install's `dod` folder: the quick walk must match the full parse.
    #[test]
    #[ignore = "needs real multi-map demos (DOD_SPLIT_DEMOS or the PRE install)"]
    fn the_quick_walk_matches_the_full_parse_on_real_multi_map_demos() {
        let folder = std::env::var_os("DOD_SPLIT_DEMOS").map(std::path::PathBuf::from).unwrap_or_else(|| {
            std::path::PathBuf::from(
                r"C:\Program Files (x86)\Steam\steamapps\common\Half-Life - PRE-Anniversary for Movies\dod",
            )
        });
        let mut checked = 0;
        for entry in
            std::fs::read_dir(&folder).unwrap_or_else(|e| panic!("{}: {e}", folder.display()))
        {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|e| e != "dem") {
                continue;
            }
            let bytes = std::fs::read(&path).unwrap();
            if quick_maps(&bytes, header_protocol(&bytes).unwrap_or(48)).len() < 2 {
                continue;
            }
            let Ok(demo) = dem::open_demo_from_bytes(&bytes) else {
                continue;
            };
            assert_eq!(
                quick_segments(&bytes),
                Some(segments(&demo)),
                "{}",
                path.display()
            );
            checked += 1;
        }
        assert!(checked > 0, "no multi-map demo in {}", folder.display());
        eprintln!("{checked} multi-map demos match");
    }

    fn server_info_bytes(map: &str) -> Vec<u8> {
        let mut b = vec![SVC_SERVERINFO];
        b.extend_from_slice(&48i32.to_le_bytes());
        b.extend_from_slice(&3i32.to_le_bytes());
        b.extend_from_slice(&0x1234_5678u32.to_le_bytes());
        b.extend_from_slice(&[0xaa; 16]);
        b.extend_from_slice(&[32, 4, 1]);
        b.extend_from_slice(b"dod\0My Server\0");
        b.extend_from_slice(format!("maps/{map}.bsp\0").as_bytes());
        b
    }

    #[test]
    fn finds_each_signon_in_raw_bytes() {
        let mut bytes = vec![0u8; 100];
        bytes.extend(server_info_bytes("dod_anzio"));
        bytes.extend([11, 48, 0, 0, 0, 1, 2, 3]); // a stray match
        bytes.extend(vec![7u8; 500]);
        bytes.extend(server_info_bytes("dod_flash"));
        assert_eq!(quick_maps(&bytes, 48), vec!["dod_anzio", "dod_flash"]);
        assert!(quick_maps(&bytes, 47).is_empty());
    }

    #[test]
    fn map_names_lose_the_folder_and_extension() {
        assert_eq!(map_stem("maps/dod_anzio.bsp"), "dod_anzio");
        assert_eq!(map_stem("dod_anzio"), "dod_anzio");
    }

    #[test]
    fn every_piece_gets_its_place_in_the_name() {
        let seg = |i: usize, map: &str| MapSegment {
            index: i,
            map: map.into(),
            map_checksum: 0,
            start_seconds: 0.0,
            end_seconds: 0.0,
            frames: 0,
        };
        let two = [seg(0, "dod_anzio"), seg(1, "dod_flash")];
        assert_eq!(split_name("m", &two, &two[0]), "m_split1_anzio");
        assert_eq!(split_name("m", &two, &two[1]), "m_split2_flash");
        // A map without the dod_ prefix keeps its name.
        let other = [seg(0, "ktp_lobby")];
        assert_eq!(split_name("m", &other, &other[0]), "m_split1_ktp_lobby");
        assert_eq!(
            old_split_name_at("m", &["dod_anzio", "dod_flash"], 1),
            "m_dod_flash"
        );
        let same = [seg(0, "dod_anzio"), seg(1, "dod_anzio")];
        assert_eq!(split_name("m", &same, &same[1]), "m_split2_anzio");
        assert_eq!(
            old_split_name_at("m", &["dod_anzio", "dod_anzio"], 1),
            "m_dod_anzio_2"
        );
    }

    #[test]
    fn a_split_keeps_the_maps_of_a_minute_or_more() {
        let seg = |i: usize, start: f32, end: f32| MapSegment {
            index: i,
            map: "dod_lennon2".into(),
            map_checksum: 0,
            start_seconds: start,
            end_seconds: end,
            frames: 0,
        };
        // wsod25_grp3_h1_dyelife: 21:28 of lennon2, then 14 s of the next half.
        assert_eq!(
            keep_at_least(&[seg(0, -2.0, 1288.0), seg(1, 1288.0, 1302.0)], 60.0),
            [0]
        );
        assert_eq!(
            keep_at_least(
                &[
                    seg(0, 0.0, 1200.0),
                    seg(1, 1200.0, 2400.0),
                    seg(2, 2400.0, 2410.0)
                ],
                60.0
            ),
            [0, 1]
        );
        // All short: keep them all rather than write nothing.
        assert_eq!(
            keep_at_least(&[seg(0, 0.0, 20.0), seg(1, 20.0, 30.0)], 60.0),
            [0, 1]
        );
    }

    #[test]
    fn the_header_protocol_is_read_without_parsing() {
        let mut h = b"HLDEMO\0\0".to_vec();
        h.extend(5i32.to_le_bytes());
        h.extend(48i32.to_le_bytes());
        assert_eq!(header_protocol(&h), Some(48));
        assert_eq!(header_protocol(b"nope"), None);
    }

    #[test]
    fn a_cancelled_split_writes_nothing_and_says_so() {
        let dir = std::env::temp_dir().join(format!("demo_split_cancel_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let demo = dir.join("m.dem");
        std::fs::write(&demo, ci_fixture()).unwrap();
        let (written, cancelled) =
            split_file_cancellable(&demo, Keep::These(&[0]), &[], &mut |_| {}, &|| true).unwrap();
        assert!(cancelled);
        assert!(written.is_empty());
        let left: Vec<_> = std::fs::read_dir(&dir).unwrap().collect();
        assert_eq!(left.len(), 1, "only the original");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn scratch_with_fixture(name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("demo_split_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let demo = dir.join("m.dem");
        std::fs::write(&demo, ci_fixture()).unwrap();
        (dir, demo)
    }

    #[test]
    fn each_map_gets_its_own_outcome_and_one_failing_does_not_stop_another() {
        let (dir, demo) = scratch_with_fixture("report");
        let mut events = Vec::new();
        // Map 6 doesn't exist: it fails, map 1 is still written.
        let report = split_file_report(
            &demo,
            Keep::These(&[0, 5, 0]),
            &[],
            &mut |p| events.push(p),
            &|| false,
        )
        .unwrap();
        assert!(!report.cancelled);
        assert_eq!(report.written.len(), 1, "map 1, once though asked twice");
        assert_eq!(report.written[0].index, 0);
        assert!(std::path::Path::new(&report.written[0].path).is_file());
        assert_eq!(report.failed.len(), 1);
        assert_eq!(report.failed[0].index, 5);
        assert!(report.failed[0].error.contains("no map 6"));
        // Callers that treat a failed map as the split failing are told the
        // file map 1 still wrote.
        let error = report.clone().into_written().unwrap_err();
        assert!(error.contains("no map 6"), "{error}");
        assert!(error.contains("(still written: m_split1_"), "{error}");

        let stages = |index: usize| -> Vec<&str> {
            let mut s: Vec<&str> = events
                .iter()
                .filter(|p| p.index == Some(index))
                .map(|p| p.stage)
                .collect();
            s.dedup();
            s
        };
        assert_eq!(stages(0), ["waiting", "writing", "checking", "finished"]);
        assert_eq!(stages(5), ["waiting", "failed"]);
        let finished = events.iter().find(|p| p.stage == "finished").unwrap();
        assert_eq!(
            finished.file.as_ref().map(|w| &w.path),
            Some(&report.written[0].path)
        );
        let failed = events.iter().find(|p| p.stage == "failed").unwrap();
        assert!(failed.error.as_deref().unwrap().contains("no map 6"));
        // Reading comes first, and the whole split ends at 100%.
        assert_eq!(events[0].stage, "reading");
        let last = events.last().unwrap();
        assert_eq!((last.done, last.parts), (2, 2));
        assert!((last.fraction - 1.0).abs() < 1e-6);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The first multi-map demo in `DOD_SPLIT_DEMOS` (or the PRE install's
    /// `dod`), copied to a scratch folder: every map is written, side by side.
    #[test]
    #[ignore = "needs real multi-map demos (DOD_SPLIT_DEMOS or the PRE install)"]
    fn every_map_of_a_real_multi_map_demo_is_written() {
        let folder = std::env::var_os("DOD_SPLIT_DEMOS").map(std::path::PathBuf::from).unwrap_or_else(|| {
            std::path::PathBuf::from(
                r"C:\Program Files (x86)\Steam\steamapps\common\Half-Life - PRE-Anniversary for Movies\dod",
            )
        });
        let source = std::fs::read_dir(&folder)
            .unwrap_or_else(|e| panic!("{}: {e}", folder.display()))
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|e| e == "dem"))
            .find(|p| {
                let bytes = std::fs::read(p).unwrap();
                quick_maps(&bytes, header_protocol(&bytes).unwrap_or(48)).len() >= 2
                    && dem::open_demo_from_bytes(&bytes).is_ok()
            })
            .unwrap_or_else(|| panic!("no multi-map demo in {}", folder.display()));
        let dir = std::env::temp_dir().join(format!("demo_split_real_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let demo = dir.join("m.dem");
        std::fs::copy(&source, &demo).unwrap();
        let maps = quick_demo_segments(&demo).unwrap();
        let all: Vec<usize> = maps.iter().map(|s| s.index).collect();
        let started = std::time::Instant::now();
        let report =
            split_file_report(&demo, Keep::These(&all), &[], &mut |_| {}, &|| false).unwrap();
        eprintln!(
            "{}: {} maps in {:.1} s",
            source.display(),
            all.len(),
            started.elapsed().as_secs_f32()
        );
        assert!(report.failed.is_empty(), "{:?}", report.failed);
        assert_eq!(
            report.written.iter().map(|w| w.index).collect::<Vec<_>>(),
            all
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_cancelled_split_says_so_for_every_map() {
        let (dir, demo) = scratch_with_fixture("cancel_each");
        let mut events = Vec::new();
        let report = split_file_report(
            &demo,
            Keep::These(&[0]),
            &[],
            &mut |p| events.push(p),
            &|| true,
        )
        .unwrap();
        assert!(report.cancelled && report.written.is_empty() && report.failed.is_empty());
        assert_eq!(events.last().map(|p| p.stage), Some("cancelled"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn checks_run_side_by_side_up_to_the_limit_and_keep_their_order() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let now = AtomicUsize::new(0);
        let most = AtomicUsize::new(0);
        let pulled = AtomicUsize::new(0);
        let jobs = (0..10).inspect(|_| {
            // A job is pulled only once one of the 3 is free.
            assert!(now.load(Ordering::SeqCst) < 3);
            pulled.fetch_add(1, Ordering::SeqCst);
        });
        let work = |j: usize| {
            let n = now.fetch_add(1, Ordering::SeqCst) + 1;
            most.fetch_max(n, Ordering::SeqCst);
            std::thread::sleep(std::time::Duration::from_millis(30));
            now.fetch_sub(1, Ordering::SeqCst);
            j * 2
        };
        let out = run_bounded(3, jobs, &work);
        assert_eq!(out, (0..10).map(|j| j * 2).collect::<Vec<_>>());
        assert_eq!(pulled.load(Ordering::SeqCst), 10);
        let most = most.load(Ordering::SeqCst);
        assert!(most > 1, "ran side by side");
        assert!(most <= 3, "never more than the limit: {most}");
    }

    #[test]
    fn a_saved_map_never_takes_a_name_already_there() {
        let dir = std::env::temp_dir().join(format!("demo_split_save_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("m_split1_anzio.dem"), b"old").unwrap();
        let p = save_new(&dir, "m_split1_anzio", b"new").unwrap();
        assert_eq!(p, dir.join("m_split1_anzio (2).dem"));
        assert_eq!(
            std::fs::read(dir.join("m_split1_anzio.dem")).unwrap(),
            b"old"
        );
        assert_eq!(std::fs::read(&p).unwrap(), b"new");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
