//! The clips a batch that ended early never got to (#1434).
//!
//! A cancelled batch, or one stopped by the game closing, leaves its later
//! clips without a take. They aren't missing: nothing went wrong with them,
//! the batch stopped before it reached them. The take check (`capture_manager`'s
//! `check_takes`) marks them, after the disk and the batch's own reports have
//! had their say, so the toast and Last Batch can tell them from a take that
//! should be on disk and isn't.
//!
//! Where the batch stopped comes from the hook's markers, as
//! `capture_manager::mark_cut_short` reads them: the demo playing (the last
//! `DEMO_START`) and the last clip it started (`START_RECORD`'s demo and
//! tick). Every clip of an earlier demo was reached, so a demo the hook's
//! batch skipped (it sends no markers for it) stays missing, an error. In the
//! demo playing, the clips up to the last one started were reached. Anything
//! on disk or reported by the game also counts as reached, wherever it is.
//!
//! A batch with no markers at all (nothing reached the game's pipe) can't say
//! where it stopped, so a clip it never reached can't be told from one it
//! reached and lost: there, only the clips after the last one with anything
//! to show for it count as not reached.

use native::capture_engine::clip_verdict::{demo_ordinals, recording_order};

use crate::capture_manager::{CaptureManifest, Records, VerifiedBlock};

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

/// The first position in recording order the batch did not reach, from its
/// markers: past the last clip the demo playing started, or that demo's
/// first clip when it started none. `None` without markers.
fn first_unreached_by_markers(
    manifest: &CaptureManifest,
    order: &[usize],
    records: &Records,
) -> Option<usize> {
    let playing = records.demo()?;
    let demo = demo_ordinals(&manifest.blocks);
    let demo_first = order
        .iter()
        .position(|&i| demo[i] >= playing)
        .unwrap_or(order.len());
    if records.last_start_demo() != Some(playing) {
        return Some(demo_first);
    }
    let last_started = records.last_start_tick().and_then(|tick| {
        order.iter().position(|&i| {
            demo[i] == playing && i64::from(manifest.blocks[i].record_start_tick) == tick
        })
    });
    // A start with no tick, or none matching: at least the demo's first clip.
    Some(last_started.unwrap_or(demo_first) + 1)
}

/// Marks every clip of a batch that ended early that it never reached, and
/// returns how many. `records` are the batch's markers. Only for a batch that
/// ended before its last clip: a complete batch reached every clip, and a
/// take it left out is missing.
pub(crate) fn mark_not_reached(
    manifest: &CaptureManifest,
    verified: &mut [VerifiedBlock],
    records: &Records,
) -> usize {
    let order = recording_order(&manifest.blocks);
    let last_with_evidence = order
        .iter()
        .rposition(|&i| verified.get(i).is_some_and(shows_it_was_reached))
        .map_or(0, |at| at + 1);
    let first_unreached = match first_unreached_by_markers(manifest, &order, records) {
        Some(at) => at,
        None => last_with_evidence,
    };
    let mut marked = 0;
    for &i in order.iter().skip(first_unreached) {
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
        manifest_of(&[clips])
    }

    /// A batch over several demos with `clips[d]` clips each, in recording
    /// order; a demo's clips start at ticks 100, 200, ...
    fn manifest_of(clips: &[usize]) -> CaptureManifest {
        let blocks = clips
            .iter()
            .enumerate()
            .flat_map(|(d, &n)| (0..n).map(move |i| (d, i)));
        CaptureManifest {
            session_id: "s".into(),
            blocks: blocks
                .map(|(d, i)| native::patch::types::CaptureBlock {
                    demo_name: format!("demo{d}"),
                    block_index: i,
                    drive_index: 0,
                    take_folder: format!("demo{d}_b{i}").into(),
                    take_key: format!("s/demo{d}_b{i}"),
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

    /// The markers of a batch stopped while demo `demo` (1-based) played,
    /// after it started its clip at `last_tick`, if any.
    fn stopped_in(demo: u32, last_tick: Option<i64>) -> Records {
        let mut records = Records::default();
        records.demo_started(demo);
        if let Some(tick) = last_tick {
            records.clip_started(Some(tick));
        }
        records
    }

    /// The issue's case: cancelled after clip 1 of 3.
    #[test]
    fn a_cancel_after_the_first_clip_leaves_the_rest_not_reached() {
        let m = manifest(3);
        let mut blocks = verified(&m, 1);
        assert_eq!(
            mark_not_reached(&m, &mut blocks, &stopped_in(1, Some(100))),
            2
        );
        assert_eq!(not_reached(&blocks), [false, true, true]);
    }

    /// A take the game started but left nothing of is missing, not unreached.
    #[test]
    fn a_clip_the_game_started_is_reached_even_with_nothing_on_disk() {
        let m = manifest(3);
        let mut blocks = verified(&m, 1);
        assert_eq!(
            mark_not_reached(&m, &mut blocks, &stopped_in(1, Some(200))),
            1
        );
        assert_eq!(not_reached(&blocks), [false, false, true]);
    }

    /// The clip Cancel cut short was reached, and so was everything before it.
    #[test]
    fn a_cut_short_clip_counts_as_reached() {
        let m = manifest(4);
        let mut blocks = verified(&m, 1);
        blocks[1].cut_short = true;
        assert_eq!(
            mark_not_reached(&m, &mut blocks, &stopped_in(1, Some(200))),
            2
        );
        assert_eq!(not_reached(&blocks), [false, false, true, true]);
    }

    /// Cancelled once the demo loaded, before its first clip started.
    #[test]
    fn a_cancel_before_any_clip_leaves_every_clip_not_reached() {
        let m = manifest(2);
        let mut blocks = verified(&m, 0);
        assert_eq!(mark_not_reached(&m, &mut blocks, &stopped_in(1, None)), 2);
        assert_eq!(not_reached(&blocks), [true, true]);
    }

    /// Demo 2 skipped by the hook (no markers), cancelled in demo 3's first
    /// clip: demo 2's clips were passed, so they stay missing (an error).
    #[test]
    fn a_skipped_demo_before_the_stop_stays_missing() {
        let m = manifest_of(&[2, 2, 2]);
        let mut blocks = verified(&m, 2);
        let mut records = Records::default();
        records.demo_started(1);
        records.clip_started(Some(100));
        records.clip_started(Some(200));
        records.demo_started(3);
        records.clip_started(Some(100));
        assert_eq!(mark_not_reached(&m, &mut blocks, &records), 1);
        assert_eq!(
            not_reached(&blocks),
            [false, false, false, false, false, true]
        );
    }

    /// Demo 1 skipped, cancelled as demo 2's first clip started with nothing
    /// on disk yet: demo 1's clips are missing, only demo 2's later clips
    /// were not reached.
    #[test]
    fn a_skipped_first_demo_is_missing_when_the_next_one_has_just_started() {
        let m = manifest_of(&[3, 3]);
        let mut blocks = verified(&m, 0);
        assert_eq!(
            mark_not_reached(&m, &mut blocks, &stopped_in(2, Some(100))),
            2
        );
        assert_eq!(
            not_reached(&blocks),
            [false, false, false, false, true, true]
        );
    }

    /// No markers at all: only the clips after the last one with a take.
    #[test]
    fn without_markers_only_clips_after_the_last_take_are_not_reached() {
        let m = manifest(4);
        let mut blocks = verified(&m, 0);
        blocks[1].captured = true;
        assert_eq!(mark_not_reached(&m, &mut blocks, &Records::default()), 2);
        assert_eq!(not_reached(&blocks), [false, false, true, true]);
    }
}
