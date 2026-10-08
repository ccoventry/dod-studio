// patch/decal_strip/survey.rs
// The read-only walk over a parsed demo: every decal the engine accepted, the
// spawn, the floor under the player, and the cameras inside each clip.

use super::geometry::{decal_position, decal_texture_index, distance, is_world_decal};
use super::{DecalCleanOptions, MIN_POSITION_SPACING};
use dem::types::{EngineMessage, FrameData, MessageData, NetMessage, TempEntity};

/// TE_GUNSHOTDECAL — the bullet-hole message. Not emitted (it plays a ricochet
/// sound), but its texture index is the one worth borrowing: a small hole
/// rather than the large scorch a TE_WORLDDECAL index usually denotes.
const TE_GUNSHOTDECAL: u8 = 109;

/// Everything the survey pass needs to pick a flush coordinate and place bursts.
pub(in crate::patch) struct Survey {
    /// Positions of decals the engine actually accepted during playback.
    pub(in crate::patch) harvested: Vec<[f32; 3]>,
    /// The subset of those stamped on world geometry rather than on a brush
    /// entity. Only these are durable enough to contribute to `decal_atlas` —
    /// see `is_world_decal`.
    pub(in crate::patch) world_harvested: Vec<[f32; 3]>,
    pub(in crate::patch) texture_index: Option<u8>,
    /// Earliest camera eye position seen in playback.
    pub(in crate::patch) spawn_eye: Option<[f32; 3]>,
    /// Player origin once the spawn has settled onto solid ground — see the
    /// grounded-run detection below. This, not `spawn_eye`, is the spawn
    /// reference worth trusting.
    pub(in crate::patch) grounded_origin: Option<[f32; 3]>,
    /// Floor points sampled beneath the player wherever they stood on solid
    /// ground. A sweep needs far more distinct positions than a demo has
    /// decals, and every one of these is a surface the demo proves exists —
    /// the player was standing on it. Walking naturally spreads them out, so
    /// they satisfy the no-overlap requirement for free.
    pub(in crate::patch) floor_candidates: Vec<[f32; 3]>,
    /// Camera (eye position, forward vector) pairs sampled inside the capture
    /// windows. The forward vector is what makes a real "is it on screen?"
    /// test possible, rather than distance alone.
    pub(in crate::patch) window_cameras: Vec<([f32; 3], [f32; 3])>,
}

/// Running frame ordinal, matching `engine.rs`'s `frame_counter`: every frame
/// record in file order, across all directory entries, 1-based.
///
/// This — NOT `Frame::frame` — is the tick space the rest of the patch pipeline
/// schedules in. `Frame::frame` is the engine's tick, and several frame records
/// share one of those (a DemoBuffer, a ClientData and a NetworkMessage per
/// tick), so the two spaces differ by roughly 2.6x on a real demo. Mixing them
/// silently targets the wrong frames.
pub(in crate::patch) fn frame_ordinals(demo: &dem::types::Demo) -> Vec<(usize, usize, i32)> {
    let mut out = Vec::new();
    let mut ordinal = 0i32;
    for (entry_idx, entry) in demo.directory.entries.iter().enumerate() {
        for frame_idx in 0..entry.frames.len() {
            ordinal += 1;
            out.push((entry_idx, frame_idx, ordinal));
        }
    }
    out
}

pub(super) fn in_window(ordinal: i32, keep_windows: &[(i32, i32)]) -> bool {
    keep_windows
        .iter()
        .any(|&(s, e)| ordinal >= s && ordinal <= e)
}

/// How long before a clip the sweep should finish, in seconds of demo time.
///
/// Two seconds rather than the ~0.6 the old flat frame count worked out to. It
/// is comfortably more margin than the value that passed in game, comfortably
/// inside a default pre-roll so the burst cannot reach back into an earlier
/// clip, and it gives the engine room to ingest a maximum sweep's 4,112 decals
/// before the first recorded frame rather than finishing just in time.
pub const DEFAULT_LEAD_SECONDS: f32 = 2.0;

