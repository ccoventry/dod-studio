//! Converting a protocol-47 demo to protocol 48 (#239).
//!
//! Today's game refuses a demo whose header says network protocol 47 ("demo
//! protocol outdated") and, past that, one whose `svc_serverinfo` says 47
//! ("Server returned version 47, not 48"). Nothing else about a protocol-47
//! demo differs: its frames, messages and delta encoding are laid out as 48's
//! (`docs/goldsrc_protocol_47.md`). So the conversion rewrites those two
//! numbers and nothing else: four bytes in the header and four in each
//! `svc_serverinfo`, in place. Every other byte, the directory included,
//! stays what the recorder wrote.
//!
//! The idea and the two places to change come from HLAE's DemoTools ("Try
//! to convert demo version (47 -> 48)"), `AfxCppCli/old/tools/demotools/`
//! `demotools.cpp` (`bSet_NetworkVersion`, `bSet_ProtocolVersion` in
//! `copy_gamedata`) and `demotoolswizard.cpp`, advancedfx commit
//! 41b158171b275f9bb8ead737e8c7d0a159daf0af. No code is copied; this file
//! is our own, written from that description. Their notice, as their MIT
//! licence asks for anything derived from it (also in
//! `docs/third_party_notices.md`):
//!
//! ```text
//!     Copyright (c) 2021 advancedfx.org
//!
//!     Permission is hereby granted, free of charge, to any person obtaining a
//!     copy of this software and associated documentation files (the
//!     "Software"), to deal in the Software without restriction, including
//!     without limitation the rights to use, copy, modify, merge, publish,
//!     distribute, sublicense, and/or sell copies of the Software, and to
//!     permit persons to whom the Software is furnished to do so, subject to
//!     the following conditions:
//!
//!     The above copyright notice and this permission notice shall be included
//!     in all copies or substantial portions of the Software.
//!
//!     THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
//!     OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
//!     MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.
//!     IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
//!     CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT,
//!     TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE
//!     SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
//! ```
//!
//! HLAE's tool reads each network frame's first messages one by one
//! (`svc_nop`, `svc_time`, `svc_print`, `svc_hltv`) and gives up at the first
//! other one before the server info. This pass finds `svc_serverinfo` the way
//! `demo_split::quick_maps` does, wherever it sits in the frame: its id and
//! the protocol, confirmed by a game folder, a host name and a `maps/*.bsp`
//! where they belong.

use dem::protocol::{NETWORK_PROTOCOL_47, NETWORK_PROTOCOL_48};
use std::fmt;
use std::path::{Path, PathBuf};

use crate::patch::{
    DEMO_HEADER_SIZE, DIRECTORY_OFFSET_POS, FRAME_HEADER_SIZE, NETMSG_INFO_SIZE,
    NETWORK_HEADER_ALIGNMENT,
};

/// The header's network protocol field.
const HEADER_NETWORK_PROTOCOL: std::ops::Range<usize> = 12..16;
const SVC_SERVERINFO: u8 = 11;
/// From `svc_serverinfo`'s id to its game folder string: protocol, spawn
/// count, map checksum (4 each), client.dll hash (16), max players, player
/// index, deathmatch (1 each). As `demo_split::TO_GAME_DIR`.
const TO_GAME_DIR: usize = 1 + 4 + 4 + 4 + 16 + 3;
/// The suffix a converted copy gets beside its source: `<stem>_p48.dem`.
pub const CONVERTED_SUFFIX: &str = "_p48";

