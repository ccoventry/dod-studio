//! GoldSrc demo parser and writer
//!
//! # Example
//!
//! ```no_run
//! use dem::open_demo;
//! use dem::types::{EngineMessage, FrameData, MessageData, NetMessage};
//!
//! let mut demo = open_demo("./src/tests/demotest.dem").unwrap();
//!
//! for entry in &mut demo.directory.entries {
//!     for frame in &mut entry.frames {
//!         if let FrameData::NetworkMessage(ref mut box_type) = &mut frame.frame_data {
//!             let data = &mut box_type.as_mut().1;
//!             
//!             if let MessageData::Parsed(messages) = &mut data.messages {
//!                 messages.push(NetMessage::EngineMessage(Box::new(EngineMessage::SvcBad)));
//!             };
//!         }
//!     }
//! }
//!
//! demo.write_to_file("./src/tests/demo2test.dem").unwrap();
//! ```
#![allow(mismatched_lifetime_syntaxes)]

use nom::{
    Parser,
    combinator::all_consuming,
    multi::{fold_many0, many0},
};
use std::{ffi::OsStr, path::Path};
use types::{AuxRefCell, ByteVec, DeltaDecoderTable, Demo, NetMessage};

use nom_helper::Result;

mod byte_writer;
mod delta;
mod delta_fields;
mod nom_helper;
mod utils;

pub mod bit;
pub mod demo_parser;
pub mod demo_writer;
pub mod netmsg_doer;
pub mod prelude;
pub mod progress;
pub mod types;

pub use utils::bitslice_to_string;

// /// Re-exporting hldemo to have latest changes than 0.3.0 hldemo
// pub extern crate hldemo;

/// Re-exporting bitvec to avoid clogging the main project
pub extern crate bitvec;

/// Parses all bytes in `data.msg` for each demo frame.
///
///
pub fn parse_netmsg(i: &[u8], aux: AuxRefCell) -> Result<Vec<NetMessage>> {
    let parser = move |i| NetMessage::parse(i, aux.clone());
    all_consuming(many0(parser)).parse(i)
}

/// [`parse_netmsg`] leaving out every `svc_sound`
/// ([`types::MessageDataParseMode::ParseWithoutSounds`]). Fails where it
/// fails: the sounds are still read through, just not kept.
pub fn parse_netmsg_without_sounds(i: &[u8], aux: AuxRefCell) -> Result<Vec<NetMessage>> {
    let parser = move |i| NetMessage::parse_unless_sound(i, aux.clone());
    let keep = |mut kept: Vec<NetMessage>, message: Option<NetMessage>| {
        kept.extend(message);
        kept
    };
    all_consuming(fold_many0(parser, Vec::new, keep)).parse(i)
}

/// R&D (#37 Tier 4): [`parse_netmsg_without_sounds`] that also leaves out
/// entity snapshots and client data.
pub fn parse_netmsg_tier4(
    i: &[u8],
    aux: AuxRefCell,
    flavour: netmsg_doer::Tier4Flavour,
) -> Result<Vec<NetMessage>> {
    let parser = move |i| NetMessage::parse_tier4(i, aux.clone(), flavour);
    let keep = |mut kept: Vec<NetMessage>, message: Option<NetMessage>| {
        kept.extend(message);
        kept
    };
    all_consuming(fold_many0(parser, Vec::new, keep)).parse(i)
}

/// Should be used for replacing `data.msg` of each frame.
pub fn write_netmsg(i: &Vec<NetMessage>, aux: AuxRefCell) -> ByteVec {
    let mut res: ByteVec = vec![];

    for message in i {
        res.append(&mut message.write(aux.clone()))
    }

    res
}

/// Opens a demo
///
/// # Example
/// ```no_run
/// use dem::open_demo;
///
/// let demo = open_demo("./tests/demotest.dem").unwrap();
/// ```
pub fn open_demo(demo_path: impl AsRef<Path> + AsRef<OsStr>) -> eyre::Result<Demo> {
    Demo::parse_from_file(demo_path, types::MessageDataParseMode::Parse)
}