/// When the timestamps cannot answer, fall back to the frame count this used to
/// be — the same shape of fallback `builder::find_tick_backwards` uses when
/// `frame_times` is truncated. Never worse than the behaviour that shipped.
pub(super) const FALLBACK_LEAD_FRAMES: i32 = 300;

/// The frame ordinal that sits `lead_seconds` of demo time before `window_start`.
///
/// Walks the frames' own timestamps rather than dividing by an average rate.
/// Confined to the directory entry the window starts in, because times restart
/// per entry — a LOADING entry's near-zero timestamps would otherwise satisfy
/// any target and drag the deadline to the front of the demo.
///
/// `None` when the entry's timestamps cannot answer, leaving the caller to fall
/// back to a frame count.
pub(super) fn deadline_before(
    window_start: i32,
    lead_seconds: f32,
    frames: &[(usize, usize, i32)],
    times: &[f32],
) -> Option<i32> {
    if lead_seconds <= 0.0 {
        return Some(window_start);
    }
    let anchor_slot = frames.iter().position(|&(_, _, ord)| ord == window_start)?;
    let (anchor_entry, _, _) = frames[anchor_slot];
    let anchor_time = *times.get(anchor_slot)?;
    if !anchor_time.is_finite() {
        return None;
    }
    let target = anchor_time - lead_seconds;

    // Walk back to the last frame at or before the target, in this entry only.
    let mut slot = anchor_slot;
    while slot > 0 {
        slot -= 1;
        let (entry, _, ord) = frames[slot];
        if entry != anchor_entry {
            return None;
        }
        if times[slot] <= target {
            return Some(ord);
        }
    }
    None
}

