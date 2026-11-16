// messages/render.rs
// Activity-log lines of the render pipeline's export-drive reservations,
// frame-rate choice, stopping on Studio's way out and segmented renders
// (`hlcr/renderer.rs`) (#237, #1439, #1451).

use std::fmt::{Debug, Display};

const GB: f64 = 1024.0 * 1024.0 * 1024.0;

/// A job's reservation on an export drive was given back.
pub fn reservation_released(
    job_id: impl Display,
    amount: u64,
    dir: impl Debug,
    remaining: u64,
) -> String {
    format!(
        "[render-reservation] job {job_id} released {:.2} GB on {dir:?} ({:.2} GB still reserved by other jobs)",
        amount as f64 / GB,
        remaining as f64 / GB,
    )
}

/// A job claimed room on an export drive.
pub fn reservation_made(
    job_id: impl Display,
    estimate: u64,
    dir: impl Debug,
    already_reserved: u64,
    live_free_text: impl Display,
) -> String {
    format!(
        "[render-reservation] job {job_id} reserved {:.2} GB on {dir:?} (already had {:.2} GB reserved by other jobs, {live_free_text} live free)",
        estimate as f64 / GB,
        already_reserved as f64 / GB,
    )
}

/// Which frame rate the job reads its frames at, and where that came from.
pub fn render_fps(job_id: impl Display, fps_used: u32, source: impl Display) -> String {
    format!("[render-fps] job {job_id} — reading the frames at {fps_used} fps, {source}")
}

/// Why a render is refused when no export folder has room for it: `needed`
/// is the job's estimate plus the usual safety margin; `note` is added after
/// (the Windows-drive note, #1430, or nothing).
pub fn no_export_folder_with_room(folders: usize, needed: u64, note: impl Display) -> String {
    format!(
        "Not rendered: every export folder ({folders}) has less than {:.1} GB free once the other renders running now are counted. Free some space or add another export folder in Configuration → Render Settings.{note}",
        needed as f64 / GB
    )
}

pub const FPS_FROM_TAKE: &str = "the rate the take was captured at";

pub fn fps_from_render_settings(take_folder: impl Display) -> String {
    format!(
        "Render Settings' Source FPS (the take doesn't say what it was captured at; {take_folder})"
    )
}

/// A clip name cut at render time to keep its path within Windows' limit in
/// an export folder longer than the ones it was cut for when queued (#1430).
pub fn clip_name_shortened(name: &str, folder: impl Display) -> String {
    format!(
        "[render] Clip name \"{name}\" shortened to fit the path limit in {folder}, an export folder longer than the ones it was named for"
    )
}

/// A render's FFmpeg could not be tied to Studio's job object (#1439): it
/// would outlive a Studio that exits while it runs.
pub fn not_tied_to_studio(job_id: impl Display, err: impl Display) -> String {
    format!(
        "[render] job {job_id}: FFmpeg could not be set to end with DoD Studio ({err}); closing DoD Studio while it runs may leave it running"
    )
}

/// Why a finished render kept its partial name.
pub fn rename_failed(partial: impl Display, out_file: impl Display, err: impl Display) -> String {
    format!("Rendered, but could not rename {partial} to {out_file}: {err}")
}

/// Studio's way out could not end its render FFmpegs.
pub fn exit_stop_failed(err: impl Display) -> String {
    format!("[render] DoD Studio is closing: could not stop its running renders: {err}")
}

/// Studio's way out stopped `stopped` renders and removed `removed` of their
/// unfinished files (one not written yet has none to remove).
pub fn exit_renders_stopped(stopped: usize, removed: usize, took: std::time::Duration) -> String {
    format!(
        "[render] DoD Studio is closing: stopped {stopped} running render(s) and removed {removed} unfinished file(s) in {} ms",
        took.as_millis()
    )
}

/// A render took a name whose unfinished file a render that never finished
/// left behind (#1439): it is written over.
pub fn leftover_partial_replaced(partial: impl Display) -> String {
    format!("[render] {partial} was left by a render that never finished: writing over it")
}

