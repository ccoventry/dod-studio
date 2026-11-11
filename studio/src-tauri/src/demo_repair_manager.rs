//! Repairing damaged demos (#15): the Demo Auditor's Damaged Demos tab, the
//! Master Queue's and the Demo Analyzer's Repair buttons. The work is
//! `hl_demo_auditor::repair` (files) over `analysis::repair` (bytes); this is
//! the IPC around it: progress events, cancelling, and the activity log.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use analysis::repair::{Hole, RepairHooks, RepairProgress, RepairReport};
use hl_demo_auditor::repair::{DamagedDemo, FileRepair, find_damaged_demos, repair_file};
use native::utils::duration_text::duration_text;
use tauri::{AppHandle, Emitter};

/// Progress events at most this often (~30 fps), per the event-loop rule.
const PROGRESS_EVERY_MS: u32 = 33;

#[derive(Default)]
pub struct DemoRepairManager {
    /// The running Find Damaged Demos' cancel flag.
    pub scan_cancel: Arc<AtomicBool>,
    /// Each running repair's cancel flag, by the demo's path.
    pub repairs: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

/// What a repair came to, for the frontend.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RepairVerdict {
    Repaired,
    NotDamaged,
    /// Built, failed a check, not written.
    Refused,
    Cancelled,
}

/// One repair's result.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RepairResult {
    pub verdict: RepairVerdict,
    /// The repaired demo, when one was written.
    pub written: Option<String>,
    pub holes: Vec<Hole>,
    pub seconds_lost: f32,
    pub percent_kept: f32,
    /// The recording never resumes after the last hole.
    pub ends_at_last_hole: bool,
    /// Why a refused repair was not written, in words.
    pub reasons: Vec<String>,
}

impl RepairResult {
    fn plain(verdict: RepairVerdict) -> Self {
        Self {
            verdict,
            written: None,
            holes: Vec::new(),
            seconds_lost: 0.0,
            percent_kept: 100.0,
            ends_at_last_hole: false,
            reasons: Vec::new(),
        }
    }

    fn from_report(verdict: RepairVerdict, report: &RepairReport) -> Self {
        Self {
            holes: report.holes.clone(),
            seconds_lost: report.seconds_lost(),
            percent_kept: report.percent_kept(),
            ends_at_last_hole: report.ends_at_last_hole,
            ..Self::plain(verdict)
        }
    }
}

/// Lists the demos in `folder` (and its subfolders when `recursive`) that
/// have a hole, emitting `repair_scan_progress` as it reads them.
#[tauri::command]
pub async fn find_damaged_demos_cmd(
    app_handle: AppHandle,
    state: tauri::State<'_, DemoRepairManager>,
    folder: String,
    recursive: bool,
) -> Result<Vec<DamagedDemo>, String> {
    let cancel = Arc::clone(&state.scan_cancel);
    cancel.store(false, Ordering::SeqCst);
    crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        let folder = PathBuf::from(&folder);
        if !folder.is_dir() {
            return Err(crate::messages::repair::not_a_folder(
                &folder.display().to_string(),
            ));
        }
        let started = Instant::now();
        native::log_markdown(&crate::messages::repair::damaged_scan_started(
            &folder.display().to_string(),
            recursive,
        ));
        let last = AtomicU32::new(0);
        let mut progress = |done: usize, total: usize, demo: &Path| {
            let now = started.elapsed().as_millis() as u32;
            if done < total && now.saturating_sub(last.load(Ordering::Relaxed)) < PROGRESS_EVERY_MS
            {
                return;
            }
            last.store(now, Ordering::Relaxed);
            // A dropped progress event only leaves the bar a step behind.
            let _ = app_handle.emit(
                "repair_scan_progress",
                serde_json::json!({
                    "done": done,
                    "total": total,
                    "demo": demo.file_name().map(|n| n.to_string_lossy().to_string()),
                }),
            );
        };
        let found = find_damaged_demos(&folder, recursive, &cancel, &mut progress);
        native::log_markdown(&crate::messages::repair::damaged_scan_ended(
            found.len(),
            &duration_text(started.elapsed().as_secs_f64()),
            cancel.load(Ordering::Relaxed),
        ));
        Ok(found)
    }))
    .await
}

