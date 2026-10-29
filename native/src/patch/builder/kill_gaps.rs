//! Long gaps between kills inside a clip, recorded sped up (#831):
//! Configuration's "Gaps between kills over N s: Fast-forward".
//!
//! Each gap longer than N s between two kills of one take gets a ramp: the
//! helper cfg's `sys_ramp_on` (`dem_speed <speed>`, `speed.rs`) at
//! [`AFTER_KILL_SECONDS`] after the first kill and `sys_ramp_off`
//! (`dem_speed 1`) at [`BEFORE_KILL_SECONDS`] before the next, the margins
//! Review's fast-forward (#665, goldsrc-hooks `review.rs` `fast_spans`) plays
//! a gap with. #831's probe recorded exactly that pair, at 4x, on both
//! builds: the take holds the gap at exactly the speed, every frame after the
//! ramp matches a 1x take of the same moment, and the sound stays in step.
//!
//! Only the hook's batch (Seek Between Clips) can do it: `dem_speed` scales
//! the demo player's clock, and a patched demo played with `playdemo` has
//! none. A batch that ends up patched keeps its gaps ([`keep_gaps`]).

use crate::hlcr::take_plan::SpedUp;
use crate::patch::types::{CaptureBlock, PatchJob};
use crate::patch::viewdemo_clock::Clock;

use super::timing::{find_tick_backwards, find_tick_forwards};

/// The helper cfg alias that starts a ramp.
pub const RAMP_ON: &str = "sys_ramp_on";
/// The helper cfg alias that ends a ramp.
pub const RAMP_OFF: &str = "sys_ramp_off";

/// How long after a gap's first kill the ramp starts: Review's `TAIL`.
pub const AFTER_KILL_SECONDS: f32 = 2.0;
/// How long before a gap's next kill the ramp ends: Review's `LEAD`.
pub const BEFORE_KILL_SECONDS: f32 = 4.0;

/// One gap's ramp, in the demo's frame ordinals (the scheduled commands'
/// ticks).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ramp {
    pub on_tick: i32,
    pub off_tick: i32,
}

/// One demo's playback times, which place a tick in seconds.
pub(super) struct Timeline<'a> {
    pub frame_times: &'a [f32],
    pub fps: f32,
    /// The `viewdemo` clock the hook's batch runs on, when the demo's kills
    /// have its times: it measures a gap between two kills exactly (each
    /// kill is one of its anchors), where the demo's average frame rate is
    /// off by the recorder's frame-rate swings.
    pub clock: Option<&'a Clock<'a>>,
}

impl Timeline<'_> {
    /// Seconds between frames `from` and `to`: on the `viewdemo` clock when
    /// there is one, else from the frame times when both are in them, else
    /// by the demo's fps.
    fn seconds_between(&self, from: usize, to: usize) -> f32 {
        if let Some(clock) = self.clock {
            let tick = |frame: usize| i32::try_from(frame).unwrap_or(i32::MAX);
            return (clock.seconds(tick(to)) - clock.seconds(tick(from))) as f32;
        }
        match (self.frame_times.get(from), self.frame_times.get(to)) {
            (Some(a), Some(b)) => b - a,
            _ => {
                let fps = if self.fps > 0.0 { self.fps } else { 100.0 };
                (to as f32 - from as f32) / fps
            }
        }
    }
}

/// The ramps for one take: one per gap longer than `over_seconds` between
/// two of `kill_frames` (sorted), from [`AFTER_KILL_SECONDS`] after the
/// first kill to [`BEFORE_KILL_SECONDS`] before the next. A gap too short
/// to leave anything between the margins has none, and so has one whose
/// ramp would leave the take's record window (`record_ticks`, start and stop),
/// which only an end-of-demo clamp can cause.
pub(super) fn ramps(
    kill_frames: &[i32],
    over_seconds: f32,
    timeline: &Timeline,
    record_ticks: (i32, i32),
) -> Vec<Ramp> {
    kill_frames
        .windows(2)
        .filter_map(|pair| {
            let (first, next) = (pair[0].max(0) as usize, pair[1].max(0) as usize);
            if timeline.seconds_between(first, next) <= over_seconds {
                return None;
            }
            let on_tick = find_tick_forwards(
                first,
                AFTER_KILL_SECONDS,
                timeline.frame_times,
                timeline.fps,
            );
            let off_tick = find_tick_backwards(
                next,
                BEFORE_KILL_SECONDS,
                timeline.frame_times,
                timeline.fps,
            );
            let inside = on_tick >= record_ticks.0 && off_tick < record_ticks.1;
            (off_tick > on_tick && inside).then_some(Ramp { on_tick, off_tick })
        })
        .collect()
}

/// `ramps` as the take plan's sped-up stretches (#1084's checks), at
/// `speed`, where `offset` places a tick in seconds from the recording's
/// first frame on the clock the batch runs on.
pub(super) fn sped_up(ramps: &[Ramp], speed: f32, offset: impl Fn(i32) -> f64) -> Vec<SpedUp> {
    ramps
        .iter()
        .map(|ramp| SpedUp {
            from_secs: offset(ramp.on_tick),
            to_secs: offset(ramp.off_tick),
            speed: f64::from(speed),
        })
        .collect()
}

/// Takes every ramp out of `jobs`, for a batch that is patched rather than
/// run by the hook: there `dem_speed` does nothing, so its gaps are kept.
/// Returns how many ramps came out; the caller says so in the activity log
/// ([`kept_gaps_message`]).
pub fn keep_gaps(jobs: &mut [PatchJob]) -> usize {
    let mut removed = 0;
    for job in jobs.iter_mut() {
        let before = job.scheduled_commands.len();
        job.scheduled_commands
            .retain(|(_, command)| command != RAMP_ON && command != RAMP_OFF);
        removed += (before - job.scheduled_commands.len()) / 2;
        job.blocks.iter_mut().for_each(keep_block_gaps);
    }
    removed
}

