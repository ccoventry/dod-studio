//! `ds_debug_record_frame_times`: the game's own frame times, for the OBS
//! capture test (#646). While it is `1`, every frame's time since the one
//! before is kept; when it goes back to `0`, the summary is printed to the
//! console and the times are written to
//! `%APPDATA%\dod-studio\frame_times.txt` (one microsecond count per line),
//! where Studio reads them after the test clip. No admin rights needed,
//! unlike PresentMon.
//!
//! The clock is read in `commands::poll`, which runs once per frame from
//! `HUD_Frame` on the engine's thread. The engine draws once per host frame,
//! so the time between two polls is the time between two drawn frames. Only
//! that thread writes, into a buffer allocated on the first start: two
//! atomic stores a frame, no lock. Sorting and writing the file happen on a
//! thread of their own after the stop, and the summary line comes back to the
//! engine thread through an atomic pointer to be printed on a later frame
//! (the console isn't safe to call from another thread).

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::time::Instant;

use crate::engine::CvarSPartial;
use crate::names::console_name;

pub const NAME: &str = console_name!("debug_record_frame_times");

/// Frames kept at most: 9 minutes at 480 fps. 1 MB, allocated on the first
/// start only.
const CAPACITY: usize = 1 << 18;

/// The file Studio reads, under `%APPDATA%\dod-studio`.
pub const FILE_NAME: &str = "frame_times.txt";

static CVAR: AtomicPtr<CvarSPartial> = AtomicPtr::new(std::ptr::null_mut());
static RECORDING: AtomicBool = AtomicBool::new(false);
static COUNT: AtomicUsize = AtomicUsize::new(0);
/// Microseconds since `EPOCH` at the last frame, 0 before the first.
static LAST_US: AtomicU64 = AtomicU64::new(0);
static BUFFER: OnceLock<Box<[AtomicU32]>> = OnceLock::new();
static EPOCH: OnceLock<Instant> = OnceLock::new();
/// The writer thread's summary line, for the engine thread to print.
static SUMMARY: AtomicPtr<String> = AtomicPtr::new(std::ptr::null_mut());

pub fn set_cvar(cvar: *mut CvarSPartial) {
    CVAR.store(cvar, Ordering::Release);
}

fn now_us() -> u64 {
    // +1 so a real reading is never the "none yet" 0.
    EPOCH.get_or_init(Instant::now).elapsed().as_micros() as u64 + 1
}

/// Follows the cvar and times the frame. Called every frame from
/// `commands::poll`.
pub fn poll() {
    let line = SUMMARY.swap(std::ptr::null_mut(), Ordering::AcqRel);
    if !line.is_null() {
        // Safety: made by `Box::into_raw` in `stop`, taken exactly once.
        let line = unsafe { Box::from_raw(line) };
        crate::commands::console_print(&line);
        unsafe { crate::debug::report(line.trim_end()) };
    }
    let cvar = CVAR.load(Ordering::Acquire);
    if cvar.is_null() {
        return;
    }
    let wanted = unsafe { (*cvar).value } != 0.0;
    let recording = RECORDING.load(Ordering::Relaxed);
    if wanted && !recording {
        start();
    } else if !wanted && recording {
        stop();
        return;
    }
    if wanted {
        tick();
    }
}

fn start() {
    BUFFER.get_or_init(|| (0..CAPACITY).map(|_| AtomicU32::new(0)).collect());
    COUNT.store(0, Ordering::Relaxed);
    LAST_US.store(0, Ordering::Relaxed);
    RECORDING.store(true, Ordering::Relaxed);
}

fn tick() {
    let now = now_us();
    let last = LAST_US.swap(now, Ordering::Relaxed);
    if last == 0 {
        return;
    }
    let Some(buffer) = BUFFER.get() else {
        return;
    };
    let i = COUNT.load(Ordering::Relaxed);
    if i < buffer.len() {
        buffer[i].store(
            now.saturating_sub(last).min(u64::from(u32::MAX)) as u32,
            Ordering::Relaxed,
        );
        COUNT.store(i + 1, Ordering::Relaxed);
    }
}

