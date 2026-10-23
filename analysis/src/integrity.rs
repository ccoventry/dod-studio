//! Demo integrity (#41): does every delta in a demo decode against a frame the
//! file actually has?
//!
//! A GoldSrc demo is a run of length-prefixed frames with no checksum and no
//! expected frame count, so a copy that lost frames in the middle still parses
//! cleanly and then crashes the engine on playback. What the engine cannot
//! survive is a broken *delta chain*: most network frames encode their entities
//! and client data as a difference against an earlier frame, named by the low 8
//! bits of that frame's `incoming_sequence`. When that frame is missing from the
//! file, the engine decodes against whatever stale state sits in its 64-slot
//! frame ring.
//!
//! The check is exact: on the healthy library it finds nothing (0 of ~524k
//! deltas across 6 demos in the #674 probe), and a single dropped network frame
//! is enough to make it fire. Frame time is no substitute: it never steps
//! backwards when frames are dropped, only forwards.
//!
//! [`check_demo_bytes`] also says when a demo has no camera data (#920,
//! [`crate::camera_data`]): it decodes fine and still plays as a frozen view.
//!
//! Pure over a parsed [`Demo`], so it also builds for the wasm web analyzer.

use crate::camera_data::{CameraFrames, camera_frames};
use dem::bit::BitSliceCast;
use dem::types::{Demo, EngineMessage, FrameData, MessageData, NetMessage};

/// `CL_UPDATE_BACKUP - 1`: the engine discards an entity packet whose from-frame
/// is this many packets back or more (`CL_FlushEntityPacket`, `hw.dll` .data
/// `0x1e3afcc`; see `docs/goldsrc_demo_salvage.md`).
const FLUSH_LIMIT: i32 = 63;

/// Which delta-compressed message lost its from-frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeltaKind {
    /// `SvcDeltaPacketEntities`.
    PacketEntities,
    /// `SvcClientData` with a delta-from frame.
    ClientData,
}

/// One delta the engine would decode against a frame the file does not have.
#[derive(Clone, Debug, PartialEq)]
pub struct BrokenDelta {
    /// Index of the frame holding the message, counting every frame of every
    /// directory entry from 0.
    pub frame_index: usize,
    /// The viewdemo clock (the last `SvcTime`) at that frame, in seconds; the
    /// frame's own time before the first `SvcTime`.
    pub time: f32,
    pub kind: DeltaKind,
    /// The frame's own `incoming_sequence`.
    pub incoming_seq: i32,
    /// The low 8 bits of the sequence the message is a delta from.
    pub delta_seq: u8,
}

/// What [`check_delta_continuity`] found.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IntegrityReport {
    /// Frames walked, every frame type counted.
    pub frames: usize,
    /// Delta-compressed messages checked (both kinds).
    pub delta_messages: usize,
    pub broken: Vec<BrokenDelta>,
    /// [`BrokenDelta::time`] of the first broken delta.
    pub first_broken_time: Option<f32>,
    /// The playback frames that carry a view (#920), counted by
    /// [`check_demo_bytes`]; [`check_delta_continuity`] leaves it empty.
    pub camera: CameraFrames,
}

impl IntegrityReport {
    pub fn is_ok(&self) -> bool {
        self.broken.is_empty() && !self.camera.is_missing()
    }

    fn push_broken(&mut self, broken: BrokenDelta) {
        self.first_broken_time.get_or_insert(broken.time);
        self.broken.push(broken);
    }

    /// One line for a person: "OK", or how many deltas are broken and where
    /// the first one is on the viewdemo clock, and whether the demo has no
    /// camera data.
    pub fn verdict(&self) -> String {
        let mut parts = Vec::new();
        let n = self.broken.len();
        if n > 0 {
            let what = if n == 1 {
                "1 delta built on a frame the file doesn't have".to_string()
            } else {
                format!("{n} deltas built on frames the file doesn't have")
            };
            parts.push(match self.first_broken_time {
                Some(t) => format!("{what}, first at {}", clock(t)),
                None => what,
            });
        }
        if self.camera.is_missing() {
            parts.push(format!(
                "no camera data: none of its {} playback frames carries a view, so it plays as a frozen camera",
                self.camera.network_frames
            ));
        }
        if parts.is_empty() {
            "OK".to_string()
        } else {
            parts.join("; ")
        }
    }
}

