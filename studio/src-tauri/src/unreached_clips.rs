//! The clips a batch that ended early never got to (#1434).
//!
//! A cancelled batch, or one stopped by the game closing, leaves its later
//! clips without a take. They aren't missing: nothing went wrong with them,
//! the batch stopped before it reached them. The take check (`capture_manager`'s
//! `check_takes`) marks them, after the disk and the batch's own reports have
//! had their say, so the toast and Last Batch can tell them from a take that
//! should be on disk and isn't.
//!
//! A clip counts as reached when the game started recording it (the hook's
//! `START_RECORD` markers), or when anything shows it was: a take on disk, a
//! cut-short or late take, or a report from the game. Every clip before the
//! last reached one in recording order was reached too, which covers a demo
//! the hook's batch skipped (it sends no markers, so the marker count runs
//! behind the clips it passed).

use native::capture_engine::clip_verdict::recording_order;

use crate::capture_manager::{CaptureManifest, VerifiedBlock};

/// Whether anything on disk or from the game shows the batch got to `block`.
fn shows_it_was_reached(block: &VerifiedBlock) -> bool {
    block.captured
        || block.rendered
        || block.cut_short
        || block.bytes > 0
        || block.short_take.is_some()
        || block.started_late_ms.is_some()
        || block.clip_verdict.is_some()
}

/// Marks every clip of a batch that ended early that it never reached, and
/// returns how many. `started` is how many clips the game started recording
/// (0 when it sent no markers). Only for a batch that ended before its last
/// clip: a complete batch reached every clip, and a take it left out is
/// missing.
pub(crate) fn mark_not_reached(
    manifest: &CaptureManifest,
    verified: &mut [VerifiedBlock],
    started: usize,
) -> usize {
    let order = recording_order(&manifest.blocks);
    let last_reached = order
        .iter()
        .rposition(|&i| verified.get(i).is_some_and(shows_it_was_reached));
    let reached = started.max(last_reached.map_or(0, |at| at + 1));
    let mut marked = 0;
    for &i in order.iter().skip(reached) {
        let Some(block) = verified.get_mut(i) else {
            continue;
        };
        if !shows_it_was_reached(block) {
            block.not_reached = true;
            marked += 1;
        }
    }
    marked
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A batch of `clips` clips over one demo, in recording order.
    fn manifest(clips: usize) -> CaptureManifest {
        CaptureManifest {
            session_id: "s".into(),
            blocks: (0..clips)
                .map(|i| native::patch::types::CaptureBlock {
                    demo_name: "a".into(),
                    block_index: i,
                    drive_index: 0,
                    take_folder: format!("a_b{i}").into(),
                    take_key: format!("s/a_b{i}"),
                    source_streak_indices: vec![i],
                    start_tick: 0,
                    end_tick: 0,
                    record_start_tick: 100 * (i as i32 + 1),
                    record_stop_tick: 0,
                    ramped_gaps: 0,
                    plan: None,
                })
                .collect(),
            capture_fps: 60,
            capture_mode: "frames".into(),
            agr_fps: 0,
            streak_uids: vec![],
            streak_sources: vec![],
            project_path: None,
            settings: None,
        }
    }

    /// The take check's result with the first `on_disk` takes on disk.
    fn verified(manifest: &CaptureManifest, on_disk: usize) -> Vec<VerifiedBlock> {
        manifest
            .blocks
            .iter()
            .enumerate()
            .map(|(i, b)| VerifiedBlock {
                take_key: b.take_key.clone(),
                take_folder: b.take_folder.to_string_lossy().into_owned(),
                demo_name: b.demo_name.clone(),
                block_index: b.block_index,
                source_streak_indices: vec![],
                captured: i < on_disk,
                renderable: i < on_disk,
                rendered: false,
                bytes: 0,
                cut_short: false,
                cut_by: None,
                short_take: None,
                started_late_ms: None,
                clip_verdict: None,
                ramped_gaps: 0,
                not_reached: false,
            })
            .collect()
    }

    fn not_reached(blocks: &[VerifiedBlock]) -> Vec<bool> {
        blocks.iter().map(|b| b.not_reached).collect()
    }

    /// The issue's case: cancelled after clip 1 of 3.
    #[test]
    fn a_cancel_after_the_first_clip_leaves_the_rest_not_reached() {
        let m = manifest(3);
        let mut blocks = verified(&m, 1);
        assert_eq!(mark_not_reached(&m, &mut blocks, 1), 2);
        assert_eq!(not_reached(&blocks), [false, true, true]);
    }

    /// A take the game started but left nothing of is missing, not unreached.
    #[test]
    fn a_clip_the_game_started_is_reached_even_with_nothing_on_disk() {
        let m = manifest(3);
        let mut blocks = verified(&m, 1);
        assert_eq!(mark_not_reached(&m, &mut blocks, 2), 1);
        assert_eq!(not_reached(&blocks), [false, false, true]);
    }

    /// A take missing before one on disk: the batch passed it, so it is
    /// missing, even when the marker count runs behind (a skipped demo).
    #[test]
    fn a_gap_before_a_later_take_is_missing_not_unreached() {
        let m = manifest(4);
        let mut blocks = verified(&m, 0);
        blocks[2].captured = true;
        assert_eq!(mark_not_reached(&m, &mut blocks, 0), 1);
        assert_eq!(not_reached(&blocks), [false, false, false, true]);
    }

    /// The clip Cancel cut short was reached, and so was everything before it.
    #[test]
    fn a_cut_short_clip_counts_as_reached() {
        let m = manifest(4);
        let mut blocks = verified(&m, 1);
        blocks[1].cut_short = true;
        assert_eq!(mark_not_reached(&m, &mut blocks, 0), 2);
        assert_eq!(not_reached(&blocks), [false, false, true, true]);
    }

    /// Cancelled before the first clip started: nothing was reached.
    #[test]
    fn a_cancel_before_any_clip_leaves_every_clip_not_reached() {
        let m = manifest(2);
        let mut blocks = verified(&m, 0);
        assert_eq!(mark_not_reached(&m, &mut blocks, 0), 2);
        assert_eq!(not_reached(&blocks), [true, true]);
    }
}