fn stop() {
    RECORDING.store(false, Ordering::Relaxed);
    let n = COUNT.load(Ordering::Relaxed);
    let Some(buffer) = BUFFER.get() else {
        return;
    };
    // A copy, so the next start can't change the frames being written.
    let frames: Vec<u32> = buffer[..n]
        .iter()
        .map(|f| f.load(Ordering::Relaxed))
        .collect();
    let _ = std::thread::Builder::new()
        .name("ds_frame_times".into())
        .spawn(move || {
            let summary = Summary::of(&frames);
            let mut line = format!("{NAME}: {}", summary.line());
            match write_file(&frames, &summary) {
                Ok(path) => line.push_str(&format!(", written to {}", path.display())),
                Err(why) => line.push_str(&format!(", not written: {why}")),
            }
            line.push('\n');
            let old = SUMMARY.swap(Box::into_raw(Box::new(line)), Ordering::AcqRel);
            if !old.is_null() {
                // Safety: an unprinted earlier line, ours to drop.
                drop(unsafe { Box::from_raw(old) });
            }
        });
}

fn write_file(frames: &[u32], summary: &Summary) -> Result<std::path::PathBuf, String> {
    let appdata = std::env::var_os("APPDATA").ok_or("APPDATA is not set")?;
    let dir = std::path::PathBuf::from(appdata).join("dod-studio");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(FILE_NAME);
    let mut text = String::with_capacity(frames.len() * 6 + 200);
    text.push_str(
        "# DoD Studio frame times (ds_debug_record_frame_times): microseconds per frame\n",
    );
    text.push_str(&format!("# {}\n", summary.line()));
    for f in frames {
        text.push_str(&f.to_string());
        text.push('\n');
    }
    std::fs::write(&path, text).map_err(|e| e.to_string())?;
    Ok(path)
}

/// What the console line says.
#[derive(Debug, PartialEq)]
pub struct Summary {
    pub frames: usize,
    pub avg_fps: f64,
    /// 1e6 / the 99th percentile frame time.
    pub low1_fps: f64,
    /// 1e6 / the 99.9th percentile frame time.
    pub low01_fps: f64,
    pub max_ms: f64,
}

impl Summary {
    pub fn of(frames: &[u32]) -> Self {
        if frames.is_empty() {
            return Self {
                frames: 0,
                avg_fps: 0.0,
                low1_fps: 0.0,
                low01_fps: 0.0,
                max_ms: 0.0,
            };
        }
        let mut sorted = frames.to_vec();
        sorted.sort_unstable();
        let total: u64 = sorted.iter().map(|&f| u64::from(f)).sum();
        let pct = |p: f64| {
            let i = ((sorted.len() as f64 * p).ceil() as usize).clamp(1, sorted.len()) - 1;
            f64::from(sorted[i].max(1))
        };
        Self {
            frames: sorted.len(),
            avg_fps: sorted.len() as f64 * 1e6 / total.max(1) as f64,
            low1_fps: 1e6 / pct(0.99),
            low01_fps: 1e6 / pct(0.999),
            max_ms: f64::from(*sorted.last().unwrap_or(&0)) / 1000.0,
        }
    }

    pub fn line(&self) -> String {
        format!(
            "{} frames, {:.1} fps average, 1% low {:.1} fps, 0.1% low {:.1} fps, longest frame {:.2} ms",
            self.frames, self.avg_fps, self.low1_fps, self.low01_fps, self.max_ms
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_steady_run_and_its_slow_frames() {
        // 990 frames at 4 ms (250 fps) and 10 at 20 ms.
        let mut frames = vec![4000u32; 990];
        frames.extend([20000u32; 10]);
        let s = Summary::of(&frames);
        assert_eq!(s.frames, 1000);
        assert!((s.avg_fps - 1000.0 * 1e6 / (990.0 * 4000.0 + 10.0 * 20000.0)).abs() < 1e-6);
        // The 99th percentile is the 990th frame: still 4 ms.
        assert_eq!(s.low1_fps, 250.0);
        assert_eq!(s.low01_fps, 50.0);
        assert_eq!(s.max_ms, 20.0);
        assert!(s.line().starts_with("1000 frames, "));
    }

    #[test]
    fn nothing_recorded() {
        assert_eq!(Summary::of(&[]).frames, 0);
    }

    #[test]
    fn the_name_starts_no_other() {
        assert_eq!(NAME, "ds_debug_record_frame_times");
    }
}
