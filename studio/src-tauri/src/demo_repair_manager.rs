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
use hl_demo_auditor::repair::{
    DamagedDemo, FileRepair, find_damaged_demos, is_repairable, repair_file,
};
use native::utils::duration_text::duration_text;
use tauri::{AppHandle, Emitter};

/// Progress events at most this often (~30 fps), per the event-loop rule.
const PROGRESS_EVERY_MS: u32 = 33;

#[derive(Default)]
pub struct DemoRepairManager {
    /// The running Find Damaged Demos' cancel flag.
    pub scan_cancel: Arc<AtomicBool>,
    /// Each running repair's cancel flag, by [`repair_key`] of the demo's path.
    pub repairs: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

/// One demo's key in [`DemoRepairManager::repairs`]: the Master Queue, the
/// Analyzer and the Auditor can spell one file's path differently.
fn repair_key(path: &str) -> String {
    path.replace('/', "\\").to_lowercase()
}

impl DemoRepairManager {
    /// Claims the demo at `path` for one repair: its cancel flag, or `None`
    /// while another repair of it runs. Two at once wrote two repairs of one
    /// demo, and the first to end dropped the second's cancel flag.
    fn claim(&self, path: &str) -> Option<Arc<AtomicBool>> {
        let mut repairs = self.repairs.lock().unwrap_or_else(|p| p.into_inner());
        let key = repair_key(path);
        if repairs.contains_key(&key) {
            return None;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        repairs.insert(key, Arc::clone(&cancel));
        Some(cancel)
    }

    fn release(&self, path: &str) {
        self.repairs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&repair_key(path));
    }
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

/// A damaged demo as the Damaged Demos tab lists it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DamagedRow {
    #[serde(flatten)]
    pub demo: DamagedDemo,
    /// One per signon, whatever map the damage is on: read by scanning the
    /// bytes for the server info (`native::demo_split::quick_maps`), which the
    /// damage doesn't stop. Two or more offer Repair & Split.
    pub maps: Vec<String>,
}

/// The maps of the demo at `path`, by a byte scan; none when unreadable.
fn maps_of(path: &Path) -> Vec<String> {
    std::fs::read(path)
        .map(|bytes| maps_in(&bytes))
        .unwrap_or_default()
}

/// The maps a demo's `bytes` signon to, one per signon, by a byte scan for
/// the server info (`native::demo_split::quick_maps`), which damage doesn't
/// stop; none for a file without a demo header.
fn maps_in(bytes: &[u8]) -> Vec<String> {
    native::demo_split::header_protocol(bytes)
        .map(|protocol| native::demo_split::quick_maps(bytes, protocol))
        .unwrap_or_default()
}

/// What the Master Queue and the Demo Analyzer learn about a demo the
/// analyzer flags as damaged before they offer Repair (and Repair & Split).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct RepairCheck {
    /// The repair has a hole to cut out (`hl_demo_auditor::repair::is_repairable`);
    /// damage inside a frame whose header reads is none.
    pub repairable: bool,
    /// Every map the demo recorded, as the Damaged Demos tab lists them
    /// ([`maps_in`]). The analyzer's own list stops at the damage, so a demo
    /// damaged before its level change has more maps here, and its repair has
    /// more than one to split.
    pub maps: Vec<String>,
}

/// [`RepairCheck`] of a demo's `bytes`: one read of the file answers both.
fn repair_check(bytes: &[u8]) -> RepairCheck {
    RepairCheck {
        repairable: is_repairable(bytes),
        maps: maps_in(bytes),
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
) -> Result<Vec<DamagedRow>, String> {
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
        let found: Vec<DamagedRow> = find_damaged_demos(&folder, recursive, &cancel, &mut progress)
            .into_iter()
            .map(|demo| DamagedRow {
                maps: maps_of(&demo.path),
                demo,
            })
            .collect();
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
    let Some(cancel) = state.claim(&path) else {
        let shown = Path::new(&path).display().to_string();
        native::log_markdown(&crate::messages::repair::already_repairing_log(&shown));
        return Err(crate::messages::repair::already_repairing(&shown));
    };
    let key = path.clone();
    let result = crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        repair_with_events(&app_handle, Path::new(&path), &cancel)
    }))
    .await;
    state.release(&key);
    result
}

