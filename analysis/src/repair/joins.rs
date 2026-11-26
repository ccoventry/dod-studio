//! Where a spliced demo's joins are (#15): the network frames at which the
//! recording resumes after a hole.
//!
//! A join is found by the jump in `incoming_sequence`, never by frame time.
//! Frame time is not monotonic across a recording (`m1_h2` resets to 0.00 s
//! inside its intact prefix), and "largest forward time jump" then points at
//! the start of playback. `incoming_sequence` advances by one per network
//! frame and jumps by hundreds or thousands at a splice, whatever the clock
//! does.

use dem::bit::BitSliceCast;
use dem::types::{Demo, EngineMessage, FrameData, MessageData, NetMessage};

/// A jump in `incoming_sequence` larger than this between two network frames
/// is a splice; a real recording advances it by one.
const JOIN_SEQUENCE_JUMP: i64 = 100;

/// One join, located in the playback frames (every entry after the signon,
/// counted from 0).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Join {
    /// The first network frame after the hole.
    pub frame_index: usize,
    /// The time of the last network frame before it, seconds.
    pub time_before: f32,
    /// The last `incoming_sequence` before the hole.
    pub sequence_before: i32,
    /// The `delta_sequence` of the last entity delta before the hole.
    pub delta_sequence_before: u32,
    /// The server clock (`svc_time`) of the last network frame before the
    /// hole that carried one. It runs ahead of the frame time by however long
    /// the server had been up when the recording started (10.6 s in `m1_h2`).
    pub server_time_before: Option<f32>,
}

/// Every join in `demo`, in order.
pub(crate) fn find_joins(demo: &Demo) -> Vec<Join> {
    let mut joins = Vec::new();
    let mut prev: Option<(i32, f32)> = None;
    let mut last_delta_sequence = 0u32;
    let mut last_server_time = None;
    let frames = demo
        .directory
        .entries
        .iter()
        .skip(1)
        .flat_map(|e| e.frames.iter());
    for (index, frame) in frames.enumerate() {
        let FrameData::NetworkMessage(bt) = &frame.frame_data else {
            continue;
        };
        let sequence = bt.1.sequence_info.incoming_sequence;
        if let Some((prev_sequence, prev_time)) = prev
            && (sequence as i64 - prev_sequence as i64).abs() > JOIN_SEQUENCE_JUMP
        {
            joins.push(Join {
                frame_index: index,
                time_before: prev_time,
                sequence_before: prev_sequence,
                delta_sequence_before: last_delta_sequence,
                server_time_before: last_server_time,
            });
        }
        prev = Some((sequence, frame.time));
        if let MessageData::Parsed(msgs) = &bt.1.messages {
            for m in msgs {
                if let NetMessage::EngineMessage(em) = m {
                    match &**em {
                        EngineMessage::SvcDeltaPacketEntities(pe) => {
                            last_delta_sequence = pe.delta_sequence.to_u32();
                        }
                        EngineMessage::SvcTime(t) => last_server_time = Some(t.time),
                        _ => {}
                    }
                }
            }
        }
    }
    joins
}
