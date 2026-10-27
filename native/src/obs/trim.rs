//! Where a clip sits inside its OBS recording (#84).
//!
//! OBS starts recording at `AUDIO_SYNC`, about a second before the clip's
//! first frame, and stops a moment after its last, so every OBS file has a
//! head and a tail that an HLAE take of the same clip doesn't have. Cutting
//! them needs the clip's start and end as positions in the file, and the
//! file's timeline is OBS's frame clock: frame `k` is at `k / fps`.
//!
//! **Placing the recording's frame clock on Studio's clock.** obs-websocket's
//! `GetRecordStatus` answers `outputDuration`, which is the recording's frame
//! count times the frame time, not a wall clock. One answer says the
//! recording's first frame went out within one frame period of
//! `now - outputDuration`; a burst of answers taken across a frame boundary
//! narrows that to a millisecond or two, because the count steps at the
//! boundary. [`FrameClock`] intersects those bounds.
//!
//! **Placing a game moment on that clock.** The game echoes `START_RECORD` and
//! `STOP_RECORD` on the frames HLAE would start and stop recording, and Studio
//! stamps each marker as it arrives (`Marker::received`). A moment's position
//! in the file is then its arrival time minus the first frame's time, plus the
//! fixed delays between the two: how long the marker takes to arrive and how
//! long a game frame takes to reach OBS's output ([`CAPTURE_DELAY_SECONDS`]).

/// How long after the game shows a frame OBS's output has counted the frame
/// that holds it, less how long the marker for that frame takes to reach
/// Studio over the game's events pipe.
///
/// Measured, not derived: the clip start this module places, against the clip
/// start found by matching each OBS file's pictures to an HLAE take of the same
/// clip (`docs/obs_alternate_capture.md`, "Trimming the head and tail").
pub const CAPTURE_DELAY_SECONDS: f64 = 0.0;

/// How much later a marker read from `qconsole.log` arrives than one from the
/// game's events pipe: the log is flushed and polled, the pipe is not.
/// Measured over a 17-block batch: 21-40 ms after the tick that wrote it.
pub const LOG_MARKER_LATENCY_SECONDS: f64 = 0.030;

/// Bounds on when the recording's first frame went out, in seconds on a
/// caller-chosen clock (Studio uses seconds since the block's `StartRecord`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameClock {
    /// One frame period, in seconds.
    frame: f64,
    /// The first frame went out after `lo` ...
    lo: f64,
    /// ... and no later than `hi`.
    hi: f64,
    /// Answers taken.
    samples: u32,
    /// Set when two answers contradict each other, which happens when OBS
    /// stalls or skips frames mid-burst: the bounds are then from the last
    /// answer alone.
    contradicted: bool,
}

impl FrameClock {
    /// `None` for a frame rate that isn't a positive number.
    pub fn new(fps: f64) -> Option<Self> {
        (fps.is_finite() && fps > 0.0).then(|| Self {
            frame: 1.0 / fps,
            lo: f64::NEG_INFINITY,
            hi: f64::INFINITY,
            samples: 0,
            contradicted: false,
        })
    }

    /// Adds one `GetRecordStatus` answer: `output_ms` is its `outputDuration`,
    /// `sent` and `received` when the request went out and the answer came
    /// back.
    ///
    /// OBS counted `n` frames at some moment `q` between `sent` and
    /// `received`. Frame `n` went out at `first + (n - 1) * frame <= q`, and
    /// frame `n + 1` had not yet, `q < first + n * frame`. So
    /// `sent - n * frame < first <= received - (n - 1) * frame`.
    pub fn add(&mut self, sent: f64, received: f64, output_ms: u64) {
        let n = self.frames_in(output_ms);
        let (lo, hi) = if n == 0 {
            // Nothing out yet: the first frame is still to come.
            (sent, f64::INFINITY)
        } else {
            let n = n as f64;
            (sent - n * self.frame, received - (n - 1.0) * self.frame)
        };
        let (new_lo, new_hi) = (self.lo.max(lo), self.hi.min(hi));
        if new_lo <= new_hi {
            (self.lo, self.hi) = (new_lo, new_hi);
        } else {
            self.contradicted = true;
            (self.lo, self.hi) = (lo, hi);
        }
        self.samples += 1;
    }

    /// `outputDuration` back into a frame count. obs-websocket computes it as
    /// `frames * frame_time_ns / 1_000_000`, truncated, so rounding to the
    /// nearest frame recovers the count exactly at any rate below 1000 fps.
    fn frames_in(&self, output_ms: u64) -> u64 {
        (output_ms as f64 / (self.frame * 1000.0)).round() as u64
    }

    /// When the first frame went out, as the middle of the bounds, with half
    /// their width as the uncertainty. `None` until an answer has bounded it
    /// on both sides.
    pub fn first_frame(&self) -> Option<(f64, f64)> {
        (self.lo.is_finite() && self.hi.is_finite())
            .then(|| ((self.lo + self.hi) / 2.0, (self.hi - self.lo) / 2.0))
    }

