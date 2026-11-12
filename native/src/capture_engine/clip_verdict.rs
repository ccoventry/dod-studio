//! Whether each clip a batch recorded is whole, from what the game said
//! about it (#1401).
//!
//! The hook DLL sums up every clip it records (`CLIP_SUMMARY`, #1221):
//! whether its `START_RECORD` and `STOP_RECORD` came, the demo times they
//! ran at and the frames HLAE wrote. A batch used to be checked only by
//! what its take folders held, so a clip the game never started, or one
//! that stopped before its highlight's last kill, still counted as
//! captured. Two kinds of verdict:
//!
//! - **Failed**: no start, no stop or no frames. Nothing usable was
//!   recorded (#1395's stale start wrote no video at all), so the take is
//!   not captured and its highlights stay to capture again.
//! - **Short**: the recording ran, but for less than its plan (`TakePlan`),
//!   or a kill the plan holds falls outside what it recorded (#1395's nested
//!   highlight stopped 1.1 s before its last kill, with the take matching
//!   its wrongly planned length to the frame). The take may still be
//!   usable, so it stays captured with a warning.
//!
//! Each summary is matched to its planned take by the demo it came from
//! (`DEMO_START`'s index) and its place in that demo (`NEXT_CLIP`'s), which
//! the hook carries as `demo` and `demo_clip`.

use crate::capture_engine::ClipSummary;
use crate::hlcr::take_plan::{self, KILL_MARGIN_SECS, TakePlan};
use crate::patch::types::CaptureBlock;

/// How much shorter than planned a recording may be and still count as
/// whole, in seconds. Its ends are set by the demo's frames, not the
/// capture's: the recording starts and stops on the demo frame after its
/// command, so each end can be a frame or two late. At the 20-30 frames a
/// second of an HLTV demo that is up to about 0.1 s; a quarter second
/// leaves room for that at both ends while still far under the smallest
/// loss that matters (a highlight's 2 s trail; #1395's take lost 1.1 s).
/// Takes measured against their plans came within a frame (#1398's test:
/// 16.75 s of 16.76 s, 1192 frames of 1192).
pub const SHORT_TOLERANCE_SECS: f64 = 0.25;

/// Why a clip failed: nothing usable was recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClipFailure {
    /// The game never started recording it (no `START_RECORD`).
    NoStart,
    /// It never stopped (no `STOP_RECORD`): the next clip or the batch's
    /// end closed it.
    NoStop,
    /// It started and stopped on the same frame: no frames written.
    NoFrames,
}

/// A recording shorter than its plan, or missing a planned kill.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct ShortClip {
    /// What it recorded, in seconds.
    pub recorded_secs: f64,
    /// What it was planned to record, in the same seconds.
    pub planned_secs: f64,
    /// Planned kills outside what it recorded.
    pub kills_outside: usize,
}

/// A clip that isn't whole. A whole one has no verdict.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClipVerdict {
    Failed(ClipFailure),
    Short(ShortClip),
}

/// The verdict on one clip from its summary and its take's plan.
/// `video_fps` is the rate HLAE wrote it at (`None` for OBS and AGR, whose
/// frames the hook does not count), used only when the summary has no demo
/// times to measure the recording by.
pub fn judge(
    clip: &ClipSummary,
    plan: Option<&TakePlan>,
    video_fps: Option<f64>,
) -> Option<ClipVerdict> {
    if !clip.saw("START_RECORD") {
        return Some(ClipVerdict::Failed(ClipFailure::NoStart));
    }
    if !clip.saw("STOP_RECORD") {
        return Some(ClipVerdict::Failed(ClipFailure::NoStop));
    }
    if clip.frames == Some(0) {
        return Some(ClipVerdict::Failed(ClipFailure::NoFrames));
    }
    let plan = plan?;
    let (recorded_secs, planned_secs, kills_outside) = match (clip.start, clip.stop) {
        // The demo's clock: the plan's own seconds, sped-up stretches and all.
        (Some(start), Some(stop)) => {
            let span = stop - start;
            let outside = plan
                .kill_offsets_secs
                .iter()
                .filter(|&&at| at < -KILL_MARGIN_SECS || at > span + KILL_MARGIN_SECS)
                .count();
            (span, plan.length_secs, outside)
        }
        // The video's frames, which a sped-up stretch makes fewer.
        _ => {
            let fps = video_fps.filter(|f| f.is_finite() && *f > 0.0)?;
            let secs = f64::from(clip.frames?) / fps;
            let outside = take_plan::kills_outside(plan, secs).len();
            (secs, plan.video_length_secs(), outside)
        }
    };
    if !recorded_secs.is_finite() {
        return None;
    }
    let short = planned_secs - recorded_secs > SHORT_TOLERANCE_SECS;
    (short || kills_outside > 0).then_some(ClipVerdict::Short(ShortClip {
        recorded_secs,
        planned_secs,
        kills_outside,
    }))
}

