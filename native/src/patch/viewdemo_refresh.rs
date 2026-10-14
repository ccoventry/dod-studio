//! Highlights whose kills have no demo-player (`viewdemo`) times, filled in
//! from a fresh analysis of their demo before a Seek Between Clips batch
//! (#434). A project saved before `viewdemo_times` existed carries none, and
//! without them the batch has nothing to place its seeks by, so it used to be
//! patched instead, silently. The analyzer gives every kill its time, so the
//! demo is read again (from the analyzer cache when it has it) and each
//! kill's time is matched back by its frame.

use std::collections::HashMap;

use crate::patch::types::CaptureStreak;

/// Whether `streak`'s kills carry usable demo-player times, the test
/// `runtime_batch` applies when it places ticks.
pub fn has_viewdemo_times(streak: &CaptureStreak) -> bool {
    streak
        .kills
        .iter()
        .zip(&streak.viewdemo_times)
        .any(|(_, vd)| vd.is_finite() && *vd > 0.0)
}

/// The demos, each once in order, with a highlight that has kills but no
/// demo-player times.
pub fn demos_needing_times(streaks: &[CaptureStreak]) -> Vec<String> {
    let mut demos: Vec<String> = Vec::new();
    for s in streaks {
        if !s.kills.is_empty()
            && !has_viewdemo_times(s)
            && !demos.iter().any(|d| d == &s.source_demo)
        {
            demos.push(s.source_demo.clone());
        }
    }
    demos
}

/// Gives every highlight of `demo` in `streaks` the demo-player times
/// `fresh` (the demo's highlights as scanned now) has for the same kills,
/// matched by frame. A kill the fresh scan doesn't know keeps no time (0),
/// which `runtime_batch` ignores. Returns how many highlights gained times.
pub fn fill_times(streaks: &mut [CaptureStreak], demo: &str, fresh: &[CaptureStreak]) -> usize {
    let by_tick: HashMap<i32, f32> = fresh
        .iter()
        .filter(|f| f.source_demo == demo)
        .flat_map(|f| {
            f.kills
                .iter()
                .zip(&f.viewdemo_times)
                .filter(|(_, vd)| vd.is_finite() && **vd > 0.0)
                .map(|(kill, vd)| (kill.0, *vd))
        })
        .collect();
    let mut filled = 0;
    for s in streaks
        .iter_mut()
        .filter(|s| s.source_demo == demo && !has_viewdemo_times(s))
    {
        let times: Vec<f32> = s
            .kills
            .iter()
            .map(|k| by_tick.get(&k.0).copied().unwrap_or(0.0))
            .collect();
        if times.iter().any(|t| *t > 0.0) {
            s.viewdemo_times = times;
            filled += 1;
        }
    }
    filled
}

#[cfg(test)]
mod tests {
    use super::*;

    fn streak(demo: &str, kills: &[i32], times: &[f32]) -> CaptureStreak {
        CaptureStreak {
            start_tick: kills.first().copied().unwrap_or(0),
            end_tick: kills.last().copied().unwrap_or(0),
            source_demo: demo.to_string(),
            target_player: None,
            kill_count: kills.len(),
            timeline_string: String::new(),
            duration_string: String::new(),
            player_index: 0,
            kills: kills
                .iter()
                .map(|&t| (t, t as f32 / 100.0, "Weapon::Kar".to_string()))
                .collect(),
            start_index: 0,
            end_index: kills.len().saturating_sub(1),
            total_demo_frames: 20_000,
            demo_fps: 100.0,
            viewdemo_times: times.to_vec(),
            frame_times: std::sync::Arc::new(Vec::new()),
            status: Default::default(),
            match_start_tick: None,
            source_key: None,
            spectated: false,
        }
    }

    #[test]
    fn a_demo_needs_times_when_its_kills_have_none() {
        let streaks = [
            streak("a.dem", &[100, 200], &[]),
            streak("a.dem", &[300], &[0.0]),
            streak("b.dem", &[100], &[55.5]),
            streak("c.dem", &[100], &[f32::NAN]),
        ];
        assert!(!has_viewdemo_times(&streaks[0]));
        assert!(!has_viewdemo_times(&streaks[1]));
        assert!(has_viewdemo_times(&streaks[2]));
        assert_eq!(demos_needing_times(&streaks), ["a.dem", "c.dem"]);
    }

    #[test]
    fn times_are_matched_back_by_kill_frame() {
        let mut streaks = [
            streak("a.dem", &[100, 200], &[]),
            streak("a.dem", &[900], &[]),
            streak("b.dem", &[100], &[]),
        ];
        // The fresh scan groups the kills differently, and knows 100, 200
        // and 300 but not 900.
        let fresh = [
            streak("a.dem", &[100, 200, 300], &[10.5, 11.5, 12.5]),
            streak("b.dem", &[100], &[99.0]),
        ];
        assert_eq!(fill_times(&mut streaks, "a.dem", &fresh), 1);
        assert_eq!(streaks[0].viewdemo_times, [10.5, 11.5]);
        assert!(!has_viewdemo_times(&streaks[1]));
        // Another demo's highlights are left alone.
        assert!(streaks[2].viewdemo_times.is_empty());
        assert!(demos_needing_times(&streaks).contains(&"a.dem".to_string()));
    }

    #[test]
    fn a_highlight_that_has_times_keeps_them() {
        let mut streaks = [streak("a.dem", &[100], &[40.0])];
        let fresh = [streak("a.dem", &[100], &[41.0])];
        assert_eq!(fill_times(&mut streaks, "a.dem", &fresh), 0);
        assert_eq!(streaks[0].viewdemo_times, [40.0]);
    }
}
