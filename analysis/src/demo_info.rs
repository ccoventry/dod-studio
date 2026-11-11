//! What a demo's header and frames say about the recording itself, as opposed
//! to the match inside it.

use crate::{camera_data, integrity};
use dem::{
    bit::BitSliceCast,
    types::{Demo, EngineMessage, FrameData, MessageData, NetMessage},
};

#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct DemoInfo {
    /// Version of the demo protocol used to encode the demo.
    pub demo_protocol: i32,

    /// Name of the map the demo was recorded on.
    pub map_name: String,

    /// Version of the network protocol used during the game.
    pub network_protocol: i32,

    /// Playback time of the demo in seconds.
    pub playback_time: f32,

    /// Total frames in the demo playback.
    pub playback_frames: i32,

    /// Game directory / mod name.
    pub game_directory: String,

    /// Type of demo: "HLTV" or "POV"
    pub demo_type: String,

    /// Map checksum / CRC.
    pub map_checksum: u32,

    /// The most entities any one snapshot carries (`svc_packetentities` or
    /// `svc_deltapacketentities`). The pre-Anniversary engine closes to the
    /// desktop at the first snapshot over 256 (#207), so this says before a
    /// capture whether a demo can play there. `None` in an analysis cached
    /// before it was counted.
    #[serde(default)]
    pub peak_packet_entities: Option<u32>,

    /// Where the demo stopped being read, when a frame partway through does
    /// not parse (#811): the analysis covers everything before it. `None` for
    /// a demo that reads to the end, and in an analysis cached before this
    /// was noted (a damaged demo had no analysis then).
    #[serde(default)]
    pub damage: Option<DemoDamage>,

    /// No playback frame carries a view (#920): a file rebuilt from server
    /// traffic, whose kill feed and scoreboard play while the camera stays
    /// frozen at the map origin. See [`camera_data`].
    #[serde(default)]
    pub no_camera_data: bool,
}

/// Where a damaged demo stopped being read, and why (#811).
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DemoDamage {
    /// The `viewdemo` clock (the last `SvcTime`) at the last frame read, in
    /// seconds: the analysis covers the demo up to here.
    pub read_up_to_secs: f32,
    /// Index of the first frame that does not parse, counting every frame of
    /// every directory entry from 0.
    pub frame_index: usize,
    /// Where that frame starts in the file.
    pub byte_offset: u64,
    /// What was not read, from there to the end of the frames.
    pub bytes_unread: u64,
    /// Why the frame does not parse, in words.
    pub reason: String,
}

impl DemoDamage {
    pub(crate) fn new(demo: &Demo, damage: dem::demo_parser::ParseDamage) -> Self {
        Self {
            read_up_to_secs: last_svc_time(demo)
                .or(damage.last_frame_time)
                .filter(|t| t.is_finite() && *t >= 0.0)
                .unwrap_or(0.0),
            frame_index: damage.frame_index,
            byte_offset: damage.byte_offset as u64,
            bytes_unread: damage.bytes_unread as u64,
            reason: damage.reason,
        }
    }

    /// "read up to 2:05", on the `viewdemo` clock.
    pub fn read_up_to_text(&self) -> String {
        format!("read up to {}", integrity::clock(self.read_up_to_secs))
    }

    /// One line for a person: "Damaged demo: read up to 2:05, the rest
    /// (14.2 MB) does not parse."
    pub fn summary(&self) -> String {
        format!(
            "Damaged demo: {}, the rest ({}) does not parse.",
            self.read_up_to_text(),
            size_text(self.bytes_unread)
        )
    }
}

/// A byte count as "14.2 MB" or "312 KB".
fn size_text(bytes: u64) -> String {
    if bytes >= 1 << 20 {
        format!("{:.1} MB", bytes as f64 / (1u64 << 20) as f64)
    } else {
        format!("{} KB", bytes.div_ceil(1 << 10))
    }
}

