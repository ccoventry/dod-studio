//! Where a clip sits inside its OBS recording (#84).
//!
//! OBS starts recording at `AUDIO_SYNC`, about a second before the clip's
//! first frame, and stops a moment after its last, so every OBS file has a
//! head and a tail that an HLAE take of the same clip doesn't have. Cutting
//! them needs the clip's start and end as positions in the file.
//!
//! The game echoes `START_RECORD` and `STOP_RECORD` on the frames HLAE would
//! start and stop recording on, and Studio stamps each marker as it arrives
//! (`Marker::received`). The file's first frame is a fixed delay after
//! `StartRecord` reports the recording started
//! ([`RECORDING_START_DELAY_SECONDS`]), so a marker's position in the file is
//! its arrival time since then, less that delay.
//!
//! **Not from OBS's frame count.** obs-websocket's `outputDuration` counts
//! the frames the muxer has written, and the muxer holds video back until it
//! has the audio to interleave with it. That hold is OBS's audio buffering,
//! which OBS raises when an audio source lags and never lowers: measured on
//! one OBS session, the count ran 0.37 s behind the file in one batch and,
//! after OBS logged "adding 661 milliseconds of audio buffering", over 1.0 s
//! behind in the next, by which time it had not counted a single frame when
//! the clip started.

/// How long after `StartRecord` reports the recording started (its
/// `OUTPUT_STARTED` event) the moment in the file's first frame happened, as
/// seen by a marker from the game's events pipe.
///
/// Measured on POST at 60 fps, OBS 33.0.0, against HLAE takes of the same
/// three clips over three batches, at both ends of each clip
/// (`docs/obs_alternate_capture.md`, "Cutting the head and tail"): 21-46 ms,
/// mean 35. The spread is mostly per clip, the same in every batch.
pub const RECORDING_START_DELAY_SECONDS: f64 = 0.035;

/// How much later a marker read from `qconsole.log` arrives than one from the
/// game's events pipe: the log is flushed and polled, the pipe is not.
/// Measured over a 17-block batch: 21-40 ms after the tick that wrote it.
pub const LOG_MARKER_LATENCY_SECONDS: f64 = 0.030;

/// Where a game moment sits in the recording, in seconds from the file's
/// start, given when its marker arrived: `arrived` seconds after `StartRecord`
/// reported the recording started. `via_log` takes off the log's extra
/// latency. Never below zero: a moment before the recording is its first
/// frame.
pub fn position(arrived: f64, via_log: bool) -> f64 {
    let latency = if via_log {
        LOG_MARKER_LATENCY_SECONDS
    } else {
        0.0
    };
    (arrived - latency - RECORDING_START_DELAY_SECONDS).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_moment_is_placed_after_the_recordings_start_delay() {
        let at = position(1.0, false);
        assert!((at - (1.0 - RECORDING_START_DELAY_SECONDS)).abs() < 1e-12);
    }

    #[test]
    fn a_marker_from_the_log_is_moved_back_by_the_logs_latency() {
        let pipe = position(1.0, false);
        let log = position(1.0, true);
        assert!((pipe - log - LOG_MARKER_LATENCY_SECONDS).abs() < 1e-12);
    }

    #[test]
    fn a_moment_before_the_recording_is_its_first_frame() {
        assert_eq!(position(0.0, false), 0.0);
        assert_eq!(position(-5.0, true), 0.0);
    }
}
