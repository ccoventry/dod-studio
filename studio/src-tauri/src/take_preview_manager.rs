//! Take pictures (#1069): the Render tab's thumbnail on each take row, and
//! the contact sheet a click on it opens.
//!
//! The frames themselves are `native::hlcr::take_frames`: picked across the
//! stream, cached beside the take, made once. This layer adds what only the
//! Render tab knows: a take must be one of its rows (so a call can't write a
//! cache folder anywhere else), which FFmpeg to use, the progress events, and
//! the cancel that Delete Take (#644) uses so FFmpeg isn't holding a take's
//! files open while it moves to the Recycle Bin.

use crate::render_manager::{RenderManager, resolve_ffmpeg};
use crate::settings_manager::SettingsManager;
use native::hlcr::take_frames::{self, Sheet};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

/// The cancel flag of the pass running now. Each pass gets a new one, so a
/// cancel only ever stops the pass it was meant for.
#[derive(Default)]
pub struct TakePreviewManager {
    cancel: std::sync::Mutex<Arc<AtomicBool>>,
}

impl TakePreviewManager {
    fn start(&self) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        *self.cancel.lock().unwrap() = flag.clone();
        flag
    }
}

/// The row's thumbnail: the middle frame.
#[derive(Serialize)]
pub struct TakeThumb {
    pub uri: String,
    /// Whether the frames were already made.
    pub cached: bool,
}

#[derive(Serialize)]
pub struct SheetFrameView {
    pub uri: String,
    pub frame: Option<u64>,
    pub secs: Option<f64>,
}

/// The contact sheet: every cached frame, in clip order.
#[derive(Serialize)]
pub struct TakeSheetView {
    pub kind: String,
    pub duration_secs: Option<f64>,
    pub fps: Option<f64>,
    pub frames: Vec<SheetFrameView>,
}

pub(crate) fn check_listed(render: &RenderManager, take_folder: &str) -> Result<(), String> {
    if render.take_statuses(take_folder).is_empty() {
        return Err(crate::messages::take_not_listed_for_preview(take_folder));
    }
    Ok(())
}

pub(crate) fn ffmpeg_path(settings: &SettingsManager) -> PathBuf {
    resolve_ffmpeg(settings.inner.lock().unwrap().ffmpeg_path.as_ref())
}

/// The take's frames for `stream`, made when they aren't cached yet, with
/// `take_preview_progress` events (frames done of total, at most one every
/// ~33 ms plus the last).
async fn sheet_for(
    app: AppHandle,
    render: &RenderManager,
    settings: &SettingsManager,
    cancel: Arc<AtomicBool>,
    take_folder: String,
    stream: String,
    fps: Option<f64>,
) -> Result<(Sheet, bool), String> {
    check_listed(render, &take_folder)?;
    let ffmpeg = ffmpeg_path(settings);
    let mut last: Option<Instant> = None;
    let key = take_folder.clone();
    take_frames::make_sheet(
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
                "take_preview_progress",
                serde_json::json!({ "take_folder": key, "done": done, "total": total }),
            );
        },
    )
    .await
    .map_err(crate::messages::failed_to_make_take_preview)
}

/// Makes (or reads back) a take's frames and returns the middle one.
#[tauri::command]
pub async fn take_preview_thumb(
    app: AppHandle,
    render: tauri::State<'_, RenderManager>,
    settings: tauri::State<'_, SettingsManager>,
    previews: tauri::State<'_, TakePreviewManager>,
    take_folder: String,
    stream: String,
    fps: Option<f64>,
) -> Result<TakeThumb, String> {
    let cancel = previews.start();
    let (sheet, cached) = sheet_for(
        app,
        &render,
        &settings,
        cancel,
        take_folder.clone(),
        stream.clone(),
        fps,
    )
    .await?;
    let middle = sheet
        .middle()
        .ok_or_else(|| crate::messages::take_preview_empty(&take_folder))?;
    let dir = take_frames::cache_dir(Path::new(&take_folder), &stream);
    Ok(TakeThumb {
        uri: take_frames::frame_data_uri(&dir.join(&middle.file))
            .map_err(crate::messages::failed_to_read_take_frame)?,
        cached,
    })
}

/// Every frame of a take's contact sheet, made first when it isn't cached.
/// The Render tab opens one only once its thumbnail is made, so it is read
/// back, not made beside the background pass.
#[tauri::command]
pub async fn take_preview_sheet(
    app: AppHandle,
    render: tauri::State<'_, RenderManager>,
    settings: tauri::State<'_, SettingsManager>,
    take_folder: String,
    stream: String,
    fps: Option<f64>,
) -> Result<TakeSheetView, String> {
    // Not the background pass's flag: opening a sheet must not take over
    // what pausing that pass cancels.
    let cancel = Arc::new(AtomicBool::new(false));
    let (sheet, _) = sheet_for(
        app,
        &render,
        &settings,
        cancel,
        take_folder.clone(),
        stream.clone(),
        fps,
    )
    .await?;
    let dir = take_frames::cache_dir(Path::new(&take_folder), &stream);
    let frames = sheet
        .frames
        .iter()
        .map(|f| {
            Ok(SheetFrameView {
                uri: take_frames::frame_data_uri(&dir.join(&f.file))
                    .map_err(crate::messages::failed_to_read_take_frame)?,
                frame: f.frame,
                secs: f.secs,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(TakeSheetView {
        kind: sheet.kind,
        duration_secs: sheet.duration_secs,
        fps: sheet.fps,
        frames,
    })
}

/// Stops the pass running now, if any: its call returns the cancelled error.
#[tauri::command]
pub fn cancel_take_previews(previews: tauri::State<'_, TakePreviewManager>) {
    previews
        .cancel
        .lock()
        .unwrap()
        .store(true, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_pass_gets_its_own_cancel_flag() {
        let previews = TakePreviewManager::default();
        let first = previews.start();
        previews
            .cancel
            .lock()
            .unwrap()
            .store(true, Ordering::Relaxed);
        let second = previews.start();
        assert!(first.load(Ordering::Relaxed));
        assert!(!second.load(Ordering::Relaxed));
    }
}
