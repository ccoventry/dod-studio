//! A backward seek starts over (#830, #1085): which seeks rewind, where they
//! start over from, and where they empty the kill feed. `demo_seek`'s player
//! sets the clock and steps it.
//!
//! Going back replays nothing, so the client keeps what the later position
//! left it. In an HLTV demo that includes each player's zoom, which only the
//! `HLTV` user message carries (`hltv_fov.rs`): a sniper scoped in at the
//! landing showed unzoomed after Review's Replay. In a POV demo it is the
//! flags and capture icons and the scoreboard: a jump back showed an Allied
//! star on a flag the Axis held, a capture icon from later in the demo, and
//! the later kills and deaths in every row (tested on both builds,
//! 2026-11-06). So every backward seek ([`rewinds`]) clears the client's zoom
//! table, sets the clock back, waits for that frame to be sent, and steps
//! forward to the target like a long forward seek. Every message before the
//! landing arrives in order, so flags, teams, scores and names are right.
//! Only a `ds_demo_seek_skip_between` seek still jumps back at once.
//!
//! Where it starts over is the demo's first recorded frame: in a POV demo
//! that is past the gap before the recording (`demo_seek`'s module doc, #893),
//! which would otherwise cost a step per 5 s of nothing with each step waiting
//! [`MAX_WAIT_FRAMES`](super::MAX_WAIT_FRAMES) for a frame that never comes
//! (7-15 s, against 1.0-2.6 s from the first frame on 5 POV demos from 5
//! players). Jumping the gap skips no message: the LOADING frames before it
//! were read at the demo's load and their sign-on still holds. An HLTV demo
//! has no such gap and starts over from the clock's start, as it always has
//! ([`recording_start`]).
//!
//! The kill feed is emptied on the way: the steps replay every `DeathMsg` of
//! the demo within a second or two of real time, which would put the last
//! few kills before the landing in the feed however long ago they were. So a
//! rewind steps to exactly `hud_deathnotice_time` before the target, empties
//! the feed there, and steps on: the kills it then replays are the ones
//! playing up to the target would still show (#1085: on `solifotw-pb2` a
//! clear just before the last step also took a kill 2.5 s before the target,
//! since that step starts anywhere up to 5 s back).

use super::{GAP_LANDING_BEFORE, GAP_SECONDS, STEP_SECONDS, Step, next_step};

/// One frame of a stepped seek, a rewind or a forward one (`chain.rs`), still
/// to empty the kill feed at `feed_at`: step to exactly there, then clear;
/// `None` once cleared, stepping as any seek.
pub(super) fn rewind_step(now: f64, target: f64, feed_at: Option<f64>) -> Step {
    match feed_at {
        Some(at) if now < at - 0.001 => Step::To((now + STEP_SECONDS).min(at)),
        Some(_) => Step::ClearFeed,
        None => next_step(now, target),
    }
}

/// Frames the kill feed is emptied on during a rewind: the last step's
/// `DeathMsg`s may reach the client a frame after the player sent them, so
/// one clear could come before they arrive.
pub(super) const FEED_CLEAR_FRAMES: u32 = 2;

/// The longest notice lifetime a rewind allows for: past this, steps would
/// replay more than a few seconds' worth of kills after the clear anyway.
const MAX_FEED_SECONDS: f64 = 60.0;

/// Where a rewind to `target` empties the kill feed, for notices that live
/// `lifetime` seconds.
pub(super) fn feed_clear_at(target: f64, lifetime: Option<f64>) -> f64 {
    target - lifetime.unwrap_or(0.0).clamp(0.0, MAX_FEED_SECONDS)
}

/// Whether a seek from `now` to `to` goes back to the start first and steps
/// forward from there (#830, #1085): any backward seek, since a jump back
/// replays none of the messages before the landing (the module doc). A
/// `ds_demo_seek_skip_between` seek skips the in-between on purpose.
pub(super) fn rewinds(now: f64, to: f64, skip_between: bool) -> bool {
    !skip_between && to < now
}

/// How many frames from the clock's start [`recording_start`] looks through
/// for the gap, and how far: the LOADING frames are a handful spanning about
/// 14 ms, so either cap is far past them.
const RECORDING_LOOK_FRAMES: u32 = 256;
const RECORDING_LOOK_SECONDS: f64 = 30.0;

/// Where a POV demo's recording starts, for a rewind (#1085): 0.01 s before
/// the first frame after a stretch of more than [`GAP_SECONDS`] with no
/// frames, among the first frames from `start`, where the player's own gap
/// jump would put the clock. `frame_at(t)` is the time of the frame at or
/// after `t`. `None` when the demo has no such gap near its start (an HLTV
/// demo, or a POV recording started at the join): start from `start`.
pub(super) fn recording_start(
    start: f64,
    mut frame_at: impl FnMut(f64) -> Option<f32>,
) -> Option<f64> {
    let mut previous = start;
    let mut t = start;
    for _ in 0..RECORDING_LOOK_FRAMES {
        if previous - start > RECORDING_LOOK_SECONDS {
            return None;
        }
        let frame = frame_at(t)?;
        let time = f64::from(frame);
        if time - previous > GAP_SECONDS {
            return Some(f64::from(frame - GAP_LANDING_BEFORE));
        }
        previous = time;
        // Frame times are floats: the next one up, so the look moves on
        // even where 0.1 ms is below a float's step (past 2048 s).
        t = f64::from(next_float_up(frame));
    }
    None
}