pub fn open_demo_from_bytes(demo_bytes: &[u8]) -> eyre::Result<Demo> {
    Demo::parse_from_bytes(demo_bytes, types::MessageDataParseMode::Parse)
}

/// [`open_demo_from_bytes`], calling `progress(bytes_read, bytes_total)` as
/// it goes (see [`progress`]).
pub fn open_demo_from_bytes_with_progress(
    demo_bytes: &[u8],
    progress: &mut dyn FnMut(usize, usize),
) -> eyre::Result<Demo> {
    progress::with_progress(progress, || open_demo_from_bytes(demo_bytes))
}

/// [`open_demo_from_bytes_with_progress`] for a demo that may be damaged
/// (#811): what parses up to the first frame that does not, and where that is
/// ([`demo_parser::parse_demo_tolerant`]). For reading a demo only; anything
/// that writes one keeps to the strict parse.
pub fn open_demo_from_bytes_tolerant_with_progress(
    demo_bytes: &[u8],
    progress: &mut dyn FnMut(usize, usize),
) -> eyre::Result<(Demo, Option<demo_parser::ParseDamage>)> {
    open_demo_from_bytes_tolerant_in_mode(demo_bytes, types::MessageDataParseMode::Parse, progress)
}

/// [`open_demo_from_bytes_tolerant_with_progress`] decoding the network
/// messages as `mode` says (e.g.
/// [`types::MessageDataParseMode::ParseWithoutSounds`]).
pub fn open_demo_from_bytes_tolerant_in_mode(
    demo_bytes: &[u8],
    mode: types::MessageDataParseMode,
    progress: &mut dyn FnMut(usize, usize),
) -> eyre::Result<(Demo, Option<demo_parser::ParseDamage>)> {
    progress::with_progress(progress, || {
        Demo::parse_from_bytes_tolerant(demo_bytes, mode)
    })
}

/// Writes a [`u32`] into [`types::BitVec`]
#[macro_export]
macro_rules! nbit_num {
    ($num:expr, $bit:expr) => {{
        use $crate::bit::BitWriter;

        let mut writer = BitWriter::new();
        writer.append_u32_range($num as u32, $bit);
        writer.data
    }};
}

/// Writes a string into [`types::BitVec`]
#[macro_export]
macro_rules! nbit_str {
    ($name:expr) => {{
        use $crate::bit::BitWriter;

        let mut writer = BitWriter::new();
        $name.as_bytes().iter().for_each(|s| writer.append_u8(*s));
        writer.data
    }};
}

#[cfg(test)]
mod test {
    use super::*;

