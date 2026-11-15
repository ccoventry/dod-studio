//! What a take was planned to hold, and whether the take on disk matches
//! (#1084).
//!
//! A batch plans each recording block: the demo frames recording starts and
//! stops on, and the kills inside them. Those become seconds on the
//! `viewdemo` clock (`patch::viewdemo_clock`), the clock the hook DLL runs a
//! Seek Between Clips batch on, so the plan is the window it records (a
//! patched demo plays the same frames on the demo's own clock, which keeps
//! pace with it). Not the scanner's `frame_times`: they stop at the end of
//! the demo's loading segment, a few dozen frames, so they never reach a
//! clip. The video's frame rate turns the span into a frame count: HLAE
//! writes one frame per `1 / mirv_movie_fps` seconds of demo time. The plan
//! is written beside the take (`take_meta`), so the checks can hold the
//! video to it long after the batch is gone:
//!
//! - **Length**: a video well short of or past its planned frame count is a
//!   capture problem (frames lost, or recording started or stopped at the
//!   wrong moment). The tolerance is [`SHORT_SHARE`] / [`SHORT_MIN_SECONDS`],
//!   the same slack #1105's partial-take check allows: the recording starts
//!   and stops on the frame after its command and the demo's clock is read
//!   at the demo's own frame rate, so a few frames either way are normal, and
//!   a take that is wrong is wrong by seconds.
//! - **Kills in the clip**: each kill's place in the video is its demo time
//!   minus the recording's start, less what any sped-up stretch before it
//!   saves (#831). One outside the video is a bad seek or a wrong recording
//!   window.
//!
//! A take captured before the plan was recorded, or whose demo's kills have
//! no `viewdemo` times (a scan older than them), has none, and is reported as
//! unknown, never as a problem.

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
    /// Where each of the block's kills falls in the recording, in seconds of
    /// demo time from its first frame (negative: before it starts).
    pub kill_offsets_secs: Vec<f64>,
    /// The stretches recorded sped up (#831's fast-forwarded gaps between
    /// kills). The video holds each at its speed, so it is shorter than
    /// `length_secs` and a kill after one sits earlier in it; the checks go
    /// through [`TakePlan::video_secs`]. Empty for a take recorded at real
    /// time, and in every plan written before #831.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sped_up: Vec<SpedUp>,
}

/// One stretch of a recording played faster than real time.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SpedUp {
    /// Where it starts and ends, in seconds of demo time from the
    /// recording's first frame.
    pub from_secs: f64,
    pub to_secs: f64,
    /// How many times real time it plays at.
    pub speed: f64,
}

impl TakePlan {
    /// Where `demo_secs` (demo time from the recording's first frame) falls
    /// in the video: less the time each sped-up stretch before it saves. A
    /// stretch with a speed that isn't above 1 saves nothing.
    pub fn video_secs(&self, demo_secs: f64) -> f64 {
        let saved: f64 = self
            .sped_up
            .iter()
            .filter(|s| s.speed.is_finite() && s.speed > 1.0)
            .map(|s| {
                let played = (demo_secs.min(s.to_secs) - s.from_secs.max(0.0)).max(0.0);
                played * (1.0 - 1.0 / s.speed)
            })
            .sum();
        demo_secs - saved
    }

    /// The video's planned length in seconds.
    pub fn video_length_secs(&self) -> f64 {
        self.video_secs(self.length_secs)
    }
}