/// Why a demo was not converted.
#[derive(Debug)]
pub enum ConvertError {
    /// The bytes don't start with a demo header.
    NotADemo,
    /// The header names another protocol than 47 (48 needs no conversion).
    NotProtocol47 { network_protocol: i32 },
    /// No `svc_serverinfo` saying 47 was found in the frames, so the game
    /// would still refuse the copy.
    NoServerInfo,
    /// The output would overwrite the demo being converted.
    OutputIsInput { path: PathBuf },
    /// The demo could not be read.
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    /// The converted copy could not be written (nothing is left behind).
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl fmt::Display for ConvertError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use crate::messages::protocol47 as m;
        match self {
            Self::NotADemo => f.write_str(m::NOT_A_DEMO),
            Self::NotProtocol47 { network_protocol } => {
                f.write_str(&m::not_protocol_47(*network_protocol))
            }
            Self::NoServerInfo => f.write_str(m::NO_SERVER_INFO),
            Self::OutputIsInput { path } => f.write_str(&m::output_is_input(path.display())),
            Self::Read { path, source } => f.write_str(&m::cannot_read(path.display(), source)),
            Self::Write { path, source } => f.write_str(&m::cannot_write(path.display(), source)),
        }
    }
}

impl std::error::Error for ConvertError {}

/// What a conversion changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Converted {
    /// `svc_serverinfo` messages rewritten: one per map the demo recorded.
    pub server_infos: usize,
    /// Frames walked to find them.
    pub frames: usize,
}

/// Whether `bytes` are a demo whose header says protocol 47: one the game
/// refuses until [`convert_in_place`] has run.
pub fn is_protocol_47(bytes: &[u8]) -> bool {
    header_protocol(bytes) == Some(NETWORK_PROTOCOL_47)
}

/// The network protocol a demo header names, or `None` for bytes that are
/// not a demo.
fn header_protocol(bytes: &[u8]) -> Option<i32> {
    if bytes.get(..8)? != b"HLDEMO\0\0" {
        return None;
    }
    Some(i32::from_le_bytes(
        bytes.get(HEADER_NETWORK_PROTOCOL)?.try_into().ok()?,
    ))
}

/// Rewrites a protocol-47 demo's `bytes` to protocol 48, in place: the
/// header's protocol and the protocol of every `svc_serverinfo` in its
/// network frames. The length and every other byte stay as they were.
///
/// Fails, changing nothing, on bytes that are not a protocol-47 demo, and on
/// one with no `svc_serverinfo` to rewrite (the game would refuse the copy).
pub fn convert_in_place(bytes: &mut [u8]) -> Result<Converted, ConvertError> {
    match header_protocol(bytes) {
        None => return Err(ConvertError::NotADemo),
        Some(NETWORK_PROTOCOL_47) => {}
        Some(network_protocol) => return Err(ConvertError::NotProtocol47 { network_protocol }),
    }
    let (spots, frames) = server_info_protocols(bytes);
    if spots.is_empty() {
        return Err(ConvertError::NoServerInfo);
    }
    let to = NETWORK_PROTOCOL_48.to_le_bytes();
    for at in &spots {
        bytes[*at..*at + 4].copy_from_slice(&to);
    }
    bytes[HEADER_NETWORK_PROTOCOL].copy_from_slice(&to);
    Ok(Converted {
        server_infos: spots.len(),
        frames,
    })
}

/// For a patcher that copies a demo frame by frame (`engine::StreamPatcher`):
/// sets a protocol-47 `header`'s protocol to 48 and returns `true`, or
/// returns `false` and leaves any other header alone. When `true`, every
/// network frame's messages then go through [`convert_messages_in_place`].
pub fn convert_header_in_place(header: &mut [u8]) -> bool {
    if !is_protocol_47(header) {
        return false;
    }
    header[HEADER_NETWORK_PROTOCOL].copy_from_slice(&NETWORK_PROTOCOL_48.to_le_bytes());
    true
}

/// Sets every `svc_serverinfo` saying protocol 47 in one network frame's
/// `messages` to 48, in place, and returns how many it changed.
pub fn convert_messages_in_place(messages: &mut [u8]) -> usize {
    let found = server_infos_in(messages);
    for start in &found {
        messages[start + 1..start + 5].copy_from_slice(&NETWORK_PROTOCOL_48.to_le_bytes());
    }
    found.len()
}