    /// Bounded to within `tolerance` seconds either way.
    pub fn settled(&self, tolerance: f64) -> bool {
        self.first_frame().is_some_and(|(_, err)| err <= tolerance)
    }

    pub fn samples(&self) -> u32 {
        self.samples
    }

    pub fn contradicted(&self) -> bool {
        self.contradicted
    }

    /// Where a game moment that reached Studio at `arrived` (same clock as
    /// the answers) sits in the file, in seconds from its start. `via_log`
    /// takes off the log's extra latency. Never below zero.
    pub fn position(&self, arrived: f64, via_log: bool) -> Option<f64> {
        let (first, _) = self.first_frame()?;
        let latency = if via_log {
            LOG_MARKER_LATENCY_SECONDS
        } else {
            0.0
        };
        Some((arrived - latency - first + CAPTURE_DELAY_SECONDS).max(0.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FPS: f64 = 60.0;
    const T: f64 = 1.0 / FPS;

    /// What obs-websocket answers at `q` for a recording whose first frame went
    /// out at `first`.
    fn answer(first: f64, q: f64) -> u64 {
        if q < first {
            return 0;
        }
        let frames = ((q - first) / T).floor() as u64 + 1;
        // frames * frame_time_ns / 1e6, truncated, as obs-websocket does.
        frames * 16_666_667 / 1_000_000
    }

    #[test]
    fn a_burst_across_a_frame_boundary_places_the_first_frame_to_a_millisecond() {
        let first = 0.087_3;
        let mut clock = FrameClock::new(FPS).unwrap();
        // 1 ms apart, 0.2 ms round trips, about a second in.
        let mut t = 0.9;
        while t < 0.9 + 1.5 * T {
            clock.add(t, t + 0.000_2, answer(first, t + 0.000_1));
            t += 0.001;
        }
        let (at, err) = clock.first_frame().unwrap();
        assert!((at - first).abs() <= 0.001, "{at} vs {first}");
        assert!(err <= 0.001, "{err}");
        assert!(!clock.contradicted());
        assert!(clock.settled(0.002));
    }

    #[test]
    fn one_answer_bounds_the_first_frame_to_one_frame() {
        let first = 0.05;
        let mut clock = FrameClock::new(FPS).unwrap();
        clock.add(1.0, 1.001, answer(first, 1.0005));
        let (at, err) = clock.first_frame().unwrap();
        assert!((at - first).abs() <= T / 2.0 + 0.001);
        assert!(err <= T / 2.0 + 0.001);
        assert!(!clock.settled(0.002));
    }

    #[test]
    fn nothing_recorded_yet_only_bounds_from_below() {
        let mut clock = FrameClock::new(FPS).unwrap();
        clock.add(0.01, 0.011, 0);
        assert_eq!(clock.first_frame(), None);
        assert_eq!(clock.position(1.0, false), None);
    }

    /// A stall mid-burst makes two answers disagree. The bounds then come from
    /// the latest answer, still right to a frame, and say so.
    #[test]
    fn answers_that_contradict_fall_back_to_the_latest() {
        let mut clock = FrameClock::new(FPS).unwrap();
        clock.add(1.0, 1.001, answer(0.0, 1.0005));
        // As if the first frame had gone out 100 ms later.
        clock.add(1.01, 1.011, answer(0.1, 1.0105));
        assert!(clock.contradicted());
        let (at, _) = clock.first_frame().unwrap();
        assert!((at - 0.1).abs() <= T, "{at}");
    }

    #[test]
    fn a_moment_is_placed_after_the_first_frame_and_the_log_is_corrected() {
        let first = 0.08;
        let mut clock = FrameClock::new(FPS).unwrap();
        let mut t = 0.9;
        while t < 0.9 + 1.5 * T {
            clock.add(t, t + 0.000_2, answer(first, t + 0.000_1));
            t += 0.001;
        }
        let pipe = clock.position(1.0, false).unwrap();
        assert!((pipe - (1.0 - first + CAPTURE_DELAY_SECONDS)).abs() <= 0.001);
        let log = clock.position(1.0, true).unwrap();
        assert!((pipe - log - LOG_MARKER_LATENCY_SECONDS).abs() < 1e-9);
        // A moment before the recording's start is its first frame.
        assert_eq!(clock.position(-5.0, false), Some(0.0));
    }

    #[test]
    fn a_rate_that_isnt_a_rate_has_no_clock() {
        assert!(FrameClock::new(0.0).is_none());
        assert!(FrameClock::new(f64::NAN).is_none());
        assert!(FrameClock::new(-60.0).is_none());
    }

    #[test]
    fn output_duration_is_turned_back_into_the_exact_frame_count() {
        for fps in [30.0, 59.94, 60.0, 120.0, 240.0] {
            let clock = FrameClock::new(fps).unwrap();
            let frame_ns = (1e9 / fps).round() as u64;
            for frames in [1u64, 2, 59, 60, 61, 1234, 12_345] {
                assert_eq!(clock.frames_in(frames * frame_ns / 1_000_000), frames);
            }
        }
    }
}
