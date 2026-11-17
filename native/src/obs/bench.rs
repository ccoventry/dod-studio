//! The OBS capture test (#646): one short clip recorded through the normal
//! batch at the current OBS settings, then judged three ways, since each
//! catches a failure the others don't:
//!
//! - **OBS's own counters** (`GetStats`, read while the clip records): frames
//!   **skipped** because the encoder lagged and frames **missed** because
//!   OBS's rendering lagged. An encoder that can't keep up is the common
//!   failure: 240 fps at 1440p on an RTX 3070 Ti skipped 95-98% (#645).
//! - **Repeated frames in the file**: frames OBS had to show twice because
//!   the game drew no new one in time. OBS's counters don't see these
//!   (1.5-2.1% with `fps_max` at OBS's rate, #645). Counted with FFmpeg's
//!   `signalstats` YDIF: a repeated frame decodes (near) identical to the one
//!   before it. Only meaningful on a demo with no lag of its own, which is why
//!   the test plays the bundled `dodstudio_benchmark_anzio.dem`, a match
//!   recorded with no lost packets (many real recordings freeze for
//!   100-380 ms at a time, and those freezes are in HLAE's takes too).
//! - **Sound**: there is an audio stream and it isn't silence. Every OBS clip
//!   was silent with the game behind other windows before #1058.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::Command;

use crate::patch::CaptureStreak;

use super::client::ObsClient;

/// A frame whose mean luma difference from the one before it (YDIF, on a
/// [`YDIF_WIDTH`]-wide copy) is under this is the same picture: a repeat
/// decodes to 0-0.05 after H.264 at CQP 12-20. The #645 measurements used
/// the same cut. Whether it is a repeat or a still picture is
/// `count_repeats`' call.
pub const REPEAT_YDIF: f64 = 0.15;

/// The width the frames are scaled to before YDIF: enough to see movement,
/// and decoding plus scaling a 1440p clip runs several times faster than
/// real time.
pub const YDIF_WIDTH: u32 = 640;

/// The file's first half second isn't counted for repeats: the recording
/// starts as the game leaves its pre-roll and the first frames can repeat
/// while it settles, which says nothing about the rate it holds.
pub const SETTLE_SECS: f64 = 0.5;

/// At or under these shares a measure is steady: a frame now and then. Clean
/// runs measured 0-0.4% skipped and 0.07-0.37% repeated (#645, #1060).
pub const STEADY_DROP_SHARE: f64 = 0.005;
pub const STEADY_REPEAT_SHARE: f64 = 0.01;

/// At or over this share a measure is not steady: the same 5% the live
/// warning and the activity log's per-clip warning use. Every lagging run
/// measured so far was far past it (32-98%).
pub const FAILING_SHARE: f64 = 0.05;

/// A sound track whose loudest moment is under this is silence: digital
/// silence reads -91 dB, and the game's gunfire peaks near 0.
pub const SILENT_PEAK_DB: f64 = -60.0;

/// How often OBS is asked while the clip records.
pub const POLL_INTERVAL: Duration = Duration::from_millis(250);

/// The OBS Capture FPS rates the "highest steady FPS" run tries, lowest
/// first. 60 is every editor's rate, 120 and 240 the slow-motion ones, and
/// 144/165/180/200 the steps between that monitors come in.
pub const FPS_LADDER: &[i32] = &[60, 120, 144, 165, 180, 200, 240];

/// What a cancelled file analysis returns.
pub const CANCELLED: &str = "The OBS capture test was cancelled.";

/// `GetStats`'s four frame counters at one moment. They run for the whole
/// OBS process, so every figure here is the difference between two reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct BenchCounters {
    pub output_total: i64,
    /// Skipped because the encoder lagged.
    pub output_skipped: i64,
    pub render_total: i64,
    /// Missed because rendering lagged.
    pub render_skipped: i64,
}

impl BenchCounters {
    pub fn from_stats(stats: &Value) -> Option<Self> {
        Some(Self {
            output_total: stats["outputTotalFrames"].as_i64()?,
            output_skipped: stats["outputSkippedFrames"].as_i64()?,
            render_total: stats["renderTotalFrames"].as_i64()?,
            render_skipped: stats["renderSkippedFrames"].as_i64()?,
        })
    }

