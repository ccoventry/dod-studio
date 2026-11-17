//! R&D probe for #1496: does a combine's FFmpeg end with the process that
//! started it?
//!
//!     combine_exit_probe <ffmpeg.exe> <output.mp4> <clip> <clip> [<clip>...]
//!
//! Plans and runs one combine through `native::hlcr::combine`, exactly as
//! Studio's Combine does, printing this process's PID first. Kill it mid-run
//! (`taskkill /F /PID <pid>`) and look for the `ffmpeg.exe` whose parent was
//! that PID: with the combine's FFmpeg in Studio's job it is gone, without it
//! it runs on to the end.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a probe run by hand: a panic is its error report"
)]

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

#[tokio::main]
async fn main() {
    let args: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    if args.len() < 4 {
        eprintln!("usage: combine_exit_probe <ffmpeg.exe> <output.mp4> <clip> <clip> [<clip>...]");
        std::process::exit(2);
    }
    let (ffmpeg, output, clips) = (&args[0], &args[1], &args[2..]);
    println!("probe pid {}", std::process::id());
    let plan = native::hlcr::combine::plan(ffmpeg, clips).await.unwrap();
    println!(
        "combining {} clips, {:.1} s, stream copy {}",
        plan.clips.len(),
        plan.total_secs,
        plan.stream_copy
    );
    let started = std::time::Instant::now();
    native::hlcr::combine::combine(
        ffmpeg,
        &plan,
        output,
        Arc::new(AtomicBool::new(false)),
        |_| {},
    )
    .await
    .unwrap();
    println!("combined in {:.1} s", started.elapsed().as_secs_f64());
}