/// The [`RepairCheck`] of the demo at `path`: whether a repair would find a
/// hole to cut out, and every map it recorded. The Master Queue and the Demo
/// Analyzer ask before they offer Repair on a demo the analyzer flags as
/// damaged, since damage inside a frame whose header reads is not a hole, and
/// for Repair & Split, since their own map list stops at the damage. Reads
/// the whole file once, off the main thread.
#[tauri::command]
pub async fn demo_repairable(path: String) -> Result<RepairCheck, String> {
    crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        let path = Path::new(&path);
        std::fs::read(path)
            .map(|bytes| repair_check(&bytes))
            .map_err(|e| crate::messages::repair::cannot_read(&path.display().to_string(), e))
    }))
    .await
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
        .get(&repair_key(&path))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_demo_is_repaired_by_one_job_at_a_time_however_its_path_is_spelt() {
        let manager = DemoRepairManager::default();
        let first = manager.claim(r"C:\demos\m1_h1.dem").expect("free");
        assert!(manager.claim("c:/Demos/M1_H1.dem").is_none());
        assert!(manager.claim(r"C:\demos\m3_h2.dem").is_some());
        manager.release("c:/Demos/M1_H1.dem");
        assert!(!first.load(Ordering::SeqCst));
        assert!(manager.claim(r"C:\demos\m1_h1.dem").is_some());
    }

    fn fixture() -> Vec<u8> {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../test-fixtures/ci_fixture.dem"
        );
        std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"))
    }

    /// A server info as a level change writes it: `maps/<map>.bsp` where
    /// `quick_maps` looks for it.
    fn server_info(map: &str) -> Vec<u8> {
        let mut b = vec![11];
        b.extend_from_slice(&48i32.to_le_bytes());
        b.extend_from_slice(&[0; 4 + 4 + 16]);
        b.extend_from_slice(&[32, 4, 1]);
        b.extend_from_slice(b"dod\0My Server\0");
        b.extend_from_slice(format!("maps/{map}.bsp\0").as_bytes());
        b
    }

    #[test]
    fn a_demo_damaged_before_its_level_change_lists_the_maps_after_it() {
        let mut bytes = fixture();
        let first = maps_in(&bytes);
        assert_eq!(first.len(), 1, "the fixture records one map");
        // Damage mid-file, then a level change to another map after it: the
        // analyzer's parse stops at the damage and never sees that map.
        let at = bytes.len() / 2;
        bytes[at..at + 300_000].fill(0xEE);
        let change = server_info("dod_flash");
        let later = at + 400_000;
        bytes[later..later + change.len()].copy_from_slice(&change);
        let check = repair_check(&bytes);
        assert!(check.repairable);
        assert_eq!(check.maps, [first[0].as_str(), "dod_flash"]);
        // A healthy demo: nothing to repair, its one map.
        assert_eq!(
            repair_check(&fixture()),
            RepairCheck {
                repairable: false,
                maps: first
            }
        );
    }

    /// `monday-wsod25_r07_m1_h2.dem` (3 maps, its first break at 11:58,
    /// before the level change), from `DOD_DAMAGED_MULTIMAP_DEMO` or the PRE
    /// install's `dod` folder: the check finds the break and all three maps.
    #[test]
    #[ignore = "needs a demo damaged before its level change (DOD_DAMAGED_MULTIMAP_DEMO or the PRE install)"]
    fn a_real_demo_damaged_before_its_level_change_lists_all_its_maps() {
        let path = std::env::var_os("DOD_DAMAGED_MULTIMAP_DEMO")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(
                    r"C:\Program Files (x86)\Steam\steamapps\common\Half-Life - PRE-Anniversary for Movies\dod\monday-wsod25_r07_m1_h2.dem",
                )
            });
        let started = Instant::now();
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let check = repair_check(&bytes);
        eprintln!(
            "{}: {:?} in {:.2} s",
            path.display(),
            check,
            started.elapsed().as_secs_f64()
        );
        assert!(check.repairable);
        assert_eq!(check.maps.len(), 3, "{:?}", check.maps);
    }
}