pub(in crate::patch) fn survey(
    demo: &dem::types::Demo,
    keep_windows: &[(i32, i32)],
    opts: &DecalCleanOptions,
) -> Survey {
    let mut out = Survey {
        harvested: Vec::new(),
        world_harvested: Vec::new(),
        texture_index: None,
        spawn_eye: None,
        grounded_origin: None,
        floor_candidates: Vec::new(),
        window_cameras: Vec::new(),
    };
    // A world decal's index is read straight from the byte after the coords,
    // so prefer harvesting from the same message type we intend to emit.
    let mut fallback_index: Option<u8> = None;

    // Spawn points are not guaranteed to sit flush on the floor — many maps
    // place them slightly above it and let the player drop. Sampling at the
    // spawn instant can therefore return a mid-air position, whose "floor"
    // would be open space. So wait for a run of consecutive frames that report
    // on_ground with a stable Z before trusting the position.
    let mut grounded_run = 0usize;
    let mut last_z: Option<f32> = None;
    let mut camera_stride = 0usize;

    let mut ordinal = 0i32;
    for entry in &demo.directory.entries {
        for frame in &entry.frames {
            ordinal += 1;
            let FrameData::NetworkMessage(net_msg_box) = &frame.frame_data else {
                continue;
            };

            let rp = &net_msg_box.1.info.refparams;
            let origin = &rp.view_origin;
            if origin.len() >= 3 {
                let pos = [origin[0], origin[1], origin[2]];
                if pos != [0.0, 0.0, 0.0] {
                    if out.spawn_eye.is_none() {
                        out.spawn_eye = Some(pos);
                    }

                    // Floor beneath the player wherever they are actually
                    // standing. Sampled sparsely and only when far enough from
                    // the last sample to be a genuinely separate spot.
                    if rp.on_ground != 0 {
                        let sim = &rp.sim_org;
                        let origin = if sim.len() >= 3 && sim[2] != 0.0 {
                            [sim[0], sim[1], sim[2]]
                        } else {
                            let vh = rp.view_height.get(2).copied().unwrap_or(28.0);
                            [pos[0], pos[1], pos[2] - vh]
                        };
                        let floor = [origin[0], origin[1], origin[2] - opts.floor_drop];
                        let far_enough = out
                            .floor_candidates
                            .last()
                            .map(|last| distance(&floor, last) >= MIN_POSITION_SPACING)
                            .unwrap_or(true);
                        if far_enough {
                            out.floor_candidates.push(floor);
                        }
                    }

                    if out.grounded_origin.is_none() {
                        let settled = last_z.map(|z| (pos[2] - z).abs() < 2.0).unwrap_or(false);
                        if rp.on_ground != 0 && settled {
                            grounded_run += 1;
                            if grounded_run >= opts.grounded_settle_frames {
                                // Prefer the engine's own player origin; fall
                                // back to backing the view offset out of the
                                // eye position when sim_org isn't populated.
                                let sim = &rp.sim_org;
                                let resolved = if sim.len() >= 3 && sim[2] != 0.0 {
                                    [sim[0], sim[1], sim[2]]
                                } else {
                                    let view_z = rp.view_height.get(2).copied().unwrap_or(28.0);
                                    [pos[0], pos[1], pos[2] - view_z]
                                };
                                out.grounded_origin = Some(resolved);
                            }
                        } else {
                            grounded_run = 0;
                        }
                        last_z = Some(pos[2]);
                    }

                    // Subsampled: every candidate decal is scored against this
                    // whole set, and consecutive frames sit a few units apart,
                    // so a stride costs no meaningful accuracy.
                    if in_window(ordinal, keep_windows) {
                        camera_stride += 1;
                        if opts.collect_diagnostics || camera_stride.is_multiple_of(4) {
                            let fwd = &rp.forward;
                            if fwd.len() >= 3 {
                                out.window_cameras.push((pos, [fwd[0], fwd[1], fwd[2]]));
                            }
                        }
                    }
                }
            }

            let MessageData::Parsed(messages) = &net_msg_box.1.messages else {
                continue;
            };
            for msg in messages {
                let NetMessage::EngineMessage(eng) = msg else {
                    continue;
                };
                let EngineMessage::SvcTempEntity(te) = eng.as_ref() else {
                    continue;
                };
                // TE_BSPDECAL carries its fields in a different shape to the
                // rest (a 16-bit texture index rather than 8-bit, ahead of the
                // entity index), so it is unpacked separately instead of being
                // forced through the shared byte-offset table. It was already
                // in the strip set; leaving it out of the harvest set meant a
                // demo whose only decals were BSP decals offered no anchor.
                if let TempEntity::TeBspDecal(d) = &te.entity {
                    if let Some(pos) = decal_position(&d.unknown1) {
                        out.harvested.push(pos);
                        if is_world_decal(te.entity_type, &d.unknown1) {
                            out.world_harvested.push(pos);
                        }
                    }
                    if d.unknown1.len() >= 8 {
                        let raw = i16::from_le_bytes([d.unknown1[6], d.unknown1[7]]);
                        // The emitted TE_WORLDDECAL writes this index as one
                        // byte, so an index that doesn't fit is unusable.
                        if (0..=255).contains(&raw) {
                            fallback_index.get_or_insert(raw as u8);
                        }
                    }
                    continue;
                }

                // A spray marks a proven surface just as well as a bullet hole,
                // so its position is worth harvesting — but its texture index
                // never is: that index is somebody's logo, and a stack of those
                // is the most conspicuous thing that could be left on a wall.
                // Its layout also differs (a leading player index before the
                // coordinates), hence the separate arm.
                if let TempEntity::TePlayerDecal(p) = &te.entity {
                    if p.len() >= 7
                        && let Some(pos) = decal_position(&p[1..7])
                    {
                        out.harvested.push(pos);
                    }
                    continue;
                }

                let payload: &[u8] = match &te.entity {
                    TempEntity::TeWorldDecal(p)
                    | TempEntity::TeWorldDecalHigh(p)
                    | TempEntity::TeGunshotDecal(p)
                    | TempEntity::TeDecal(p)
                    | TempEntity::TeDecalHigh(p) => p,
                    _ => continue,
                };
                if let Some(pos) = decal_position(payload) {
                    out.harvested.push(pos);
                    if is_world_decal(te.entity_type, payload) {
                        out.world_harvested.push(pos);
                    }
                }
                if let Some(idx) = decal_texture_index(te.entity_type, payload) {
                    // Prefer a bullet-hole texture. TE_GUNSHOTDECAL indices are
                    // small and unremarkable; TE_WORLDDECAL ones are typically
                    // grenade scorches — large, dark and immediately obvious.
                    // Flush decals exist to be unnoticed, so the small mark
                    // wins and the scorch is only a fallback.
                    if te.entity_type == TE_GUNSHOTDECAL {
                        out.texture_index.get_or_insert(idx);
                    } else {
                        fallback_index.get_or_insert(idx);
                    }
                }
            }
        }
    }

    if out.texture_index.is_none() {
        out.texture_index = fallback_index;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Burst deadline ──────────────────────────────────────────────────────
    //
    // A frame count is not a duration. Records are not evenly spaced and there
    // are several per rendered frame, so `seconds * fps` drifts — the mistake
    // `docs/goldsrc_dod_quirks.md` records as "Never Convert Seconds to Ticks
    // With an Average FPS".

    /// `(entry, frame, ordinal)` plus the matching timestamps, the two arrays
    /// `deadline_before` walks.
    fn frames_at(times: &[(usize, f32)]) -> (Vec<(usize, usize, i32)>, Vec<f32>) {
        let mut frames = Vec::new();
        let mut stamps = Vec::new();
        let mut per_entry = std::collections::HashMap::new();
        for (i, &(entry, time)) in times.iter().enumerate() {
            let idx = per_entry.entry(entry).or_insert(0usize);
            frames.push((entry, *idx, i as i32 + 1));
            *idx += 1;
            stamps.push(time);
        }
        (frames, stamps)
    }

    #[test]
    fn the_deadline_is_walked_through_real_timestamps_not_divided_by_a_rate() {
        // Deliberately uneven spacing: a rate-based answer would land in the
        // wrong place precisely because the gaps differ.
        let (frames, times) = frames_at(&[
            (1, 0.0),
            (1, 0.1),
            (1, 5.0),
            (1, 5.05),
            (1, 5.1),
            (1, 5.9),
            (1, 6.0),
        ]);

        // 1s before the frame at t=6.0 means target 5.0, and the last frame at
        // or before that is ordinal 3 (t=5.0).
        assert_eq!(deadline_before(7, 1.0, &frames, &times), Some(3));
        // Shave the lead and the deadline moves later, across the tight cluster
        // rather than proportionally — which is the whole point of walking.
        assert_eq!(deadline_before(7, 0.95, &frames, &times), Some(4));
        // A lead longer than the entry's whole history cannot be answered.
        assert_eq!(deadline_before(7, 30.0, &frames, &times), None);
    }

    #[test]
    fn the_walk_stops_at_the_entry_boundary() {
        // Timestamps restart per directory entry: a LOADING entry's near-zero
        // stamps satisfy any target and would drag the deadline to the front of
        // the demo. Better to answer None and let the caller fall back.
        let (frames, times) = frames_at(&[(0, 0.0), (0, 0.01), (1, 0.0), (1, 0.5), (1, 1.0)]);

        // A lead the entry can satisfy on its own is answered from the entry.
        assert_eq!(deadline_before(5, 0.9, &frames, &times), Some(3));
        assert_eq!(deadline_before(5, 0.4, &frames, &times), Some(4));

        // One it cannot gives up rather than crossing: the frames before are a
        // different entry whose clock restarted, and their near-zero stamps
        // would satisfy any target and drag the deadline to the demo's front.
        assert_eq!(
            deadline_before(5, 2.0, &frames, &times),
            None,
            "must not reach into the previous entry"
        );
    }

    #[test]
    fn a_zero_lead_is_the_window_itself_and_an_unknown_anchor_is_none() {
        let (frames, times) = frames_at(&[(1, 0.0), (1, 1.0)]);

        assert_eq!(deadline_before(2, 0.0, &frames, &times), Some(2));
        assert_eq!(deadline_before(99, 1.0, &frames, &times), None);
    }
}
