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
use lock_recover::lock_or_recover;
use native::hlcr::take_cleanup::folder_key;
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
    /// The take (`folder_key`) the pass running now makes pictures of, so
    /// take cleanup can stop it before deleting that take (#1438).
    running: std::sync::Mutex<Option<String>>,
}

/// The pass for one take: clears [`TakePreviewManager::running`] however the
/// pass ends, unless a newer pass has taken it over.
struct Running<'a> {
    previews: &'a TakePreviewManager,
    key: String,
}

impl Drop for Running<'_> {
    fn drop(&mut self) {
        let mut running = lock_or_recover(&self.previews.running);
        if running.as_deref() == Some(self.key.as_str()) {
            *running = None;
        }
    }
}

impl TakePreviewManager {
    fn start(&self, take_folder: &str) -> (Arc<AtomicBool>, Running<'_>) {
        let flag = Arc::new(AtomicBool::new(false));
        let key = folder_key(take_folder);
        let mut running = lock_or_recover(&self.running);
        *lock_or_recover(&self.cancel) = flag.clone();
        *running = Some(key.clone());
        (
            flag,
            Running {
                previews: self,
                key,
            },
        )
    }

    /// Stops the pass running now when it makes pictures of a take in
    /// `folder` (a take's folder or its capture block's), and waits up to
    /// `wait` for it to end. Take cleanup calls it before deleting a take:
    /// the Render tab makes a take's thumbnail while it renders, and FFmpeg
    /// still writing `dodstudio_preview` when the delete ran failed it with
    /// "being used by another process", so the take stayed and a held batch
    /// stopped for want of its space (#1438). True when it stopped one.
    pub(crate) fn stop_for_take(&self, folder: &Path, wait: Duration) -> bool {
        let key = folder_key(&folder.to_string_lossy());
        let inside = format!("{key}\\");
        let in_take = |running: &Option<String>| {
            running
                .as_deref()
                .is_some_and(|k| k == key || k.starts_with(&inside))
        };
        {
            let running = lock_or_recover(&self.running);
            if !in_take(&running) {
                return false;
            }
            lock_or_recover(&self.cancel).store(true, Ordering::Relaxed);
        }
        let end = Instant::now() + wait;
        while Instant::now() < end && in_take(&lock_or_recover(&self.running)) {
            std::thread::sleep(Duration::from_millis(20));
        }
        true
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
    resolve_ffmpeg(lock_or_recover(&settings.inner).ffmpeg_path.as_ref())
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
    let (cancel, _running) = previews.start(&take_folder);
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
        uri: take_frames::frame_data_uri(&dir.join(&middle.file))?,
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
                uri: take_frames::frame_data_uri(&dir.join(&f.file))?,
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
    lock_or_recover(&previews.cancel).store(true, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_pass_gets_its_own_cancel_flag() {
        let previews = TakePreviewManager::default();
        let (first, _a) = previews.start(r"D:\takes\b0\take0000");
        previews
            .cancel
            .lock()
            .unwrap()
            .store(true, Ordering::Relaxed);
        let (second, _b) = previews.start(r"D:\takes\b1\take0000");
        assert!(first.load(Ordering::Relaxed));
        assert!(!second.load(Ordering::Relaxed));
    }

    #[test]
    fn a_pass_in_a_take_being_deleted_is_stopped_and_waited_for() {
        let previews = Arc::new(TakePreviewManager::default());
        let (flag, running) = previews.start(r"D:\Takes\b0\take0000");
        let key = running.key.clone();
        std::mem::forget(running);
        // The pass: ends (as FFmpeg is killed) once its flag is set.
        let pass = {
            let previews = Arc::clone(&previews);
            let flag = Arc::clone(&flag);
            std::thread::spawn(move || {
                while !flag.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(5));
                }
                std::thread::sleep(Duration::from_millis(50));
                drop(Running {
                    previews: &previews,
                    key,
                });
            })
        };
        // By its capture block's folder, spelled another way.
        assert!(previews.stop_for_take(Path::new("d:/takes/b0"), Duration::from_secs(5)));
        assert!(flag.load(Ordering::Relaxed));
        assert!(previews.running.lock().unwrap().is_none());
        pass.join().unwrap();
    }

    #[test]
    fn a_pass_on_another_take_is_left_alone() {
        let previews = TakePreviewManager::default();
        let (flag, _running) = previews.start(r"D:\takes\b10\take0000");
        assert!(!previews.stop_for_take(Path::new(r"D:\takes\b1"), Duration::from_secs(1)));
        assert!(!flag.load(Ordering::Relaxed));
    }

    #[test]
    fn a_finished_pass_leaves_nothing_to_stop() {
        let previews = TakePreviewManager::default();
        let (flag, running) = previews.start(r"D:\takes\b0\take0000");
        drop(running);
        assert!(!previews.stop_for_take(Path::new(r"D:\takes\b0"), Duration::from_secs(1)));
        assert!(!flag.load(Ordering::Relaxed));
    }
}
