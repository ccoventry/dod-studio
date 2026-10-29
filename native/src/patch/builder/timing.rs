//! Frame-time and timing helpers: tick walks, the roll floors, the take
//! separation, and when two highlights' windows collide.

use super::*;

/// Walk backwards from `start_frame` (0-indexed) through `frame_times` until
/// `gap_seconds` of real demo time has been accumulated. Returns the 0-indexed
/// frame where that time boundary is reached. Clamps to frame 0 if the gap
/// exceeds the available history before the start frame.
pub(super) fn find_tick_backwards(
    start_frame: usize,
    gap_seconds: f32,
    frame_times: &[f32],
    fps: f32,
) -> i32 {
    let fps = if fps > 0.0 { fps } else { 100.0 };
    if gap_seconds <= 0.0 {
        return start_frame as i32;
    }
    // If frame_times is empty or start_frame is out-of-bounds, fall back to linear math.
    // This is safe, consistent, and spaces commands chronologically when frame_times
    // is truncated or absent.
    if frame_times.is_empty() || start_frame >= frame_times.len() {
        let ticks_gap = (gap_seconds * fps).round() as i32;
        return (start_frame as i32 - ticks_gap).max(0);
    }
    let anchor_time = frame_times[start_frame];
    let target_time = anchor_time - gap_seconds;
    // Walk backwards until we cross target_time
    let mut frame = start_frame;
    while frame > 0 {
        frame -= 1;
        if frame_times[frame] <= target_time {
            return frame as i32;
        }
    }
    0
}

/// Walk forwards from `start_frame` (0-indexed) through `frame_times` until
/// `gap_seconds` of real demo time has accumulated. Returns the 0-indexed
/// frame where that time boundary is reached. Clamps to the last valid frame
/// if the end of the array is reached before the gap is satisfied.
pub(super) fn find_tick_forwards(
    start_frame: usize,
    gap_seconds: f32,
    frame_times: &[f32],
    fps: f32,
) -> i32 {
    let fps = if fps > 0.0 { fps } else { 100.0 };
    if gap_seconds <= 0.0 {
        return start_frame as i32;
    }
    if frame_times.is_empty() || start_frame >= frame_times.len() {
        let ticks_gap = (gap_seconds * fps).round() as i32;
        return start_frame as i32 + ticks_gap;
    }
    let anchor_time = frame_times[start_frame];
    let target_time = anchor_time + gap_seconds;
    let last = frame_times.len().saturating_sub(1);
    let mut frame = start_frame;
    while frame < last {
        frame += 1;
        if frame_times[frame] >= target_time {
            return frame as i32;
        }
    }
    last as i32
}

pub(super) const LOG_TAG: &str = "[dod-studio]";

/// How far ahead of the record start `stopsound` fires, to flush audio the
/// fast-forward left in a bad state. Clamped down to the pre-roll when the
/// pre-roll is shorter, so it never lands while still fast-forwarding.
pub(super) const SOUND_FLUSH_LEAD_SECONDS: f32 = 1.0;

/// Minimum breathing room between one take's `mirv_recordmovie_stop` and the
/// next one's start. Two highlights closer than this are merged into a single
/// take rather than risking a stop/start cycle that tight.
///
/// Measured on the pre-Anniversary build (#9): after `mirv_recordmovie_stop`
/// on a 2 s 720p take, HLAE spends about 0.28 s of wall time finalising it
/// (closing the frame files, writing the WAV) inside one engine frame, and
/// since the engine is back on real time once recording stops, that frame
/// moves the demo clock by the same amount. A next take asked for sooner
/// starts late and loses the frames in between: 0.25 s lost 1 frame, 0.1 s
/// lost 6, each with its audio cut short. From 0.5 s up nothing was lost.
///
/// 0.5 s rather than the ~0.3 s floor because finalising grows with the take
/// (a longer `sound.wav`, more files to close) and with a slower disk, and the
/// cost of being wrong is a short take, while the cost of merging is half a
/// second of connective footage inside one clip.
pub const MIN_TAKE_SEPARATION_SECONDS: f32 = 0.5;

/// The separation this batch's capture mode actually needs.
///
/// `MIN_TAKE_SEPARATION_SECONDS` above is HLAE's. The OBS figure is measured
/// separately, and is larger, so the mode has to choose.
///
/// Public so `find_overlaps` and the tests ask the same question the merge loop
/// does, rather than re-deriving it and drifting.
pub fn take_separation_seconds(config: &PatcherConfig) -> f32 {
    match config.capture_mode {
        crate::patch::CaptureMode::Obs => crate::patch::types::OBS_TAKE_SEPARATION_SECONDS,
        _ => MIN_TAKE_SEPARATION_SECONDS,
    }
}

