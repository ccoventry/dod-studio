//! Rebuilding the client's state at each join (#15): a full entity snapshot,
//! and a ramp of no-op frames from the last frame before the hole up to it.
//! Moved from the `snapshot_inject` probe.
//!
//! Every choice below was a live crash first; `docs/goldsrc_demo_salvage.md`
//! ("What each live crash taught") has each one's evidence.

use dem::types::{
    ClientDataWeaponData, Delta, Demo, EngineMessage, EntityState, Frame, FrameData, MessageData,
    NetMessage, NetworkMessage, SvcClientData, SvcDeltaPacketEntities, SvcPacketEntities, SvcTime,
    UserMessage,
};

use super::join_state::{ClientState, JoinState};
use super::joins::Join;

/// A join's `incoming_sequence` gap is ramped over at most this many frames.
/// Real holes (gaps of 582 to 2,658) ramp whole; the cap keeps a splice
/// artefact with a gap of 100,000+ from becoming a 100,000-frame ramp.
const MAX_RAMP_FRAMES: usize = 3000;
/// The widest index gap increment/difference encoding can carry (6 bits).
const MAX_INDEX_DIFFERENCE: u32 = 63;

/// What the repair did at one join.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct JoinReport {
    /// Entities in the injected snapshot (entity 0 not counted: it is never
    /// listed).
    pub snapshot_entities: usize,
    /// Entities the map's baseline names that no real packet before the join
    /// listed, so the snapshot leaves them out.
    pub left_out: usize,
    /// No-op frames ramping the sequence up to the join.
    pub ramp_frames: usize,
    /// Whether a delta packet after the join took the snapshot. `false` when
    /// no frame carrying entity data followed it.
    pub snapshot_placed: bool,
}

/// The full `SvcPacketEntities` for `state`.
///
/// Encoded as the engine's own full snapshots are, measured over ~1.65M real
/// entity observations: entity 0 (the world) is never listed (it appears in
/// none of them; the client keeps its signon baseline), and indices are
/// increment/difference from 0, absolute only for a gap too wide for 6 bits
/// (absolute appears in 0 of 2,337 entities of real full snapshots). Either
/// one done otherwise is `svc_bad`.
fn full_snapshot(state: &JoinState) -> SvcPacketEntities {
    let mut running: u32 = 0;
    let mut states = Vec::with_capacity(state.entities.len());
    for (index, e) in state.entities.iter().filter(|(i, _)| **i != 0) {
        let index = *index as u32;
        // Ascending keys with entity 0 left out: the gap is always > 0.
        let gap = index - running;
        let (increment, is_absolute, absolute, difference) = if gap == 1 {
            (true, None, None, None)
        } else if gap <= MAX_INDEX_DIFFERENCE {
            (false, Some(false), None, Some(dem::nbit_num!(gap, 6)))
        } else {
            (false, Some(true), Some(dem::nbit_num!(index, 11)), None)
        };
        running = index;
        states.push(EntityState {
            entity_index: index as u16,
            increment_entity_number: increment,
            is_absolute_entity_index: is_absolute,
            absolute_entity_index: absolute,
            entity_index_difference: difference,
            has_custom_delta: e.custom,
            has_baseline_index: false,
            baseline_index: None,
            delta: e.fields.clone(),
        });
    }
    SvcPacketEntities {
        entity_count: dem::nbit_num!(states.len() as u32, 16),
        entity_states: states,
    }
}

/// Where a ramp starts and ends.
struct RampSpan<'a> {
    join: &'a Join,
    /// The first network frame after the hole: the ramp's template and end.
    template: &'a Frame,
    entity_count: u32,
    client: &'a ClientState,
}