/// `m:ss`. A negative or non-finite time (a damaged `SvcTime`) shows as 0:00;
/// the cast saturates rather than panicking.
pub(crate) fn clock(seconds: f32) -> String {
    let total = if seconds.is_finite() && seconds > 0.0 {
        seconds as u32
    } else {
        0
    };
    format!("{}:{:02}", total / 60, total % 60)
}

/// How the engine would treat a delta, and whether the file can back it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Link {
    /// The from-frame is in the file and still inside the ring.
    Ok,
    /// Too far back: the engine discards an entity packet like this.
    Flushed,
    /// The engine accepts it, but the frame it means is not in the file.
    Absent,
}

/// The frames seen so far that a later delta could name, keyed by the low 8
/// bits of their sequence the way the wire format names them.
struct DeltaRing {
    frames: [Option<i32>; 256],
}

impl Default for DeltaRing {
    fn default() -> Self {
        Self {
            frames: [None; 256],
        }
    }
}

impl DeltaRing {
    /// Resolves an 8-bit from-sequence as the engine would at `current`.
    fn resolve(&self, current: i32, from: u8) -> Link {
        let gap = current.wrapping_sub(from as i32) & 0xFF;
        if gap >= FLUSH_LIMIT {
            return Link::Flushed;
        }
        // The frame the server meant: `gap` packets back, with these low bits.
        // A slot still holding a frame from an earlier lap of 256 is stale.
        let meant = current.wrapping_sub(gap);
        if self.frames[from as usize] == Some(meant) {
            Link::Ok
        } else {
            Link::Absent
        }
    }

    fn record(&mut self, sequence: i32) {
        self.frames[(sequence & 0xFF) as usize] = Some(sequence);
    }
}

/// Walks every network frame and reports each packet-entity or client-data
/// delta whose from-frame the file does not have (#41).
pub fn check_delta_continuity(demo: &Demo) -> IntegrityReport {
    let mut report = IntegrityReport::default();
    let mut entities = DeltaRing::default();
    let mut client_data = DeltaRing::default();
    let mut svc_time: Option<f32> = None;

    let frames = demo.directory.entries.iter().flat_map(|e| &e.frames);
    for (frame_index, frame) in frames.enumerate() {
        report.frames += 1;
        let FrameData::NetworkMessage(boxed) = &frame.frame_data else {
            continue;
        };
        let net = &boxed.1;
        let seq = net.sequence_info.incoming_sequence;
        let MessageData::Parsed(messages) = &net.messages else {
            continue;
        };
        for message in messages {
            let NetMessage::EngineMessage(engine) = message else {
                continue;
            };
            let at = |kind, delta_seq| BrokenDelta {
                frame_index,
                time: svc_time.unwrap_or(frame.time),
                kind,
                incoming_seq: seq,
                delta_seq,
            };
            match engine.as_ref() {
                EngineMessage::SvcTime(t) => svc_time = Some(t.time),
                EngineMessage::SvcPacketEntities(_) => entities.record(seq),
                EngineMessage::SvcDeltaPacketEntities(p) => {
                    report.delta_messages += 1;
                    let from = p.delta_sequence.to_u8();
                    match entities.resolve(seq, from) {
                        Link::Ok => entities.record(seq),
                        // Discarded whole: nothing enters the ring, and the
                        // world is missing rather than garbled.
                        Link::Flushed => {}
                        Link::Absent => {
                            report.push_broken(at(DeltaKind::PacketEntities, from));
                            // Recorded anyway, so the deltas built on this one
                            // link: one hole is reported once, not cascaded.
                            entities.record(seq);
                        }
                    }
                }
                // HLTV demos carry no delta mask here (`dem-patch` does not
                // parse their client data), so they are skipped by construction.
                EngineMessage::SvcClientData(c) => {
                    if let Some(mask) = &c.delta_update_mask {
                        report.delta_messages += 1;
                        let from = mask.to_u8();
                        // Unlike entity packets there is no discard rule for
                        // client data, so a from-frame past the ring decodes
                        // against stale state too. Healthy demos have neither.
                        if client_data.resolve(seq, from) != Link::Ok {
                            report.push_broken(at(DeltaKind::ClientData, from));
                        }
                    }
                    client_data.record(seq);
                }
                _ => {}
            }
        }
    }
    report
}