/// An unfinished render file Studio's way out could not remove.
pub fn exit_partial_left(path: impl Display, err: impl Display) -> String {
    format!("[render] DoD Studio is closing: could not remove the unfinished render {path}: {err}")
}

/// A render cut into segments (#1451) started: how many, how long each, where
/// they are kept until joined, and `reused` ([`segments_reused`], or
/// nothing).
pub fn segments_started(
    job_id: impl Display,
    segments: usize,
    segment_frames: usize,
    dir: impl Display,
    reused: impl Display,
) -> String {
    format!(
        "[render] job {job_id}: rendering in {segments} segments of {segment_frames} frames, kept in {dir} until they are joined{reused}"
    )
}

/// The segments a stopped render finished, which this one keeps.
pub fn segments_reused(ranges: &str, count: usize, total: usize) -> String {
    format!(
        "; reusing segment(s) {ranges} ({count} of {total}) finished by a render that was stopped"
    )
}

/// A segmented render's segments were joined into its clip.
pub fn segments_joined(job_id: impl Display, segments: usize, took: std::time::Duration) -> String {
    format!(
        "[render] job {job_id}: joined {segments} segments into the clip in {} ms",
        took.as_millis()
    )
}

/// Why a segmented render's finished segments were removed without a clip.
pub const SEGMENTS_CANCELLED: &str = "the render was cancelled";
pub const SEGMENTS_FAILED: &str = "the render failed";
pub const SEGMENTS_SHORT: &str = "a segment came out short, so the take renders in one piece";

/// A segment came out with fewer frames than it was cut for: the take's
/// pictures or sound end early there, which one piece trims to as it always
/// did (`number` counts from 1).
pub fn segment_short(
    job_id: impl Display,
    number: usize,
    frames: usize,
    expected: usize,
) -> String {
    format!(
        "[render] job {job_id}: segment {number} came out with {frames} of its {expected} frames (the take ends early there); rendering it in one piece"
    )
}

/// A segmented render that ended without its clip removed its segments.
pub fn segments_removed(job_id: impl Display, removed: usize, why: &str) -> String {
    format!("[render] job {job_id}: removed {removed} finished segment(s): {why}")
}

/// Whose segments a sweep removed, for [`leftover_segments_removed`].
pub const LEFTOVER_OTHER_SETTINGS: &str = "a render of this take with other settings";
pub const LEFTOVER_OTHER_FOLDER: &str = "a stopped render of this take in another export folder (this render's room is in a different one)";
pub const LEFTOVER_TAKE_GONE: &str = "a take that is no longer there";
pub const LEFTOVER_ONE_PIECE: &str =
    "a stopped render of this take, which is now rendering in one piece";

/// Segments no render will join, thrown away: `why` is whose they were.
pub fn leftover_segments_removed(dir: impl Display, why: &str) -> String {
    format!("[render] removed the segments of {why}: {dir}")
}

/// No export folder has room for a segmented render's segments and clip
/// together, so it renders in one piece, which needs room for the clip only.
pub fn segments_no_room(job_id: impl Display, segmented: u64) -> String {
    format!(
        "[render] job {job_id}: no export folder has {:.2} GB free for the segments and the clip together; rendering it in one piece",
        segmented as f64 / GB
    )
}

/// The export folder's path is too long for the join to run in its segment
/// folder (Windows' limit on a working folder), so the render runs whole.
pub fn segments_path_too_long(job_id: impl Display, export_dir: impl Display) -> String {
    format!(
        "[render] job {job_id}: the export folder's path is too long for the segment folder Windows would run the join in ({export_dir}); rendering it in one piece"
    )
}

/// The Render tab row's speed cell while the segments are joined.
pub fn joining_segments(count: usize) -> String {
    format!("Joining {count} segments")
}

