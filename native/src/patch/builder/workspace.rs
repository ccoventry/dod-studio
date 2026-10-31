//! The batch workspace (junctions, exit trigger) and the legacy
//! channel-based patch worker.

use super::*;

pub struct WorkspaceGuard {
    pub session_junction: std::path::PathBuf,
    pub exit_trigger: std::path::PathBuf,
    pub pool_junctions: Vec<std::path::PathBuf>,
    /// The `_route_N` junctions `build_batch_queue` creates beside `hl.exe`, one
    /// per drive a batch routes blocks to.
    ///
    /// These were created but never unlinked: the only thing that removed one
    /// was the *next* batch reusing that index, so a `_route_0` sat in the game
    /// folder indefinitely after a capture. Tracked here so they go the same way
    /// as the pool junctions — when the batch ends, not when another begins.
    pub route_junctions: Vec<std::path::PathBuf>,
    pub auto_clear_logs: bool,
    pub auto_clear_temp_demos: bool,
    pub auto_clear_previews: bool,
    pub save_local_patched_copy: bool,
}

impl Drop for WorkspaceGuard {
    fn drop(&mut self) {
        // Junction link: remove_dir unlinks without touching the junction target.
        if let Err(e) = std::fs::remove_dir(&self.session_junction)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            crate::log_markdown(&format!(
                "[cleanup] Failed to remove session_junction {}: {}",
                self.session_junction.display(),
                e
            ));
        }
        // Unlink every dod_pool_N and _route_N junction. `remove_dir` unlinks a
        // junction without touching what it points at, and NotFound is expected
        // for any index this batch did not route to.
        for junction in self
            .pool_junctions
            .iter()
            .chain(self.route_junctions.iter())
        {
            if let Err(e) = std::fs::remove_dir(junction)
                && e.kind() != std::io::ErrorKind::NotFound
            {
                crate::log_markdown(&format!(
                    "[cleanup] Failed to remove pool junction {}: {}",
                    junction.display(),
                    e
                ));
            }
        }
        // Signal dirs (DOD_STUDIO_EXIT_TRIGGER) are directories, not files.
        // Use remove_dir_all; silently ignore NotFound, log anything else.
        if let Err(e) = std::fs::remove_dir_all(&self.exit_trigger)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            crate::log_markdown(&format!(
                "[cleanup] Failed to remove exit_trigger {}: {}",
                self.exit_trigger.display(),
                e
            ));
        }
        if let Some(game_root) = self.exit_trigger.parent() {
            crate::shared::paths::clear_capture_scratch(
                game_root,
                self.auto_clear_logs,
                self.auto_clear_temp_demos,
                self.auto_clear_previews,
                self.save_local_patched_copy,
            );
        }
    }
}

/// Makes the `_route_<N>` junction beside `hl.exe` for every drive in
/// `drives` that is a configured capture directory, pointing at its session
/// folder (`<capture_dir>/<session_id>`, made if missing), so a block's
/// `mirv_movie_filename _route_<N>/...` lands on that drive. A junction left
/// from an earlier batch is replaced. Errors only when a capture directory's
/// path can't be made absolute; a junction that fails to link shows up as a
/// take that never arrives.
pub(super) fn make_route_junctions(
    config: &PatcherConfig,
    drives: &std::collections::HashSet<usize>,
) -> std::io::Result<()> {
    let game_path_buf = std::path::PathBuf::from(&config.game_path);
    let hl_exe_parent = game_path_buf.parent().unwrap_or(std::path::Path::new(""));
    for &drive_idx in drives {
        if let Some(out_dir) = config.capture_directories.get(drive_idx) {
            let absolute_drive = std::path::absolute(out_dir)?;
            let session_dir = if !config.session_id.is_empty() {
                absolute_drive.join(&config.session_id)
            } else {
                absolute_drive
            };

            if !session_dir.exists() {
                let _ = std::fs::create_dir_all(&session_dir);
            }

            let junction_path = hl_exe_parent.join(format!("_route_{}", drive_idx));
            let _ = std::fs::remove_dir(&junction_path);

            let junction_str = junction_path.to_str().unwrap_or_default();
            let target_str = session_dir.to_str().unwrap_or_default();

            if !junction_str.is_empty() && !target_str.is_empty() {
                let _ = std::process::Command::new("cmd")
                    .args(["/C", "mklink", "/J", junction_str, target_str])
                    .output();
            }
        }
    }
    Ok(())
}

// ── Channel-based worker spawner ──────────────────────────────────────────────
// Retained for the cancellation test and any callers that still reference it.
// The primary patch path now uses the inline patch_worker in capture/select.rs.

pub fn spawn_patch_batch(
    jobs: Vec<PatchJob>,
    config: PatcherConfig,
    cancel_token: Arc<AtomicBool>,
) -> CaptureWorker {
    let (tx, rx) = std::sync::mpsc::channel();
    let cancel_token_clone = cancel_token.clone();

    let handle = std::thread::spawn(move || {
        let total_jobs = jobs.len();
        if tx.send(PatchEvent::Starting(total_jobs)).is_err() {
            return;
        }

        let mut cancelled = false;
        for (idx, job) in jobs.iter().enumerate() {
            let start_pct = (idx as f32 / total_jobs as f32) * 100.0;
            if tx
                .send(PatchEvent::Progress(job.source_demo.clone(), start_pct))
                .is_err()
            {
                return;
            }

            let patcher = StreamPatcher::new(&job.source_demo, &job.output_demo);
            match patcher.patch(job, &config, &cancel_token_clone) {
                Ok(()) => {
                    let end_pct = ((idx + 1) as f32 / total_jobs as f32) * 100.0;
                    if tx
                        .send(PatchEvent::Progress(job.source_demo.clone(), end_pct))
                        .is_err()
                    {
                        return;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {
                    std::fs::remove_file(&job.output_demo).ok();
                    let _ = tx.send(PatchEvent::Cancelled);
                    cancelled = true;
                    break;
                }
                Err(e) => {
                    if tx
                        .send(PatchEvent::Error(format!(
                            "Failed to patch {}: {}",
                            job.source_demo, e
                        )))
                        .is_err()
                    {
                        return;
                    }
                }
            }
        }

        if !cancelled {
            let _ = tx.send(PatchEvent::Completed);
        }
    });

    CaptureWorker {
        receiver: rx,
        is_running: true,
        cancel_token,
        handle: Some(handle),
    }
}

// ── Preview-only patcher ──────────────────────────────────────────────────────
// Builds minimal PatchJobs that inject ONLY svc_director STUFFTEXT events into
// a copy of the original demo — no capture commands, no chaining, no CFG files.
// One output demo per source demo, saved as "<stem>_preview.dem" next to the
// original (or inside `output_dir` if configured).