/// Each block's demo, as the 1-based index `DEMO_START` names: a demo's
/// blocks are contiguous and demos play in the manifest's order.
pub fn demo_ordinals(blocks: &[CaptureBlock]) -> Vec<u32> {
    let mut ordinal = 0u32;
    blocks
        .iter()
        .enumerate()
        .map(|(i, b)| {
            if i == 0 || b.demo_name != blocks[i - 1].demo_name {
                ordinal += 1;
            }
            ordinal
        })
        .collect()
}

/// Each block's position in recording order: demos play in the manifest's
/// order, and a demo's clips by `block_index`. The builder already sorts a
/// demo's blocks that way; sorting here keeps the order from depending on
/// it.
pub fn recording_order(blocks: &[CaptureBlock]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..blocks.len()).collect();
    let demo = demo_ordinals(blocks);
    order.sort_by_key(|&i| (demo[i], blocks[i].block_index));
    order
}

/// The block a summary is about: the `demo_clip`th clip, in recording
/// order, of the `demo`th demo. `None` without both, or past the demo's
/// planned clips (a clip the game recorded that no take was planned for).
pub fn block_for(blocks: &[CaptureBlock], clip: &ClipSummary) -> Option<usize> {
    let (demo, _) = clip.demo?;
    let (n, _) = clip.demo_clip?;
    let ordinals = demo_ordinals(blocks);
    recording_order(blocks)
        .into_iter()
        .filter(|&i| ordinals[i] == demo)
        .nth((n as usize).checked_sub(1)?)
}