/// Why a join that FFmpeg finished without an error is no clip: it holds
/// fewer frames than the segments did when each was counted, so one was
/// damaged on the disk since. The segments are removed with it, so the next
/// render of the take starts afresh.
pub fn join_short(frames: usize, total: usize) -> String {
    format!(
        "Joining the segments gave {frames} of the take's {total} frames: a segment was damaged on the disk. The segments were removed; render the take again to render it afresh."
    )
}

/// Neither the joined clip's header nor FFmpeg said how many frames it
/// holds, so the clip is kept unchecked.
pub fn join_uncounted(job_id: impl Display) -> String {
    format!("[render] job {job_id}: could not count the joined clip's frames; keeping it unchecked")
}

/// Which FFmpeg a render batch runs, logged once as it starts: `banner` is
/// the first line of its `-version`. A user's log then shows the build if a
/// join ever fails (#1451).
pub fn ffmpeg_version(banner: &str, path: impl Display, resumable: bool) -> String {
    format!(
        "[render] Rendering with FFmpeg {banner} ({path}); resumable renders {}",
        on_off(resumable)
    )
}

/// "on" or "off", for a setting in a log line.
fn on_off(on: bool) -> &'static str {
    if on { "on" } else { "off" }
}

/// The batch's FFmpeg version could not be read; the batch goes ahead.
pub fn ffmpeg_version_unread(configured: &str, why: impl Display, resumable: bool) -> String {
    format!(
        "[render] could not read the version of the FFmpeg this batch renders with ({configured}): {why}; resumable renders {}",
        on_off(resumable)
    )
}

/// A segmented render's join failed: the activity log's line.
pub fn join_failed(job_id: impl Display, message: &str) -> String {
    format!("[render] job {job_id}: joining the segments failed: {message}")
}

/// Segments that could not be removed.
pub fn segments_left(dir: impl Display, err: impl Display) -> String {
    format!("[render] could not remove the render segments in {dir}: {err}")
}

/// Another render of this take and stream with the same settings holds its
/// segment folder (or a clean-up of old segments is removing it), so this
/// one runs whole.
pub fn segments_busy(job_id: impl Display) -> String {
    format!(
        "[render] job {job_id}: another render of this take with the same settings, or a clean-up of old segments, is using its segment folder; rendering it in one piece"
    )
}

/// The segment folder could not be made, so the render runs whole.
pub fn segments_folder_failed(
    job_id: impl Display,
    dir: impl Display,
    err: impl Display,
) -> String {
    format!(
        "[render] job {job_id}: could not make the segment folder {dir} ({err}); rendering it in one piece"
    )
}

/// Why a segmented render failed before its join.
pub fn join_list_failed(list: impl Display, err: impl Display) -> String {
    format!("Rendered every segment, but could not write the list to join them, {list}: {err}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_lines_keep_their_wording() {
        let gb = 1024 * 1024 * 1024;
        assert_eq!(
            reservation_released("j1", gb, "D:/x", 2 * gb),
            "[render-reservation] job j1 released 1.00 GB on \"D:/x\" (2.00 GB still reserved by other jobs)"
        );
        assert_eq!(
            reservation_made("j1", gb, "D:/x", 0, "unknown"),
            "[render-reservation] job j1 reserved 1.00 GB on \"D:/x\" (already had 0.00 GB reserved by other jobs, unknown live free)"
        );
        assert_eq!(
            render_fps("j1", 60, FPS_FROM_TAKE),
            "[render-fps] job j1 — reading the frames at 60 fps, the rate the take was captured at"
        );
        assert_eq!(
            fps_from_render_settings("D:/t"),
            "Render Settings' Source FPS (the take doesn't say what it was captured at; D:/t)"
        );
    }

    #[test]
    fn a_short_join_says_how_many_frames_it_got() {
        assert_eq!(
            join_short(300, 1000),
            "Joining the segments gave 300 of the take's 1000 frames: a segment was damaged on the disk. The segments were removed; render the take again to render it afresh."
        );
    }
}
