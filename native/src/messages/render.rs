// messages/render.rs
// Activity-log lines of the render pipeline's export-drive reservations,
// frame-rate choice and stopping on Studio's way out (`hlcr/renderer.rs`)
// (#237, #1439).

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
}
