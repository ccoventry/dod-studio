//! What a take was planned to hold, and whether the take on disk matches
//! (#1084).
//!
//! A batch plans each recording block: the demo frames recording starts and
//! stops on, and the kills inside them. Those become seconds of demo time
//! (`frame_times`, the demo's own clock), and the capture rate turns the span
//! into a frame count: HLAE writes one frame per `1 / mirv_movie_fps` seconds
//! of demo time. The plan is written beside the take (`take_meta`), so the
//! checks can hold the video to it long after the batch is gone:
//!
//! - **Length**: a video well short of or past its planned frame count is a
//!   capture problem (frames lost, or recording started or stopped at the
//!   wrong moment). The tolerance is [`SHORT_SHARE`] / [`SHORT_MIN_SECONDS`],
//!   the same slack #1105's partial-take check allows: the recording starts
//!   and stops on the frame after its command and the demo's clock is read
//!   at the demo's own frame rate, so a few frames either way are normal, and
//!   a take that is wrong is wrong by seconds.
//! - **Kills in the clip**: each kill's place in the video is its demo time
//!   minus the recording's start. One outside the video is a bad seek or a
//!   wrong recording window.
//!
//! A take captured before the plan was recorded, or whose demo had no frame
//! times, has none, and is reported as unknown, never as a problem.

use crate::hlcr::take_length::{SHORT_MIN_SECONDS, SHORT_SHARE};

/// How far outside the video a kill may fall before it counts as outside:
/// demo times are rounded to the demo's frames (10 ms at 100 fps) and
/// recording starts a frame after its command, so a tenth of a second keeps
/// that rounding from being a warning while a real miss is far more.
pub const KILL_MARGIN_SECS: f64 = 0.1;

/// What one recording block was planned to hold.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TakePlan {
    /// The recording's planned length in seconds of demo time.
    pub length_secs: f64,
    /// Where each of the block's kills falls in the recording, in seconds
    /// from its first frame (negative: before it starts).
    pub kill_offsets_secs: Vec<f64>,
}

/// The plan for a block that records frames `start_frame` to `stop_frame` of
/// a demo whose frame `i` is at `frame_times[i]` seconds, with kills on
/// `kill_frames`. `None` when `frame_times` doesn't reach the recording's
/// last frame (a demo scanned only in part) or the window is empty: nothing
/// trustworthy to plan from.
pub fn plan_for_block(
    frame_times: &[f32],
    start_frame: i32,
    stop_frame: i32,
    kill_frames: &[i32],
) -> Option<TakePlan> {
    let at = |frame: i32| usize::try_from(frame).ok().and_then(|i| frame_times.get(i));
    let start = f64::from(*at(start_frame)?);
    let stop = f64::from(*at(stop_frame)?);
    if stop <= start {
        return None;
    }
    let kill_offsets_secs = kill_frames
        .iter()
        .filter_map(|&frame| at(frame).map(|&t| f64::from(t) - start))
        .collect();
    Some(TakePlan {
        length_secs: stop - start,
        kill_offsets_secs,
    })
}

/// A video whose frame count differs from the plan's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LengthMismatch {
    pub planned_frames: u64,
    pub actual_frames: u64,
}

/// The planned frame count for `plan` at `fps`.
pub fn planned_frames(plan: &TakePlan, fps: f64) -> u64 {
    (plan.length_secs * fps).round().max(0.0) as u64
}

/// Whether `actual_frames` is noticeably short of or past the plan at `fps`.
/// `None` when it is within tolerance, or `fps` isn't a positive rate.
pub fn length_mismatch(plan: &TakePlan, fps: f64, actual_frames: u64) -> Option<LengthMismatch> {
    if !(fps.is_finite() && fps > 0.0) {
        return None;
    }
    let planned = planned_frames(plan, fps);
    let slack_secs = (plan.length_secs * SHORT_SHARE).max(SHORT_MIN_SECONDS);
    let off_secs = (actual_frames as f64 - planned as f64).abs() / fps;
    (off_secs > slack_secs).then_some(LengthMismatch {
        planned_frames: planned,
        actual_frames,
    })
}

