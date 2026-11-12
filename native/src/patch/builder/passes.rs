//! The batch planner (#1085, UI facelift step 10): which passes a batch
//! makes, which recording blocks each pass records, and what each block
//! needs on disk, worked out with no side effects.
//!
//! A pass is one load of a demo for one player. `build_batch_queue` turns
//! each pass into a chain job, and the take cleanup's disk estimate
//! (`estimate_batch_capture_blocks`) reads the same passes, so the figure a
//! batch makes room by and the batch itself can't disagree about which
//! highlights share a take. `build_batch_queue` can't be asked for that
//! itself: it deletes the last batch's cfgs and makes folders.
//!
//! Today a batch has one pass per demo and player. Variants (step 11) and
//! load-only settings (step 13) will add to what tells two passes apart;
//! whether a variant records in its clip's pass, by seeking back, or in a
//! pass of its own is still open (Q5), so nothing here assumes either.

use super::*;

/// One load of a demo for one player: its highlights, and the recording
/// blocks they are cut into.
pub(super) struct Pass<'a> {
    /// The demo, the player, and their highlights in start order.
    pub group: HighlightGroup<'a>,
    /// The rate the pass's ticks are converted to seconds at: the first
    /// highlight's, or 30 when that isn't positive.
    pub demo_fps: f32,
    /// The recording blocks, each with the batch indices it covers.
    pub blocks: Blocks,
    /// What each block is expected to need on its capture drive, index-aligned
    /// with `blocks`: the figure drives are allocated by.
    pub block_bytes: Vec<u64>,
}

/// The rate a demo's ticks are read at when its highlights carry none.
const FALLBACK_DEMO_FPS: f32 = 30.0;

/// Plans a batch of `highlights` (as dispatched, so block sources index into
/// it): one [`Pass`] per demo and player, in the order their first
/// highlights start, each cut into blocks at the pass's demo rate.
pub(super) fn plan_batch<'a>(
    highlights: &'a [CaptureStreak],
    config: &PatcherConfig,
) -> Vec<Pass<'a>> {
    group_highlights(highlights)
        .into_iter()
        .map(|group| plan_pass(group, config))
        .collect()
}

/// Cuts one demo and player's highlights into blocks and prices each block.
fn plan_pass<'a>(group: HighlightGroup<'a>, config: &PatcherConfig) -> Pass<'a> {
    let demo_fps = Some(group.first().demo_fps)
        .filter(|&fps| fps > 0.0)
        .unwrap_or(FALLBACK_DEMO_FPS);
    let blocks = cut_blocks(&group.highlights, config, demo_fps);
    let block_bytes = blocks
        .streaks
        .iter()
        .map(|streak| block_byte_estimate(config, streak, demo_fps))
        .collect();
    Pass {
        group,
        demo_fps,
        blocks,
        block_bytes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn streak(demo: &str, player: &str, kills: &[i32], fps: f32) -> CaptureStreak {
        CaptureStreak {
            start_tick: kills[0],
            end_tick: kills[kills.len() - 1],
            source_demo: demo.to_string(),
            target_player: Some(player.to_string()),
            kill_count: kills.len(),
            timeline_string: String::new(),
            duration_string: String::new(),
            player_index: 0,
            kills: kills
                .iter()
                .map(|&t| (t, t as f32 / 100.0, "Weapon::Kar".to_string()))
                .collect(),
            start_index: 0,
            end_index: kills.len() - 1,
            total_demo_frames: 50_000,
            demo_fps: fps,
            viewdemo_times: Vec::new(),
            frame_times: std::sync::Arc::new(Vec::new()),
            match_start_tick: None,
            source_key: None,
            spectated: false,
            steam_id: None,
            recorded_this_demo: false,
            status: Default::default(),
        }
    }

    fn config() -> PatcherConfig {
        PatcherConfig {
            record_start_lead: 1.0,
            record_stop_trail: 1.0,
            resolution_width: 100,
            resolution_height: 100,
            capture_fps: 10,
            ..PatcherConfig::default()
        }
    }

    #[test]
    fn one_pass_per_demo_and_player_in_start_order() {
        let highlights = [
            streak("b.dem", "Nifty", &[9000], 100.0),
            streak("a.dem", "m00cat", &[3000], 100.0),
            streak("b.dem", "stealthlol", &[8000], 100.0),
            streak("a.dem", "m00cat", &[20000], 100.0),
        ];
        let passes = plan_batch(&highlights, &config());
        let order: Vec<(&str, Option<&str>, Vec<usize>)> = passes
            .iter()
            .map(|p| {
                (
                    p.group.source_demo,
                    p.group.target_player,
                    p.group.highlights.iter().map(|(i, _)| *i).collect(),
                )
            })
            .collect();
        assert_eq!(
            order,
            vec![
                ("a.dem", Some("m00cat"), vec![1, 3]),
                ("b.dem", Some("stealthlol"), vec![2]),
                ("b.dem", Some("Nifty"), vec![0]),
            ]
        );
    }

    #[test]
    fn highlights_whose_recordings_overlap_share_a_block() {
        let highlights = [
            streak("a.dem", "m00cat", &[3000, 3100], 100.0),
            streak("a.dem", "m00cat", &[3200], 100.0),
            streak("a.dem", "m00cat", &[9000], 100.0),
        ];
        let passes = plan_batch(&highlights, &config());
        assert_eq!(passes.len(), 1);
        assert_eq!(passes[0].blocks.sources, vec![vec![0, 1], vec![2]]);
    }

    #[test]
    fn each_block_is_priced_as_the_drives_are_allocated() {
        let highlights = [
            streak("a.dem", "m00cat", &[3000, 3200], 100.0),
            streak("a.dem", "m00cat", &[9000], 100.0),
        ];
        let config = config();
        let pass = &plan_batch(&highlights, &config)[0];
        // Two seconds of action plus a second each side, then just the two.
        assert_eq!(
            pass.block_bytes,
            vec![
                crate::sys::disk::calculate_raw_sequence_bytes(100, 100, 10, 4.0),
                crate::sys::disk::calculate_raw_sequence_bytes(100, 100, 10, 2.0),
            ]
        );
    }

    #[test]
    fn a_demo_with_no_rate_is_read_at_30_fps() {
        let highlights = [streak("a.dem", "m00cat", &[3000], 0.0)];
        assert_eq!(plan_batch(&highlights, &config())[0].demo_fps, 30.0);
    }

    #[test]
    fn no_highlights_plan_no_passes() {
        assert!(plan_batch(&[], &config()).is_empty());
    }
}
