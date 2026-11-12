// messages/capture.rs
// Activity-log lines and status texts of the capture batch's Tauri layer
// (`capture_manager.rs`, `capture_room_manager.rs`): the take check after a
// batch, the batch start-up failures and render-while-capturing (#237). Each
// function returns the text only; the caller still decides where it goes.

use std::fmt::Display;

// ── capture_manager.rs: take verification ────────────────────────────────

/// The capture settings file next to a take could not be written.
pub fn take_verify_settings_not_recorded(folder: impl Display, err: impl Display) -> String {
    format!(
        "[take-verify] could not record capture settings at {folder} — {err}. Render Studio will not be able to warn about an FPS mismatch for this take."
    )
}

/// The settings a batch records beside each of its takes (part of #1085).
pub fn take_settings_recorded(s: &native::hlcr::take_settings::CaptureSettings) -> String {
    let hook = if s.hook.is_empty() {
        "none found"
    } else {
        s.hook.as_str()
    };
    format!(
        "[capture] Each take records the settings it was captured with: {mode} {width}x{height} at {fps} fps, FOV {fov}, {install}, hook DLL {hook}, {lines} Initial Commands, {commands} Scheduled Commands, {before} s before and {after} s after. Hash {hash}.",
        mode = s.mode,
        width = s.width,
        height = s.height,
        fps = s.fps,
        fov = s.fov,
        install = s.install,
        lines = s.lines.len(),
        commands = s.commands.len(),
        before = s.before,
        after = s.after,
        hash = s.hash,
    )
}

/// OBS began recording after the clip's first frame.
pub fn take_verify_started_late(take_key: &str, ms: impl Display, folder: &str) -> String {
    format!(
        "[take-verify] {take_key}: OBS started recording {ms} ms after the clip's first frame, so the take is missing its opening: not counted as captured. It is left at {folder}."
    )
}

/// A take whose recording stopped before the clip ended.
pub fn take_verify_cut_short(take_key: &str, why: impl Display, folder: &str) -> String {
    format!(
        "[take-verify] {take_key} {why}: not counted as captured. Its partial take is left at {folder}."
    )
}

/// The `why` of a cut-short take that stopped writing video.
pub fn take_stopped_writing_video(video_secs: f64, audio_secs: f64) -> String {
    format!("stopped writing video early ({video_secs:.2} s of video, {audio_secs:.2} s of audio)")
}

/// One line per session after the take check.
pub fn take_verify_session_summary(
    session_id: impl Display,
    captured: usize,
    total: usize,
    renderable: usize,
) -> String {
    format!(
        "[take-verify] session {session_id}: {captured}/{total} takes on disk or rendered while capturing, {renderable} renderable or rendered"
    )
}

/// One line per take after the take check.
pub fn take_verify_block_line(
    take_key: &str,
    captured: bool,
    renderable: bool,
    folder: &str,
) -> String {
    format!("[take-verify] {take_key} captured={captured} renderable={renderable} at {folder}")
}

// ── capture_manager.rs: batch start-up ───────────────────────────────────

/// A `[capture]` line carrying an error that stopped the batch.
pub fn capture_error_line(err: impl Display) -> String {
    format!("[capture] {err}")
}

/// Building the queue of jobs failed.
pub fn build_batch_queue_failed(err: impl Display) -> String {
    format!("[capture] build_batch_queue failed: {err}")
}

pub const BUILD_BATCH_QUEUE_NO_JOBS: &str = "[capture] build_batch_queue produced no jobs";

/// The status the footer shows when the queue came out empty.
pub const STATUS_NO_JOBS: &str = "No jobs produced";

/// The batch was refused before it started.
pub fn batch_not_started_line(status: impl Display) -> String {
    format!("⚠️ **Batch not started** — {status}")
}

/// A seeking batch fell back to patching the demos.
pub fn batch_patched_instead_of_seeking(why: impl Display) -> String {
    format!("ℹ️ **This batch is patched instead of seeking between clips** — {why}")
}

/// Patching a source demo failed.
pub fn patch_failed_line(demo: impl Display, err: impl Display) -> String {
    format!("[capture] Failed to patch {demo}: {err}")
}

