//! Take checks (#1084): whether each take on the Render tab is good, black
//! or frozen picture and missing, silent or clipping sound, shown on its row.
//!
//! The checks themselves are `native::hlcr::take_checks`: one FFmpeg pass
//! over the take, cached beside its pictures (#1069). This layer adds what
//! only the Render tab knows, as the pictures' layer does: the take must be
//! one of its rows, which FFmpeg to use, the progress events, and a cancel
//! of its own that Delete Take (#644) uses, apart from the pictures' one so
//! stopping either pass never stops the other's next one.

use crate::render_manager::RenderManager;
use crate::settings_manager::SettingsManager;
use crate::take_preview_manager::{check_listed, ffmpeg_path};
use native::hlcr::take_checks::{self, TakeChecks};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

/// The cancel flag of the check running now; each check gets a new one.
#[derive(Default)]
pub struct TakeChecksManager {
    cancel: std::sync::Mutex<Arc<AtomicBool>>,
}

impl TakeChecksManager {
    fn start(&self) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        *self.cancel.lock().unwrap() = flag.clone();
        flag
    }
}

/// Checks a take's stream, or reads back its cached results, with
/// `take_checks_progress` events (seconds of the clip done of its length, at
/// most one every ~33 ms plus the last).
#[tauri::command]
pub async fn take_checks(
    app: AppHandle,
    render: tauri::State<'_, RenderManager>,
    settings: tauri::State<'_, SettingsManager>,
    checks: tauri::State<'_, TakeChecksManager>,
    take_folder: String,
    stream: String,
    fps: Option<f64>,
) -> Result<TakeChecks, String> {
    check_listed(&render, &take_folder)?;
    let ffmpeg = ffmpeg_path(&settings);
    let cancel = checks.start();
    let mut last: Option<Instant> = None;
    let key = take_folder.clone();
    let (result, _) = take_checks::make_checks(
        &ffmpeg,
        Path::new(&take_folder),
        &stream,
        fps,
        cancel,
        move |done, total| {
            let due =
                done >= total || last.is_none_or(|t| t.elapsed() >= Duration::from_millis(33));
            if !due {
                return;
            }
            last = Some(Instant::now());
            let _ = app.emit(
                "take_checks_progress",
                serde_json::json!({ "take_folder": key, "done": done, "total": total }),
            );
        },
    )
    .await
    .map_err(crate::messages::failed_to_check_take)?;
    Ok(result)
}

/// Stops the check running now, if any: its call returns the cancelled error.
#[tauri::command]
pub fn cancel_take_checks(checks: tauri::State<'_, TakeChecksManager>) {
    checks.cancel.lock().unwrap().store(true, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_check_gets_its_own_cancel_flag() {
        let checks = TakeChecksManager::default();
        let first = checks.start();
        checks.cancel.lock().unwrap().store(true, Ordering::Relaxed);
        let second = checks.start();
        assert!(first.load(Ordering::Relaxed));
        assert!(!second.load(Ordering::Relaxed));
    }
}