pub(super) fn build_safe_echos(tick: i32, message: &str) -> Vec<(i32, String)> {
    let mut result = Vec::new();
    let mut current_tick = tick;

    let mut words: Vec<&str> = message.split(' ').collect();
    if words.is_empty() {
        return result;
    }

    let mut current_chunk = String::new();
    let mut is_first = true;

    let mut i = 0;
    while i < words.len() {
        let word = words[i];
        let prefix = if is_first {
            format!("{} ", LOG_TAG)
        } else {
            "[dodstudio] ->".to_string()
        };

        let test_message = if current_chunk.is_empty() {
            word.to_string()
        } else {
            format!("{} {}", current_chunk, word)
        };

        let full_string = format!("{}{}", prefix, test_message);

        if full_string.len() > crate::patch::MAX_ECHO_CHUNK_SIZE {
            if current_chunk.is_empty() {
                let limit = crate::patch::MAX_ECHO_CHUNK_SIZE.saturating_sub(prefix.len());
                let (part1, part2) = word.split_at(limit.min(word.len()));

                let cmd = format!("echo \"{}{}\"", prefix, part1);
                result.push((current_tick, cmd));
                current_tick += 1;

                is_first = false;
                words[i] = part2;
                continue;
            } else {
                let cmd = format!("echo \"{}{}\"", prefix, current_chunk);
                result.push((current_tick, cmd));
                current_tick += 1;

                current_chunk.clear();
                is_first = false;
                continue;
            }
        } else {
            current_chunk = test_message;
            i += 1;
        }
    }

    if !current_chunk.is_empty() {
        let prefix = if is_first {
            format!("{} ", LOG_TAG)
        } else {
            "[dodstudio] ->".to_string()
        };
        let cmd = format!("echo \"{}{}\"", prefix, current_chunk);
        result.push((current_tick, cmd));
    }

    result
}

/// The shortest real-time run-up the engine will tolerate before recording.
///
/// `docs/goldsrc_dod_quirks.md`: fast-forwarding breaks the engine's audio
/// buffers, and the speed must return to real time "2 to 4 seconds prior to
/// injecting `mirv_recordmovie_start`" to flush and resync. This is the lower
/// end of that range — the floor, not the recommendation.
pub const AUDIO_RESYNC_SECONDS: f32 = 2.0;

/// What the pre-roll and post-roll have to cover, and which requirement is
/// currently setting the bar.
///
/// The rolls stopped being a matter of taste once other things started being
/// measured against them: the audio resync, the sound flush, the decal sweep's
/// lead, and any Scheduled Command's offset. Each is knowable, so the app can
/// say when a roll is too short instead of leaving it to be discovered in a
/// capture that looks almost right.
#[derive(Debug, Clone, PartialEq)]
pub struct RollFloors {
    pub pre_roll: f32,
    /// Which term set `pre_roll`.
    pub pre_roll_binding: &'static str,
    pub post_roll: f32,
    pub post_roll_binding: &'static str,
    pub audio_resync: f32,
    pub sound_flush: f32,
    /// The decal sweep's lead, or 0 when the flush is off.
    pub flush_lead: f32,
    /// Largest "Before" offset among the Scheduled Commands.
    pub scheduled_before: f32,
    /// Largest "After" offset among them.
    pub scheduled_after: f32,
}

/// Compute the floors for a configuration.
///
/// Deliberately does NOT fold in the burst's own span. At a 4,096 ring that
/// spans several seconds and would demand an enormous pre-roll — but the burst
/// is network messages in the demo stream rather than console commands, so
/// whether it needs real-time playback at all is unverified. Guessing a floor
/// from an unknown would be worse than leaving it out and saying so.
pub fn roll_floors(config: &PatcherConfig) -> RollFloors {
    let flush_lead = if config.decal_flush {
        crate::patch::DEFAULT_LEAD_SECONDS
    } else {
        0.0
    };
    let offset_for = |want_after: bool| {
        config
            .custom_commands
            .iter()
            .filter(|c| matches!(c.relation, CommandRelation::After) == want_after)
            .map(|c| c.offset)
            .fold(0.0f32, f32::max)
    };
    // Scheduled offsets anchor to the KILL, not to the record start — while the
    // speed drop sits a pre-roll before the record start, which is itself a
    // start-lead before the kill. So the real-time window opens
    // `record_start_lead + pre_roll` ahead of the kill, and a Before offset only
    // needs the pre-roll to cover what the start lead does not.
    //
    // Comparing the raw offset against the pre-roll alone is what made this warn
    // about a 10s command with a 5s lead and a 5s pre-roll, which lands exactly
    // on the speed drop and is fine.
    let scheduled_before = (offset_for(false) - config.record_start_lead).max(0.0);
    let scheduled_after = (offset_for(true) - config.record_stop_trail).max(0.0);

    // Highest wins, and the label names it so the message can be acted on.
    let pre_terms = [
        (AUDIO_RESYNC_SECONDS, "the audio resync after fast-forward"),
        (SOUND_FLUSH_LEAD_SECONDS, "the stopsound flush"),
        (flush_lead, "the decal flush's lead"),
        (
            scheduled_before,
            "a Scheduled Command set before the highlight",
        ),
    ];
    let (pre_roll, pre_roll_binding) = pre_terms.iter().copied().fold(
        (0.0f32, "nothing"),
        |acc, t| if t.0 > acc.0 { t } else { acc },
    );

    let (post_roll, post_roll_binding) = if scheduled_after > 0.0 {
        (
            scheduled_after,
            "a Scheduled Command set after the highlight",
        )
    } else {
        (0.0, "nothing")
    };

    RollFloors {
        pre_roll,
        pre_roll_binding,
        post_roll,
        post_roll_binding,
        audio_resync: AUDIO_RESYNC_SECONDS,
        sound_flush: SOUND_FLUSH_LEAD_SECONDS,
        flush_lead,
        scheduled_before,
        scheduled_after,
    }
}