    /// What happened between `base` and `self`. A counter that went down
    /// (OBS restarted) reads as nothing rather than as negative.
    pub fn since(self, base: Self) -> Self {
        Self {
            output_total: (self.output_total - base.output_total).max(0),
            output_skipped: (self.output_skipped - base.output_skipped).max(0),
            render_total: (self.render_total - base.render_total).max(0),
            render_skipped: (self.render_skipped - base.render_skipped).max(0),
        }
    }

    pub fn skipped_share(&self) -> f64 {
        share(self.output_skipped, self.output_total)
    }

    pub fn missed_share(&self) -> f64 {
        share(self.render_skipped, self.render_total)
    }
}

fn share(part: i64, total: i64) -> f64 {
    if total <= 0 {
        0.0
    } else {
        (part.max(0) as f64 / total as f64).min(1.0)
    }
}

/// What OBS's counters said about the recording.
#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct RecordingStats {
    /// OBS answered at least once.
    pub connected: bool,
    /// A recording was seen: the counters cover it.
    pub saw_recording: bool,
    pub counters: BenchCounters,
    pub recorded_secs: f64,
    /// The highest `averageFrameRenderTime` seen while recording, in ms.
    pub peak_render_ms: f64,
}

/// Turns polls into [`RecordingStats`]: the counters from the first poll that
/// saw OBS recording to the last one. Pure, so it is tested without OBS.
#[derive(Default)]
pub struct RecordingTracker {
    stats: RecordingStats,
    base: Option<(f64, BenchCounters)>,
}

impl RecordingTracker {
    /// One poll: seconds since the watch started, whether OBS was recording,
    /// and its `GetStats` answer (`None` when that failed).
    pub fn on_poll(&mut self, at_secs: f64, recording: Option<bool>, stats: Option<&Value>) {
        if recording.is_some() || stats.is_some() {
            self.stats.connected = true;
        }
        if recording != Some(true) {
            return;
        }
        let Some(stats) = stats else {
            return;
        };
        let Some(now) = BenchCounters::from_stats(stats) else {
            return;
        };
        match self.base {
            None => self.base = Some((at_secs, now)),
            Some((started, base)) => {
                self.stats.saw_recording = true;
                self.stats.counters = now.since(base);
                self.stats.recorded_secs = at_secs - started;
            }
        }
        if let Some(ms) = stats["averageFrameRenderTime"].as_f64() {
            self.stats.peak_render_ms = self.stats.peak_render_ms.max(ms);
        }
    }

    pub fn finish(self) -> RecordingStats {
        self.stats
    }
}

/// Watches OBS on its own thread and connection while a test clip records,
/// so a slow `GetStats` can never hold up the batch's `StartRecord` and
/// `StopRecord`. OBS answers `GetStats` fine while recording: a second client
/// polled it every 250 ms through whole batches with no effect (#645).
pub struct StatsWatch {
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<RecordingStats>>,
}

impl StatsWatch {
    /// `url` is `ws://host:port`. Connects (and reconnects) in the background:
    /// OBS may only be reached once the batch has set it up.
    pub fn start(url: String, password: String) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let handle = std::thread::Builder::new()
            .name("obs_capture_test".into())
            .spawn(move || watch(&url, &password, &flag))
            .ok();
        Self { stop, handle }
    }

    /// Stops the watch and returns what it saw.
    pub fn finish(mut self) -> RecordingStats {
        self.stop.store(true, Ordering::Relaxed);
        self.handle
            .take()
            .and_then(|h| h.join().ok())
            .unwrap_or_default()
    }
}

impl Drop for StatsWatch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn watch(url: &str, password: &str, stop: &AtomicBool) -> RecordingStats {
    let started = Instant::now();
    let mut tracker = RecordingTracker::default();
    let mut client: Option<ObsClient> = None;
    let mut next_connect = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        if client.is_none() && Instant::now() >= next_connect {
            client = ObsClient::connect(url, password).ok();
            next_connect = Instant::now() + Duration::from_secs(2);
        }
        if let Some(c) = client.as_mut() {
            let recording = c.record_active();
            let stats = c.stats();
            if recording.is_none() && stats.is_none() {
                client = None;
            }
            tracker.on_poll(started.elapsed().as_secs_f64(), recording, stats.as_ref());
        }
        std::thread::sleep(POLL_INTERVAL);
    }
    tracker.finish()
}