/// The kills that fall outside a video `video_secs` long, as their offsets
/// from its start, in plan order.
pub fn kills_outside(plan: &TakePlan, video_secs: f64) -> Vec<f64> {
    plan.kill_offsets_secs
        .iter()
        .copied()
        .filter(|&at| at < -KILL_MARGIN_SECS || at > video_secs + KILL_MARGIN_SECS)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A demo at 100 fps: frame `i` is at `i / 100` seconds.
    fn clock(frames: usize) -> Vec<f32> {
        (0..frames).map(|i| i as f32 / 100.0).collect()
    }

    fn plan(length_secs: f64, kills: &[f64]) -> TakePlan {
        TakePlan {
            length_secs,
            kill_offsets_secs: kills.to_vec(),
        }
    }

    #[test]
    fn a_plan_is_the_recording_window_and_each_kill_in_it() {
        let p = plan_for_block(&clock(5000), 1000, 2900, &[1500, 2000]).unwrap();
        assert!((p.length_secs - 19.0).abs() < 1e-4);
        assert!((p.kill_offsets_secs[0] - 5.0).abs() < 1e-4);
        assert!((p.kill_offsets_secs[1] - 10.0).abs() < 1e-4);
    }

    #[test]
    fn a_demo_without_frame_times_has_no_plan() {
        assert_eq!(plan_for_block(&[], 100, 300, &[150]), None);
    }

    #[test]
    fn a_window_past_the_scanned_frames_has_no_plan() {
        assert_eq!(plan_for_block(&clock(2000), 1000, 2900, &[1500]), None);
    }

    #[test]
    fn an_empty_or_backwards_window_has_no_plan() {
        assert_eq!(plan_for_block(&clock(5000), 1000, 1000, &[]), None);
        assert_eq!(plan_for_block(&clock(5000), 2000, 1000, &[]), None);
        assert_eq!(plan_for_block(&clock(5000), -3, 1000, &[]), None);
    }

    #[test]
    fn a_kill_past_the_scanned_frames_is_left_out_not_guessed() {
        let p = plan_for_block(&clock(3000), 1000, 2900, &[1500, 9000]).unwrap();
        assert_eq!(p.kill_offsets_secs.len(), 1);
    }

    #[test]
    fn the_planned_frame_count_is_the_length_at_the_capture_rate() {
        assert_eq!(planned_frames(&plan(19.0, &[]), 300.0), 5700);
    }

    #[test]
    fn a_take_a_few_frames_off_its_plan_is_fine() {
        let p = plan(19.0, &[]);
        assert_eq!(length_mismatch(&p, 300.0, 5700), None);
        assert_eq!(length_mismatch(&p, 300.0, 5700 + 6), None);
        assert_eq!(length_mismatch(&p, 300.0, 5700 - 6), None);
    }

    #[test]
    fn a_take_within_a_tenth_of_its_length_is_fine() {
        // 19 s plans 5700 frames; 1.9 s (a tenth) is 570 frames of slack.
        assert_eq!(length_mismatch(&plan(19.0, &[]), 300.0, 5700 - 560), None);
    }

    #[test]
    fn a_take_much_shorter_than_planned_is_a_mismatch() {
        let m = length_mismatch(&plan(19.0, &[]), 300.0, 1500).unwrap();
        assert_eq!((m.planned_frames, m.actual_frames), (5700, 1500));
    }

    #[test]
    fn a_take_much_longer_than_planned_is_a_mismatch() {
        let m = length_mismatch(&plan(19.0, &[]), 300.0, 9000).unwrap();
        assert_eq!((m.planned_frames, m.actual_frames), (5700, 9000));
    }

    #[test]
    fn a_short_clip_gets_at_least_half_a_second_of_slack() {
        let p = plan(2.0, &[]);
        assert_eq!(length_mismatch(&p, 300.0, 600 + 140), None);
        assert!(length_mismatch(&p, 300.0, 600 + 160).is_some());
    }

    #[test]
    fn no_capture_rate_means_no_length_verdict() {
        assert_eq!(length_mismatch(&plan(19.0, &[]), 0.0, 10), None);
        assert_eq!(length_mismatch(&plan(19.0, &[]), f64::NAN, 10), None);
    }

    #[test]
    fn kills_inside_the_video_are_not_outside() {
        assert!(kills_outside(&plan(19.0, &[2.0, 10.0, 17.0]), 19.0).is_empty());
    }

    #[test]
    fn a_kill_past_the_end_of_the_video_is_outside() {
        let p = plan(19.0, &[5.0, 12.0]);
        assert_eq!(kills_outside(&p, 8.0), vec![12.0]);
    }

    #[test]
    fn a_kill_before_the_video_starts_is_outside() {
        let p = plan(19.0, &[-2.5, 5.0]);
        assert_eq!(kills_outside(&p, 19.0), vec![-2.5]);
    }

    #[test]
    fn a_kill_a_hair_off_either_end_is_inside_the_margin() {
        let p = plan(19.0, &[-0.05, 19.05]);
        assert!(kills_outside(&p, 19.0).is_empty());
    }

    #[test]
    fn a_plan_round_trips_through_json() {
        let p = plan(19.0, &[5.0]);
        let text = serde_json::to_string(&p).unwrap();
        assert_eq!(serde_json::from_str::<TakePlan>(&text).unwrap(), p);
    }
}
