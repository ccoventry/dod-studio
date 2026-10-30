//! Which highlights a batch records together: one chain job per demo and
//! player, and inside each job one recording block per run of highlights
//! close enough to share a take.

use super::*;

/// One chain job's highlights: every highlight of one demo and one player,
/// sorted by start tick, each with its index in the batch's highlight list.
pub(super) struct HighlightGroup<'a> {
    pub source_demo: &'a str,
    pub target_player: Option<&'a str>,
    /// Never empty: a group exists because a highlight named it.
    pub highlights: Vec<(usize, &'a CaptureStreak)>,
}

impl HighlightGroup<'_> {
    /// The group's earliest highlight, which stands for the demo: its fps,
    /// frame count, match start and spectator flag are the job's.
    pub fn first(&self) -> &CaptureStreak {
        // `group_highlights` makes a group only from a highlight it pushes.
        self.highlights[0].1
    }
}

/// Groups a batch's highlights into chain jobs, one per demo and player, in
/// the order their first highlights start.
///
/// Each highlight carries its index in `highlights` so the blocks built from
/// a group can point back at the exact highlights the caller dispatched, even
/// after [`cut_blocks`] collapses several into one recording.
pub(super) fn group_highlights(highlights: &[CaptureStreak]) -> Vec<HighlightGroup<'_>> {
    let mut grouped: std::collections::HashMap<(&str, Option<&str>), Vec<(usize, &CaptureStreak)>> =
        std::collections::HashMap::new();
    for (idx, streak) in highlights.iter().enumerate() {
        grouped
            .entry((streak.source_demo.as_str(), streak.target_player.as_deref()))
            .or_default()
            .push((idx, streak));
    }

    let mut sorted_groups: Vec<_> = grouped.into_iter().collect();
    sorted_groups
        .sort_by_key(|(_, streaks)| streaks.iter().map(|(_, s)| s.start_tick).min().unwrap_or(0));
    sorted_groups
        .into_iter()
        .map(|((source_demo, target_player), mut highlights)| {
            highlights.sort_by_key(|(_, s)| s.start_tick);
            HighlightGroup {
                source_demo,
                target_player,
                highlights,
            }
        })
        .collect()
}

/// A chain job's recording blocks. The four lists are index-aligned: entry
/// `i` of each describes block `i`.
pub(super) struct Blocks {
    /// Each block as one highlight: the first highlight it covers, its
    /// `end_tick` and kill list stretched over the ones merged into it.
    pub streaks: Vec<CaptureStreak>,
    /// The batch indices of the highlights each block covers. The merge keeps
    /// only the first streak's fields, so the mapping back to the caller's
    /// highlights would otherwise be lost.
    pub sources: Vec<Vec<usize>>,
    /// Whether the block runs straight on from the one before at normal
    /// speed, with no fast-forward between them.
    pub chained_to_previous: Vec<bool>,
    /// Every kill each block records, for the gaps between them (#831). The
    /// merge keeps only an absorbed highlight's last kill in the block's own
    /// list, which would hide the gaps inside it.
    pub kill_frames: Vec<Vec<i32>>,
}

/// Cuts one chain job's highlights (sorted by start tick) into recording
/// blocks, at the demo's `demo_fps`.
///
/// Two separate questions, deliberately not conflated (they used to be,
/// which made every roll-window collision collapse into one take full of
/// dead air between the two highlights):
///
///  1. Do the recordings themselves overlap? Only then must the two
///     highlights become a single take — you can't run two
///     mirv_recordmovie sessions at once.
///  2. Otherwise they stay separate takes, but if the fast-forward
///     round trip between them doesn't fit, it's dropped and playback
///     just stays at normal speed across the gap. Costs a couple of
///     seconds of real-time playback and yields two clean clips instead
///     of one blob. The stopsound flush goes with it — that exists to
///     repair audio the fast-forward desyncs, and there's no
///     fast-forward here to repair.
pub(super) fn cut_blocks(
    highlights: &[(usize, &CaptureStreak)],
    config: &PatcherConfig,
    demo_fps: f32,
) -> Blocks {
    let dynamic_pre_roll_ticks = (config.pre_roll_seconds * demo_fps) as i32;
    let dynamic_post_roll_ticks = (config.post_roll_seconds * demo_fps) as i32;
    let start_lead_ticks = (config.record_start_lead * demo_fps) as i32;
    let stop_trail_ticks = (config.record_stop_trail * demo_fps) as i32;

    let mut merged_streaks: Vec<CaptureStreak> = Vec::new();
    let mut merged_sources: Vec<Vec<usize>> = Vec::new();
    let mut chained_to_previous: Vec<bool> = Vec::new();
    let mut merged_kill_frames: Vec<Vec<i32>> = Vec::new();

    for &(payload_idx, current) in highlights {
        let current = current.clone();
        let current_kills = block_kill_frames(&current);
        if merged_streaks.is_empty() {
            merged_streaks.push(current);
            merged_sources.push(vec![payload_idx]);
            chained_to_previous.push(false);
            merged_kill_frames.push(current_kills);
            continue;
        }

        // Past the first highlight the four lists are non-empty: they grow
        // together, and the first highlight always pushes.
        let prev_stop = last_kill_frame(merged_streaks.last().unwrap());
        let next_start = first_kill_frame(&current);

        // Recordings overlap, or sit too close for a safe stop/start cycle.
        // OBS needs longer between takes than HLAE does: ~1.065s to
        // finalise a file after `StopRecord` returned, against HLAE's
        // ~0.28s (#9). So each mode brings its own separation, and the
        // HLAE one would ask OBS for a stop/start cycle it cannot service.
        let min_separation_ticks = (take_separation_seconds(config) * demo_fps) as i32;
        if blocks_merge(
            prev_stop,
            next_start,
            start_lead_ticks,
            stop_trail_ticks + min_separation_ticks,
        ) {
            let last = merged_streaks.last_mut().unwrap();
            last.end_tick = last.end_tick.max(current.end_tick);
            // The record-stop mark is derived from kills[end_index], not
            // from end_tick, so the absorbed highlight's final kill has to
            // join this block's kill list too. Without it recording stops at
            // the *first* highlight's last kill and everything merged in
            // after that is missing from the take — while still looking like
            // one successfully captured block.
            let absorbed_last = current.end_index.min(current.kills.len().saturating_sub(1));
            if let Some(kill) = current.kills.get(absorbed_last).cloned() {
                last.kills.push(kill);
                last.end_index = last.kills.len() - 1;
            }
            merged_sources.last_mut().unwrap().push(payload_idx);
            let kills = merged_kill_frames.last_mut().unwrap();
            kills.extend(current_kills);
            kills.sort_unstable();
            kills.dedup();
        } else {
            let rolls_collide = blocks_merge(
                prev_stop,
                next_start,
                start_lead_ticks + dynamic_pre_roll_ticks,
                stop_trail_ticks + dynamic_post_roll_ticks,
            );
            merged_streaks.push(current);
            merged_sources.push(vec![payload_idx]);
            chained_to_previous.push(rolls_collide);
            merged_kill_frames.push(current_kills);
        }
    }

    Blocks {
        streaks: merged_streaks,
        sources: merged_sources,
        chained_to_previous,
        kill_frames: merged_kill_frames,
    }
}
