// messages/render.rs
// Activity-log lines of the render pipeline's export-drive reservations and
// frame-rate choice (`hlcr/renderer.rs`) (#237).

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

pub const FPS_FROM_TAKE: &str = "the rate the take was captured at";

pub fn fps_from_render_settings(take_folder: impl Display) -> String {
    format!(
        "Render Settings' Source FPS (the take doesn't say what it was captured at; {take_folder})"
    )
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
