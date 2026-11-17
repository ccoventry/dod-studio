//! Studio's way out while a combine runs (#1496). Its own test binary, so
//! the process-wide "Studio is closing" flag and the ending of every child
//! in Studio's job reach no other test.
#![cfg(windows)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "integration test: its helpers sit outside a #[test], where clippy.toml's test allowance does not reach (#1179)"
)]

use native::hlcr::combine::{ClipInfo, CombinePlan, combine};
use native::hlcr::renderer::stop_renders_for_exit;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

/// A stand-in FFmpeg: writes "half" to its last argument (the output, as
/// FFmpeg's) and then waits, like a combine still running. A batch file, so
/// no FFmpeg is needed for CI.
const FAKE_FFMPEG: &str = "@echo off\r
:next\r
if \"%~2\"==\"\" goto write\r
shift\r
goto next\r
:write\r
echo half> \"%~1\"\r
waitfor /t 30 dodstudio_fake_ffmpeg_%RANDOM% >nul 2>&1\r
";

fn scratch() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dodstudio_combine_exit_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn clip(dir: &Path, name: &str) -> ClipInfo {
    let path = dir.join(name);
    std::fs::write(&path, b"clip").unwrap();
    ClipInfo {
        path: path.to_string_lossy().into_owned(),
        duration_secs: 5.0,
        ..ClipInfo::default()
    }
}

/// A combine running when Studio closes: it is stopped, nothing is left
/// under the name it was saving to, and the good file already there (a
/// re-combine over an earlier one) is untouched.
#[tokio::test(flavor = "multi_thread")]
async fn closing_studio_mid_combine_leaves_the_existing_output_as_it_was() {
    let dir = scratch();
    let ffmpeg = dir.join("fake_ffmpeg.cmd");
    std::fs::write(&ffmpeg, FAKE_FFMPEG).unwrap();
    let output = dir.join("joined.mp4");
    std::fs::write(&output, b"good").unwrap();
    let plan = CombinePlan {
        clips: vec![clip(&dir, "a.mp4"), clip(&dir, "b.mp4")],
        stream_copy: true,
        total_secs: 10.0,
    };

    let running = {
        let (ffmpeg, output) = (ffmpeg.clone(), output.clone());
        tokio::spawn(async move {
            combine(
                &ffmpeg,
                &plan,
                &output,
                Arc::new(AtomicBool::new(false)),
                |_| {},
            )
            .await
        })
    };
    // Until the stand-in has written something, wherever it writes.
    let partial = dir.join("joined.partial.mp4");
    let began = Instant::now();
    while !partial.exists()
        && std::fs::read(&output).unwrap_or_default() == b"good"
        && began.elapsed() < Duration::from_secs(15)
    {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    tokio::task::spawn_blocking(stop_renders_for_exit)
        .await
        .unwrap();
    let result = tokio::time::timeout(Duration::from_secs(10), running)
        .await
        .expect("the combine ends once Studio's way out stops its FFmpeg")
        .unwrap();

    assert!(result.is_err(), "a stopped combine does not report success");
    assert_eq!(std::fs::read(&output).unwrap(), b"good");
    assert!(!partial.exists(), "the unfinished file is removed");
    let _ = std::fs::remove_dir_all(&dir);
}