/// The no-op frames between the last frame before a hole and the first after.
///
/// An instant jump of thousands in `incoming_sequence` and tens of seconds in
/// time is nothing a real recording produces: a ~30 s stall under `playdemo`,
/// `svc_bad` under `viewdemo`. So the gap is walked one sequence at a time,
/// `delta_sequence` in lockstep (the flush gap stays what it was), the outer
/// frame time interpolated (`info.timestamp` is 0.0 on every real frame).
fn ramp(span: RampSpan) -> Vec<Frame> {
    let FrameData::NetworkMessage(template) = &span.template.frame_data else {
        return Vec::new();
    };
    let start_sequence = span.join.sequence_before;
    let end_sequence = template.1.sequence_info.incoming_sequence;
    let sequence_gap = (end_sequence as i64 - start_sequence as i64).max(0);
    let steps = (sequence_gap.saturating_sub(1) as usize).min(MAX_RAMP_FRAMES);
    // Every ramp frame carries `svc_clientdata`: HLTV's `Core.dll` keeps a
    // per-player client-data ring keyed by frame number, and starving it is
    // `World::ParseClientData: couldn't uncompress delta frame`. The structure
    // is cloned from the template, the content left empty.
    let clientdata_template = match &template.1.messages {
        MessageData::Parsed(msgs) => msgs.iter().find_map(|m| match m {
            NetMessage::EngineMessage(em) => match &**em {
                EngineMessage::SvcClientData(cd) => Some(cd.clone()),
                _ => None,
            },
            _ => None,
        }),
        _ => None,
    };
    let start_time = span.join.time_before;
    let end_time = span.template.time;
    (1..=steps)
        .map(|i| {
            let fraction = i as f64 / (steps + 1) as f64;
            let advance = (fraction * sequence_gap as f64).round() as i64;
            let time = start_time + (end_time - start_time) * fraction as f32;
            let delta_sequence = (span.join.delta_sequence_before as i64 + advance) as u32 & 0xff;
            let mut sequence_info = template.1.sequence_info.clone();
            sequence_info.incoming_sequence = (start_sequence as i64 + advance) as i32;
            let mut msgs = vec![NetMessage::EngineMessage(Box::new(EngineMessage::SvcTime(
                SvcTime { time },
            )))];
            let first = i == 1;
            if let Some(cd) = &clientdata_template {
                msgs.push(ramp_client_data(cd, span.client, first));
            }
            // `CurWeapon` drives which weapon's fire sound the client
            // predicts, and is not part of client data: replayed once.
            if first && let Some((id, data)) = &span.client.cur_weapon {
                msgs.push(NetMessage::UserMessage(UserMessage {
                    id: *id,
                    name: b"CurWeapon".to_vec(),
                    data: data.clone(),
                }));
            }
            msgs.push(NetMessage::EngineMessage(Box::new(
                EngineMessage::SvcDeltaPacketEntities(SvcDeltaPacketEntities {
                    entity_count: dem::nbit_num!(span.entity_count, 16),
                    delta_sequence: dem::nbit_num!(delta_sequence, 8),
                    entity_states: Vec::new(),
                }),
            )));
            Frame {
                time,
                // Unique and increasing: the client-data ring above is keyed
                // by it, and a shared ordinal collides in it.
                frame: span.template.frame.wrapping_add(i as i32),
                frame_data: FrameData::NetworkMessage(Box::new((
                    template.0.clone(),
                    NetworkMessage {
                        info: template.1.info.clone(),
                        sequence_info,
                        message_length: 0,
                        messages: MessageData::Parsed(msgs),
                        source_span: None,
                    },
                ))),
            }
        })
        .collect()
}

/// A ramp frame's `svc_clientdata`. The first carries the player's replayed
/// state once, as the snapshot does for entities; empty content on every
/// frame left the client predicting the old weapon's gunfire for seconds.
/// The rest say "no change".
fn ramp_client_data(template: &SvcClientData, client: &ClientState, first: bool) -> NetMessage {
    let (client_data, weapon_data) = if first {
        let weapons = (!client.weapons.is_empty()).then(|| {
            client
                .weapons
                .iter()
                .map(|(index, fields)| ClientDataWeaponData {
                    weapon_index: dem::nbit_num!(*index, 6),
                    weapon_data: fields.clone(),
                })
                .collect()
        });
        (client.fields.clone(), weapons)
    } else {
        (Delta::new(), None)
    };
    NetMessage::EngineMessage(Box::new(EngineMessage::SvcClientData(SvcClientData {
        has_delta_update_mask: template.has_delta_update_mask,
        delta_update_mask: template.delta_update_mask.clone(),
        client_data,
        weapon_data,
    })))
}

/// The most frames a ramp is moved back over: the non-network frames that
/// open the stretch after a hole, ahead of its first network frame.
const MAX_HEAD_FRAMES: usize = 32;

/// Where a ramp goes in `frames` (everything before the join so far): before
/// the frames timed after `ramp_middle`, the ramp's middle frame, which are
/// the ones from after the hole. The stretch after
/// a hole opens with a few client-data and demo-buffer frames before its first
/// network frame; a ramp after them would hold playback on their time for the
/// whole hole and then play every ramp frame at once. Before them, frame times
/// rise through the join, as in the R&D builds, which sorted every frame by
/// time (#15). Never moved past a network frame.
fn ramp_position(frames: &[Frame], ramp_middle: Option<f32>) -> usize {
    let Some(middle) = ramp_middle else {
        return frames.len();
    };
    let head = frames
        .iter()
        .rev()
        .take(MAX_HEAD_FRAMES)
        .take_while(|f| f.time > middle && !matches!(f.frame_data, FrameData::NetworkMessage(_)))
        .count();
    frames.len() - head
}