#[tauri::command]
pub fn cancel_damaged_scan(state: tauri::State<'_, DemoRepairManager>) -> Result<(), String> {
    state.scan_cancel.store(true, Ordering::SeqCst);
    Ok(())
}

/// Repairs the demo at `path`, writing `<name>_repaired.dem` next to it, and
/// emits `repair_progress` (`{ path, progress: RepairProgress }`) as it goes:
/// at most every [`PROGRESS_EVERY_MS`], and always at a new step.
/// [`cancel_demo_repair`] stops it; nothing is written then.
#[tauri::command]
pub async fn repair_demo_cmd(
    app_handle: AppHandle,
    state: tauri::State<'_, DemoRepairManager>,
    path: String,
) -> Result<RepairResult, String> {
    let cancel = Arc::new(AtomicBool::new(false));
    state
        .repairs
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .insert(path.clone(), Arc::clone(&cancel));
    let key = path.clone();
    let result = crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        repair_with_events(&app_handle, Path::new(&path), &cancel)
    }))
    .await;
    state
        .repairs
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .remove(&key);
    result
}

/// Stops the running repair of the demo at `path`, at its next check.
#[tauri::command]
pub fn cancel_demo_repair(
    state: tauri::State<'_, DemoRepairManager>,
    path: String,
) -> Result<(), String> {
    if let Some(flag) = state
        .repairs
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .get(&path)
    {
        flag.store(true, Ordering::SeqCst);
    }
    Ok(())
}

/// Runs one repair with its progress events and activity-log lines.
fn repair_with_events(
    app_handle: &AppHandle,
    path: &Path,
    cancel: &AtomicBool,
) -> Result<RepairResult, String> {
    let shown = path.display().to_string();
    let started = Instant::now();
    native::log_markdown(&crate::messages::repair::repair_started(&shown));
    let last = AtomicU32::new(0);
    let mut last_step = None;
    let mut progress = |p: RepairProgress| {
        let now = started.elapsed().as_millis() as u32;
        if last_step == Some(p.step)
            && p.fraction < 1.0
            && now.saturating_sub(last.load(Ordering::Relaxed)) < PROGRESS_EVERY_MS
        {
            return;
        }
        last_step = Some(p.step);
        last.store(now, Ordering::Relaxed);
        // A dropped progress event only leaves the bar a step behind.
        let _ = app_handle.emit(
            "repair_progress",
            serde_json::json!({ "path": shown, "progress": p }),
        );
    };
    let is_cancelled = || cancel.load(Ordering::Relaxed);
    let outcome = repair_file(
        path,
        RepairHooks {
            progress: &mut progress,
            is_cancelled: &is_cancelled,
        },
    );
    let took = duration_text(started.elapsed().as_secs_f64());
    let result = match outcome {
        Err(_) if cancel.load(Ordering::Relaxed) => {
            native::log_markdown(&crate::messages::repair::repair_cancelled(&shown, &took));
            RepairResult::plain(RepairVerdict::Cancelled)
        }
        Err(e) => {
            native::log_markdown(&crate::messages::repair::repair_failed(&shown, &e, &took));
            return Err(e);
        }
        Ok(FileRepair::NotDamaged) => {
            native::log_markdown(&crate::messages::repair::repair_not_damaged(&shown, &took));
            RepairResult::plain(RepairVerdict::NotDamaged)
        }
        Ok(FileRepair::Refused { report, reasons }) => {
            let reasons: Vec<String> = reasons
                .iter()
                .map(hl_demo_auditor::messages::refusal)
                .collect();
            native::log_markdown(&crate::messages::repair::repair_refused(
                &shown, &reasons, &took,
            ));
            RepairResult {
                reasons,
                ..RepairResult::from_report(RepairVerdict::Refused, &report)
            }
        }
        Ok(FileRepair::Repaired {
            path: written,
            report,
        }) => {
            native::log_markdown(&crate::messages::repair::repair_done(
                &shown,
                &written.display().to_string(),
                report.holes.len(),
                report.seconds_lost(),
                &took,
            ));
            RepairResult {
                written: Some(written.to_string_lossy().to_string()),
                ..RepairResult::from_report(RepairVerdict::Repaired, &report)
            }
        }
    };
    Ok(result)
}
