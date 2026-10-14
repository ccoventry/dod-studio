//! Delete Take (#644): the Render tab's button that sends a finished take's
//! folder to the Recycle Bin.
//!
//! Two calls, so the confirm can name the size: `measure_take_folders` checks
//! and measures, `delete_take_folders` checks again and moves. The safety check
//! itself (a real take folder inside a Destinations folder) is
//! `native::hlcr::take_delete`; this layer adds what only the Render tab knows,
//! whether a take is rendering. A queued take may go: deleting it pulls its
//! job from the queue (the confirm says so). Only a rendering one is refused,
//! since FFmpeg holds its files open and the move would fail.

use crate::render_manager::RenderManager;
use crate::settings_manager::SettingsManager;
use native::hlcr::take_delete::{TakeDeletion, plan_take_deletion, recycle_take};
use native::log_markdown;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

/// Why a take can't be deleted right now, from the statuses of its jobs, or
/// `None` when it can. A take with a HUD pair has two jobs, and either one
/// rendering refuses it. Queued is fine: removing the rows pulls the job.
pub(crate) fn refusal(take_folder: &str, statuses: &[&str]) -> Option<String> {
    if statuses.is_empty() {
        Some(crate::messages::take_not_listed(take_folder))
    } else if statuses.contains(&"Rendering") {
        Some(crate::messages::take_is_rendering(take_folder))
    } else {
        None
    }
}

/// One take as the confirm and the result show it.
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct TakeSize {
    pub take_folder: String,
    pub take_key: String,
    pub size_bytes: u64,
}

impl TakeSize {
    fn of(take_folder: &str, plan: &TakeDeletion) -> Self {
        Self {
            take_folder: take_folder.to_string(),
            take_key: plan.take_key.clone(),
            size_bytes: plan.size_bytes,
        }
    }
}

#[derive(Serialize)]
pub struct TakeDeleteFailure {
    pub take_folder: String,
    pub error: String,
}

#[derive(Serialize)]
pub struct DeleteTakesResult {
    pub deleted: Vec<TakeSize>,
    pub failed: Vec<TakeDeleteFailure>,
}

/// The picked folders in order, each once: a take with a HUD pair is two rows
/// with the same folder.
fn unique(take_folders: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    take_folders
        .into_iter()
        .filter(|f| seen.insert(f.clone()))
        .collect()
}

/// The configured Destinations folders: the only places a take is ever
/// deleted from.
fn capture_dirs(settings: &SettingsManager) -> Vec<PathBuf> {
    settings
        .inner
        .lock()
        .unwrap()
        .target_drives
        .iter()
        .filter(|d| !d.trim().is_empty())
        .map(PathBuf::from)
        .collect()
}

/// `take_delete_progress` events: which step, and takes done of total. At most
/// one every ~33 ms, plus the first and the last.
struct Progress {
    app: AppHandle,
    step: &'static str,
    total: usize,
    last: Option<Instant>,
}

impl Progress {
    fn new(app: AppHandle, step: &'static str, total: usize) -> Self {
        Self {
            app,
            step,
            total,
            last: None,
        }
    }

    fn report(&mut self, done: usize) {
        let due = done == 0
            || done >= self.total
            || self
                .last
                .is_none_or(|t| t.elapsed() >= Duration::from_millis(33));
        if !due {
            return;
        }
        self.last = Some(Instant::now());
        let _ = self.app.emit(
            "take_delete_progress",
            serde_json::json!({ "step": self.step, "done": done, "total": self.total }),
        );
    }
}

fn check_listed_and_idle(render: &RenderManager, folders: &[String]) -> Result<(), String> {
    for folder in folders {
        let statuses = render.take_statuses(folder);
        let statuses: Vec<&str> = statuses.iter().map(String::as_str).collect();
        if let Some(refusal) = refusal(folder, &statuses) {
            return Err(refusal);
        }
    }
    Ok(())
}