    /// Regenerates `./src/tests/demotest.dem`, the small tracked fixture the
    /// four tests below need. Not run automatically -- there is nothing to
    /// regenerate *from*, this constructs a minimal demo entirely in code
    /// rather than trimming a real recording, so running it again only
    /// matters if the on-disk format itself changes.
    ///
    /// A real DoD/HLTV demo carries thousands of frames; this carries three
    /// -- DemoStart, one ConsoleCommand, NextSection -- which is the smallest
    /// shape `parse_directory`/`write_to_bytes` actually round-trip: a
    /// `NetworkMessage` frame needs a fully populated `DemoInfo` (ref params,
    /// usercmd, movevars, ...) for no benefit here, since these tests exist to
    /// exercise open/write/parse-mode plumbing, not to be a realistic demo.
    /// `frame_count`/`frame_offset`/`file_length`/`directory_offset` are left
    /// at 0 -- `Demo::write_to_bytes` recomputes all four from the actual
    /// frames and byte offsets rather than trusting these fields, which is
    /// also why a hand-built `Demo` can produce a valid file at all.
    #[test]
    #[ignore = "regenerates the tracked fixture; run manually with `cargo test -p dem --lib test::regenerate_fixture -- --ignored`"]
    fn regenerate_fixture() {
        use crate::types::{ConsoleCommand, Directory, DirectoryEntry, Frame, FrameData, Header};

        let demo = Demo {
            header: Header {
                magic: b"HLDEMO\x00\x00".to_vec(),
                demo_protocol: 5,
                network_protocol: 48,
                map_name: "dod_test".into(),
                game_directory: "dod".into(),
                map_checksum: 0,
                directory_offset: 0,
            },
            directory: Directory {
                entries: vec![DirectoryEntry {
                    type_: 1,
                    description: "fixture".into(),
                    flags: 0,
                    cd_track: 0,
                    track_time: 0.0,
                    frame_count: 0,
                    frame_offset: 0,
                    file_length: 0,
                    frames: vec![
                        Frame {
                            time: 0.0,
                            frame: 0,
                            frame_data: FrameData::DemoStart,
                        },
                        Frame {
                            time: 0.0,
                            frame: 0,
                            frame_data: FrameData::ConsoleCommand(ConsoleCommand {
                                command: "echo dem-patch test fixture".into(),
                            }),
                        },
                        Frame {
                            time: 0.0,
                            frame: 1,
                            frame_data: FrameData::NextSection,
                        },
                    ],
                }],
            },
            _aux: None,
        };

        demo.write_to_file("./src/tests/demotest.dem").unwrap();
    }

    #[test]
    fn open() {
        open_demo("./src/tests/demotest.dem").unwrap();
    }

    #[test]
    fn open_without_netmessage() {
        Demo::parse_from_file(
            "./src/tests/demotest.dem",
            types::MessageDataParseMode::Parse,
        )
        .unwrap();
    }

    #[test]
    fn write() {
        let dem = open_demo("./src/tests/demotest.dem").unwrap();
        dem.write_to_file("./src/tests/demotest_out.dem").unwrap();
    }

    #[test]
    fn read_a_lot() {
        let folder = "./src/tests/";

        std::fs::read_dir(folder)
            .map(|res| {
                res.filter_map(|entry| entry.ok()).for_each(|entry| {
                    let path = entry.path();

                    if path.is_dir() {
                        return;
                    }

                    if path.extension().map(|ext| ext != "dem").unwrap_or(false) {
                        return;
                    }

                    assert!(
                        open_demo(path.as_path()).is_ok(),
                        "error opening {}",
                        path.display()
                    )
                })
            })
            .unwrap_or_else(|e| panic!("could not read the test fixture directory: {e}"));
    }

