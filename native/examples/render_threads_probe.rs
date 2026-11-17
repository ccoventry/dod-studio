#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "dev-only probe run by hand: a panic on bad input is its error report (#1179)"
)]
//! Measures #1512: a long segmented render beside two short ones, with its
//! FFmpeg threads fixed at the start (`old`, how renders worked before) or
//! recounted per segment from a live share (`new`), or the long one alone
//! (`alone`). Renders synthetic 2560x1440 300 fps BMP takes to ProRes 422
//! through the real renderer. The BMPs are hard links to 300 distinct
//! frames, so a 10,500-frame take costs 300 frames of disk.
//!
//! Usage (set DOD_STUDIO_LOG_DIR so the activity log, which holds the
//! thread-change line, stays out of your own):
//!   cargo run -p native --example render_threads_probe -- <ffmpeg.exe> <work dir> <alone|old|new>

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use native::hlcr::config::{RenderCodec, RenderConfig};
use native::hlcr::renderer::thread_share::{Game, LiveThreadShare, ShareCount, thread_share};
use native::hlcr::renderer::{RenderUpdate, run_render_job};
use native::hlcr::scanner::ClipData;

const FPS: u32 = 300;
const DISTINCT_FRAMES: usize = 300;
/// A 5-kill clip in the issue's batch: about 35 s, four segments.
const LONG_FRAMES: usize = 10_500;
/// 15 s: one piece (up to two segments' length renders whole).
const SHORT_FRAMES: usize = 4_500;
const RENDERS_AT_ONCE: usize = 3;

/// The scheduler's share with `running` renders and no capture.
fn share_with(running: usize) -> usize {
    thread_share(ShareCount {
        running,
        batch_renders: RENDERS_AT_ONCE,
        game: Game::Idle,
    })
}

fn ffmpeg_ok(ffmpeg: &str, cwd: &Path, args: &[&str]) {
    let out = std::process::Command::new(ffmpeg)
        .current_dir(cwd)
        .args(["-y", "-hide_banner", "-loglevel", "error"])
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A BMP take of `frames` frames with a wav as long, made once.
fn make_take(ffmpeg: &str, work: &Path, name: &str, frames: usize) -> PathBuf {
    let distinct = work.join("frames");
    if !distinct
        .join(format!("{:05}.bmp", DISTINCT_FRAMES - 1))
        .is_file()
    {
        std::fs::create_dir_all(&distinct).unwrap();
        let source = format!("testsrc2=size=2560x1440:rate={FPS}");
        let count = DISTINCT_FRAMES.to_string();
        ffmpeg_ok(
            ffmpeg,
            &distinct,
            &[
                "-f",
                "lavfi",
                "-i",
                &source,
                "-frames:v",
                &count,
                "-start_number",
                "0",
                "%05d.bmp",
            ],
        );
    }
    let take = work.join(name).join("take0000");
    let all = take.join("all");
    if !all.join(format!("{:05}.bmp", frames - 1)).is_file() {
        std::fs::create_dir_all(&all).unwrap();
        for i in 0..frames {
            let link = all.join(format!("{i:05}.bmp"));
            if !link.is_file() {
                std::fs::hard_link(
                    distinct.join(format!("{:05}.bmp", i % DISTINCT_FRAMES)),
                    link,
                )
                .unwrap();
            }
        }
        let tone = format!(
            "aevalsrc=0.3*sin(2*PI*440*t):s=48000:d={}",
            frames as f64 / f64::from(FPS)
        );
        ffmpeg_ok(
            ffmpeg,
            &take,
            &["-f", "lavfi", "-i", &tone, "-c:a", "pcm_s16le", "sound.wav"],
        );
    }
    take
}

fn clip_for(take: &Path, name: &str, frames: usize) -> ClipData {
    ClipData {
        clip_name: Some(name.to_string()),
        take_folder: take.to_string_lossy().into_owned(),
        clip_type: "single".to_string(),
        img_folder: "all".to_string(),
        wav_file: Some("sound.wav".to_string()),
        base_name: name.to_string(),
        frame_count: frames,
        // Unknown size: the reservation asks its fixed 2 GiB rather than the
        // raw frames' 116 GB, which no test drive has.
        width: 0,
        height: 0,
        date: "-".to_string(),
        video_file: None,
        alpha_folder: None,
    }
}

struct Ran {
    name: &'static str,
    took: Duration,
    speed: String,
}

/// Runs one job and deletes its clip once it is done (disk is short).
async fn run(
    name: &'static str,
    clip: ClipData,
    config: RenderConfig,
    reservations: Arc<Mutex<HashMap<PathBuf, u64>>>,
) -> Ran {
    let (tx, rx) = mpsc::channel();
    let began = Instant::now();
    run_render_job(
        name.to_string(),
        clip,
        config,
        tx,
        Arc::new(AtomicBool::new(false)),
        reservations,
    )
    .await;
    let took = began.elapsed();
    let mut speed = String::new();
    for update in rx.try_iter() {
        match update {
            RenderUpdate::Speed(_, s) => speed = s,
            RenderUpdate::OutputPath(_, path) => {
                let _ = std::fs::remove_file(path);
            }
            RenderUpdate::Finished(_, ok, err) => assert!(ok, "{name}: {err:?}"),
            _ => {}
        }
    }
    Ran { name, took, speed }
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (ffmpeg, work, mode) = (&args[1], PathBuf::from(&args[2]), args[3].as_str());
    let long_take = make_take(ffmpeg, &work, "long", LONG_FRAMES);
    let short_take = make_take(ffmpeg, &work, "short", SHORT_FRAMES);
    let export = work.join("export");
    std::fs::create_dir_all(&export).unwrap();
    let reservations = Arc::new(Mutex::new(HashMap::new()));
    let config = |share: usize, live: LiveThreadShare| RenderConfig {
        ffmpeg_path: ffmpeg.clone(),
        export_directories: vec![export.clone()],
        fps: FPS,
        target_codec: RenderCodec::ProRes422,
        max_concurrent_renders: share,
        resumable_renders: true,
        live_thread_share: live,
        ..RenderConfig::default()
    };
    let began = Instant::now();
    let mut ran = Vec::new();
    if mode == "alone" {
        ran.push(
            run(
                "long",
                clip_for(&long_take, "long", LONG_FRAMES),
                config(1, LiveThreadShare::new(1)),
                reservations,
            )
            .await,
        );
    } else {
        let share = share_with(RENDERS_AT_ONCE);
        let live = LiveThreadShare::new(share);
        let long_live = if mode == "new" {
            live.clone()
        } else {
            LiveThreadShare::default()
        };
        let long = tokio::spawn(run(
            "long",
            clip_for(&long_take, "long", LONG_FRAMES),
            config(share, long_live),
            reservations.clone(),
        ));
        let shorts: Vec<_> = ["short1", "short2"]
            .into_iter()
            .map(|name| {
                tokio::spawn(run(
                    name,
                    clip_for(&short_take, name, SHORT_FRAMES),
                    config(share, live.clone()),
                    reservations.clone(),
                ))
            })
            .collect();
        for short in shorts {
            ran.push(short.await.unwrap());
        }
        // What the scheduler's next pass does once both have finished.
        live.set(share_with(1));
        println!("shorts done at {:.1} s", began.elapsed().as_secs_f64());
        ran.push(long.await.unwrap());
    }
    for r in &ran {
        println!(
            "{mode} {}: {:.1} s, speed cell {:?}",
            r.name,
            r.took.as_secs_f64(),
            r.speed
        );
    }
    println!("{mode} batch: {:.1} s", began.elapsed().as_secs_f64());
}