/// Checks each take and measures what deleting it frees, for the confirm.
#[tauri::command]
pub async fn measure_take_folders(
    app: AppHandle,
    render: tauri::State<'_, RenderManager>,
    settings: tauri::State<'_, SettingsManager>,
    take_folders: Vec<String>,
) -> Result<Vec<TakeSize>, String> {
    let folders = unique(take_folders);
    if folders.is_empty() {
        return Err(crate::messages::NO_TAKES_TO_DELETE.to_string());
    }
    check_listed_and_idle(&render, &folders)?;
    let dirs = capture_dirs(&settings);
    crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        let mut progress = Progress::new(app, "measuring", folders.len());
        let mut sizes = Vec::with_capacity(folders.len());
        for (i, folder) in folders.iter().enumerate() {
            progress.report(i);
            let plan = plan_take_deletion(Path::new(folder), &dirs)?;
            sizes.push(TakeSize::of(folder, &plan));
        }
        progress.report(folders.len());
        Ok(sizes)
    }))
    .await
}

/// Checks every take again (the disk may have changed since the confirm),
/// takes their rows out of the list, then sends each folder to the Recycle
/// Bin. One take failing doesn't stop the others; the result lists both.
#[tauri::command]
pub async fn delete_take_folders(
    app: AppHandle,
    render: tauri::State<'_, RenderManager>,
    settings: tauri::State<'_, SettingsManager>,
    take_folders: Vec<String>,
) -> Result<DeleteTakesResult, String> {
    let folders = unique(take_folders);
    if folders.is_empty() {
        return Err(crate::messages::NO_TAKES_TO_DELETE.to_string());
    }
    check_listed_and_idle(&render, &folders)?;
    let dirs = capture_dirs(&settings);
    let plans: Vec<(String, TakeDeletion)> =
        crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking({
            let folders = folders.clone();
            move || {
                folders
                    .into_iter()
                    .map(|f| plan_take_deletion(Path::new(&f), &dirs).map(|p| (f, p)))
                    .collect::<Result<Vec<_>, String>>()
            }
        }))
        .await?;

    render.remove_take_rows(&app, &folders)?;

    crate::messages::spawn_blocking_result(tokio::task::spawn_blocking(move || {
        let mut progress = Progress::new(app, "recycling", plans.len());
        let mut result = DeleteTakesResult {
            deleted: Vec::new(),
            failed: Vec::new(),
        };
        for (i, (folder, plan)) in plans.iter().enumerate() {
            progress.report(i);
            match recycle_take(plan) {
                Ok(()) => {
                    log_markdown(&format!(
                        "[take-delete] moved {} ({} bytes) to the Recycle Bin",
                        plan.take_folder.display(),
                        plan.size_bytes
                    ));
                    result.deleted.push(TakeSize::of(folder, plan));
                }
                Err(error) => {
                    log_markdown(&format!("[take-delete] {}", error));
                    result.failed.push(TakeDeleteFailure {
                        take_folder: folder.clone(),
                        error,
                    });
                }
            }
        }
        progress.report(plans.len());
        result
    }))
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_take_is_refused_while_either_of_its_jobs_is_rendering() {
        let take = "D:/captures/session/chain_01_b0/take0000";
        assert!(refusal(take, &["Finished"]).is_none());
        assert!(refusal(take, &["Error", "Cancelled"]).is_none());
        // Deleting a queued take pulls its job from the queue instead.
        assert!(refusal(take, &["Queued"]).is_none());
        assert!(refusal(take, &["Finished", "Queued"]).is_none());
        // The HUD half still rendering refuses the whole take.
        assert!(
            refusal(take, &["Finished", "Rendering"])
                .unwrap()
                .contains("is rendering")
        );
    }

    #[test]
    fn a_take_not_in_the_list_is_refused() {
        assert!(
            refusal("D:/elsewhere", &[])
                .unwrap()
                .contains("isn't in the Render tab's list")
        );
    }

    #[test]
    fn a_hud_pair_is_one_take() {
        let picked = vec!["a".to_string(), "b".to_string(), "a".to_string()];
        assert_eq!(unique(picked), vec!["a".to_string(), "b".to_string()]);
    }
}
