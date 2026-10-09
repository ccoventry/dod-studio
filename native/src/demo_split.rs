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

/// The file name for map `segment` of `demo_stem`: `<demo>_<map>`, with the
/// map's place added when the same map appears twice.
pub fn split_name(demo_stem: &str, segments: &[MapSegment], segment: &MapSegment) -> String {
    let repeats = segments.iter().filter(|s| s.map == segment.map).count() > 1;
    if repeats {
        format!("{demo_stem}_{}_{}", segment.map, segment.index + 1)
    } else {
        format!("{demo_stem}_{}", segment.map)
    }
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
    pub path: String,
    pub map: String,
    pub seconds: f32,
    pub size_bytes: u64,
}

/// A path next to `dir/name.dem` that doesn't exist yet: `name.dem`, then
/// `name (2).dem`, and so on.
fn free_path(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    let first = dir.join(format!("{name}.dem"));
    if !first.exists() {
        return first;
    }
    (2..)
        .map(|n| dir.join(format!("{name} ({n}).dem")))
        .find(|p| !p.exists())
        .expect("an unused name")
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
    let bytes =
        std::fs::read(path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
    let demo =
        dem::open_demo_from_bytes(&bytes).map_err(|e| format!("could not read the demo: {e}"))?;
    let segs = segments(&demo);
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
    let mut written = Vec::new();
    for &i in keep {
        let seg = segs
            .get(i)
            .ok_or_else(|| format!("the demo has no map {}", i + 1))?;
        let part = extract(&demo, i, &find_bsp)
            .ok_or_else(|| format!("map {} ({}) could not be cut out", i + 1, seg.map))?;
        let out = part
            .write_to_bytes_reusing_source_cancellable(&bytes, &|| false)
            .ok_or("writing was cancelled")?;
        let back = dem::open_demo_from_bytes(&out)
            .map_err(|e| format!("map {} ({}) did not read back: {e}", i + 1, seg.map))?;
        let back_segs = segments(&back);
        if back_segs.len() != 1 || back_segs[0].map != seg.map {
            return Err(format!(
                "map {} ({}) read back as {} map(s); nothing more was written",
                i + 1,
                seg.map,
                back_segs.len()
            ));
        }
        let target = free_path(dir, &split_name(&stem, &segs, seg));
        std::fs::write(&target, &out)
            .map_err(|e| format!("could not write {}: {e}", target.display()))?;
        written.push(Written {
            path: target.to_string_lossy().to_string(),
            map: seg.map.clone(),
            seconds: back_segs[0].seconds(),
            size_bytes: out.len() as u64,
        });
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn a_repeated_map_gets_its_place_in_the_name() {
        let seg = |i: usize, map: &str| MapSegment {
            index: i,
            map: map.into(),
            map_checksum: 0,
            start_seconds: 0.0,
            end_seconds: 0.0,
            frames: 0,
        };
        let two = [seg(0, "dod_anzio"), seg(1, "dod_flash")];
        assert_eq!(split_name("m", &two, &two[1]), "m_dod_flash");
        let same = [seg(0, "dod_anzio"), seg(1, "dod_anzio")];
        assert_eq!(split_name("m", &same, &same[1]), "m_dod_anzio_2");
    }

    #[test]
    fn the_header_protocol_is_read_without_parsing() {
        let mut h = b"HLDEMO\0\0".to_vec();
        h.extend(5i32.to_le_bytes());
        h.extend(48i32.to_le_bytes());
        assert_eq!(header_protocol(&h), Some(48));
        assert_eq!(header_protocol(b"nope"), None);
    }
}