/// What the recorded file shows.
#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct FileReport {
    pub video: String,
    pub duration_secs: f64,
    /// The frame rate the file says it has, as FFmpeg prints it.
    pub nominal_fps: String,
    /// Frames decoded.
    pub frames: u64,
    /// Frames the repeat count looked at: the moving ones after the first
    /// [`SETTLE_SECS`] (`count_repeats`).
    pub counted: u64,
    pub repeated: u64,
    /// The longest run of repeats in a row, in frames.
    pub longest_repeat: u32,
    /// Frames of a still picture, not counted.
    pub still: u64,
    pub has_audio: bool,
    pub audio_peak_db: Option<f64>,
    pub audio_mean_db: Option<f64>,
}

impl FileReport {
    pub fn repeated_share(&self) -> f64 {
        share(self.repeated as i64, self.counted as i64)
    }
}

/// One `metadata=print` line's YDIF, if it is one.
pub fn parse_ydif_line(line: &str) -> Option<f64> {
    line.trim()
        .strip_prefix("lavfi.signalstats.YDIF=")?
        .parse()
        .ok()
}

/// What [`count_repeats`] found.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RepeatCount {
    /// Frames that moved, plus the repeats among them.
    pub counted: u64,
    pub repeated: u64,
    pub longest: u32,
    /// Frames of a still picture, left out.
    pub still: u64,
}

/// Counts the frames the game repeated: a run of at most `max_run` frames
/// under [`REPEAT_YDIF`] with a moving frame (at least [`MOVING_YDIF`]) on
/// each side. A longer run, or one beside a frame that barely moved, is the
/// picture standing still (the player not moving or turning), not a repeat:
/// measured on the bundled demo at 120 fps with the game at 240 fps, 23% of
/// frames were under the cut, all in nine still stretches of 8-157 frames,
/// and not one in a short run between moving frames; with `fps_max 120`
/// the same clip had 34 such single repeats (1.9% of the moving frames, the
/// 1.5-2.1% #645 measured against HLAE takes). Starts after the first
/// `skip` frames; the very first frame (YDIF always 0) is never counted.
pub fn count_repeats(ydif: &[f64], skip: usize, max_run: usize) -> RepeatCount {
    let mut out = RepeatCount::default();
    let start = skip.max(1);
    let mut i = start;
    while i < ydif.len() {
        if ydif[i] >= REPEAT_YDIF {
            out.counted += 1;
            i += 1;
            continue;
        }
        let mut j = i;
        while j < ydif.len() && ydif[j] < REPEAT_YDIF {
            j += 1;
        }
        let len = j - i;
        let moving_before = ydif[i - 1] >= MOVING_YDIF;
        let moving_after = ydif.get(j).is_some_and(|&d| d >= MOVING_YDIF);
        if len <= max_run && moving_before && moving_after {
            out.counted += len as u64;
            out.repeated += len as u64;
            out.longest = out.longest.max(len as u32);
        } else {
            out.still += len as u64;
        }
        i = j;
    }
    out
}

/// A frame at least this different from the one before it is moving.
pub const MOVING_YDIF: f64 = 0.5;

/// The longest run [`count_repeats`] still calls repeats: 100 ms of frames,
/// at least 2. The game stalling longer than that shows in its own frame
/// times (`GameFrames`).
pub fn max_repeat_run(fps: f64) -> usize {
    ((fps * 0.1).round() as usize).max(2)
}

/// `(mean_volume, max_volume)` in dB from FFmpeg's `volumedetect` report.
pub fn parse_volumedetect(report: &str) -> (Option<f64>, Option<f64>) {
    let find = |key: &str| {
        report.lines().find_map(|l| {
            let at = l.find(key)?;
            l[at + key.len()..]
                .trim()
                .trim_end_matches("dB")
                .trim()
                .parse::<f64>()
                .ok()
        })
    };
    (find("mean_volume:"), find("max_volume:"))
}

/// The recording in a take folder: the biggest video file in it.
pub fn find_video(take_folder: &Path) -> Option<PathBuf> {
    crate::sys::folder_walk::Walker::new()
        .files(take_folder)
        .into_iter()
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "mp4" | "mkv" | "mov"))
        })
        .max_by_key(|p| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0))
}

/// Longest the file analysis may take before it gives up: a 1-minute 240 fps
/// clip decodes in well under a minute.
const ANALYSIS_TIMEOUT: Duration = Duration::from_secs(600);

