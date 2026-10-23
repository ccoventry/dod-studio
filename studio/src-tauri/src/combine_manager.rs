//! Combine Clips (#107): Tauri commands over `native::hlcr::combine`, which
//! joins rendered clips into one video.

use native::hlcr::combine::{self, CombinePlan, CombineProgress};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter};

#[derive(Default)]
pub struct CombineManager {
    cancel: Arc<AtomicBool>,
}

fn paths(clips: &[String]) -> Vec<PathBuf> {
    clips.iter().map(PathBuf::from).collect()
}

/// How `clips` would be joined: without re-encoding, or not.
#[tauri::command]
pub async fn combine_plan(
    clips: Vec<String>,
    ffmpeg_path: Option<String>,
) -> Result<CombinePlan, String> {
    let ffmpeg = crate::render_manager::resolve_ffmpeg(ffmpeg_path.as_ref());
    combine::plan(&ffmpeg, &paths(&clips)).await
}

/// Joins `clips`, in order, into `output`. Emits `combine_progress`: a
/// `checking` event per clip while they are re-read, then `joining` ones
/// (at most every 33 ms) while FFmpeg writes (#955).
#[tauri::command]
pub async fn combine_clips(
    app_handle: AppHandle,
    state: tauri::State<'_, CombineManager>,
    clips: Vec<String>,
    output: String,
    ffmpeg_path: Option<String>,
) -> Result<(), String> {
    let cancel = Arc::clone(&state.cancel);
    cancel.store(false, Ordering::SeqCst);
    let ffmpeg = crate::render_manager::resolve_ffmpeg(ffmpeg_path.as_ref());
    let emit = |p: CombineProgress| {
        let _ = app_handle.emit("combine_progress", p);
    };
    let started = std::time::Instant::now();
    native::log_markdown(&format!("[combine] started: {} clips", clips.len()));
    let result = async {
        let plan = combine::plan_with_progress(&ffmpeg, &paths(&clips), |done, total| {
            emit(CombineProgress::Checking { done, total })
        })
        .await?;
        let output = PathBuf::from(&output);
        combine::combine(&ffmpeg, &plan, &output, cancel, |fraction| {
            emit(CombineProgress::Joining { fraction })
        })
        .await
    }
    .await;
    native::log_markdown(&end_log(&result, &output, started.elapsed().as_secs_f64()));
    result
}

/// The activity log's last line for a join.
fn end_log(result: &Result<(), String>, output: &str, secs: f64) -> String {
    match result {
        Ok(()) => format!(
            "[combine] done in {secs:.1} s: {}",
            std::path::Path::new(output).display()
        ),
        Err(e) if combine::is_cancelled(e) => {
            format!("[combine] cancelled after {secs:.1} s")
        }
        Err(e) => format!("[combine] failed after {secs:.1} s: {e}"),
    }
}

#[tauri::command]
pub fn combine_cancel(state: tauri::State<'_, CombineManager>) {
    state.cancel.store(true, Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_end_line_says_how_it_ended_and_how_long_it_took() {
        assert_eq!(
            end_log(&Ok(()), "D:/renders/reel.mp4", 42.04),
            "[combine] done in 42.0 s: D:/renders/reel.mp4"
        );
        assert_eq!(
            end_log(&Err(combine::cancelled_error()), "x.mp4", 3.0),
            "[combine] cancelled after 3.0 s"
        );
        assert_eq!(
            end_log(&Err("FFmpeg: not found".into()), "x.mp4", 0.31),
            "[combine] failed after 0.3 s: FFmpeg: not found"
        );
    }
}