/// Parses a demo and runs [`check_delta_continuity`] on it, and counts its
/// camera frames. A demo that does not parse at all is a byte-level break,
/// which this reports as the error.
pub fn check_demo_bytes(bytes: &[u8]) -> Result<IntegrityReport, String> {
    let demo = dem::open_demo_from_bytes(bytes).map_err(|e| {
        format!(
            "cannot parse the demo: {}",
            parse_error_reason(&e.to_string())
        )
    })?;
    let mut report = check_delta_continuity(&demo);
    report.camera = camera_frames(&demo);
    crate::release_frames(demo);
    Ok(report)
}

/// The text a `dem-patch` parse error carries, rather than its byte dump.
/// `nom_fail` puts its message in the nom error's `input`, which `Display`
/// prints as a list of byte values; any other nom error's `input` is the rest
/// of the file, so that is cut short instead of printed whole.
pub fn parse_error_reason(error: &str) -> String {
    const MAX: usize = 200;
    let message = error
        .split_once("input: [")
        .and_then(|(_, rest)| rest.split_once(']'))
        .and_then(|(list, _)| {
            list.split(", ")
                .map(|n| n.trim().parse::<u8>())
                .collect::<Result<Vec<u8>, _>>()
                .ok()
        })
        .filter(|bytes| !bytes.is_empty() && bytes.len() <= MAX)
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .filter(|text| text.chars().all(|c| !c.is_control()));
    match message {
        Some(text) => text,
        None if error.chars().count() > MAX => {
            format!("{}...", error.chars().take(MAX).collect::<String>())
        }
        None => error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_finds_a_recent_frame() {
        let mut ring = DeltaRing::default();
        ring.record(1000);
        assert_eq!(ring.resolve(1003, 1000u32 as u8), Link::Ok);
    }

    #[test]
    fn resolve_reports_a_frame_the_file_does_not_have() {
        let mut ring = DeltaRing::default();
        ring.record(1000);
        assert_eq!(ring.resolve(1003, 1001u32 as u8), Link::Absent);
    }

    #[test]
    fn resolve_rejects_a_slot_left_over_from_an_earlier_lap() {
        // Same low byte, 256 packets earlier: the server meant 1250, which the
        // file does not have, and the slot still holds 994.
        let mut ring = DeltaRing::default();
        ring.record(994);
        assert_eq!(ring.resolve(1253, 1250u32 as u8), Link::Absent);
    }

    #[test]
    fn resolve_matches_the_engine_flush_predicate() {
        let mut ring = DeltaRing::default();
        ring.record(1000);
        // 62 back is kept, 63 back is flushed (`>= CL_UPDATE_BACKUP - 1`).
        assert_eq!(ring.resolve(1062, 1000u32 as u8), Link::Ok);
        assert_eq!(ring.resolve(1063, 1000u32 as u8), Link::Flushed);
        // The predicate works on the low byte: 255 back is a gap of 255.
        assert_eq!(ring.resolve(1255, 1000u32 as u8), Link::Flushed);
    }

    #[test]
    fn resolve_survives_negative_and_wrapping_sequences() {
        let mut ring = DeltaRing::default();
        ring.record(-1);
        assert_eq!(ring.resolve(1, 0xFF), Link::Ok);
        ring.record(i32::MAX);
        assert_eq!(ring.resolve(i32::MIN, 0xFF), Link::Ok);
    }

    #[test]
    fn verdict_says_ok_or_counts_and_places_the_first_break() {
        let mut report = IntegrityReport::default();
        assert_eq!(report.verdict(), "OK");
        let broken = BrokenDelta {
            frame_index: 7,
            time: 125.9,
            kind: DeltaKind::PacketEntities,
            incoming_seq: 10,
            delta_seq: 8,
        };
        report.broken.push(broken.clone());
        report.first_broken_time = Some(125.9);
        assert_eq!(
            report.verdict(),
            "1 delta built on a frame the file doesn't have, first at 2:05"
        );
        report.broken.push(broken);
        assert_eq!(
            report.verdict(),
            "2 deltas built on frames the file doesn't have, first at 2:05"
        );
    }

    #[test]
    fn a_demo_with_no_camera_data_is_not_ok_and_says_why() {
        let mut report = IntegrityReport {
            camera: CameraFrames {
                hltv: false,
                network_frames: 118_291,
                with_view: 0,
            },
            ..Default::default()
        };
        assert!(!report.is_ok());
        assert_eq!(
            report.verdict(),
            "no camera data: none of its 118291 playback frames carries a view, so it plays as a frozen camera"
        );
        report.first_broken_time = Some(1.0);
        report.broken.push(BrokenDelta {
            frame_index: 7,
            time: 1.0,
            kind: DeltaKind::ClientData,
            incoming_seq: 10,
            delta_seq: 8,
        });
        assert!(report.verdict().starts_with(
            "1 delta built on a frame the file doesn't have, first at 0:01; no camera data"
        ));
        report.camera.with_view = 1;
        report.broken.clear();
        assert!(report.is_ok());
    }

    #[test]
    fn clock_never_panics_on_a_damaged_time() {
        assert_eq!(clock(f32::NAN), "0:00");
        assert_eq!(clock(-3.0), "0:00");
        assert_eq!(clock(f32::INFINITY), "0:00");
        assert_eq!(clock(3599.99), "59:59");
        assert_eq!(clock(3600.0), "60:00");
    }

    #[test]
    fn parse_errors_read_as_text_not_byte_dumps() {
        let fail = "cannot parse demo: Parsing Failure: Error { input: [109, 101, 115, \
                    115, 97, 103, 101, 32, 116, 111, 111, 32, 108, 111, 110, 103], code: Fail }";
        assert_eq!(parse_error_reason(fail), "message too long");
        let dump = format!(
            "Parsing Error: Error {{ input: [{}], code: Eof }}",
            vec!["7"; 5000].join(", ")
        );
        let short = parse_error_reason(&dump);
        assert!(short.len() < 300 && short.ends_with("..."), "{short}");
        assert_eq!(parse_error_reason("plain"), "plain");
    }

    #[test]
    fn a_file_that_is_not_a_demo_is_an_error_not_a_panic() {
        let err = check_demo_bytes(b"not a demo at all").unwrap_err();
        assert!(err.starts_with("cannot parse the demo: "), "{err}");
    }

    fn fixture_demo() -> Demo {
        dem::open_demo_from_bytes(&crate::tests_fixture::fixture()).expect("ci_fixture.dem parses")
    }

    #[test]
    fn the_ci_fixture_is_ok() {
        let report = check_delta_continuity(&fixture_demo());
        assert!(report.delta_messages > 1000, "{report:?}");
        assert!(
            report.is_ok(),
            "{:?}",
            &report.broken[..5.min(report.broken.len())]
        );
        assert_eq!(report.verdict(), "OK");
        assert_eq!(report.first_broken_time, None);
    }

    /// The sequence and from-sequence of a frame's entity delta, if it has one.
    fn entity_delta(frame: &dem::types::Frame) -> Option<(i32, u8)> {
        let FrameData::NetworkMessage(boxed) = &frame.frame_data else {
            return None;
        };
        let MessageData::Parsed(messages) = &boxed.1.messages else {
            return None;
        };
        messages.iter().find_map(|m| match m {
            NetMessage::EngineMessage(e) => match e.as_ref() {
                EngineMessage::SvcDeltaPacketEntities(p) => Some((
                    boxed.1.sequence_info.incoming_sequence,
                    p.delta_sequence.to_u8(),
                )),
                _ => None,
            },
            _ => None,
        })
    }

    #[test]
    fn dropping_a_run_of_frames_breaks_the_chain_right_after_the_hole() {
        // What a copy that silently lost frames looks like: the frames after
        // the hole are intact but delta from frames that are gone.
        let mut demo = fixture_demo();
        let entries = &mut demo.directory.entries;
        let longest = (0..entries.len())
            .max_by_key(|&i| entries[i].frames.len())
            .expect("the fixture has entries");
        let entries_before: usize = entries[..longest].iter().map(|e| e.frames.len()).sum();
        let entry = &mut entries[longest];
        let hole_start = entry.frames.len() / 2;
        let removed: Vec<_> = entry.frames.drain(hole_start..hole_start + 300).collect();
        let removed_seqs: Vec<u8> = removed
            .iter()
            .filter_map(|f| match &f.frame_data {
                FrameData::NetworkMessage(b) => Some(b.1.sequence_info.incoming_sequence as u8),
                _ => None,
            })
            .collect();
        assert!(removed_seqs.len() > 10, "the hole holds network frames");

        // Where the first broken delta must be: the first entity delta after
        // the hole, which names a frame inside it.
        let (offset, (seq, from)) = entry.frames[hole_start..]
            .iter()
            .enumerate()
            .find_map(|(i, f)| entity_delta(f).map(|d| (i, d)))
            .expect("entity deltas after the hole");
        assert!(removed_seqs.contains(&from), "the delta names a lost frame");
        let expected_index = entries_before + hole_start + offset;

        let report = check_delta_continuity(&demo);
        let first = report.broken.first().expect("the hole is reported");
        // Nothing before the hole; the frame's client-data delta, written
        // ahead of its entities, may be reported first, at the same frame.
        assert!(
            (entries_before + hole_start..=expected_index).contains(&first.frame_index),
            "{first:?}, expected frame {expected_index}"
        );
        let entity = report
            .broken
            .iter()
            .find(|b| b.kind == DeltaKind::PacketEntities)
            .expect("an entity delta is reported");
        assert_eq!(entity.frame_index, expected_index);
        assert_eq!((entity.incoming_seq, entity.delta_seq), (seq, from));
        assert_eq!(report.first_broken_time, Some(first.time));
        assert!(
            report
                .verdict()
                .contains("built on frames the file doesn't have")
        );
    }

    /// The pair from #41: a thumb-drive copy of one POV demo that lost ~62% of
    /// its frames and still parses, and the good copy. Neither is in the tree
    /// (they are large); set `DOD_INTEGRITY_DEMOS` to the folder holding both,
    /// or keep them in the PRE install's `dod\`.
    #[test]
    #[ignore = "needs the damaged/good armory_h2 pair from #41"]
    fn the_damaged_copy_from_41_is_broken_and_the_good_one_is_ok() {
        const DAMAGED: &str = "wsod25-po_r3_sf-m00cat_ih_m3_armory_h2.dem";
        const GOOD: &str = "wsod25-po_r3_sf-m00cat_ih_m3_armory_h2-1.dem";
        let folder = std::env::var_os("DOD_INTEGRITY_DEMOS")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(
                    r"C:\Program Files (x86)\Steam\steamapps\common\Half-Life - PRE-Anniversary for Movies\dod",
                )
            });
        let read = |name: &str| {
            let path = folder.join(name);
            std::fs::read(&path).unwrap_or_else(|e| {
                panic!(
                    "{}: {e} (set DOD_INTEGRITY_DEMOS to the folder holding both demos)",
                    path.display()
                )
            })
        };
        let (damaged, good) = (read(DAMAGED), read(GOOD));

        let good = check_demo_bytes(&good).expect("the good copy parses");
        assert!(good.is_ok(), "{GOOD}: {}", good.verdict());
        let damaged = check_demo_bytes(&damaged).expect("the damaged copy parses");
        assert!(!damaged.is_ok(), "{DAMAGED}: {}", damaged.verdict());
        println!("{DAMAGED}: {}", damaged.verdict());
    }
}