/// Reads `video` with FFmpeg: its length and rate, every frame's YDIF, and
/// the sound's level. `progress(frames done, frames expected)` is called as
/// frames are read, at most every ~33 ms. Stops when `cancel` is raised.
pub async fn analyze_file(
    ffmpeg: &Path,
    video: &Path,
    cancel: &AtomicBool,
    progress: &mut (dyn FnMut(u64, u64) + Send),
) -> Result<FileReport, String> {
    let info = crate::hlcr::combine::probe(ffmpeg, video).await?;
    let fps: f64 = info.fps.parse().unwrap_or(0.0);
    let expected = (info.duration_secs * fps).round().max(1.0) as u64;
    let mut report = FileReport {
        video: video.display().to_string(),
        duration_secs: info.duration_secs,
        nominal_fps: info.fps.clone(),
        has_audio: info.audio_codec.is_some(),
        ..FileReport::default()
    };

    let filter = format!(
        "scale={YDIF_WIDTH}:-2,signalstats,metadata=print:key=lavfi.signalstats.YDIF:file=-"
    );
    let mut cmd = Command::new(ffmpeg);
    #[cfg(target_os = "windows")]
    cmd.creation_flags(0x08000000);
    cmd.kill_on_drop(true)
        .args(["-hide_banner", "-v", "error", "-nostats", "-i"])
        .arg(video)
        .args(["-map", "0:v:0", "-vf", &filter, "-f", "null", "-"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child =
        crate::sys::child_job::spawn_ending_with_studio(&mut cmd, "FFmpeg (OBS benchmark)")
            .map_err(|e| crate::messages::labeled("FFmpeg", e))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "FFmpeg gave no output to read".to_string())?;
    let mut stderr = child.stderr.take();
    let mut lines = BufReader::new(stdout).lines();
    let mut ydif: Vec<f64> = Vec::with_capacity(expected as usize);
    let deadline = Instant::now() + ANALYSIS_TIMEOUT;
    let mut last_emit = Instant::now();
    progress(0, expected);
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(CANCELLED.to_string());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "FFmpeg took over {} s to read {}",
                ANALYSIS_TIMEOUT.as_secs(),
                video.display()
            ));
        }
        // Bounded so a cancel is seen even while FFmpeg says nothing.
        let Ok(next) = tokio::time::timeout(Duration::from_millis(200), lines.next_line()).await
        else {
            continue;
        };
        match next {
            Ok(Some(line)) => {
                if let Some(d) = parse_ydif_line(&line) {
                    ydif.push(d);
                    if last_emit.elapsed() >= Duration::from_millis(33) {
                        last_emit = Instant::now();
                        progress(ydif.len() as u64, expected.max(ydif.len() as u64));
                    }
                }
            }
            Ok(None) => break,
            Err(e) => return Err(format!("reading FFmpeg's output: {e}")),
        }
    }
    let status = child
        .wait()
        .await
        .map_err(|e| crate::messages::labeled("FFmpeg", e))?;
    if !status.success() && ydif.is_empty() {
        let mut text = String::new();
        if let Some(err) = stderr.as_mut() {
            let _ = err.read_to_string(&mut text).await;
        }
        return Err(format!(
            "FFmpeg could not read {}: {}",
            video.display(),
            text.trim()
        ));
    }
    report.frames = ydif.len() as u64;
    progress(report.frames, report.frames.max(1));
    let skip = (SETTLE_SECS * fps.max(1.0)).round() as usize;
    let count = count_repeats(&ydif, skip, max_repeat_run(fps));
    report.counted = count.counted;
    report.repeated = count.repeated;
    report.longest_repeat = count.longest;
    report.still = count.still;

    if report.has_audio {
        let mut cmd = Command::new(ffmpeg);
        #[cfg(target_os = "windows")]
        cmd.creation_flags(0x08000000);
        cmd.kill_on_drop(true)
            .args(["-hide_banner", "-nostats", "-i"])
            .arg(video)
            .args(["-map", "0:a:0", "-af", "volumedetect", "-f", "null", "-"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        let out = tokio::time::timeout(
            ANALYSIS_TIMEOUT,
            crate::sys::child_job::output_ending_with_studio(&mut cmd, "FFmpeg (OBS benchmark)"),
        )
        .await
        .map_err(|_| {
            format!(
                "FFmpeg took too long to read the sound of {}",
                video.display()
            )
        })?
        .map_err(|e| crate::messages::labeled("FFmpeg", e))?;
        let (mean, peak) = parse_volumedetect(&String::from_utf8_lossy(&out.stderr));
        report.audio_mean_db = mean;
        report.audio_peak_db = peak;
    }
    Ok(report)
}

/// The hook's frame-time recorder (`ds_debug_record_frame_times`), as the
/// test turns it on and off around the clip.
pub const FRAME_TIMES_ON: &str = "ds_debug_record_frame_times 1";
pub const FRAME_TIMES_OFF: &str = "ds_debug_record_frame_times 0";

/// The file the recorder writes, under `%APPDATA%\dod-studio`: the hook
/// takes `APPDATA` from the game's environment, so this does too.
pub fn frame_times_file() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("dod-studio").join("frame_times.txt"))
}

