//! Studio's way out while renders run (#1439). Its own test binary, so the
//! process-wide "Studio is closing" flag it sets reaches no other test.

use native::hlcr::config::{RenderCodec, RenderConfig};
use native::hlcr::renderer::{RenderUpdate, run_render_job, stop_renders_for_exit};
use native::hlcr::scanner::ClipData;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

fn real_ffmpeg() -> PathBuf {
    let ffmpeg = PathBuf::from(
        std::env::var("DOD_FFMPEG").expect("DOD_FFMPEG must name ffmpeg.exe for this test"),
    );
    assert!(ffmpeg.is_file(), "no FFmpeg at {}", ffmpeg.display());
    ffmpeg
}

/// A take of `frames` 640x360 BMP frames with a sound.wav, made by FFmpeg.
fn make_take(ffmpeg: &Path, take_folder: &Path, frames: u32) {
    std::fs::create_dir_all(take_folder.join("all")).unwrap();
    let made = Command::new(ffmpeg)
        .args(["-y", "-hide_banner", "-loglevel", "error", "-f", "lavfi"])
        .args(["-i", "testsrc2=size=640x360:rate=60"])
        .args(["-frames:v", &frames.to_string(), "-start_number", "0"])
        .arg(take_folder.join("all").join("%05d.bmp"))
        .status()
        .unwrap();
    assert!(made.success());
    let made = Command::new(ffmpeg)
        .args(["-y", "-hide_banner", "-loglevel", "error", "-f", "lavfi"])
        .args(["-i", "sine=frequency=440:duration=5"])
        .arg(take_folder.join("sound.wav"))
        .status()
        .unwrap();
    assert!(made.success());
}

fn files_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// A render running when Studio closes: it is stopped, sends no `Finished`
/// (so the scheduler records no failed attempt), and leaves no file; and a
/// job started after that does nothing.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs FFmpeg: set DOD_FFMPEG to ffmpeg.exe's path (and DOD_RENDER_TEST_DIR, see below)"]
async fn closing_studio_stops_a_render_silently_and_leaves_no_file() {
    let ffmpeg = real_ffmpeg();
    // DOD_RENDER_TEST_DIR: a folder on a drive with room, when the Windows
    // drive is under its "Keep free" margin and refuses the render.
    let base = std::env::var_os("DOD_RENDER_TEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let root = base.join(format!("dod_render_exit_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let take_folder = root.join("demo").join("take0000");
    let export_dir = root.join("export");
    std::fs::create_dir_all(&export_dir).unwrap();
    make_take(&ffmpeg, &take_folder, 600);

    let clip = ClipData {
        clip_name: Some("exit_test".to_string()),
        take_folder: take_folder.to_string_lossy().into_owned(),
        clip_type: "single".to_string(),
        img_folder: "all".to_string(),
        wav_file: Some("sound.wav".to_string()),
        base_name: "t".to_string(),
        frame_count: 600,
        width: 640,
        height: 360,
        date: "-".to_string(),
        video_file: None,
        alpha_folder: None,
    };
    let config = RenderConfig {
        ffmpeg_path: ffmpeg.to_string_lossy().into_owned(),
        source_folder: String::new(),
        export_directories: vec![export_dir.clone()],
        fps: 60,
        target_codec: RenderCodec::Custom,
        // Slow on purpose, so the render is still running at the close.
        custom_codec_args: "-c:v libx264 -preset placebo -crf 0".to_string(),
        max_concurrent_renders: 1,
        clip_tag: None,
    };
    let (tx, rx) = mpsc::channel();
    let reservations = Arc::new(Mutex::new(HashMap::new()));
    let job = tokio::spawn(run_render_job(
        "1".to_string(),
        clip.clone(),
        config.clone(),
        tx.clone(),
        Arc::new(AtomicBool::new(false)),
        reservations.clone(),
    ));
    // Until FFmpeg has started writing.
    let partial = export_dir.join("exit_test.partial.mkv");
    for _ in 0..200 {
        if partial.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        partial.exists(),
        "{:?} {:?}",
        files_in(&export_dir),
        rx.try_iter().collect::<Vec<_>>()
    );

    tokio::task::spawn_blocking(stop_renders_for_exit)
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), job)
        .await
        .expect("the job ends once its FFmpeg is stopped")
        .unwrap();

    // A job the scheduler would have started on the way out.
    run_render_job(
        "2".to_string(),
        clip,
        config,
        tx,
        Arc::new(AtomicBool::new(false)),
        reservations,
    )
    .await;

    let updates: Vec<RenderUpdate> = rx.try_iter().collect();
    assert!(
        !updates
            .iter()
            .any(|u| matches!(u, RenderUpdate::Finished(..))),
        "{updates:?}"
    );
    assert!(
        files_in(&export_dir).is_empty(),
        "{:?}",
        files_in(&export_dir)
    );
    let _ = std::fs::remove_dir_all(&root);
}