/// Where each `svc_serverinfo` saying protocol 47 keeps that number (the
/// byte after its id), in the network frames of `bytes`, and how many frames
/// were walked. The walk follows each frame's own length, from the header to
/// the directory (or the end of the file when the directory is missing or
/// out of range), and stops at the first frame that doesn't fit.
fn server_info_protocols(bytes: &[u8]) -> (Vec<usize>, usize) {
    let end = frames_end(bytes);
    let mut spots = Vec::new();
    let mut frames = 0;
    let mut at = DEMO_HEADER_SIZE;
    while let Some(frame) = next_frame(bytes, at, end) {
        frames += 1;
        if let Some(messages) = &frame.messages {
            spots.extend(
                server_infos_in(&bytes[messages.clone()])
                    .into_iter()
                    .map(|i| messages.start + i + 1),
            );
        }
        at = frame.end;
    }
    (spots, frames)
}

/// Where the frames end: the directory's offset when it lies inside the file
/// past the header, else the end of the file.
fn frames_end(bytes: &[u8]) -> usize {
    let offset = bytes
        .get(DIRECTORY_OFFSET_POS..DEMO_HEADER_SIZE)
        .map(|b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .unwrap_or(0);
    match usize::try_from(offset) {
        Ok(o) if o >= DEMO_HEADER_SIZE && o <= bytes.len() => o,
        _ => bytes.len(),
    }
}

/// One frame of the walk: where it ends, and its message bytes when it is a
/// network frame.
struct FrameSpan {
    end: usize,
    messages: Option<std::ops::Range<usize>>,
}

/// The frame starting at `at`, or `None` when no whole frame starts there
/// before `end` (the frames are over, or the next one is cut or unknown).
fn next_frame(bytes: &[u8], at: usize, end: usize) -> Option<FrameSpan> {
    let body = at.checked_add(FRAME_HEADER_SIZE)?;
    if body > end {
        return None;
    }
    let read_len = |p: usize| -> Option<usize> {
        let b = bytes.get(p..p.checked_add(4)?)?;
        usize::try_from(u32::from_le_bytes([b[0], b[1], b[2], b[3]])).ok()
    };
    let (frame_end, messages) = match bytes[at] {
        // Network messages: demo info and sequence info, the length, then
        // the messages.
        0 | 1 => {
            let len_at = body + NETMSG_INFO_SIZE;
            let start = body + NETWORK_HEADER_ALIGNMENT;
            let stop = start.checked_add(read_len(len_at)?)?;
            (stop, Some(start..stop))
        }
        // Demo start, next section: no body.
        2 | 5 => (body, None),
        3 => (body + 64, None),
        4 => (body + 32, None),
        6 => (body + 84, None),
        7 => (body + 8, None),
        // Sound: channel, sample name (length-prefixed), then 16 bytes.
        8 => {
            let name = body + 4;
            (name.checked_add(4 + read_len(name)? + 16)?, None)
        }
        // Demo buffer: length-prefixed.
        9 => ((body + 4).checked_add(read_len(body)?)?, None),
        _ => return None,
    };
    (frame_end <= end).then_some(FrameSpan {
        end: frame_end,
        messages,
    })
}

/// Where each `svc_serverinfo` saying protocol 47 starts in one frame's
/// `messages`: its id, protocol 47, then a printable game folder, a host
/// name and a `maps/*.bsp` file name where they belong, so a stray match in
/// some other message's bytes fails.
fn server_infos_in(messages: &[u8]) -> Vec<usize> {
    let mut needle = vec![SVC_SERVERINFO];
    needle.extend_from_slice(&NETWORK_PROTOCOL_47.to_le_bytes());
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(i) = messages
        .get(from..)
        .and_then(|rest| rest.windows(needle.len()).position(|w| w == needle))
    {
        let start = from + i;
        from = start + 1;
        if is_server_info_at(messages, start) {
            found.push(start);
        }
    }
    found
}

/// Whether the fields after an `svc_serverinfo` id at `start` read as one.
fn is_server_info_at(messages: &[u8], start: usize) -> bool {
    let printable = |s: &[u8]| !s.is_empty() && s.iter().all(|&b| (0x20..0x7f).contains(&b));
    let Some((game_dir, next)) = c_string(messages, start + TO_GAME_DIR, 64) else {
        return false;
    };
    let Some((_host, next)) = c_string(messages, next, 256) else {
        return false;
    };
    let Some((map_file, _)) = c_string(messages, next, 128) else {
        return false;
    };
    printable(game_dir)
        && printable(map_file)
        && map_file.starts_with(b"maps/")
        && map_file.ends_with(b".bsp")
}

/// The NUL-terminated string at `at` (at most `max` bytes before the NUL)
/// and where the next field starts.
fn c_string(bytes: &[u8], at: usize, max: usize) -> Option<(&[u8], usize)> {
    let rest = bytes.get(at..)?;
    let len = rest.iter().take(max + 1).position(|&b| b == 0)?;
    Some((&rest[..len], at + len + 1))
}

/// Where a converted copy of `input` goes when no output is named:
/// `<stem>_p48.dem` beside it.
pub fn converted_path(input: &Path) -> PathBuf {
    let stem = input
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    input.with_file_name(format!("{stem}{CONVERTED_SUFFIX}.dem"))
}

/// Converts the protocol-47 demo at `input` into a protocol-48 copy at
/// `output` ([`converted_path`] when `None`) and returns where it went and
/// what changed. The input is only read. An existing `output` is replaced,
/// unless it is the input itself (by any spelling), which is refused. The
/// copy is written beside its final name and renamed into place, so a failed
/// write leaves no half-written demo.
pub fn convert_file(
    input: &Path,
    output: Option<&Path>,
) -> Result<(PathBuf, Converted), ConvertError> {
    let output = output.map_or_else(|| converted_path(input), Path::to_path_buf);
    if is_same_file(input, &output) {
        return Err(ConvertError::OutputIsInput { path: output });
    }
    let mut bytes = std::fs::read(input).map_err(|source| ConvertError::Read {
        path: input.to_path_buf(),
        source,
    })?;
    let converted = convert_in_place(&mut bytes)?;
    write_replacing(&output, &bytes).map_err(|source| ConvertError::Write {
        path: output.clone(),
        source,
    })?;
    Ok((output, converted))
}

/// Whether `a` and `b` name one file. A path that doesn't exist yet is
/// never the input.
fn is_same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Writes `bytes` to `<path>.part`, then renames it over `path`. The part
/// file is removed again when either step fails.
fn write_replacing(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut part = path.as_os_str().to_owned();
    part.push(".part");
    let part = PathBuf::from(part);
    let result = std::fs::write(&part, bytes).and_then(|()| std::fs::rename(&part, path));
    if result.is_err() {
        // Our own scratch file: failing to remove it changes nothing.
        let _ = std::fs::remove_file(&part);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `svc_serverinfo` as an HLTV proxy of 2004 sent it, saying `protocol`.
    fn server_info(protocol: i32) -> Vec<u8> {
        let mut m = vec![SVC_SERVERINFO];
        m.extend_from_slice(&protocol.to_le_bytes());
        m.extend_from_slice(&32i32.to_le_bytes()); // spawn count
        m.extend_from_slice(&(-1_114_681_149i32).to_le_bytes()); // map checksum
        m.extend_from_slice(&[0x20; 16]); // client.dll hash
        m.extend_from_slice(&[15, 8, 1]); // max players, player index, deathmatch
        m.extend_from_slice(b"dod\0Swiss league HLTV\0maps/dod_donner.bsp\0mapcycle.txt\0");
        m.push(0); // the VAC byte
        m
    }

    /// The messages a sign-on frame starts with: the proxy's print, its
    /// `svc_hltv`, the server info, then `svc_sendextrainfo`.
    fn sign_on(protocol: i32) -> Vec<u8> {
        let mut m = b"\x08Protocol Version 47, Spawn count 32 (HLTV)\n\0".to_vec();
        m.extend_from_slice(&[50, 0]);
        m.extend_from_slice(&server_info(protocol));
        m.extend_from_slice(&[54, 0, 0]);
        m
    }

    fn frame_header(out: &mut Vec<u8>, kind: u8, frame: i32) {
        out.push(kind);
        out.extend_from_slice(&0.5f32.to_le_bytes());
        out.extend_from_slice(&frame.to_le_bytes());
    }

    fn network_frame(out: &mut Vec<u8>, frame: i32, messages: &[u8]) {
        frame_header(out, 1, frame);
        out.extend_from_slice(&[0u8; NETMSG_INFO_SIZE]);
        out.extend_from_slice(&(messages.len() as u32).to_le_bytes());
        out.extend_from_slice(messages);
    }

    /// A demo of `protocol` with one segment per entry of `segments` (each a
    /// sign-on frame's messages), every other frame type between them, two
    /// next-section frames after the first (as some 2004 recordings have),
    /// and a directory at the end.
    fn demo(protocol: i32, segments: &[Vec<u8>]) -> Vec<u8> {
        let mut d = vec![0u8; DEMO_HEADER_SIZE];
        d[..8].copy_from_slice(b"HLDEMO\0\0");
        d[8..12].copy_from_slice(&5i32.to_le_bytes());
        d[12..16].copy_from_slice(&protocol.to_le_bytes());
        d[16..26].copy_from_slice(b"dod_donner");
        d[276..279].copy_from_slice(b"dod");
        for (n, sign_on) in segments.iter().enumerate() {
            network_frame(&mut d, 0, sign_on);
            frame_header(&mut d, 2, 1);
            frame_header(&mut d, 3, 2);
            // A console command holding the needle is not a network frame.
            let mut command = [0u8; 64];
            command[..5].copy_from_slice(&[SVC_SERVERINFO, 47, 0, 0, 0]);
            d.extend_from_slice(&command);
            frame_header(&mut d, 4, 3);
            d.extend_from_slice(&[0u8; 32]);
            frame_header(&mut d, 6, 4);
            d.extend_from_slice(&[0u8; 84]);
            frame_header(&mut d, 7, 5);
            d.extend_from_slice(&[0u8; 8]);
            frame_header(&mut d, 8, 6);
            d.extend_from_slice(&1i32.to_le_bytes());
            d.extend_from_slice(&5u32.to_le_bytes());
            d.extend_from_slice(b"a.wav");
            d.extend_from_slice(&[0u8; 16]);
            frame_header(&mut d, 9, 7);
            d.extend_from_slice(&3u32.to_le_bytes());
            d.extend_from_slice(&[1, 2, 3]);
            network_frame(&mut d, 8, &[1, 1, 7, 0, 0, 0, 0]);
            frame_header(&mut d, 5, 9);
            if n == 0 {
                frame_header(&mut d, 5, 9);
            }
        }
        let directory = d.len() as i32;
        d[540..544].copy_from_slice(&directory.to_le_bytes());
        d.extend_from_slice(&0i32.to_le_bytes()); // no entries: never read
        d
    }

    /// The byte ranges where `a` and `b` differ, as (start, length).
    fn differences(a: &[u8], b: &[u8]) -> Vec<(usize, usize)> {
        assert_eq!(a.len(), b.len());
        let mut out: Vec<(usize, usize)> = Vec::new();
        for i in (0..a.len()).filter(|&i| a[i] != b[i]) {
            match out.last_mut() {
                Some((start, len)) if *start + *len == i => *len += 1,
                _ => out.push((i, 1)),
            }
        }
        out
    }

    /// Where `server_info(_)`'s protocol sits in [`demo`]'s first sign-on.
    fn first_server_info_protocol() -> usize {
        let print = b"\x08Protocol Version 47, Spawn count 32 (HLTV)\n\0".len();
        DEMO_HEADER_SIZE + FRAME_HEADER_SIZE + NETWORK_HEADER_ALIGNMENT + print + 2 + 1
    }

    #[test]
    fn the_header_and_the_server_info_say_48_and_nothing_else_changes() {
        let source = demo(47, &[sign_on(47)]);
        let mut bytes = source.clone();
        let converted = convert_in_place(&mut bytes).unwrap();
        assert_eq!(converted.server_infos, 1);
        assert_eq!(
            differences(&source, &bytes),
            // The low byte of each little-endian 47 -> 48.
            vec![(12, 1), (first_server_info_protocol(), 1)]
        );
        assert_eq!(header_protocol(&bytes), Some(48));
        assert_eq!(bytes[first_server_info_protocol()], 48);
    }

    #[test]
    fn a_converted_demo_is_the_demo_recorded_at_48() {
        let mut bytes = demo(47, &[sign_on(47)]);
        convert_in_place(&mut bytes).unwrap();
        assert_eq!(bytes, demo(48, &[sign_on(48)]));
    }

    #[test]
    fn every_segment_s_server_info_is_converted() {
        let mut bytes = demo(47, &[sign_on(47), sign_on(47), sign_on(47)]);
        let converted = convert_in_place(&mut bytes).unwrap();
        assert_eq!(converted.server_infos, 3);
        // Ten frames a segment, and the first's second next-section frame.
        assert_eq!(converted.frames, 3 * 10 + 1);
        assert!(server_info_protocols(&bytes).0.is_empty(), "no 47 left");
    }

    #[test]
    fn the_needle_outside_a_network_frame_or_without_server_info_fields_is_left() {
        // The console command frame holds 0B 2F 00 00 00 too (see `demo`), and
        // the network frame below holds it followed by junk.
        let mut stray = sign_on(47);
        stray.extend_from_slice(&[SVC_SERVERINFO, 47, 0, 0, 0]);
        stray.extend_from_slice(&[0xFF; 40]);
        let source = demo(47, &[stray]);
        let mut bytes = source.clone();
        assert_eq!(convert_in_place(&mut bytes).unwrap().server_infos, 1);
        assert_eq!(differences(&source, &bytes).len(), 2);
    }

    #[test]
    fn a_frame_by_frame_copy_converts_as_the_whole_file_does() {
        let mut header = demo(47, &[sign_on(47)])[..DEMO_HEADER_SIZE].to_vec();
        assert!(convert_header_in_place(&mut header));
        assert_eq!(header_protocol(&header), Some(48));
        assert!(!convert_header_in_place(&mut header), "48 is left alone");
        let mut messages = sign_on(47);
        messages.extend_from_slice(&sign_on(47));
        assert_eq!(convert_messages_in_place(&mut messages), 2);
        let mut want = sign_on(48);
        want.extend_from_slice(&sign_on(48));
        assert_eq!(messages, want);
        assert_eq!(convert_messages_in_place(&mut messages), 0);
    }

    #[test]
    fn a_protocol_48_demo_is_refused_unchanged() {
        let source = demo(48, &[sign_on(48)]);
        let mut bytes = source.clone();
        let err = convert_in_place(&mut bytes).unwrap_err();
        assert!(matches!(
            err,
            ConvertError::NotProtocol47 {
                network_protocol: 48
            }
        ));
        assert_eq!(bytes, source);
        assert!(!is_protocol_47(&bytes));
    }

    #[test]
    fn bytes_that_are_not_a_demo_are_refused() {
        let mut zeros = vec![0u8; 4096];
        assert!(matches!(
            convert_in_place(&mut zeros),
            Err(ConvertError::NotADemo)
        ));
        assert!(matches!(
            convert_in_place(&mut []),
            Err(ConvertError::NotADemo)
        ));
        assert!(!is_protocol_47(b"HLDEMO"));
    }

    #[test]
    fn a_protocol_47_demo_without_a_server_info_is_refused_unchanged() {
        let source = demo(47, &[vec![1, 1, 1]]);
        let mut bytes = source.clone();
        assert!(matches!(
            convert_in_place(&mut bytes),
            Err(ConvertError::NoServerInfo)
        ));
        assert_eq!(bytes, source);
    }

    #[test]
    fn a_copy_cut_short_converts_up_to_the_cut() {
        let mut bytes = demo(47, &[sign_on(47), sign_on(47)]);
        // The directory is gone and the second segment cut mid-frame: the
        // header's offset now points past the end, as in a copy that stopped
        // early.
        bytes.truncate(bytes.len() - 60);
        assert_eq!(convert_in_place(&mut bytes).unwrap().server_infos, 2);
    }

    #[test]
    fn a_frame_length_past_the_end_stops_the_walk_there() {
        let mut bytes = demo(47, &[sign_on(47)]);
        let len_at = DEMO_HEADER_SIZE + FRAME_HEADER_SIZE + NETMSG_INFO_SIZE;
        bytes[len_at..len_at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(
            convert_in_place(&mut bytes),
            Err(ConvertError::NoServerInfo)
        ));
        let mut header_only = demo(47, &[sign_on(47)]);
        header_only.truncate(DEMO_HEADER_SIZE + 3);
        assert!(matches!(
            convert_in_place(&mut header_only),
            Err(ConvertError::NoServerInfo)
        ));
    }

    #[test]
    fn a_converted_copy_goes_beside_the_demo_as_p48() {
        assert_eq!(
            converted_path(Path::new(r"D:\demos\donner-vtec-dice1.dem")),
            PathBuf::from(r"D:\demos\donner-vtec-dice1_p48.dem")
        );
    }

    #[test]
    fn convert_file_writes_the_copy_and_leaves_the_demo() {
        let dir = crate::test_support::Scratch::new("protocol47_file");
        let input = dir.path().join("old match.dem");
        let source = demo(47, &[sign_on(47)]);
        std::fs::write(&input, &source).unwrap();

        let (written, converted) = convert_file(&input, None).unwrap();
        assert_eq!(written, dir.path().join("old match_p48.dem"));
        assert_eq!(converted.server_infos, 1);
        assert_eq!(std::fs::read(&input).unwrap(), source);
        assert_eq!(header_protocol(&std::fs::read(&written).unwrap()), Some(48));

        // Again: the earlier copy is replaced, and no part file stays.
        convert_file(&input, None).unwrap();
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names.len(), 2, "{names:?}");
    }

    #[test]
    fn convert_file_never_writes_over_its_input() {
        let dir = crate::test_support::Scratch::new("protocol47_same");
        let input = dir.path().join("a.dem");
        let source = demo(47, &[sign_on(47)]);
        std::fs::write(&input, &source).unwrap();
        let same = dir.path().join(".").join("a.dem");
        assert!(matches!(
            convert_file(&input, Some(&same)),
            Err(ConvertError::OutputIsInput { .. })
        ));
        assert_eq!(std::fs::read(&input).unwrap(), source);
    }

    #[test]
    fn convert_file_says_which_demo_it_could_not_read() {
        let dir = crate::test_support::Scratch::new("protocol47_missing");
        let input = dir.path().join("gone.dem");
        let err = convert_file(&input, None).unwrap_err();
        assert!(matches!(err, ConvertError::Read { .. }));
        assert!(err.to_string().contains("gone.dem"), "{err}");
        assert!(!converted_path(&input).exists());
    }

    /// `ci_fixture.dem` (protocol 48) made to say 47 where a protocol-47
    /// recording would, converts back to the fixture byte for byte: the
    /// walk finds the real server info, and touches nothing else.
    #[test]
    fn the_fixture_turned_47_converts_back_to_itself() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../test-fixtures/ci_fixture.dem"
        );
        let fixture = std::fs::read(path).expect("test-fixtures/ci_fixture.dem");
        assert!(matches!(
            convert_in_place(&mut fixture.clone()),
            Err(ConvertError::NotProtocol47 { .. })
        ));

        let mut old = fixture.clone();
        old[12] = 47;
        let mut flipped = 0;
        let needle = [SVC_SERVERINFO, 48, 0, 0, 0];
        for at in 0..old.len() - needle.len() {
            if old[at..at + 5] == needle && is_server_info_at(&old, at) {
                old[at + 1] = 47;
                flipped += 1;
            }
        }
        assert!(flipped >= 1);
        let converted = convert_in_place(&mut old).unwrap();
        assert_eq!(converted.server_infos, flipped);
        assert_eq!(old, fixture);
    }

    /// One real protocol-47 demo, converted in memory: `Err` says what failed.
    /// The copy says 48 in the header and each server info; every other byte
    /// is the recorder's (so every frame without a server info comes back as
    /// it was); it parses; and it analyses to the original's map, players,
    /// kills and rounds.
    fn check_real_conversion(source: &[u8]) -> Result<String, String> {
        let mut bytes = source.to_vec();
        let converted = convert_in_place(&mut bytes).map_err(|e| e.to_string())?;
        let changed: Vec<usize> = (0..bytes.len())
            .filter(|&i| bytes[i] != source[i])
            .collect();
        if changed.len() != 1 + converted.server_infos
            || changed[0] != 12
            || changed.iter().any(|&i| (source[i], bytes[i]) != (47, 48))
        {
            return Err(format!("unexpected bytes changed: {changed:?}"));
        }
        // Kill events, and the scoreboard's kills summed over players.
        let kills = |a: &analysis::Analysis| {
            let scoreboard: i64 = a.state.players.iter().map(|p| i64::from(p.stats.1)).sum();
            (a.state.kill_positions.len(), scoreboard)
        };
        let summary = |a: &analysis::Analysis| {
            let (events, scoreboard) = kills(a);
            format!(
                "{} {}, {} players, {events} kills ({scoreboard} on the scoreboard), {} rounds",
                a.demo_info.demo_type,
                a.demo_info.map_name,
                a.state.players.len(),
                a.state.rounds.len()
            )
        };
        let before = analysis::Analysis::try_from_bytes(source)?;
        let after = analysis::Analysis::try_from_bytes(&bytes)?;
        if after.demo_info.network_protocol != 48 {
            return Err("the analyzer still reads protocol 47".into());
        }
        if summary(&after) != summary(&before) || after.state.players.is_empty() {
            return Err(format!("{} became {}", summary(&before), summary(&after)));
        }
        if kills(&after).0 == 0 {
            return Err("no kills".into());
        }
        // A copy cut short (its directory past the end) only reads tolerantly,
        // as the original does.
        let strict_before = dem::open_demo_from_bytes(source).is_ok();
        if strict_before && dem::open_demo_from_bytes(&bytes).is_err() {
            return Err("the copy no longer parses strictly".into());
        }
        Ok(format!(
            "{}, {} server info(s)",
            summary(&after),
            converted.server_infos
        ))
    }

    /// Every protocol-47 demo in the folder `DOD_PROTOCOL47_DEMOS` names
    /// converts ([`check_real_conversion`]); one line per demo, then the
    /// failures. No such demo is in the repo (community recordings are not
    /// ours to commit):
    ///
    /// ```text
    /// DOD_PROTOCOL47_DEMOS=<folder> cargo test -p native --release --lib -- --ignored --nocapture real_protocol_47
    /// ```
    #[test]
    #[ignore]
    fn real_protocol_47_demos_convert_parse_and_analyse_the_same() {
        let folder = std::env::var_os("DOD_PROTOCOL47_DEMOS")
            .map(PathBuf::from)
            .expect("set DOD_PROTOCOL47_DEMOS to a folder of protocol-47 demos");
        let mut demos: Vec<PathBuf> = std::fs::read_dir(&folder)
            .unwrap_or_else(|e| panic!("{}: {e}", folder.display()))
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("dem")))
            .collect();
        demos.sort();
        let mut failures = Vec::new();
        let mut checked = 0;
        for path in &demos {
            let source = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            if !is_protocol_47(&source) {
                continue;
            }
            checked += 1;
            match check_real_conversion(&source) {
                Ok(line) => println!("pass {}: {line}", path.display()),
                Err(e) => {
                    println!("FAIL {}: {e}", path.display());
                    failures.push(format!("{}: {e}", path.display()));
                }
            }
        }
        println!("{checked} protocol-47 demos, {} failed", failures.len());
        assert!(checked > 0, "no protocol-47 demo in {}", folder.display());
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
}