fn is_delta_packet(m: &NetMessage) -> bool {
    matches!(m, NetMessage::EngineMessage(em) if matches!(**em, EngineMessage::SvcDeltaPacketEntities(_)))
}

/// Puts a ramp in front of each join and the join's snapshot in place of the
/// first entity delta after it. Replaced, not put in front: two messages in
/// one frame share one `incoming_sequence` and one slot of the client's frame
/// ring, and the delta parsed second would land on a slot holding a frame from
/// before the hole, the state this exists to stop the client using.
pub(crate) fn inject(demo: &mut Demo, joins: &[Join], states: &[JoinState]) -> Vec<JoinReport> {
    let snapshots: Vec<SvcPacketEntities> = states.iter().map(full_snapshot).collect();
    let mut reports: Vec<JoinReport> = snapshots
        .iter()
        .zip(states)
        .map(|(s, state)| JoinReport {
            snapshot_entities: s.entity_states.len(),
            left_out: state.left_out,
            ..Default::default()
        })
        .collect();
    let mut ramped = 0usize;
    let mut placed = 0usize;
    let mut index = 0usize;
    for entry in demo.directory.entries.iter_mut().skip(1) {
        let mut frames: Vec<Frame> = Vec::with_capacity(entry.frames.len() + 64);
        for mut frame in entry.frames.drain(..) {
            let here = index;
            index += 1;
            if ramped < joins.len() && here == joins[ramped].frame_index {
                let ramp_frames = ramp(RampSpan {
                    join: &joins[ramped],
                    template: &frame,
                    entity_count: snapshots[ramped].entity_states.len() as u32,
                    client: &states[ramped].client,
                });
                reports[ramped].ramp_frames = ramp_frames.len();
                let at = ramp_position(&frames, ramp_frames.get(ramp_frames.len() / 2).map(|f| f.time));
                frames.splice(at..at, ramp_frames);
                ramped += 1;
            }
            if placed < joins.len()
                && here > joins[placed].frame_index
                && let FrameData::NetworkMessage(bt) = &mut frame.frame_data
                && let MessageData::Parsed(msgs) = &bt.1.messages
                && let Some(at) = msgs.iter().position(is_delta_packet)
                && let MessageData::Parsed(msgs) = bt.1.messages_mut()
            {
                msgs[at] = NetMessage::EngineMessage(Box::new(EngineMessage::SvcPacketEntities(
                    snapshots[placed].clone(),
                )));
                reports[placed].snapshot_placed = true;
                placed += 1;
            }
            frames.push(frame);
        }
        entry.frames = frames;
    }
    reports
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repair::join_state::EntityFields;
    use dem::bit::BitSliceCast;

    fn state_with(indices: &[u16]) -> JoinState {
        let mut state = JoinState::default();
        for &i in indices {
            state.entities.insert(i, EntityFields::default());
        }
        state
    }

    fn frame_at(time: f32, data: FrameData) -> Frame {
        Frame {
            time,
            frame: 0,
            frame_data: data,
        }
    }

    #[test]
    fn a_ramp_goes_before_the_frames_that_open_the_stretch_after_the_hole() {
        let frames = [
            frame_at(10.0, FrameData::NextSection),
            frame_at(10.1, FrameData::NextSection),
            frame_at(40.0, FrameData::NextSection),
            frame_at(40.1, FrameData::NextSection),
        ];
        assert_eq!(ramp_position(&frames, Some(25.0)), 2);
        assert_eq!(ramp_position(&frames, None), 4);
    }

    #[test]
    fn a_snapshot_never_lists_the_world() {
        let snapshot = full_snapshot(&state_with(&[0, 1, 2]));
        let listed: Vec<u16> = snapshot
            .entity_states
            .iter()
            .map(|e| e.entity_index)
            .collect();
        assert_eq!(listed, [1, 2]);
        assert_eq!(snapshot.entity_count.to_u32(), 2);
    }

    #[test]
    fn a_snapshot_encodes_indices_as_increments_and_differences() {
        let snapshot = full_snapshot(&state_with(&[1, 5, 200]));
        let e = &snapshot.entity_states;
        assert!(e[0].increment_entity_number);
        assert_eq!(e[1].is_absolute_entity_index, Some(false));
        assert_eq!(
            e[1].entity_index_difference.as_ref().map(|d| d.to_u32()),
            Some(4)
        );
        // 195 apart: too wide for 6 bits, so absolute.
        assert_eq!(e[2].is_absolute_entity_index, Some(true));
        assert_eq!(
            e[2].absolute_entity_index.as_ref().map(|d| d.to_u32()),
            Some(200)
        );
    }
}
