//! What the game and OBS said about each clip, applied to the takes a batch
//! left on disk (#1109, #1401). The take check (`capture_manager`'s
//! `check_takes`) first asks the disk; this then adds what only the batch's
//! own reports know:
//!
//! - OBS started recording a clip after its first frame (#1109): the take is
//!   missing its opening, so it isn't captured.
//! - The game recorded a clip with no start, no stop or no frames (#1401):
//!   a failed clip, not captured, whatever its folder holds.
//! - The game recorded a clip shorter than planned, or without one of its
//!   planned kills (#1401): captured, since the take may still be usable,
//!   but flagged on Last Batch, in a toast and in the activity log.
//!
//! The verdicts themselves are `native::capture_engine::clip_verdict`'s.

use std::path::Path;

use native::capture_engine::ClipSummary;
use native::capture_engine::clip_verdict::{self, ClipVerdict};
use native::log_markdown;

use crate::capture_manager::{CaptureManifest, VerifiedBlock};

/// What the batch's listener heard about its clips while it ran.
#[derive(Debug, Default)]
pub(crate) struct ClipEvidence {
    /// The clips OBS started recording late (#1109).
    pub late_starts: Vec<native::obs::LateStart>,
    /// The hook's summary of each clip it recorded (#1221).
    pub reports: Vec<ClipSummary>,
}

/// The rate the hook's frame count can be read at: the movie modes' video
/// rate. OBS and AGR record something other than the frames the hook counts.
fn video_fps(manifest: &CaptureManifest) -> Option<f64> {
    let mode = manifest.capture_mode.as_str();
    let hlae_video = mode == native::patch::CaptureMode::DirectToVideo.to_str_id()
        || mode == native::patch::CaptureMode::FrameSequence.to_str_id();
    (hlae_video && manifest.capture_fps > 0).then_some(f64::from(manifest.capture_fps))
}

/// Marks each take OBS started recording after its clip's first frame
/// (#1109) as missing its opening: not captured, not renderable, so the
/// highlights stay to capture again and the clean-up leaves it alone.
fn mark_started_late(verified: &mut [VerifiedBlock], late_starts: &[native::obs::LateStart]) {
    for late in late_starts {
        let Some(block) = verified
            .iter_mut()
            .find(|b| Path::new(&b.take_folder) == late.take_folder)
        else {
            continue;
        };
        block.started_late_ms = Some(late.late.as_millis() as u64);
        block.captured = false;
        block.renderable = false;
        log_markdown(&crate::messages::capture::take_verify_started_late(
            &block.take_key,
            block.started_late_ms.unwrap_or_default(),
            &block.take_folder,
        ));
    }
}

/// Puts each clip's verdict on its take: a failed clip is not captured, a
/// short one stays captured with the warning. A take the batch's end cut
/// short or whose video stopped early already says why it isn't whole, and
/// keeps that. Returns how many captured takes are short.
fn mark_verdicts(
    manifest: &CaptureManifest,
    verified: &mut [VerifiedBlock],
    reports: &[ClipSummary],
) -> usize {
    let verdicts = clip_verdict::verdicts(&manifest.blocks, reports, video_fps(manifest));
    let mut short = 0;
    for (block, verdict) in verified.iter_mut().zip(verdicts) {
        let Some(verdict) = verdict else { continue };
        if block.cut_short || block.started_late_ms.is_some() {
            continue;
        }
        block.clip_verdict = Some(verdict);
        match verdict {
            ClipVerdict::Failed(why) => {
                block.captured = false;
                block.renderable = false;
                log_markdown(&crate::messages::capture::take_verify_clip_failed(
                    &block.take_key,
                    why,
                    &block.take_folder,
                ));
            }
            ClipVerdict::Short(s) => {
                short += usize::from(block.captured);
                log_markdown(&crate::messages::capture::take_verify_clip_short(
                    &block.take_key,
                    s,
                ));
            }
        }
    }
    short
}

/// Applies everything in `evidence` to `verified`. Returns how many
/// captured takes are shorter than planned, for the batch's last line.
pub(crate) fn apply(
    manifest: &CaptureManifest,
    verified: &mut [VerifiedBlock],
    evidence: &ClipEvidence,
) -> usize {
    mark_started_late(verified, &evidence.late_starts);
    mark_verdicts(manifest, verified, &evidence.reports)
}

#[cfg(test)]
mod tests {
    use super::*;
    use native::capture_engine::clip_verdict::{ClipFailure, ShortClip};
    use native::hlcr::take_plan::TakePlan;

    fn manifest(mode: &str, plans: &[Option<f64>]) -> CaptureManifest {
        CaptureManifest {
            session_id: "s".into(),
            blocks: plans
                .iter()
                .enumerate()
                .map(|(i, length)| native::patch::types::CaptureBlock {
                    demo_name: "a".into(),
                    block_index: i,
                    drive_index: 0,
                    take_folder: format!("a_b{i}").into(),
                    take_key: format!("s/a_b{i}"),
                    source_streak_indices: vec![i],
                    start_tick: 0,
                    end_tick: 0,
                    record_start_tick: 0,
                    record_stop_tick: 0,
                    ramped_gaps: 0,
                    plan: length.map(|length_secs| TakePlan {
                        length_secs,
                        kill_offsets_secs: vec![1.0],
                        sped_up: Vec::new(),
                    }),
                })
                .collect(),
            capture_fps: 60,
            capture_mode: mode.into(),
            agr_fps: 0,
            streak_uids: vec![],
            streak_sources: vec![],
            project_path: None,
            settings: None,
        }
    }