/// The game's own frame rate while the clip played, from the hook.
#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct GameFrames {
    pub frames: u64,
    pub avg_fps: f64,
    /// 1e6 / the 99th percentile frame time, the usual "1% low".
    pub low1_fps: f64,
    /// 1e6 / the 99.9th percentile frame time.
    pub low01_fps: f64,
    pub max_ms: f64,
    /// Frames that took longer than one OBS frame.
    pub slower_than_obs: u64,
}

impl GameFrames {
    /// From the recorder's file: one microsecond count per line, `#` lines
    /// skipped. `None` when it holds no frames.
    pub fn from_file_text(text: &str, obs_fps: i32) -> Option<Self> {
        let frames: Vec<u32> = text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .filter_map(|l| l.parse().ok())
            .collect();
        Self::of(&frames, obs_fps)
    }

    pub fn of(frames: &[u32], obs_fps: i32) -> Option<Self> {
        if frames.is_empty() {
            return None;
        }
        let mut sorted = frames.to_vec();
        sorted.sort_unstable();
        let total: u64 = sorted.iter().map(|&f| u64::from(f)).sum();
        let pct = |p: f64| {
            let i = ((sorted.len() as f64 * p).ceil() as usize).clamp(1, sorted.len()) - 1;
            f64::from(sorted[i].max(1))
        };
        let interval_us = 1e6 / f64::from(obs_fps.max(1));
        Some(Self {
            frames: sorted.len() as u64,
            avg_fps: sorted.len() as f64 * 1e6 / total.max(1) as f64,
            low1_fps: 1e6 / pct(0.99),
            low01_fps: 1e6 / pct(0.999),
            max_ms: f64::from(*sorted.last().unwrap_or(&0)) / 1000.0,
            slower_than_obs: sorted
                .iter()
                .filter(|&&f| f64::from(f) > interval_us)
                .count() as u64,
        })
    }

    pub fn slower_share(&self) -> f64 {
        share(self.slower_than_obs as i64, self.frames as i64)
    }
}

/// How a rate held up.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Steady,
    Borderline,
    NotSteady,
}

/// What lost the frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Cause {
    None,
    /// OBS's encoder fell behind (skipped frames).
    Encoder,
    /// OBS's rendering fell behind (missed frames).
    Rendering,
    /// The game drew no new frame in time (repeated frames in the file).
    Game,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Sound {
    Ok,
    /// The file has no sound track.
    Missing,
    /// It has one, but it is silence.
    Silent,
}

/// The test's answer for one rate.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Verdict {
    pub level: Level,
    pub cause: Cause,
    pub skipped_share: f64,
    pub missed_share: f64,
    pub repeated_share: f64,
    /// OBS's counters were read; when they weren't, only the file decided.
    pub counters_read: bool,
    pub sound: Sound,
}

fn level_of(share: f64, steady: f64) -> Level {
    if share <= steady {
        Level::Steady
    } else if share < FAILING_SHARE {
        Level::Borderline
    } else {
        Level::NotSteady
    }
}

/// Judges one test clip. The level is the worst of the three measures, and
/// the cause is the measure furthest past its steady share.
pub fn judge(recording: &RecordingStats, file: &FileReport) -> Verdict {
    let skipped = recording.counters.skipped_share();
    let missed = recording.counters.missed_share();
    let repeated = file.repeated_share();
    let measures = [
        (Cause::Encoder, skipped, STEADY_DROP_SHARE),
        (Cause::Rendering, missed, STEADY_DROP_SHARE),
        (Cause::Game, repeated, STEADY_REPEAT_SHARE),
    ];
    let level = measures
        .iter()
        .map(|(_, s, steady)| level_of(*s, *steady))
        .max()
        .unwrap_or(Level::Steady);
    let cause = if level == Level::Steady {
        Cause::None
    } else {
        measures
            .iter()
            .max_by(|a, b| (a.1 / a.2).total_cmp(&(b.1 / b.2)))
            .map(|m| m.0)
            .unwrap_or(Cause::None)
    };
    let sound = if !file.has_audio {
        Sound::Missing
    } else if file.audio_peak_db.is_none_or(|p| p < SILENT_PEAK_DB) {
        Sound::Silent
    } else {
        Sound::Ok
    };
    Verdict {
        level,
        cause,
        skipped_share: skipped,
        missed_share: missed,
        repeated_share: repeated,
        counters_read: recording.saw_recording,
        sound,
    }
}