/// Whether a scheduled command lands while playback is still fast-forwarding.
///
/// A block runs at `host_framerate 0.05` until the pre-roll drops it back to
/// real time at `speed_drop_tick`, and resumes fast-forwarding once the
/// post-roll ends at `post_roll_end_tick`. Outside that window a command still
/// executes — it just executes with the engine racing through frames and its
/// audio buffers unflushed.
///
/// Whether that matters depends entirely on the command, so this reports rather
/// than warns. Setting a cvar early is usually harmless: `hud_deathnotice_time`
/// ten seconds before a clip simply takes effect ten seconds before the clip.
/// What misbehaves is anything that depends on playback running at real speed —
/// sound, recording start/stop, rendering. Phrasing this as a hazard would cry
/// wolf on the common, correct case, which is how a warning gets ignored.
pub fn runs_during_fast_forward(
    target_tick: i32,
    speed_drop_tick: i32,
    post_roll_end_tick: i32,
) -> bool {
    target_tick < speed_drop_tick || target_tick > post_roll_end_tick
}

// ── Block merging ─────────────────────────────────────────────────────────────

/// Whether two windows around `prev_end` and `next_start` collide once the
/// given padding is applied to each side.
///
/// Called twice per highlight pair, with different padding, to answer two
/// different questions (see `build_batch_queue`'s merge loop):
///
/// - with start-lead/stop-trail: do the **recordings** overlap? If so the two
///   highlights physically cannot be separate takes and must be merged.
/// - with start-lead+pre-roll / stop-trail+post-roll: do the **speed-change**
///   windows collide? If so the clips stay separate takes, but the
///   fast-forward between them is dropped and playback just stays at normal
///   speed across the gap.
///
/// Public so the `find_overlaps` diagnostic can ask the same questions without
/// running a capture, and can't drift from the real decision.
pub fn blocks_merge(prev_end: i32, next_start: i32, lead_ticks: i32, trail_ticks: i32) -> bool {
    (next_start - lead_ticks).max(0) <= prev_end + trail_ticks
}

/// Frame index of a highlight's first recorded kill, honouring a Kill Range
/// edit. Falls back to the streak's tick bound when kill data is absent.
///
/// The merge decision and the scheduled record marks must both key off this
/// rather than `start_tick`, or a Kill Range edit moves the recording without
/// moving the decision about whether it collides with its neighbour.
pub(super) fn first_kill_frame(streak: &CaptureStreak) -> i32 {
    streak
        .kills
        .get(streak.start_index)
        .map(|k| k.0)
        .unwrap_or(streak.start_tick)
}

/// Frame index of a highlight's last recorded kill, honouring a Kill Range
/// edit. For a merged block this is the absorbed highlight's final kill.
pub(super) fn last_kill_frame(streak: &CaptureStreak) -> i32 {
    let idx = streak.end_index.min(streak.kills.len().saturating_sub(1));
    streak
        .kills
        .get(idx)
        .map(|k| k.0)
        .unwrap_or(streak.end_tick)
}

/// Frame indices of the kills a block records: its Kill Range, from the first
/// recorded kill to the last (a merged block's absorbed highlights add their
/// last kills, see the merge in `build_batch_queue`).
pub(super) fn block_kill_frames(streak: &CaptureStreak) -> Vec<i32> {
    let last = streak.end_index.min(streak.kills.len().saturating_sub(1));
    streak
        .kills
        .get(streak.start_index..=last)
        .map(|kills| kills.iter().map(|k| k.0).collect())
        .unwrap_or_default()
}

// ── Drive allocation ──────────────────────────────────────────────────────────
