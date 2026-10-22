//! How long a capture batch has left (#643).
//!
//! Each clip is weighed by the frames it records (`CaptureBlock`'s
//! `record_start_tick`..`record_stop_tick`, frame ordinals, so the demo's own
//! timing rather than an average FPS). Once a clip is done, the time taken so
//! far per frame recorded, launches, loading and fast-forwards included,
//! prices the frames still to record. The pace is the whole batch's so far,
//! so an unfocused game or a long fast-forward moves it gradually instead of
//! making it jump. Early on it runs high: the first clip carries the first
//! launch and the fast-forward to it. A plain average of the estimates kept
//! that error around (measured 2026-10-10: "about 2 min left" with 33 s to
//! go), so it isn't used.

/// The batch's clips in the order the engine records them.
#[derive(Debug, Clone, Default)]
pub struct BatchEta {
    weights: Vec<u64>,
    total: u64,
}

impl BatchEta {
    /// `weights`: the frames each clip records, in batch order. A clip with no
    /// frame range (0 or less) counts as one frame, so it still counts.
    pub fn new(weights: impl IntoIterator<Item = i64>) -> Self {
        let weights: Vec<u64> = weights.into_iter().map(|w| w.max(1) as u64).collect();
        let total = weights.iter().sum();
        Self { weights, total }
    }

    /// Seconds left once `done` clips are recorded, `elapsed_secs` after the
    /// batch started. `None` until the first clip is done (nothing to price
    /// the rest by), and once every clip is.
    pub fn seconds_left(&self, done: u32, elapsed_secs: f64) -> Option<u64> {
        let done = (done as usize).min(self.weights.len());
        let done_weight: u64 = self.weights[..done].iter().sum();
        if done == 0 || done_weight == 0 || done_weight >= self.total {
            return None;
        }
        if !(elapsed_secs.is_finite() && elapsed_secs > 0.0) {
            return None;
        }
        let rate = elapsed_secs / done_weight as f64;
        Some((rate * (self.total - done_weight) as f64).round() as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_until_a_clip_is_done() {
        let eta = BatchEta::new([100, 100, 100]);
        assert_eq!(eta.seconds_left(0, 30.0), None);
    }

    #[test]
    fn the_frames_left_are_priced_at_the_pace_so_far() {
        // One 100-frame clip in 60 s; 300 frames to go.
        let eta = BatchEta::new([100, 200, 100]);
        assert_eq!(eta.seconds_left(1, 60.0), Some(180));
    }

    #[test]
    fn the_pace_is_the_whole_batch_so_far() {
        let eta = BatchEta::new([100, 100, 100, 100]);
        // 1.0 s per frame after the first clip: 300 s left.
        assert_eq!(eta.seconds_left(1, 100.0), Some(300));
        // The second clip took 300 s: 2.0 s per frame over both, 200 left.
        assert_eq!(eta.seconds_left(2, 400.0), Some(400));
    }

    #[test]
    fn the_live_batch_that_set_the_rule() {
        // 2026-10-10, three equal clips: the first done at 108 s (launch and
        // a fast-forward to 31:00 in it), the second at 130 s, the batch
        // over at 164 s.
        let eta = BatchEta::new([1, 1, 1]);
        assert_eq!(eta.seconds_left(1, 108.0), Some(216));
        assert_eq!(eta.seconds_left(2, 130.0), Some(65));
    }

    #[test]
    fn nothing_once_every_clip_is_done() {
        let eta = BatchEta::new([100, 100]);
        assert_eq!(eta.seconds_left(2, 200.0), None);
        assert_eq!(eta.seconds_left(5, 200.0), None);
    }

    #[test]
    fn clips_without_a_frame_range_still_count() {
        let eta = BatchEta::new([0, -5, 0]);
        assert_eq!(eta.seconds_left(1, 10.0), Some(20));
    }

    #[test]
    fn an_unusable_clock_gives_no_estimate() {
        let eta = BatchEta::new([100, 100]);
        assert_eq!(eta.seconds_left(1, 0.0), None);
        assert_eq!(eta.seconds_left(1, f64::NAN), None);
    }
}