/// The smallest float above `x`, for a positive finite `x`.
fn next_float_up(x: f32) -> f32 {
    if x.is_finite() && x >= 0.0 {
        f32::from_bits(x.to_bits() + 1)
    } else {
        x
    }
}

/// Where a rewind to `to` sets the clock first: the recording's start when
/// the demo has one past `start` ([`recording_start`]), else `start`; never
/// past `to` itself, which lands at once when it lies in the gap.
pub(super) fn rewind_base(start: f64, recording: Option<f64>, to: f64) -> f64 {
    recording.map_or(start, |r| r.min(to)).max(start)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #830: stealth scoped at 940 s of `wsod25_grp2_h1_hltv`, seeked back to
    /// from 1000 s; #1085: the same for a POV demo's flags and scores.
    #[test]
    fn every_backward_seek_goes_back_to_the_start() {
        assert!(rewinds(1000.0, 940.0, false));
        assert!(
            !rewinds(1000.0, 940.0, true),
            "the skip lands at once on purpose"
        );
        assert!(
            !rewinds(900.0, 940.0, false),
            "forward replays the messages"
        );
        assert!(!rewinds(940.0, 940.0, false));
    }

    /// Frames as `frame_at` hands them out: the first at or after `t`.
    fn frames(times: &[f32]) -> impl FnMut(f64) -> Option<f32> + '_ {
        move |t| times.iter().copied().find(|&f| f64::from(f) >= t)
    }

    /// `solifotw-pb2`: the clock starts at the LOADING frames, 9:22 before
    /// the first playback frame at 9:26:50.
    #[test]
    fn a_pov_recording_starts_after_the_gap_past_its_loading_frames() {
        let times = [4.10, 4.105, 4.112, 4.114, 566.50, 566.51, 566.52];
        let got = recording_start(4.10, frames(&times)).expect("a gap");
        assert!((got - 566.49).abs() < 1e-3, "{got}");
    }

    #[test]
    fn a_demo_with_no_gap_near_its_start_starts_over_from_the_start() {
        // An HLTV demo's frames run from its first second.
        let hltv: Vec<f32> = (0..400).map(|i| 30.0 + i as f32 * 0.05).collect();
        assert_eq!(recording_start(30.0, frames(&hltv)), None);
        // A gap later in the demo is not the recording's start.
        let mut late: Vec<f32> = (0..70).map(|i| 30.0 + i as f32 * 0.5).collect();
        late.push(200.0);
        assert_eq!(
            recording_start(30.0, frames(&late)),
            None,
            "the gap starts past the 30 s look"
        );
        // No world, or a demo with no frames.
        assert_eq!(recording_start(30.0, |_| None), None);
    }

    /// Past 2048 s a float's step is above 0.1 ms: the look still moves on
    /// to the next frame instead of finding the same one again.
    #[test]
    fn the_look_moves_on_at_large_clock_values() {
        let times = [3000.0, 3000.001, 3000.002, 3400.0];
        let got = recording_start(3000.0, frames(&times)).expect("a gap");
        assert!((got - 3399.99).abs() < 1e-2, "{got}");
        assert!(next_float_up(3000.0) > 3000.0);
        assert_eq!(next_float_up(f32::INFINITY), f32::INFINITY);
    }

    /// A rewind steps to exactly the notice lifetime before the target,
    /// clears there, then steps on to the target.
    #[test]
    fn a_rewind_empties_the_kill_feed_one_notice_lifetime_before_landing() {
        let at = feed_clear_at(978.61, Some(10.0));
        assert!((at - 968.61).abs() < 1e-9);
        assert_eq!(rewind_step(566.49, 978.61, Some(at)), Step::To(571.49));
        assert_eq!(rewind_step(965.0, 978.61, Some(at)), Step::To(at));
        assert_eq!(rewind_step(968.62, 978.61, Some(at)), Step::ClearFeed);
        assert_eq!(rewind_step(968.62, 978.61, None), Step::To(973.62));
        assert_eq!(rewind_step(975.0, 978.61, None), Step::Land(978.61));
        // A target nearer the start than one lifetime clears at once.
        assert_eq!(
            rewind_step(566.49, 570.0, Some(feed_clear_at(570.0, Some(10.0)))),
            Step::ClearFeed
        );
        assert_eq!(feed_clear_at(100.0, None), 100.0);
        assert_eq!(feed_clear_at(100.0, Some(1e6)), 40.0);
    }

    #[test]
    fn a_rewind_starts_at_the_recording_unless_the_target_is_before_it() {
        assert_eq!(rewind_base(4.1, Some(566.49), 1446.0), 566.49);
        assert_eq!(rewind_base(4.1, None, 1446.0), 4.1);
        // A target in the gap: straight there, nothing to replay.
        assert_eq!(rewind_base(4.1, Some(566.49), 300.0), 300.0);
        assert_eq!(rewind_base(4.1, Some(566.49), 1.0), 4.1);
    }
}
