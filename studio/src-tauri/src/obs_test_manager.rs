//! "Test OBS capture" and "Find my highest steady FPS" (#646), the backend
//! half. The frontend (`obs_capture_test.js`) runs each test clip as a normal
//! capture batch; these commands give it the clip, watch OBS's counters while
//! it records, judge the recorded file (`native::obs::bench`) and delete it.
//!
//! Test batches use a session id starting with [`SESSION_PREFIX`]: their
//! takes are reported as `obs_test_takes_verified`, not
//! `capture_takes_verified`, so they never mark a highlight captured, never
//! fill the Last Batch panel and never start Finish clips.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use native::obs::bench::{self, FileReport, GameFrames, RecordingStats, StatsWatch, Verdict};
use serde::Serialize;
use tauri::{Emitter, Manager};

use crate::capture_manager::SerializedStreak;

/// Every test batch's session id starts with this.
pub const SESSION_PREFIX: &str = "obs_test_";

/// The test demo inside Studio's resources (`tauri.conf.json`).
const BUNDLED_DEMO: &str = "obs-test/ci_fixture.dem";

/// Whether a batch's session id is a test's.
pub fn is_test_session(session_id: &str) -> bool {
    session_id.starts_with(SESSION_PREFIX)
}

#[derive(Default)]
pub struct ObsTestManager {
    watch: Mutex<Option<StatsWatch>>,
    cancel: Arc<AtomicBool>,
}

/// The clip a test records.
#[derive(Debug, Serialize)]
pub struct ObsTestClip {
    pub demo: String,
    pub streak: SerializedStreak,
    pub pre_roll: f32,
    pub post_roll: f32,
}

/// The bundled demo: from the installed resources, or the repository's
/// `test-fixtures` in a build that runs from it.
fn bundled_demo(app: &tauri::AppHandle) -> Option<PathBuf> {
    let installed = app.path().resource_dir().ok().map(|d| d.join(BUNDLED_DEMO));
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-fixtures/ci_fixture.dem");
    installed
        .into_iter()
        .chain(std::iter::once(repo))
        .find(|p| p.is_file())
        .map(|p| without_verbatim_prefix(&p))
}

/// `p` without Windows' `\\?\` prefix, which `resource_dir` can give: the
/// hook's batch hands the path to `viewdemo`, which reads `\\?\C:\...` as no
/// drive at all and skips the demo.
fn without_verbatim_prefix(p: &Path) -> PathBuf {
    let s = p.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{rest}"))
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        p.to_path_buf()
    }
}