/// Every block's verdict from the batch's summaries, index-aligned with
/// `blocks`. A block with several summaries gets the first that isn't
/// whole; one with none (a hook DLL from before #1221, or a demo the batch
/// never reached) gets none.
pub fn verdicts(
    blocks: &[CaptureBlock],
    clips: &[ClipSummary],
    video_fps: Option<f64>,
) -> Vec<Option<ClipVerdict>> {
    let mut out = vec![None; blocks.len()];
    for clip in clips {
        let Some(i) = block_for(blocks, clip) else {
            continue;
        };
        if out[i].is_none() {
            out[i] = judge(clip, blocks[i].plan.as_ref(), video_fps);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(
        markers: &[&str],
        start: Option<f64>,
        stop: Option<f64>,
        frames: Option<u32>,
    ) -> ClipSummary {
        ClipSummary {
            number: Some(1),
            demo: Some((1, 1)),
            demo_clip: Some((1, 1)),
            start,
            stop,
            frames,
            markers: markers.iter().map(|m| m.to_string()).collect(),
        }
    }

    const BOTH: &[&str] = &["AUDIO_SYNC", "START_RECORD", "STOP_RECORD"];

    fn whole(start: f64, stop: f64, frames: u32) -> ClipSummary {
        clip(BOTH, Some(start), Some(stop), Some(frames))
    }

    fn plan(length_secs: f64, kills: &[f64]) -> TakePlan {
        TakePlan {
            length_secs,
            kill_offsets_secs: kills.to_vec(),
            sped_up: Vec::new(),
        }
    }

    /// #1398's case S on dev: the stop came before the start. The game's
    /// two lines: "Clip 1 has no START_RECORD", "Clip 2 has no STOP_RECORD
    /// ... 0 frames".
    #[test]
    fn a_clip_with_no_start_failed() {
        let c = clip(&["AUDIO_SYNC", "STOP_RECORD"], None, Some(1612.52), None);
        assert_eq!(
            judge(&c, None, Some(60.0)),
            Some(ClipVerdict::Failed(ClipFailure::NoStart))
        );
    }

    #[test]
    fn a_clip_with_no_stop_failed() {
        let c = clip(&["START_RECORD"], Some(1613.53), None, Some(0));
        assert_eq!(
            judge(&c, None, Some(60.0)),
            Some(ClipVerdict::Failed(ClipFailure::NoStop))
        );
        let c = clip(&["START_RECORD"], Some(11.0), None, Some(300));
        assert_eq!(
            judge(&c, Some(&plan(5.0, &[])), Some(60.0)),
            Some(ClipVerdict::Failed(ClipFailure::NoStop))
        );
    }

    #[test]
    fn a_clip_with_no_frames_failed() {
        let c = whole(20.0, 20.0, 0);
        assert_eq!(
            judge(&c, Some(&plan(5.0, &[])), Some(60.0)),
            Some(ClipVerdict::Failed(ClipFailure::NoFrames))
        );
    }

    /// #1398's case O: 19.87 s and 1192 frames against its plan.
    #[test]
    fn a_clip_as_long_as_its_plan_is_whole() {
        let p = plan(19.88, &[2.0, 10.0, 17.88]);
        assert_eq!(
            judge(&whole(1633.81, 1653.69, 1192), Some(&p), Some(60.0)),
            None
        );
        // A frame or two late at either end is still whole.
        assert_eq!(
            judge(&whole(1633.85, 1653.66, 1189), Some(&p), Some(60.0)),
            None
        );
        // Longer than planned is never short.
        assert_eq!(
            judge(&whole(1633.0, 1660.0, 1600), Some(&p), Some(60.0)),
            None
        );
    }

    #[test]
    fn a_clip_much_shorter_than_its_plan_is_short() {
        let p = plan(19.88, &[2.0]);
        let v = judge(&whole(100.0, 110.0, 600), Some(&p), Some(60.0));
        assert_eq!(
            v,
            Some(ClipVerdict::Short(ShortClip {
                recorded_secs: 10.0,
                planned_secs: 19.88,
                kills_outside: 0
            }))
        );
        // Just past the tolerance.
        let v = judge(&whole(100.0, 119.6, 1176), Some(&p), Some(60.0));
        assert!(matches!(v, Some(ClipVerdict::Short(_))));
    }

    /// #1398's case N on dev: the take matched its plan to the frame
    /// (16.75 s of 16.76 s), but the plan's last kill is at 17.88 s.
    #[test]
    fn a_clip_as_long_as_a_plan_that_misses_its_last_kill_is_short() {
        let p = plan(16.7596, &[1.999, 10.634, 14.790, 17.881]);
        let v = judge(&whole(1022.03, 1038.79, 1005), Some(&p), Some(60.0));
        assert!(
            matches!(
                v,
                Some(ClipVerdict::Short(ShortClip {
                    kills_outside: 1,
                    ..
                }))
            ),
            "{v:?}"
        );
    }

    #[test]
    fn without_demo_times_the_video_frames_measure_it() {
        let p = plan(20.0, &[5.0]);
        let c = clip(BOTH, None, None, Some(1200));
        assert_eq!(judge(&c, Some(&p), Some(60.0)), None);
        let c = clip(BOTH, None, None, Some(600));
        assert!(matches!(
            judge(&c, Some(&p), Some(60.0)),
            Some(ClipVerdict::Short(_))
        ));
        // OBS / AGR: the hook's frames aren't the video's, so no verdict.
        assert_eq!(judge(&c, Some(&p), None), None);
    }

    #[test]
    fn without_a_plan_a_clip_that_started_and_stopped_is_whole() {
        assert_eq!(judge(&whole(1.0, 2.0, 60), None, Some(60.0)), None);
    }

    fn block(demo: &str, index: usize, plan: Option<TakePlan>) -> CaptureBlock {
        CaptureBlock {
            demo_name: demo.into(),
            block_index: index,
            drive_index: 0,
            take_folder: format!("D:/t/{demo}_b{index}").into(),
            take_key: format!("s/{demo}_b{index}"),
            source_streak_indices: vec![index],
            start_tick: 0,
            end_tick: 0,
            record_start_tick: 0,
            record_stop_tick: 0,
            ramped_gaps: 0,
            plan,
        }
    }

    fn at(demo: u32, n: u32, mut c: ClipSummary) -> ClipSummary {
        c.demo = Some((demo, 2));
        c.demo_clip = Some((n, 2));
        c
    }

    #[test]
    fn each_summary_finds_its_block_by_demo_and_place_in_it() {
        // Demo b's blocks listed out of order: its first clip is the third.
        let blocks = [
            block("a", 0, None),
            block("b", 1, None),
            block("b", 0, None),
        ];
        let w = whole(1.0, 2.0, 60);
        assert_eq!(block_for(&blocks, &at(1, 1, w.clone())), Some(0));
        assert_eq!(block_for(&blocks, &at(2, 1, w.clone())), Some(2));
        assert_eq!(block_for(&blocks, &at(2, 2, w.clone())), Some(1));
        // Past the demo's planned clips, or no place: no block.
        assert_eq!(block_for(&blocks, &at(1, 2, w.clone())), None);
        assert_eq!(block_for(&blocks, &at(3, 1, w.clone())), None);
        let mut unplaced = w.clone();
        unplaced.demo_clip = None;
        assert_eq!(block_for(&blocks, &unplaced), None);
    }

    #[test]
    fn a_batch_gets_one_verdict_per_block() {
        let blocks = [
            block("a", 0, Some(plan(10.0, &[2.0]))),
            block("a", 1, Some(plan(10.0, &[2.0]))),
        ];
        let clips = [
            at(1, 1, whole(0.0, 10.0, 600)),
            at(1, 2, clip(&["STOP_RECORD"], None, Some(30.0), None)),
            // A clip the game recorded past the plan: matched to nothing.
            at(1, 3, clip(&["START_RECORD"], Some(31.0), None, Some(0))),
        ];
        let v = verdicts(&blocks, &clips, Some(60.0));
        assert_eq!(
            v,
            vec![None, Some(ClipVerdict::Failed(ClipFailure::NoStart))]
        );
    }

    #[test]
    fn demos_are_numbered_by_where_their_blocks_start() {
        let blocks = [
            block("a", 0, None),
            block("a", 1, None),
            block("b", 0, None),
        ];
        assert_eq!(demo_ordinals(&blocks), vec![1, 1, 2]);
        assert_eq!(recording_order(&blocks), vec![0, 1, 2]);
    }
}