/// The last `SvcTime` in the demo: the `viewdemo` clock where it ends.
fn last_svc_time(demo: &Demo) -> Option<f32> {
    let frames = demo
        .directory
        .entries
        .iter()
        .rev()
        .flat_map(|e| e.frames.iter().rev());
    for frame in frames {
        let FrameData::NetworkMessage(box_type) = &frame.frame_data else {
            continue;
        };
        let MessageData::Parsed(msgs) = &box_type.1.messages else {
            continue;
        };
        let time = msgs.iter().rev().find_map(|m| match m {
            NetMessage::EngineMessage(e) => match &**e {
                EngineMessage::SvcTime(t) => Some(t.time),
                _ => None,
            },
            _ => None,
        });
        if time.is_some() {
            return time;
        }
    }
    None
}

/// The most entities in any one snapshot of `demo` (#207).
fn peak_packet_entities(demo: &Demo) -> u32 {
    let mut peak = 0;
    for entry in &demo.directory.entries {
        for frame in &entry.frames {
            let FrameData::NetworkMessage(box_type) = &frame.frame_data else {
                continue;
            };
            let MessageData::Parsed(msgs) = &box_type.1.messages else {
                continue;
            };
            for msg in msgs {
                let NetMessage::EngineMessage(eng_msg) = msg else {
                    continue;
                };
                let count = match &**eng_msg {
                    EngineMessage::SvcPacketEntities(pe) => pe.entity_count.to_u32(),
                    EngineMessage::SvcDeltaPacketEntities(pe) => pe.entity_count.to_u32(),
                    _ => continue,
                };
                peak = peak.max(count);
            }
        }
    }
    peak
}

impl From<&Demo> for DemoInfo {
    fn from(value: &Demo) -> Self {
        let map_name = value
            .header
            .map_name
            .to_str()
            .map(|s| s.trim_end_matches('\x00'))
            .unwrap_or("unknown")
            .to_string();

        let game_directory = value
            .header
            .game_directory
            .to_str()
            .map(|s| s.trim_end_matches('\x00'))
            .unwrap_or("unknown")
            .to_string();

        let playback_time = value
            .directory
            .entries
            .iter()
            .map(|e| e.track_time)
            .sum::<f32>();

        let playback_frames = value
            .directory
            .entries
            .iter()
            .map(|e| e.frames.len() as i32)
            .sum::<i32>();

        let demo_type = if is_hltv_demo(value) {
            "HLTV".to_string()
        } else {
            "POV".to_string()
        };

        Self {
            demo_protocol: value.header.demo_protocol,
            map_name,
            network_protocol: value.header.network_protocol,
            playback_time,
            playback_frames,
            game_directory,
            demo_type,
            map_checksum: value.header.map_checksum,
            peak_packet_entities: Some(peak_packet_entities(value)),
            damage: None,
            no_camera_data: camera_data::camera_frames(value).is_missing(),
        }
    }
}

/// Whether `demo` is an HLTV proxy's recording: it carries a message only
/// one has ([`is_hltv_message`]).
pub(crate) fn is_hltv_demo(demo: &Demo) -> bool {
    demo.directory.entries.iter().any(|entry| {
        entry.frames.iter().any(|frame| {
            let FrameData::NetworkMessage(box_type) = &frame.frame_data else {
                return false;
            };
            let MessageData::Parsed(msgs) = &box_type.1.messages else {
                return false;
            };
            msgs.iter().any(
                |msg| matches!(msg, NetMessage::EngineMessage(eng_msg) if is_hltv_message(eng_msg)),
            )
        })
    })
}

/// A message only an HLTV proxy's recording carries: `svc_hltv`, or a
/// director command that drives the auto-director's camera.
///
/// Not *any* `svc_director` (#351): DoD Studio injects its own into every
/// patched preview of a POV demo, and those read as HLTV. Its injections are
/// only DRC_CMD_MESSAGE (0x06) and DRC_CMD_STUFFTEXT (0x0A) -- bookmarks and
/// console commands -- and neither moves a camera, so they don't count. The
/// same rule was worked out in `examples/hltv_shot_gap_probe.rs` (3f3663e).
pub(crate) fn is_hltv_message(message: &EngineMessage) -> bool {
    const DRC_CMD_MESSAGE: u8 = 0x06;
    const DRC_CMD_STUFFTEXT: u8 = 0x0A;
    match message {
        EngineMessage::SvcHltv(_) => true,
        EngineMessage::SvcDirector(d) => {
            d.command != DRC_CMD_MESSAGE && d.command != DRC_CMD_STUFFTEXT
        }
        _ => false,
    }
}
