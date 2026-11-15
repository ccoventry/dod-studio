// messages/render.rs
// Activity-log lines of the render queue (`render_manager.rs`) and the render
// history's helpers (`capture_history_manager.rs`, `render_autosave.rs`)
// (#237).

use std::fmt::{Debug, Display};

/// A lookup of rendered videos that had to search the export folders.
pub fn videos_located(total: usize, moved: usize, missing: usize, ms: u128) -> String {
    format!(
        "[render-locate] {total} rendered video(s): {moved} found again in the export folders, {missing} missing ({ms} ms)"
    )
}

pub fn autosave_write_failed(err: impl Display) -> String {
    format!("[render_autosave] Failed to write lockfile: {err}")
}

pub fn recover_gap(gap: impl Display) -> String {
    format!("[render] recover_render_batch: {gap}")
}

pub fn history_end_not_recorded(take_folder: impl Display, error: impl Display) -> String {
    format!("[render-history] couldn't record how a render of {take_folder} ended: {error}")
}

pub fn history_start_not_recorded(take_folder: impl Display, error: impl Display) -> String {
    format!("[render-history] couldn't record the start of a render of {take_folder}: {error}")
}

pub fn take_finished(
    job_id: impl Display,
    key: impl Debug,
    take_folder: impl Display,
    base_name: impl Display,
    clip_type: impl Display,
) -> String {
    format!(
        "[render-take-finished] job {job_id} take_key={key:?} take_folder={take_folder} base_name={base_name} clip_type={clip_type}"
    )
}

pub fn take_failed(job_id: impl Display, base_name: impl Display, error: impl Display) -> String {
    format!("[render-take-failed] job {job_id} base_name={base_name} error={error}")
}

pub const CLEAR_QUEUE_EMPTIED: &str =
    "[render] clear_render_queue: list emptied once the batch stopped";

pub const SCHEDULER_EXITING: &str =
    "[render] scheduler loop exiting: nothing left Rendering or Queued";

pub fn queue_batch_starting(
    source_dirs: usize,
    codec: impl Display,
    fps: impl Display,
    max_concurrent: impl Display,
) -> String {
    format!(
        "[render] queue_render_batch starting: {source_dirs} source dir(s), codec={codec}, fps={fps}, max_concurrent={max_concurrent}"
    )
}

pub fn queue_batch_found(
    total: usize,
    added: usize,
    refreshed: usize,
    kept: usize,
    gone: usize,
    gone_queued: usize,
) -> String {
    format!(
        "[render] queue_render_batch: {total} take(s) found; {added} new, {refreshed} refreshed, {kept} kept, {gone} gone ({gone_queued} of them queued)"
    )
}

pub const START_REJECTED: &str =
    "[render] start_queued_render rejected: a batch is already in progress";

pub fn scheduler_starting(jobs: usize) -> String {
    format!("[render] start_queued_render: starting scheduler on {jobs} job(s)")
}

pub const CANCEL_BATCH_REQUESTED: &str = "[render] cancel_render_batch requested";

pub fn job_codec_set(job_id: impl Display, codec: impl Display) -> String {
    format!("[render] set_render_job_codec {job_id} -> {codec}")
}

pub fn cancel_job_requested(job_id: impl Display) -> String {
    format!("[render] cancel_render_job requested for {job_id}")
}

pub fn job_removed(job_id: impl Display) -> String {
    format!("[render] remove_render_job {job_id} removed")
}

pub fn non_rendering_removed(count: usize) -> String {
    format!("[render] remove_non_rendering_render_jobs removed {count} job(s)")
}

pub fn requeued_into_running_batch(context: impl Display) -> String {
    format!("[render] {context} queued; the running batch picks it up")
}

pub fn requeued_waits_for_start(context: impl Display) -> String {
    format!("[render] {context} queued; waits for Start Render Batch")
}

pub fn job_reset(job_id: impl Display, previous_status: impl Debug) -> String {
    format!("[render] reset_render_job {job_id} (was {previous_status:?}) -> Queued")
}

pub fn all_jobs_reset(count: usize) -> String {
    format!("[render] reset_all_render_jobs: {count} job(s) -> Queued")
}

pub fn ticked_jobs_cancelled(cancelled: usize, ticked: usize) -> String {
    format!("[render] cancel_render_jobs: {cancelled} of {ticked} ticked job(s) cancelled")
}

pub fn queued_jobs_cancelled(cancelled: usize) -> String {
    format!("[render] cancel_queued_render_jobs: {cancelled} queued job(s) cancelled")
}

pub fn ticked_jobs_reset(reset: usize, ticked: usize) -> String {
    format!("[render] reset_render_jobs: {reset} of {ticked} ticked job(s) -> Queued")
}

pub fn ticked_jobs_removed(removed: usize, ticked: usize) -> String {
    format!("[render] remove_render_jobs: {removed} of {ticked} ticked job(s) removed")
}

pub const CLEAR_QUEUE_CANCELLING: &str =
    "[render] clear_render_queue: cancelling the running batch first";

pub fn queue_cleared(rows: usize) -> String {
    format!("[render] clear_render_queue: {rows} row(s) removed")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_lines_keep_their_wording() {
        assert_eq!(
            videos_located(3, 1, 1, 9),
            "[render-locate] 3 rendered video(s): 1 found again in the export folders, 1 missing (9 ms)"
        );
        assert_eq!(
            autosave_write_failed("denied"),
            "[render_autosave] Failed to write lockfile: denied"
        );
    }

    #[test]
    fn queue_lines_keep_their_wording() {
        assert_eq!(
            take_finished("j", Some(7), "D:/t", "b", "c"),
            "[render-take-finished] job j take_key=Some(7) take_folder=D:/t base_name=b clip_type=c"
        );
        assert_eq!(
            queue_batch_found(5, 1, 2, 3, 4, 5),
            "[render] queue_render_batch: 5 take(s) found; 1 new, 2 refreshed, 3 kept, 4 gone (5 of them queued)"
        );
        assert_eq!(
            job_reset("j", None::<u8>),
            "[render] reset_render_job j (was None) -> Queued"
        );
        assert_eq!(
            requeued_waits_for_start("reset_render_jobs"),
            "[render] reset_render_jobs queued; waits for Start Render Batch"
        );
        assert_eq!(
            ticked_jobs_cancelled(1, 2),
            "[render] cancel_render_jobs: 1 of 2 ticked job(s) cancelled"
        );
        assert_eq!(
            queued_jobs_cancelled(4),
            "[render] cancel_queued_render_jobs: 4 queued job(s) cancelled"
        );
        assert_eq!(
            queue_cleared(3),
            "[render] clear_render_queue: 3 row(s) removed"
        );
    }
}