/// The plan for a block that records frames `start_frame` to `stop_frame`,
/// with kills on `kill_frames`, where `seconds` places a frame on the clock
/// the recording runs on. `None` when the window is empty: nothing to plan.
pub fn plan_for_block(
    seconds: impl Fn(i32) -> f64,
    start_frame: i32,
    stop_frame: i32,
    kill_frames: &[i32],
) -> Option<TakePlan> {
    if start_frame < 0 || stop_frame <= start_frame {
        return None;
    }
    let start = seconds(start_frame);
    let stop = seconds(stop_frame);
    if !(start.is_finite() && stop.is_finite()) || stop <= start {
        return None;
    }
    let kill_offsets_secs = kill_frames
        .iter()
        .map(|&frame| seconds(frame) - start)
        .filter(|at| at.is_finite())
        .collect();
    Some(TakePlan {
        length_secs: stop - start,
        kill_offsets_secs,
        sped_up: Vec::new(),
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
    (plan.video_length_secs() * fps).round().max(0.0) as u64
}

/// Whether `actual_frames` is noticeably short of or past the plan at `fps`.
/// `None` when it is within tolerance, or `fps` isn't a positive rate.
pub fn length_mismatch(plan: &TakePlan, fps: f64, actual_frames: u64) -> Option<LengthMismatch> {
    if !(fps.is_finite() && fps > 0.0) {
        return None;
    }
    let planned = planned_frames(plan, fps);
    let slack_secs = (plan.video_length_secs() * SHORT_SHARE).max(SHORT_MIN_SECONDS);
    let off_secs = (actual_frames as f64 - planned as f64).abs() / fps;
    (off_secs > slack_secs).then_some(LengthMismatch {
        planned_frames: planned,
        actual_frames,
    })
}

/// The kills that fall outside a video `video_secs` long, as their offsets
/// from its start in the video, in plan order.
pub fn kills_outside(plan: &TakePlan, video_secs: f64) -> Vec<f64> {
    plan.kill_offsets_secs
        .iter()
        .map(|&at| plan.video_secs(at))
        .filter(|&at| at < -KILL_MARGIN_SECS || at > video_secs + KILL_MARGIN_SECS)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A clock at 100 fps: frame `i` is at `i / 100` seconds.
    fn clock(frame: i32) -> f64 {
        f64::from(frame) / 100.0
    }

    fn plan(length_secs: f64, kills: &[f64]) -> TakePlan {
        TakePlan {
            length_secs,
            kill_offsets_secs: kills.to_vec(),
            sped_up: Vec::new(),
        }
    }

    /// A 28 s plan with kills at 5 s and 23 s and the gap between them, 7 s
    /// to 19 s, at 4x: 12 s of it play in 3 s, so the video is 19 s.
    fn ramped() -> TakePlan {
        TakePlan {
            sped_up: vec![SpedUp {
                from_secs: 7.0,
                to_secs: 19.0,
                speed: 4.0,
            }],
            ..plan(28.0, &[5.0, 23.0])
        }
    }

    #[test]
    fn a_plan_is_the_recording_window_and_each_kill_in_it() {
        let p = plan_for_block(clock, 1000, 2900, &[1500, 2000]).unwrap();
        assert!((p.length_secs - 19.0).abs() < 1e-9);
        assert!((p.kill_offsets_secs[0] - 5.0).abs() < 1e-9);
        assert!((p.kill_offsets_secs[1] - 10.0).abs() < 1e-9);
    }

    #[test]
    fn an_empty_or_backwards_window_has_no_plan() {
        assert_eq!(plan_for_block(clock, 1000, 1000, &[]), None);
        assert_eq!(plan_for_block(clock, 2000, 1000, &[]), None);
        assert_eq!(plan_for_block(clock, -3, 1000, &[]), None);
    }

    #[test]
    fn a_clock_that_cannot_place_a_frame_gives_no_plan_or_leaves_the_kill_out() {
        let broken = |frame: i32| if frame > 2500 { f64::NAN } else { clock(frame) };
        assert_eq!(plan_for_block(broken, 1000, 2900, &[1500]), None);
        let p = plan_for_block(broken, 1000, 2400, &[1500, 2600]).unwrap();
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
    fn a_sped_up_stretch_shortens_the_video_by_what_it_saves() {
        let p = ramped();
        assert!((p.video_length_secs() - 19.0).abs() < 1e-9);
        assert_eq!(planned_frames(&p, 60.0), 1140);
    }

    #[test]
    fn a_time_before_inside_or_after_a_sped_up_stretch_maps_into_the_video() {
        let p = ramped();
        assert!((p.video_secs(5.0) - 5.0).abs() < 1e-9);
        assert!((p.video_secs(11.0) - 8.0).abs() < 1e-9);
        assert!((p.video_secs(23.0) - 14.0).abs() < 1e-9);
    }

    #[test]
    fn a_take_as_long_as_its_sped_up_plan_is_fine_and_one_at_real_time_is_not() {
        let p = ramped();
        assert_eq!(length_mismatch(&p, 60.0, 1140), None);
        // The gap recorded at real time: 28 s against 19.
        assert!(length_mismatch(&p, 60.0, 28 * 60).is_some());
    }

    #[test]
    fn a_kill_after_a_sped_up_stretch_is_inside_the_shorter_video() {
        assert!(kills_outside(&ramped(), 19.0).is_empty());
        assert_eq!(kills_outside(&ramped(), 10.0), vec![14.0]);
    }

    #[test]
    fn a_speed_of_one_or_less_saves_nothing() {
        let mut p = ramped();
        p.sped_up[0].speed = 1.0;
        assert!((p.video_length_secs() - 28.0).abs() < 1e-9);
        p.sped_up[0].speed = f64::NAN;
        assert!((p.video_length_secs() - 28.0).abs() < 1e-9);
    }

    #[test]
    fn a_plan_written_before_sped_up_stretches_reads_as_real_time() {
        let p: TakePlan =
            serde_json::from_str(r#"{"length_secs":19.0,"kill_offsets_secs":[5.0]}"#).unwrap();
        assert_eq!(p, plan(19.0, &[5.0]));
    }

    #[test]
    fn a_sped_up_plan_round_trips_through_json() {
        let text = serde_json::to_string(&ramped()).unwrap();
        assert_eq!(serde_json::from_str::<TakePlan>(&text).unwrap(), ramped());
    }

    #[test]
    fn a_plan_round_trips_through_json() {
        let p = plan(19.0, &[5.0]);
        let text = serde_json::to_string(&p).unwrap();
        assert_eq!(serde_json::from_str::<TakePlan>(&text).unwrap(), p);
    }
}