    fn on_disk(manifest: &CaptureManifest) -> Vec<VerifiedBlock> {
        manifest
            .blocks
            .iter()
            .map(|b| VerifiedBlock {
                take_key: b.take_key.clone(),
                take_folder: b.take_folder.to_string_lossy().into_owned(),
                demo_name: b.demo_name.clone(),
                block_index: b.block_index,
                source_streak_indices: b.source_streak_indices.clone(),
                captured: true,
                renderable: true,
                rendered: false,
                bytes: 1,
                cut_short: false,
                cut_by: None,
                short_take: None,
                started_late_ms: None,
                clip_verdict: None,
                ramped_gaps: 0,
            })
            .collect()
    }

    /// The hook's summary of clip `n` of demo 1.
    fn report(n: u32, line: &str) -> ClipSummary {
        let mut c = native::capture_engine::ClipSummary {
            markers: Vec::new(),
            ..Default::default()
        };
        for field in line.split(' ') {
            match field.split_once('=') {
                Some(("start", v)) => c.start = v.parse().ok(),
                Some(("stop", v)) => c.stop = v.parse().ok(),
                Some(("frames", v)) => c.frames = v.parse().ok(),
                Some(("markers", v)) => c.markers = v.split(',').map(str::to_string).collect(),
                _ => {}
            }
        }
        c.demo = Some((1, 1));
        c.demo_clip = Some((n, 2));
        c
    }

    /// #1109: a take OBS started after its clip did is missing the opening.
    #[test]
    fn a_take_obs_started_late_is_not_captured_and_says_by_how_much() {
        let m = manifest("obs", &[None, None]);
        let mut blocks = on_disk(&m);
        let evidence = ClipEvidence {
            late_starts: vec![native::obs::LateStart {
                take_folder: "a_b1".into(),
                late: std::time::Duration::from_millis(240),
            }],
            reports: vec![],
        };
        apply(&m, &mut blocks, &evidence);
        let got: Vec<_> = blocks
            .iter()
            .map(|b| (b.captured, b.renderable, b.started_late_ms))
            .collect();
        assert_eq!(got, [(true, true, None), (false, false, Some(240))]);
        assert!(blocks.iter().all(|b| !b.cut_short));
    }

    /// #1398's case S: the game's clip had no start, the take folder held
    /// something, and the highlights used to be marked Captured.
    #[test]
    fn a_failed_clip_is_not_captured_however_full_its_folder() {
        let m = manifest("direct_to_video", &[Some(10.0), Some(10.0)]);
        let mut blocks = on_disk(&m);
        let evidence = ClipEvidence {
            late_starts: vec![],
            reports: vec![
                report(
                    1,
                    "start=0 stop=10 frames=600 markers=AUDIO_SYNC,START_RECORD,STOP_RECORD",
                ),
                report(2, "stop=30 markers=AUDIO_SYNC,STOP_RECORD"),
            ],
        };
        assert_eq!(apply(&m, &mut blocks, &evidence), 0);
        assert!(blocks[0].captured && blocks[0].clip_verdict.is_none());
        assert!(!blocks[1].captured && !blocks[1].renderable);
        assert_eq!(
            blocks[1].clip_verdict,
            Some(ClipVerdict::Failed(ClipFailure::NoStart))
        );
    }

    #[test]
    fn a_short_clip_stays_captured_with_its_warning() {
        let m = manifest("direct_to_video", &[Some(20.0)]);
        let mut blocks = on_disk(&m);
        let evidence = ClipEvidence {
            late_starts: vec![],
            reports: vec![report(
                1,
                "start=0 stop=12 frames=720 markers=START_RECORD,STOP_RECORD",
            )],
        };
        assert_eq!(apply(&m, &mut blocks, &evidence), 1);
        assert!(blocks[0].captured);
        assert_eq!(
            blocks[0].clip_verdict,
            Some(ClipVerdict::Short(ShortClip {
                recorded_secs: 12.0,
                planned_secs: 20.0,
                kills_outside: 0
            }))
        );
    }

    /// The batch's end already says why a take it cut short isn't whole.
    #[test]
    fn a_take_the_batch_end_cut_short_keeps_its_own_reason() {
        let m = manifest("direct_to_video", &[Some(20.0)]);
        let mut blocks = on_disk(&m);
        blocks[0].cut_short = true;
        blocks[0].captured = false;
        let evidence = ClipEvidence {
            late_starts: vec![],
            reports: vec![report(1, "start=0 frames=300 markers=START_RECORD")],
        };
        apply(&m, &mut blocks, &evidence);
        assert_eq!(blocks[0].clip_verdict, None);
    }

    #[test]
    fn only_the_movie_modes_read_the_hooks_frame_count() {
        assert_eq!(video_fps(&manifest("direct_to_video", &[])), Some(60.0));
        assert_eq!(video_fps(&manifest("frame_sequence", &[])), Some(60.0));
        assert_eq!(video_fps(&manifest("obs", &[])), None);
        assert_eq!(video_fps(&manifest("agr", &[])), None);
    }
}