    /// A real recording (`test-fixtures/ci_fixture.dem`, see its README):
    /// every network frame's messages parse, and re-encoding the whole demo
    /// from the parsed messages gives back the original bytes. That is the
    /// message parsers and writers checked against what the game really sent,
    /// which `demotest.dem` (no network frames) cannot do.
    #[test]
    fn a_real_recording_parses_and_re_encodes_byte_for_byte() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../test-fixtures/ci_fixture.dem"
        );
        let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
        let demo = open_demo_from_bytes(&bytes).unwrap();

        let (mut network_frames, mut messages) = (0, 0);
        for frame in demo.directory.entries.iter().flat_map(|e| &e.frames) {
            if let types::FrameData::NetworkMessage(network) = &frame.frame_data {
                network_frames += 1;
                match &network.1.messages {
                    types::MessageData::Parsed(parsed) => messages += parsed.len(),
                    other => panic!("frame {} not parsed: {other:?}", frame.frame),
                }
            }
        }
        assert!(network_frames > 10_000, "{network_frames} network frames");
        assert!(messages > 50_000, "{messages} messages");

        // Directory included: the frame counts are the engine's own.
        assert_eq!(
            first_difference(&demo.write_to_bytes(), &bytes),
            None,
            "re-encoding the parsed demo changed its bytes"
        );
    }

    /// `debug` with every `addr: 0x<hex>` a `BitVec` prints taken out.
    fn without_addresses(debug: &str) -> String {
        let mut out = String::with_capacity(debug.len());
        let mut rest = debug;
        while let Some(at) = rest.find("addr: 0x") {
            out.push_str(&rest[..at]);
            rest =
                rest[at + "addr: 0x".len()..].trim_start_matches(|c: char| c.is_ascii_hexdigit());
        }
        out.push_str(rest);
        out
    }

    /// `ParseWithoutSounds` on the same recording: every network frame keeps
    /// exactly the full parse's messages minus its sounds, in order, and the
    /// demo has sounds to leave out.
    #[test]
    fn a_parse_without_sounds_keeps_every_other_message() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../test-fixtures/ci_fixture.dem"
        );
        let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
        let full = Demo::parse_from_bytes(&bytes, types::MessageDataParseMode::Parse).unwrap();
        let fast = Demo::parse_from_bytes(&bytes, types::MessageDataParseMode::ParseWithoutSounds)
            .unwrap();
        let frames = |demo: &Demo| -> Vec<Vec<NetMessage>> {
            demo.directory
                .entries
                .iter()
                .flat_map(|e| &e.frames)
                .filter_map(|frame| match &frame.frame_data {
                    types::FrameData::NetworkMessage(network) => match &network.1.messages {
                        types::MessageData::Parsed(parsed) => Some(parsed.clone()),
                        other => panic!("frame {} not parsed: {other:?}", frame.frame),
                    },
                    _ => None,
                })
                .collect()
        };
        let is_sound = |m: &NetMessage| matches!(m, NetMessage::EngineMessage(e) if matches!(**e, types::EngineMessage::SvcSound(_)));
        let (full, fast) = (frames(&full), frames(&fast));
        assert_eq!(full.len(), fast.len(), "network frames");
        let mut sounds = 0;
        for (k, (full, fast)) in full.into_iter().zip(fast).enumerate() {
            sounds += full.iter().filter(|m| is_sound(m)).count();
            assert!(!fast.iter().any(is_sound), "network frame {k} kept a sound");
            let kept: Vec<NetMessage> = full.into_iter().filter(|m| !is_sound(m)).collect();
            // The messages have no `PartialEq`; every field is in `Debug`,
            // beside the address of each bit vector's buffer.
            assert!(
                without_addresses(&format!("{kept:?}")) == without_addresses(&format!("{fast:?}")),
                "network frame {k}'s other messages differ"
            );
        }
        assert!(sounds > 1_000, "{sounds} sounds");
    }

    /// Where two files first differ, and how long each is, or `None` when
    /// they're the same: a 10 MB `assert_eq!` on the bytes prints nothing
    /// anyone can read.
    fn first_difference(written: &[u8], source: &[u8]) -> Option<(usize, usize, usize)> {
        if written == source {
            return None;
        }
        let at = written
            .iter()
            .zip(source)
            .position(|(a, b)| a != b)
            .unwrap_or(written.len().min(source.len()));
        Some((at, written.len(), source.len()))
    }

    /// The fixture's round trip, on more and longer demos than CI carries:
    /// `DOD_ROUNDTRIP_DEMO` (a `.dem` or a folder of them), else an even
    /// spread of six from the PRE install's `dod` folder. Ignored because CI
    /// has neither: `cargo test -p dem --release -- --ignored many_real_recordings`.
    ///
    /// Two differences are allowed, and printed. A demo DoD Studio wrote
    /// before the writer kept the engine's frame counts (one with a nonzero
    /// LOADING count, #1271) keeps its old counts, so they are not compared.
    /// And an entry whose bytes run on past its first `NextSection` (a second
    /// `NextSection`, in `ktps8w7-warchyld_over_harr_h1.dem`'s LOADING) loses
    /// that tail: the parser ends an entry at its first `NextSection`, as the
    /// engine does when it jumps to the next entry's offset there, so the
    /// engine never reads it either (`docs/goldsrc_viewdemo.md` §2). Every
    /// other byte must come back as it was.
    #[test]
    #[ignore]
    fn many_real_recordings_re_encode_byte_for_byte() {
        let source = std::env::var_os("DOD_ROUNDTRIP_DEMO")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(
                    r"C:\Program Files (x86)\Steam\steamapps\common\Half-Life - PRE-Anniversary for Movies\dod",
                )
            });
        let demos = roundtrip_demos(&source);
        assert!(
            !demos.is_empty(),
            "no demos in {}: set DOD_ROUNDTRIP_DEMO to a .dem or a folder",
            source.display()
        );

        for path in &demos {
            let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            let demo =
                open_demo_from_bytes(&bytes).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            let written = demo.write_to_bytes();
            let engine_counts = demo
                .directory
                .entries
                .first()
                .is_none_or(|e| e.frame_count == 0);
            let notes = assert_same_but_allowed(&written, &bytes, engine_counts)
                .unwrap_or_else(|why| panic!("{}: re-encoding changed it: {why}", path.display()));
            println!("{}: {} bytes, the same{notes}", path.display(), bytes.len());
        }
    }

    /// `written` against `source` entry by entry, allowing only what
    /// [`many_real_recordings_re_encode_byte_for_byte`] allows. `Ok` carries
    /// what was allowed, for the listing; `Err` says what differs.
    fn assert_same_but_allowed(
        written: &[u8],
        source: &[u8],
        compare_counts: bool,
    ) -> std::result::Result<String, String> {
        let i32_at = |b: &[u8], at: usize| i32::from_le_bytes(b[at..at + 4].try_into().unwrap());
        if written[..540] != source[..540] {
            return Err(format!(
                "header: {:?}",
                first_difference(&written[..540], &source[..540])
            ));
        }
        let (dir_w, dir_s) = (i32_at(written, 540) as usize, i32_at(source, 540) as usize);
        let count = i32_at(source, dir_s);
        if i32_at(written, dir_w) != count {
            return Err("directory entry count".into());
        }
        let mut notes = String::new();
        let mut next_source_entry = 544;
        for k in 0..count as usize {
            let (ew, es) = (dir_w + 4 + k * 92, dir_s + 4 + k * 92);
            // type, description, flags, CD track, track time; then frames.
            if written[ew..ew + 80] != source[es..es + 80] {
                return Err(format!("directory entry {k}'s fields"));
            }
            if compare_counts && written[ew + 80..ew + 84] != source[es + 80..es + 84] {
                return Err(format!(
                    "directory entry {k}'s frame count: {} for {}",
                    i32_at(written, ew + 80),
                    i32_at(source, es + 80)
                ));
            }
            let span = |b: &[u8], e: usize| {
                let start = i32_at(b, e + 84) as usize;
                start..start + i32_at(b, e + 88) as usize
            };
            // No bytes between the source's entries go uncompared.
            if span(source, es).start != next_source_entry {
                return Err(format!("bytes before entry {k}'s frames"));
            }
            next_source_entry = span(source, es).end;
            let (w, s) = (&written[span(written, ew)], &source[span(source, es)]);
            if !s.starts_with(w) {
                return Err(format!("entry {k}'s frames: {:?}", first_difference(w, s)));
            }
            if s.len() > w.len() {
                // The written entry ends at its NextSection: type 5, time, frame.
                if w.len() < 9 || w[w.len() - 9] != 5 {
                    return Err(format!("entry {k} is cut short, not at a NextSection"));
                }
                notes += &format!(
                    " (entry {k}: {} bytes past its NextSection dropped)",
                    s.len() - w.len()
                );
            }
        }
        let end = |dir: usize| dir + 4 + count as usize * 92;
        if next_source_entry != dir_s || written.len() != end(dir_w) || source.len() != end(dir_s) {
            return Err("bytes before or after the directory".into());
        }
        if !compare_counts {
            notes += " (frame counts not compared: an older DoD Studio wrote it)";
        }
        Ok(notes)
    }

    /// `source` itself when it's a file, else every `.dem` in it when it was
    /// named on purpose, else an even spread of six.
    fn roundtrip_demos(source: &std::path::Path) -> Vec<std::path::PathBuf> {
        if source.is_file() {
            return vec![source.to_path_buf()];
        }
        let Ok(entries) = std::fs::read_dir(source) else {
            return Vec::new();
        };
        let mut demos: Vec<_> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("dem")))
            .collect();
        demos.sort();
        if std::env::var_os("DOD_ROUNDTRIP_DEMO").is_some() || demos.len() <= 6 {
            return demos;
        }
        let step = demos.len() / 6;
        demos.into_iter().step_by(step).take(6).collect()
    }

    /// `svc_pings` splits each entry 5 (slot) + 12 (ping) + 7 (loss) bits
    /// (#1031). The fixture's recorder pinged a steady ~26 ms with no loss;
    /// these are the numbers `recording_quality_probe` (#1031's reference
    /// split) gives for the same demo: 237 samples, median 26, p95 28, loss 0.
    /// The re-encode test above covers the writer.
    #[test]
    fn a_real_recording_s_pings_are_the_scoreboard_s() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../test-fixtures/ci_fixture.dem"
        );
        let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
        let demo = open_demo_from_bytes(&bytes).unwrap();

        let mut recorder = None;
        let (mut pings, mut losses, mut entries) = (vec![], vec![], 0);
        // The recorder's slot comes from SvcServerInfo in the LOADING entry
        // (0); pings are counted from the playback entries on, as in the probe.
        for (entry, frame) in demo
            .directory
            .entries
            .iter()
            .enumerate()
            .flat_map(|(k, e)| e.frames.iter().map(move |f| (k, f)))
        {
            let types::FrameData::NetworkMessage(network) = &frame.frame_data else {
                continue;
            };
            let types::MessageData::Parsed(messages) = &network.1.messages else {
                continue;
            };
            for message in messages {
                let types::NetMessage::EngineMessage(m) = message else {
                    continue;
                };
                match &**m {
                    types::EngineMessage::SvcServerInfo(si) => recorder = Some(si.player_index),
                    types::EngineMessage::SvcPings(p) if entry > 0 => {
                        for e in p.pings.iter().filter(|e| e.has_ping_data) {
                            let slot = e.player_id.unwrap();
                            assert!(slot < 32, "slot {slot}");
                            assert!(e.ping.unwrap() < 1000, "{e:?}");
                            assert!(e.loss.unwrap() <= 100, "{e:?}");
                            entries += 1;
                            if Some(slot) == recorder {
                                pings.push(e.ping.unwrap());
                                losses.push(e.loss.unwrap());
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        assert!(entries > pings.len(), "the bots' entries are read too");
        assert_eq!(pings.len(), 237, "the recorder's ping samples");
        pings.sort_unstable();
        assert_eq!(pings[pings.len() / 2], 26, "median ping");
        assert!(pings.iter().all(|p| (5..=60).contains(p)), "{pings:?}");
        assert!(losses.iter().all(|l| *l == 0), "{losses:?}");
    }

    /// The tolerant parse on damaged copies of `ci_fixture.dem` (#811), built
    /// here so CI runs them.
    mod tolerant_parse {
        use super::*;
        use crate::demo_parser::ParseDamage;

        fn fixture() -> Vec<u8> {
            let path = concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../test-fixtures/ci_fixture.dem"
            );
            std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"))
        }

        fn tolerant(bytes: &[u8]) -> (Demo, Option<ParseDamage>) {
            Demo::parse_from_bytes_tolerant(bytes, types::MessageDataParseMode::Parse).unwrap()
        }

        fn frame_counts(demo: &Demo) -> Vec<usize> {
            demo.directory
                .entries
                .iter()
                .map(|e| e.frames.len())
                .collect()
        }

        #[test]
        fn an_intact_demo_reads_as_the_strict_parse_reads_it() {
            let bytes = fixture();
            let strict = open_demo_from_bytes(&bytes).unwrap();
            let (demo, damage) = tolerant(&bytes);
            assert_eq!(damage, None);
            assert_eq!(frame_counts(&demo), frame_counts(&strict));
            assert_eq!(demo.write_to_bytes(), strict.write_to_bytes());
        }

        #[test]
        fn a_file_cut_mid_frame_reads_up_to_the_cut() {
            let bytes = fixture();
            let cut = bytes.len() * 3 / 5;
            let cut_file = &bytes[..cut];
            assert!(open_demo_from_bytes(cut_file).is_err(), "strict refuses it");

            let (demo, damage) = tolerant(cut_file);
            let damage = damage.expect("the cut is reported");
            // The directory went with the end of the file, so the frames are
            // read as an unfinalised demo's.
            assert_eq!(demo.directory.entries.len(), 2);
            assert_eq!(damage.entry_index, 1);
            assert_eq!(damage.entry_description, "Playback");
            // In words, whichever read hit the cut first.
            assert!(
                damage.reason == "the file ends partway through a frame"
                    || damage.reason.contains("runs past the end of the file"),
                "{}",
                damage.reason
            );
            assert_eq!(
                damage.frame_index,
                frame_counts(&demo).iter().sum::<usize>()
            );
            assert!(damage.byte_offset < cut && damage.bytes_unread == cut - damage.byte_offset);
            // Less than one frame is lost: a network frame is a few hundred bytes.
            assert!(damage.bytes_unread < 70_000, "{damage:?}");
            let last = demo.directory.entries[1].frames.last().unwrap();
            assert_eq!(damage.last_frame_time, Some(last.time));

            // Every frame before the cut is the strict parse's.
            let strict = open_demo_from_bytes(&bytes).unwrap();
            let strict_frames: Vec<_> = strict
                .directory
                .entries
                .iter()
                .flat_map(|e| &e.frames)
                .collect();
            let frames: Vec<_> = demo
                .directory
                .entries
                .iter()
                .flat_map(|e| &e.frames)
                .collect();
            assert!(
                frames.len() > strict_frames.len() / 2,
                "{} frames",
                frames.len()
            );
            for (a, b) in frames.iter().zip(&strict_frames) {
                assert_eq!((a.time, a.frame), (b.time, b.frame));
            }
        }

        #[test]
        fn a_bad_message_length_ends_the_entry_at_that_frame() {
            let bytes = fixture();
            let strict = open_demo_from_bytes(&bytes).unwrap();
            // A network frame halfway through the longest entry, by its
            // payload's place in the file.
            let longest = (0..strict.directory.entries.len())
                .max_by_key(|&k| strict.directory.entries[k].frames.len())
                .unwrap();
            let before: usize = strict.directory.entries[..longest]
                .iter()
                .map(|e| e.frames.len())
                .sum();
            let entry = &strict.directory.entries[longest];
            let (in_entry, span) = entry.frames[entry.frames.len() / 2..]
                .iter()
                .enumerate()
                .find_map(|(k, f)| match &f.frame_data {
                    types::FrameData::NetworkMessage(n) => n.1.source_span.clone().map(|s| (k, s)),
                    _ => None,
                })
                .map(|(k, s)| (entry.frames.len() / 2 + k, s))
                .expect("a network frame in the second half");

            let mut mangled = bytes.clone();
            mangled[span.start - 4..span.start].copy_from_slice(&0x0010_0000u32.to_le_bytes());
            assert!(open_demo_from_bytes(&mangled).is_err(), "strict refuses it");

            let (demo, damage) = tolerant(&mangled);
            let damage = damage.expect("the bad frame is reported");
            assert_eq!(damage.reason, "message length too long: 1048576");
            assert_eq!(damage.entry_index, longest);
            assert_eq!(damage.frame_index, before + in_entry);
            assert_eq!(
                demo.directory.entries.len(),
                longest + 1,
                "later entries are dropped"
            );
            assert_eq!(demo.directory.entries[longest].frames.len(), in_entry);
            assert_eq!(
                damage.last_frame_time,
                Some(entry.frames[in_entry - 1].time)
            );
            assert!(damage.byte_offset < span.start - 4);
            let directory = i32::from_le_bytes(bytes[540..544].try_into().unwrap()) as usize;
            assert_eq!(damage.bytes_unread, directory - damage.byte_offset);
        }

        #[test]
        fn nothing_readable_is_an_error() {
            let bytes = fixture();
            assert!(
                Demo::parse_from_bytes_tolerant(&bytes[..300], types::MessageDataParseMode::Parse)
                    .is_err()
            );
            assert!(
                Demo::parse_from_bytes_tolerant(&bytes[..550], types::MessageDataParseMode::Parse)
                    .is_err()
            );
        }
    }

    /// A malformed demo must come back as `Err`, never as a panic.
    ///
    /// The distinction is not cosmetic. The release profile sets
    /// `panic = "abort"`, so the `catch_unwind` in
    /// `Analysis::try_from_bytes_with_progress` cannot contain a panic raised
    /// in here -- a single corrupted file in a scanned folder would take the
    /// whole process down instead of being skipped with a diagnostic. Before
    /// #225, 248 of 400 randomly mangled demos did exactly that.
    ///
    /// These run under the test profile, which unwinds, so a regression shows
    /// up as a failing test rather than as a dead test runner.
    mod malformed_input_is_an_error_not_a_crash {
        use super::*;

        fn fixture() -> Vec<u8> {
            std::fs::read("./src/tests/demotest.dem").expect("fixture")
        }

        /// The header's `directory_offset` is signed and is used as an index
        /// into the file.
        #[test]
        fn a_directory_offset_past_the_end_is_refused() {
            let mut bytes = fixture();
            bytes[540..544].copy_from_slice(&0x7fff_ffffu32.to_le_bytes());
            assert!(open_demo_from_bytes(&bytes).is_err());
        }

        #[test]
        fn a_negative_directory_offset_is_refused() {
            let mut bytes = fixture();
            bytes[540..544].copy_from_slice(&(-8i32).to_le_bytes());
            assert!(open_demo_from_bytes(&bytes).is_err());
        }

        /// Cutting the file anywhere past the header must not panic. Most cuts
        /// land mid-frame; a few happen to be parseable.
        #[test]
        fn truncation_at_every_offset_past_the_header_is_survivable() {
            let bytes = fixture();
            for at in 544..bytes.len() {
                let _ = open_demo_from_bytes(&bytes[..at]);
                let _ = Demo::parse_from_bytes_tolerant(
                    &bytes[..at],
                    types::MessageDataParseMode::Parse,
                );
            }
        }

        /// Every single-byte value at every position past the header. This is
        /// the fixture, so it is small enough to be exhaustive.
        #[test]
        fn any_single_byte_corruption_past_the_header_is_survivable() {
            let bytes = fixture();
            for at in 544..bytes.len() {
                for v in [0x00u8, 0x01, 0x7f, 0x80, 0xfe, 0xff] {
                    let mut mutant = bytes.clone();
                    mutant[at] = v;
                    let _ = open_demo_from_bytes(&mutant);
                    let _ = Demo::parse_from_bytes_tolerant(
                        &mutant,
                        types::MessageDataParseMode::Parse,
                    );
                }
            }
        }
    }
}