/// Marks `block` as recorded at real time throughout: no ramped gaps, and a
/// plan without sped-up stretches, so its take is checked against the full
/// record window. For a copy of a block made before [`keep_gaps`] ran (the
/// batch manifest).
pub fn keep_block_gaps(block: &mut CaptureBlock) {
    block.ramped_gaps = 0;
    if let Some(plan) = &mut block.plan {
        plan.sped_up.clear();
    }
}

/// The activity log's line for a patched batch that kept `ramps` gaps it
/// would have fast-forwarded.
pub fn kept_gaps_message(ramps: usize) -> String {
    format!(
        "ℹ️ **Gaps kept at real time** — Fast-forward gaps between kills needs Seek Between \
         Clips, and this batch is patched, so {ramps} gap{} {} recorded at real time.",
        if ramps == 1 { "" } else { "s" },
        if ramps == 1 { "is" } else { "are" }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 100 fps with no frame times: tick = seconds × 100.
    fn by_fps() -> Timeline<'static> {
        Timeline {
            frame_times: &[],
            fps: 100.0,
            clock: None,
        }
    }

    #[test]
    fn a_gap_over_the_setting_ramps_from_two_seconds_after_to_four_before() {
        // Kills at 10 s and 30 s, a 20 s gap.
        let ramps = ramps(&[1000, 3000], 8.0, &by_fps(), (500, 3500));
        assert_eq!(
            ramps,
            vec![Ramp {
                on_tick: 1200,
                off_tick: 2600
            }]
        );
    }

    #[test]
    fn a_gap_at_or_under_the_setting_is_kept() {
        assert!(ramps(&[1000, 1800], 8.0, &by_fps(), (0, 5000)).is_empty());
        assert!(ramps(&[1000, 1500], 8.0, &by_fps(), (0, 5000)).is_empty());
    }

    #[test]
    fn a_gap_with_nothing_left_between_the_margins_is_kept() {
        // 5 s apart with the setting at 3 s: 2 s after is 4 s before.
        assert!(ramps(&[1000, 1600], 3.0, &by_fps(), (0, 5000)).is_empty());
        assert!(ramps(&[1000, 1500], 3.0, &by_fps(), (0, 5000)).is_empty());
    }

    #[test]
    fn each_long_gap_gets_its_own_ramp_and_short_ones_none() {
        let ramps = ramps(&[1000, 2500, 2800, 4800], 8.0, &by_fps(), (0, 6000));
        assert_eq!(
            ramps,
            vec![
                Ramp {
                    on_tick: 1200,
                    off_tick: 2100
                },
                Ramp {
                    on_tick: 3000,
                    off_tick: 4400
                },
            ]
        );
    }

    #[test]
    fn the_frame_times_place_a_ramp_when_the_demo_has_them() {
        // 50 fps of playback times: frame n is n / 50 s.
        let times: Vec<f32> = (0..2000).map(|n| n as f32 / 50.0).collect();
        let timeline = Timeline {
            frame_times: &times,
            fps: 100.0,
            clock: None,
        };
        // Kills at 10 s and 30 s.
        let ramps = ramps(&[500, 1500], 8.0, &timeline, (0, 2000));
        assert_eq!(
            ramps,
            vec![Ramp {
                on_tick: 600,
                off_tick: 1300
            }]
        );
    }

    #[test]
    fn a_ramp_outside_the_record_window_is_dropped() {
        // The take stops before the ramp would end: an end-of-demo clamp.
        assert!(ramps(&[1000, 3000], 8.0, &by_fps(), (500, 2000)).is_empty());
    }

    #[test]
    fn ramps_become_the_plans_sped_up_stretches_from_the_record_start() {
        let ramps = [Ramp {
            on_tick: 1200,
            off_tick: 2600,
        }];
        // Recording starts at tick 500, 100 ticks a second.
        let stretches = sped_up(&ramps, 4.0, |tick| f64::from(tick - 500) / 100.0);
        assert_eq!(
            stretches,
            vec![SpedUp {
                from_secs: 7.0,
                to_secs: 21.0,
                speed: 4.0
            }]
        );
    }

    #[test]
    fn a_patched_batch_loses_its_ramps_and_the_takes_their_count() {
        let mut job = PatchJob {
            source_demo: "a.dem".into(),
            output_demo: "a_out.dem".into(),
            streaks: Vec::new(),
            target_player: None,
            init_commands: Vec::new(),
            scheduled_commands: vec![
                (100, "sys_record_start".into()),
                (200, RAMP_ON.into()),
                (300, RAMP_OFF.into()),
                (400, "sys_record_stop".into()),
            ],
            director_events: Vec::new(),
            block_routes: Vec::new(),
            blocks: Vec::new(),
        };
        let removed = keep_gaps(std::slice::from_mut(&mut job));
        assert_eq!(removed, 1);
        assert_eq!(
            job.scheduled_commands,
            vec![
                (100, "sys_record_start".to_string()),
                (400, "sys_record_stop".to_string())
            ]
        );
    }

    #[test]
    fn the_kept_gaps_line_counts_them() {
        assert!(kept_gaps_message(1).contains("1 gap is recorded"));
        assert!(kept_gaps_message(3).contains("3 gaps are recorded"));
    }
}
