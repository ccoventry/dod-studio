//! How long a capture batch has left (#643).
//!
//! Each clip is weighed by the frames it records (`CaptureBlock`'s
//! `record_start_tick`..`record_stop_tick`, frame ordinals, so the demo's own
//! timing rather than an average FPS). Once a clip is done, the time taken so
//! far per frame recorded, launches, loading and fast-forwards included,
//! prices the frames still to record. Each new estimate is averaged with the
//! last, so an unfocused game or a long fast-forward moves it gradually
//! instead of making it jump.

/// The batch's clips in the order the engine records them, and the rate the
/// estimate settled on so far.
#[derive(Debug, Clone, Default)]
pub struct BatchEta {
    weights: Vec<u64>,
    total: u64,
    rate: Option<f64>,
}

impl BatchEta {
    /// `weights`: the frames each clip records, in batch order. A clip with no
    /// frame range (0 or less) counts as one frame, so it still counts.
    pub fn new(weights: impl IntoIterator<Item = i64>) -> Self {
        let weights: Vec<u64> = weights.into_iter().map(|w| w.max(1) as u64).collect();
        let total = weights.iter().sum();
        Self {
            weights,
            total,
            rate: None,
        }
    }

    /// Seconds left once `done` clips are recorded, `elapsed_secs` after the
    /// batch started. `None` until the first clip is done (nothing to price
    /// the rest by), and once every clip is.
    pub fn seconds_left(&mut self, done: u32, elapsed_secs: f64) -> Option<u64> {
        let done = (done as usize).min(self.weights.len());
        let done_weight: u64 = self.weights[..done].iter().sum();
        if done == 0 || done_weight == 0 || done_weight >= self.total {
            return None;
        }
        if !(elapsed_secs.is_finite() && elapsed_secs > 0.0) {
            return None;
        }
        let rate = elapsed_secs / done_weight as f64;
        let rate = match self.rate {
            Some(previous) => (previous + rate) / 2.0,
            None => rate,
        };
        self.rate = Some(rate);
        Some((rate * (self.total - done_weight) as f64).round() as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_until_a_clip_is_done() {
        let mut eta = BatchEta::new([100, 100, 100]);
        assert_eq!(eta.seconds_left(0, 30.0), None);
    }

    #[test]
    fn the_frames_left_are_priced_at_the_pace_so_far() {
        // One 100-frame clip in 60 s; 300 frames to go.
        let mut eta = BatchEta::new([100, 200, 100]);
        assert_eq!(eta.seconds_left(1, 60.0), Some(180));
    }

    #[test]
    fn a_new_pace_moves_the_estimate_halfway() {
        let mut eta = BatchEta::new([100, 100, 100, 100]);
        // 1.0 s per frame after the first clip: 300 s left.
        assert_eq!(eta.seconds_left(1, 100.0), Some(300));
        // The second clip took 300 s (2.0 s per frame overall): the rate
        // settles at 1.5, so 200 frames left is 300 s, not 400.
        assert_eq!(eta.seconds_left(2, 400.0), Some(300));
    }

    #[test]
    fn nothing_once_every_clip_is_done() {
        let mut eta = BatchEta::new([100, 100]);
        assert_eq!(eta.seconds_left(2, 200.0), None);
        assert_eq!(eta.seconds_left(5, 200.0), None);
    }

    #[test]
    fn clips_without_a_frame_range_still_count() {
        let mut eta = BatchEta::new([0, -5, 0]);
        assert_eq!(eta.seconds_left(1, 10.0), Some(20));
    }

    #[test]
    fn an_unusable_clock_gives_no_estimate() {
        let mut eta = BatchEta::new([100, 100]);
        assert_eq!(eta.seconds_left(1, 0.0), None);
        assert_eq!(eta.seconds_left(1, f64::NAN), None);
    }
}
