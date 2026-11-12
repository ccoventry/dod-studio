//! `cut_blocks` and `group_highlights`: where a merged take starts and stops
//! when one highlight sits inside another (#1395).

use super::blocks::{Blocks, cut_blocks, group_highlights};
use super::tests::streak_with_kills;
use super::*;

/// Groups and cuts `highlights` the way `plan_batch` does, at the test
/// streaks' 100 fps, with the default config (no start lead or stop trail,
/// HLAE's 0.5 s take separation: highlights 50 ticks apart or closer merge).
fn cut(highlights: &[CaptureStreak]) -> Blocks {
    let groups = group_highlights(highlights);
    assert_eq!(
        groups.len(),
        1,
        "the test highlights share a demo and player"
    );
    cut_blocks(&groups[0].highlights, &PatcherConfig::default(), 100.0)
}

/// `(first kill, last kill)` of each block: where its record start and stop
/// marks key off.
fn record_marks(blocks: &Blocks) -> Vec<(i32, i32)> {
    blocks
        .streaks
        .iter()
        .map(|s| (first_kill_frame(s), last_kill_frame(s)))
        .collect()
}

#[test]
fn a_highlight_nested_in_a_longer_one_keeps_the_longer_ones_stop() {
    let outer = streak_with_kills(1000, 1400, &[1000, 1200, 1400]);
    let inner = streak_with_kills(1100, 1150, &[1100, 1150]);

    let blocks = cut(&[outer, inner]);

    assert_eq!(blocks.sources, vec![vec![0, 1]], "one take for both");
    assert_eq!(
        record_marks(&blocks),
        vec![(1000, 1400)],
        "the take must run to the outer highlight's last kill"
    );
    assert_eq!(blocks.kill_frames, vec![vec![1000, 1100, 1150, 1200, 1400]]);
}

#[test]
fn a_highlight_after_a_nested_merge_is_measured_from_the_blocks_real_stop() {
    let outer = streak_with_kills(1000, 1400, &[1000, 1200, 1400]);
    let inner = streak_with_kills(1100, 1150, &[1100, 1150]);
    // 40 ticks after the outer highlight's last kill: inside the take
    // separation of that stop, far outside it of the inner one's (1150).
    let next = streak_with_kills(1440, 1600, &[1440, 1600]);
    // Beyond the separation from 1600, so it gets its own take.
    let apart = streak_with_kills(1700, 1800, &[1700, 1800]);

    let blocks = cut(&[outer, inner, next, apart]);

    assert_eq!(blocks.sources, vec![vec![0, 1, 2], vec![3]]);
    assert_eq!(record_marks(&blocks), vec![(1000, 1600), (1700, 1800)]);
}

#[test]
fn an_ordinary_overlapping_merge_is_unchanged() {
    let first = streak_with_kills(1000, 1200, &[1000, 1200]);
    let second = streak_with_kills(1240, 1500, &[1240, 1500]);

    let blocks = cut(&[first, second]);

    assert_eq!(blocks.sources, vec![vec![0, 1]]);
    assert_eq!(record_marks(&blocks), vec![(1000, 1500)]);
    // The block keeps the first highlight's kills plus the absorbed one's
    // last kill, which its stop mark points at: the shape before #1395.
    let block = &blocks.streaks[0];
    let kill_ticks: Vec<i32> = block.kills.iter().map(|k| k.0).collect();
    assert_eq!(kill_ticks, vec![1000, 1200, 1500]);
    assert_eq!((block.start_index, block.end_index), (0, 2));
    assert_eq!((block.start_tick, block.end_tick), (1000, 1500));
    assert_eq!(blocks.kill_frames, vec![vec![1000, 1200, 1240, 1500]]);
}

#[test]
fn a_highlight_whose_range_starts_late_is_not_the_takes_start() {
    // A Kill Range edit in the game's Review moves `start_index` but leaves
    // `start_tick` at the whole highlight's first kill. Sorted by
    // `start_tick`, this one would open the block, and the other, whose
    // kills all come before its range, would merge in and never be recorded.
    let mut edited = streak_with_kills(500, 2200, &[500, 2000, 2200]);
    edited.start_index = 1;
    let earlier = streak_with_kills(1000, 1100, &[1000, 1100]);

    let blocks = cut(&[edited, earlier]);

    assert_eq!(blocks.sources, vec![vec![1], vec![0]]);
    assert_eq!(record_marks(&blocks), vec![(1000, 1100), (2000, 2200)]);
}