/// The same failure as the footer shows it.
pub fn patch_failed_status(demo: impl Display, err: impl Display) -> String {
    format!("Failed to patch {demo}: {err}")
}

/// The event-listener thread panicked.
pub fn listener_panicked_line(msg: impl Display) -> String {
    format!("[capture] Event listener thread panicked: {msg}")
}

/// The same panic as the footer shows it.
pub fn listener_panicked_status(msg: impl Display) -> String {
    format!("Internal error in capture event listener: {msg}")
}

// ── capture_room_manager.rs: render while capturing ──────────────────────

/// Render-while-capturing switched on for a batch.
pub fn room_on(clips: usize, renders: usize, can_hold: bool) -> String {
    let tail = if can_hold {
        ", and the batch holds between clips when the next one would not fit"
    } else {
        "; this batch is patched, so it can't hold for space"
    };
    let at_once = if renders == 1 {
        "one render at a time".to_string()
    } else {
        format!("up to {renders} renders at a time")
    };
    format!(
        "[render-while-capturing] on for {clips} clip(s): each take renders once recorded, {at_once}{tail}"
    )
}

/// The next clip would not fit beside the one recording.
pub fn room_holding_after_clip(
    clip: usize,
    needed: impl Display,
    free: impl Display,
    decision: impl std::fmt::Debug,
) -> String {
    format!(
        "[render-while-capturing] Clip {clip} needs {needed} free with the one recording now (free {free}): {decision:?}. Holding the batch after this clip."
    )
}

/// A clip never settled into a take.
pub fn room_clip_did_not_settle(clip: usize, folder: impl Display) -> String {
    format!(
        "[render-while-capturing] Clip {clip} at {folder} didn't settle into a take to render; left for after the batch"
    )
}

/// OBS started recording after the clip did.
pub fn room_clip_started_late(clip: usize, folder: impl Display) -> String {
    format!(
        "[render-while-capturing] Clip {clip} at {folder}: OBS started recording after the clip did; not rendering a take missing its opening"
    )
}

/// The clip's video stopped early.
pub fn room_clip_stopped_early(
    clip: usize,
    folder: impl Display,
    video_secs: f64,
    audio_secs: f64,
) -> String {
    format!(
        "[render-while-capturing] Clip {clip} at {folder} stopped writing video early ({video_secs:.2} s of video, {audio_secs:.2} s of audio); not rendering a partial take"
    )
}

/// A clip was recorded and its render is queued.
pub fn room_clip_recorded(clip: usize, size: impl Display) -> String {
    format!("[render-while-capturing] Clip {clip} recorded ({size}); queueing its render")
}

pub const ROOM_BATCH_HOLDING: &str = "[render-while-capturing] The batch is holding between clips";

/// Room opened up and the batch goes on.
pub fn room_going_on(clip: usize, free: impl Display, waited_secs: f64) -> String {
    format!(
        "[render-while-capturing] Room for clip {clip} ({free} free); going on after {waited_secs:.0} s"
    )
}

/// The batch stops before a clip because there is no room.
pub fn room_stopping_batch(clip: usize, why: impl Display) -> String {
    format!(
        "[render-while-capturing] Stopping the batch before clip {clip}: {why}. The clips recorded so far are kept."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn take_lines_keep_their_wording() {
        assert_eq!(
            take_verify_started_late("k", 40, "/f"),
            "[take-verify] k: OBS started recording 40 ms after the clip's first frame, so the take is missing its opening: not counted as captured. It is left at /f."
        );
        assert_eq!(
            take_stopped_writing_video(1.0, 2.5),
            "stopped writing video early (1.00 s of video, 2.50 s of audio)"
        );
    }

    #[test]
    fn room_lines_keep_their_wording() {
        assert!(
            room_on(3, 1, false).ends_with("; this batch is patched, so it can't hold for space")
        );
        assert!(room_on(3, 1, true).contains("once recorded, one render at a time, and"));
        assert!(room_on(3, 2, true).contains("once recorded, up to 2 renders at a time, and"));
        assert_eq!(
            room_going_on(2, "5 GB", 12.4),
            "[render-while-capturing] Room for clip 2 (5 GB free); going on after 12 s"
        );
    }
}