/// The highlight a test records, and the roll around it that makes the clip
/// the length asked for.
#[derive(Clone, Debug)]
pub struct TestClip {
    pub streak: CaptureStreak,
    pub pre_roll: f32,
    pub post_roll: f32,
}

/// Least roll on either side of the kills.
const MIN_ROLL_SECS: f32 = 1.0;

/// Picks a stretch of `streaks` about `seconds` long with the most kills in
/// it (the most action, so the most movement on screen), the shortest such
/// stretch on a tie, and the earliest after that. Only kills inside one
/// highlight are combined. `None` when no kill leaves room for the roll
/// inside the demo (`demo_secs` long).
pub fn pick_clip(streaks: &[CaptureStreak], seconds: f32, demo_secs: f32) -> Option<TestClip> {
    let span_max = seconds - 2.0 * MIN_ROLL_SECS;
    if span_max < 0.0 {
        return None;
    }
    // (kills, span, start time, streak, i, j)
    let mut best: Option<(usize, f32, f32, usize, usize, usize)> = None;
    for (si, s) in streaks.iter().enumerate() {
        for i in 0..s.kills.len() {
            for j in i..s.kills.len() {
                let (t0, t1) = (s.kills[i].1, s.kills[j].1);
                let span = t1 - t0;
                if span > span_max {
                    break;
                }
                let roll = (seconds - span) / 2.0;
                if t0 - roll < 0.0 || t1 + roll > demo_secs {
                    continue;
                }
                let kills = j - i + 1;
                let better = match best {
                    None => true,
                    Some((bk, bspan, bt0, ..)) => {
                        kills > bk || (kills == bk && (span < bspan || (span == bspan && t0 < bt0)))
                    }
                };
                if better {
                    best = Some((kills, span, t0, si, i, j));
                }
            }
        }
    }
    let (_, span, _, si, i, j) = best?;
    let mut streak = streaks[si].clone();
    streak.start_index = i;
    streak.end_index = j;
    streak.update_visuals();
    let roll = (seconds - span) / 2.0;
    Some(TestClip {
        streak,
        pre_roll: roll,
        post_roll: roll,
    })
}

/// A fixed stretch of a demo, `start..end` on the demo player's clock
/// (`viewdemo` seconds, the clock `CaptureStreak::viewdemo_times` uses), at
/// least `seconds` long: widened evenly around its middle when shorter. The
/// clip is built on the kills inside it (the most kills of one highlight,
/// the earliest on a tie), with the roll reaching out to the stretch's ends.
/// `None` when no kill sits at least a second inside it.
pub fn pick_clip_in(
    streaks: &[CaptureStreak],
    start: f32,
    end: f32,
    seconds: f32,
) -> Option<TestClip> {
    let widen = (seconds - (end - start)).max(0.0) / 2.0;
    let (start, end) = (start - widen, end + widen);
    // (kills, start time, streak, i, j)
    let mut best: Option<(usize, f32, usize, usize, usize)> = None;
    for (si, s) in streaks.iter().enumerate() {
        let times = &s.viewdemo_times;
        if times.len() != s.kills.len() {
            continue;
        }
        let inside: Vec<usize> = (0..times.len())
            .filter(|&k| times[k] - start >= MIN_ROLL_SECS && end - times[k] >= MIN_ROLL_SECS)
            .collect();
        let (Some(&i), Some(&j)) = (inside.first(), inside.last()) else {
            continue;
        };
        let kills = j - i + 1;
        if best.is_none_or(|(bk, bt0, ..)| kills > bk || (kills == bk && times[i] < bt0)) {
            best = Some((kills, times[i], si, i, j));
        }
    }
    let (_, _, si, i, j) = best?;
    let mut streak = streaks[si].clone();
    let (t0, t1) = (streak.viewdemo_times[i], streak.viewdemo_times[j]);
    streak.start_index = i;
    streak.end_index = j;
    streak.update_visuals();
    Some(TestClip {
        streak,
        pre_roll: t0 - start,
        post_roll: end - t1,
    })
}

#[cfg(test)]
#[path = "bench_tests.rs"]
mod tests;