/// The clip a test records: `seconds` of the bundled demo with the most kills
/// in it, or of `demo` when given (comparing against a real match).
#[tauri::command]
pub async fn obs_test_clip(
    app: tauri::AppHandle,
    seconds: f32,
    demo: Option<String>,
) -> Result<ObsTestClip, String> {
    let path = match demo.filter(|d| !d.trim().is_empty()) {
        Some(d) => PathBuf::from(d),
        None => bundled_demo(&app).ok_or_else(|| crate::messages::OBS_TEST_NO_DEMO.to_string())?,
    };
    tokio::task::spawn_blocking(move || {
        let (_, streaks, _, _, _, match_start_tick, frame_times) =
            native::patch::scan_demo_for_highlights(&path)
                .map_err(|e| crate::messages::obs_test_unreadable_demo(&path, e))?;
        let demo_secs = streaks
            .first()
            .filter(|s| s.demo_fps > 0.0)
            .map(|s| s.total_demo_frames as f32 / s.demo_fps)
            .unwrap_or(0.0);
        let clip = bench::pick_clip(&streaks, seconds, demo_secs)
            .ok_or_else(|| crate::messages::obs_test_no_clip(&path, seconds))?;
        let mut streak = clip.streak;
        streak.match_start_tick = match_start_tick;
        streak.frame_times = frame_times;
        Ok(ObsTestClip {
            demo: path.display().to_string(),
            streak: SerializedStreak::from(streak),
            pre_roll: clip.pre_roll,
            post_roll: clip.post_roll,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Starts watching OBS's counters, before the test batch starts.
#[tauri::command(async)]
pub fn obs_test_watch_start(
    state: tauri::State<'_, ObsTestManager>,
    host: String,
    port: u16,
    password: String,
) {
    state.cancel.store(false, Ordering::Relaxed);
    let host = if host.is_empty() {
        "127.0.0.1".to_string()
    } else {
        host
    };
    let port = if port == 0 { 4455 } else { port };
    // The last clip's frame times must not pass for this one's.
    if let Some(file) = bench::frame_times_file() {
        let _ = std::fs::remove_file(file);
    }
    let watch = StatsWatch::start(format!("ws://{host}:{port}"), password);
    *state.watch.lock().unwrap_or_else(|p| p.into_inner()) = Some(watch);
}

/// One test clip's whole result.
#[derive(Debug, Serialize)]
pub struct ObsTestResult {
    pub recording: RecordingStats,
    pub file: FileReport,
    pub verdict: Verdict,
    /// The game's own frame rate from the hook (`ds_debug_record_frame_times`),
    /// `None` when the hook wrote none (a hook DLL from before it).
    pub game: Option<GameFrames>,
}

/// The game's frame times for the clip just recorded at `obs_fps`.
fn read_game_frames(obs_fps: i32) -> Option<GameFrames> {
    let text = std::fs::read_to_string(bench::frame_times_file()?).ok()?;
    GameFrames::from_file_text(&text, obs_fps)
}

/// After the test batch: stops the watch, reads the take's recording and
/// judges it. `obs_test_progress` reports the frames read, ~30 times a second.
#[tauri::command]
pub async fn obs_test_analyze(
    app: tauri::AppHandle,
    state: tauri::State<'_, ObsTestManager>,
    take_folder: String,
    ffmpeg_path: Option<String>,
    obs_fps: i32,
) -> Result<ObsTestResult, String> {
    let watch = state.watch.lock().unwrap_or_else(|p| p.into_inner()).take();
    let recording = match watch {
        Some(w) => tokio::task::spawn_blocking(move || w.finish())
            .await
            .unwrap_or_default(),
        None => RecordingStats::default(),
    };
    let folder = PathBuf::from(&take_folder);
    let video = {
        let folder = folder.clone();
        tokio::task::spawn_blocking(move || bench::find_video(&folder))
            .await
            .map_err(|e| e.to_string())?
    }
    .ok_or_else(|| crate::messages::obs_test_no_video(&folder))?;
    let ffmpeg = crate::render_manager::resolve_ffmpeg(ffmpeg_path.as_ref());
    let cancel = Arc::clone(&state.cancel);
    let emitter = app.clone();
    let mut progress = move |done: u64, total: u64| {
        let _ = emitter.emit(
            "obs_test_progress",
            serde_json::json!({ "done": done, "total": total }),
        );
    };
    let file = bench::analyze_file(&ffmpeg, &video, &cancel, &mut progress).await?;
    let verdict = bench::judge(&recording, &file);
    // Written by the hook on its own thread when the clip's end turned the
    // recorder off, long before the batch ended.
    let game = tokio::task::spawn_blocking(move || read_game_frames(obs_fps))
        .await
        .unwrap_or_default();
    Ok(ObsTestResult {
        recording,
        file,
        verdict,
        game,
    })
}

/// Stops a test: the watch, and a file analysis in progress. The batch
/// itself is cancelled the usual way (`cancel_capture_batch`).
#[tauri::command(async)]
pub fn obs_test_cancel(state: tauri::State<'_, ObsTestManager>) {
    state.cancel.store(true, Ordering::Relaxed);
    state.watch.lock().unwrap_or_else(|p| p.into_inner()).take();
}

/// The test session folder `take_folder` is in: the nearest folder above it
/// (or itself) named like a test's session. `None` for anything else, so a
/// user's own takes can never be deleted through here.
fn test_session_folder(take_folder: &Path) -> Option<PathBuf> {
    take_folder
        .ancestors()
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(is_test_session)
        })
        .map(Path::to_path_buf)
}

/// Deletes a test's session folder, given one of its take folders.
#[tauri::command]
pub async fn obs_test_delete_take(take_folder: String) -> Result<(), String> {
    let folder = test_session_folder(Path::new(&take_folder))
        .ok_or_else(|| crate::messages::obs_test_not_a_test_folder(&take_folder))?;
    tokio::task::spawn_blocking(move || match std::fs::remove_dir_all(&folder) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(crate::messages::obs_test_delete_failed(&folder, e)),
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_test_session_folder_is_found() {
        let take = Path::new("D:/captures/obs_test_20261027_010203/dod_x_take/take0000");
        assert_eq!(
            test_session_folder(take),
            Some(PathBuf::from("D:/captures/obs_test_20261027_010203"))
        );
        assert_eq!(
            test_session_folder(Path::new("D:/captures/session_20261027_010203/dod_x_take")),
            None
        );
        assert!(is_test_session("obs_test_1"));
        assert!(!is_test_session("session_obs_test_1"));
    }

    #[test]
    fn the_verbatim_prefix_goes() {
        assert_eq!(
            without_verbatim_prefix(Path::new(
                r"\\?\C:\Program Files\DoD Studio\obs-test\ci_fixture.dem"
            )),
            PathBuf::from(r"C:\Program Files\DoD Studio\obs-test\ci_fixture.dem")
        );
        assert_eq!(
            without_verbatim_prefix(Path::new(r"\\?\UNC\server\share\a.dem")),
            PathBuf::from(r"\\server\share\a.dem")
        );
        assert_eq!(
            without_verbatim_prefix(Path::new(r"D:\a.dem")),
            PathBuf::from(r"D:\a.dem")
        );
    }

    #[test]
    fn the_test_demo_is_found_from_the_repository() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-fixtures/ci_fixture.dem");
        assert!(repo.is_file());
    }
}
