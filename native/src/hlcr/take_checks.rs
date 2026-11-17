//! Whether a take is good (#1084): plain warnings for what goes wrong in a
//! capture without anything reporting it, found in one FFmpeg pass over the
//! take and cached beside its pictures (#1069).
//!
//! What it finds, and why each one:
//!
//! - **Black picture**: HLAE direct-to-video's failure mode is silent;
//!   capture and render both succeed over a black rectangle
//!   (`docs/direct_to_video_capture.md`). FFmpeg's `blackdetect`.
//! - **Frozen picture**: a demo with no camera data records a still view
//!   (#920). FFmpeg's `freezedetect`, a run of identical frames at least
//!   [`FROZEN_MIN_SECS`] long.
//! - **Brief holds**: shorter runs, the recording freezing 100-380 ms at a
//!   time when the recorder lagged (#645). The demo's, not the capture's,
//!   so they are reported as a note, never a warning: a player standing
//!   still in a quiet corner looks the same, and re-capturing changes
//!   neither.
//! - **No sound, silent sound, clipping sound**: every OBS clip was silent
//!   behind other windows before #1058, and HLAE's sound clipped hard on
//!   BAR bursts before `mirv_movie_sound_volume 0.25`. FFmpeg's `astats`
//!   over HLAE's `sound.wav`, or over the video's own audio for an OBS take.
//!
//! - **Length and kills** (`take_plan.rs`): the take's frame count against
//!   the length the batch planned, and each kill's place against the video,
//!   from the plan recorded beside the take. A take without a plan (captured
//!   before it was recorded) gets a note saying so, never a warning.
//!
//! A video shorter than its sound is #1105's partial take
//! (`take_length.rs`), shown on the row already; a video far short of its
//! plan is reported here too, since that check also catches a take whose
//! sound is short as well.
//!
//! Every finding says whether re-capturing helps ([`Cause::Capture`]) or the
//! demo itself is the reason ([`Cause::Demo`]), because the fix differs.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::hlcr::take_frames::{self, SourceStamp, StreamMedia};
use crate::hlcr::take_plan::{self, TakePlan};

/// The checks' results, beside the stream's pictures.
pub const CHECKS_FILE: &str = "checks.json";
/// Bumped when what is checked or how changes, so older results are made
/// again.
const CHECKS_VERSION: u32 = 2;

/// A black run shorter than this is a cut or a flash, not a black take.
pub const BLACK_MIN_SECS: f64 = 0.5;
/// How dark a pixel must be to count as black (share of full brightness);
/// FFmpeg's own default, which keeps a dark map corner from counting.
pub const BLACK_PIXEL_SHARE: f64 = 0.10;
/// How long the picture may stand perfectly still before it is a frozen
/// picture rather than the recorder lagging.
pub const FROZEN_MIN_SECS: f64 = 2.0;
/// The shortest hold counted at all. At a 300 fps capture of a ~100 fps
/// demo every frame repeats two or three times (~10 ms); a tenth of a
/// second is well past that and catches #645's shortest lags.
pub const HOLD_MIN_SECS: f64 = 0.1;
/// How alike two frames must be to count as the same: FFmpeg's default
/// (-60 dB), measured on the full-size frames, so a gun's idle sway still
/// counts as movement.
pub const FREEZE_NOISE_DB: i32 = -60;
/// Sound whose loudest moment stays under this is silent.
pub const SILENT_PEAK_DB: f64 = -50.0;
/// Sound whose loudest moment reaches this hits full scale.
pub const CLIP_PEAK_DB: f64 = -0.1;
/// Share of samples at full scale that makes clipping worth a warning: a
/// thousandth (about 20 ms of a 20 s take), where a single loud shot that
/// touches full scale is a handful of samples.
pub const CLIP_SHARE: f64 = 0.001;
/// Longest the pass may take. It decodes every frame, ~0.7x real time on a
/// 300 fps 1280x1024 HLAE take read cold from disk; this only catches a file
/// FFmpeg hangs on.
const CHECK_TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// Decoder threads for a video take. Unlimited, a 1440p 300 fps utvideo
/// take decodes on ~9 cores; with 4 it takes ~5 and the same results, and a
/// pass reading a take cold from disk is no slower, the disk being the
/// limit (~24 s for 12.9 GB either way).
const DECODE_THREADS: u32 = 4;

/// Whether re-capturing helps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cause {
    /// Re-capturing helps.
    Capture,
    /// The demo itself; re-capturing won't change it.
    Demo,
    /// Nothing to say either way: a check that couldn't be made.
    Unknown,
}

/// What a finding is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Check {
    Black,
    Frozen,
    Holds,
    NoSound,
    Silent,
    Clipping,
    /// The video's frame count is off the planned length.
    Length,
    /// A kill the highlight lists is outside the video.
    KillOutside,
    /// The take has no plan to check its length and kills against.
    NoPlan,
}

/// One thing the checks found.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    pub check: Check,
    pub cause: Cause,
    /// A warning; `false` for a note (the brief holds).
    pub warn: bool,
    /// Where in the clip, in seconds (the longest run, for several).
    pub at_secs: Option<f64>,
    /// How long it lasts, in seconds (the longest run, for several).
    pub secs: Option<f64>,
    /// How many runs, for findings that sum several.
    pub count: Option<u32>,
    /// Share of samples at full scale, for clipping.
    pub share: Option<f64>,
    /// The video's frame count, and the planned one, for a length finding.
    pub frames: Option<u64>,
    pub planned_frames: Option<u64>,
}

impl Finding {
    fn new(check: Check, cause: Cause, warn: bool) -> Self {
        Self {
            check,
            cause,
            warn,
            at_secs: None,
            secs: None,
            count: None,
            share: None,
            frames: None,
            planned_frames: None,
        }
    }
}

/// A stream's check results.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TakeChecks {
    pub version: u32,
    pub stream: String,
    /// The picture the results were made from.
    pub source: SourceStamp,
    /// The `sound.wav` they were made from, when the take has one.
    pub sound: Option<SourceStamp>,
    /// The clip's length in seconds, as FFmpeg decoded it.
    pub duration_secs: Option<f64>,
    /// The video's frames, as decoded (a sequence's files).
    pub frames: Option<u64>,
    /// The plan the length and kill findings were made against, so a take
    /// that gets one (or a different one) is checked again.
    pub plan: Option<TakePlan>,
    /// The sound's loudest moment and its average, in dBFS, when it has
    /// sound.
    pub peak_db: Option<f64>,
    pub rms_db: Option<f64>,
    pub findings: Vec<Finding>,
}

/// One run FFmpeg reported: start and length in seconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Run {
    pub start: f64,
    pub secs: f64,
}

/// What `astats` said about the whole sound.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SoundStats {
    pub peak_db: Option<f64>,
    pub rms_db: Option<f64>,
    /// Times the sound reached its peak level.
    pub peak_count: Option<f64>,
    /// Samples per channel.
    pub samples: Option<f64>,
}

/// What one pass's report says.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Report {
    pub black: Vec<Run>,
    pub frozen: Vec<Run>,
    /// `None` when the pass had no sound to measure.
    pub sound: Option<SoundStats>,
}

/// `-?inf` and numbers: astats prints `-inf` for digital silence.
fn parse_number(text: &str) -> Option<f64> {
    let t = text.trim();
    match t {
        "-inf" => Some(f64::NEG_INFINITY),
        "inf" => Some(f64::INFINITY),
        _ => t.parse::<f64>().ok().filter(|v| !v.is_nan()),
    }
}

/// The value after `key` in `line` (`key:1.5`, `key: 1.5`), up to the next
/// space.
fn value_after(line: &str, key: &str) -> Option<f64> {
    let at = line.find(key)? + key.len();
    let rest = line[at..].trim_start();
    parse_number(rest.split_whitespace().next()?)
}

/// Reads FFmpeg's stderr from a [`pass_args`] run. A freeze still going at
/// the end of the clip has a start and no end: it runs to `clip_secs`.
pub fn parse_report(stderr: &str, clip_secs: Option<f64>) -> Report {
    let mut report = Report::default();
    let mut open_freeze: Option<f64> = None;
    let mut in_overall = false;
    let mut sound = SoundStats::default();
    let mut saw_sound = false;
    for line in stderr.lines() {
        if line.contains("blackdetect") && line.contains("black_start") {
            if let (Some(start), Some(secs)) = (
                value_after(line, "black_start:"),
                value_after(line, "black_duration:"),
            ) {
                report.black.push(Run { start, secs });
            }
        } else if let Some(start) = value_after(line, "lavfi.freezedetect.freeze_start:") {
            open_freeze = Some(start);
        } else if let Some(secs) = value_after(line, "lavfi.freezedetect.freeze_duration:") {
            if let Some(start) = open_freeze.take() {
                report.frozen.push(Run { start, secs });
            }
        } else if line.contains("astats") {
            if line.trim_end().ends_with("Overall") {
                in_overall = true;
                saw_sound = true;
            } else if in_overall {
                let field = |key: &str| value_after(line, key);
                if let Some(v) = field("Peak level dB:") {
                    sound.peak_db = Some(v);
                } else if let Some(v) = field("RMS level dB:") {
                    sound.rms_db = Some(v);
                } else if line.contains("] Peak count:") {
                    sound.peak_count = field("Peak count:");
                } else if let Some(v) = field("Number of samples:") {
                    sound.samples = Some(v);
                }
            }
        }
    }
    if let (Some(start), Some(end)) = (open_freeze, clip_secs)
        && end > start
    {
        report.frozen.push(Run {
            start,
            secs: end - start,
        });
    }
    if saw_sound {
        report.sound = Some(sound);
    }
    report
}

/// Whether a still run is mostly a black one: a black picture never moves
/// either, and it is already reported as black.
fn mostly_black(run: &Run, black: &[Run]) -> bool {
    let covered: f64 = black
        .iter()
        .map(|b| ((run.start + run.secs).min(b.start + b.secs) - run.start.max(b.start)).max(0.0))
        .sum();
    covered * 2.0 >= run.secs
}

/// The findings in a report, warnings first.
pub fn findings(report: &Report) -> Vec<Finding> {
    let mut out = Vec::new();
    let still: Vec<&Run> = report
        .frozen
        .iter()
        .filter(|r| !mostly_black(r, &report.black))
        .collect();
    for run in report.black.iter().filter(|r| r.secs >= BLACK_MIN_SECS) {
        out.push(Finding {
            at_secs: Some(run.start),
            secs: Some(run.secs),
            ..Finding::new(Check::Black, Cause::Capture, true)
        });
    }
    for run in still.iter().filter(|r| r.secs >= FROZEN_MIN_SECS) {
        out.push(Finding {
            at_secs: Some(run.start),
            secs: Some(run.secs),
            ..Finding::new(Check::Frozen, Cause::Demo, true)
        });
    }
    match report.sound {
        None => out.push(Finding::new(Check::NoSound, Cause::Capture, true)),
        Some(s) => {
            let peak = s.peak_db.unwrap_or(f64::NEG_INFINITY);
            if peak < SILENT_PEAK_DB {
                out.push(Finding::new(Check::Silent, Cause::Capture, true));
            } else if peak >= CLIP_PEAK_DB {
                let share = match (s.peak_count, s.samples) {
                    (Some(c), Some(n)) if n > 0.0 => c / n,
                    _ => 0.0,
                };
                if share >= CLIP_SHARE {
                    out.push(Finding {
                        share: Some(share),
                        ..Finding::new(Check::Clipping, Cause::Capture, true)
                    });
                }
            }
        }
    }
    let holds: Vec<&&Run> = still
        .iter()
        .filter(|r| r.secs >= HOLD_MIN_SECS && r.secs < FROZEN_MIN_SECS)
        .collect();
    if let Some(longest) = holds.iter().max_by(|a, b| a.secs.total_cmp(&b.secs)) {
        out.push(Finding {
            at_secs: Some(longest.start),
            secs: Some(longest.secs),
            count: Some(holds.len() as u32),
            ..Finding::new(Check::Holds, Cause::Demo, false)
        });
    }
    out
}

/// Where the pass reads the picture from.
#[derive(Debug, Clone, PartialEq)]
pub enum PictureInput {
    Video(PathBuf),
    /// A frame sequence: its file pattern (`%05d.bmp`), first number and
    /// rate.
    Sequence {
        pattern: PathBuf,
        start: u64,
        fps: f64,
    },
}

/// A sequence's FFmpeg input: HLAE numbers its files with a fixed count of
/// digits, so the first file's name gives the pattern. `None` for names that
/// aren't plain numbers.
pub fn sequence_input(files: &[PathBuf], fps: f64) -> Option<PictureInput> {
    let first = files.first()?;
    let stem = first.file_stem()?.to_str()?;
    let ext = first.extension()?.to_str()?;
    if stem.is_empty() || !stem.bytes().all(|b| b.is_ascii_digit()) || fps <= 0.0 {
        return None;
    }
    Some(PictureInput::Sequence {
        pattern: first.with_file_name(format!("%0{}d.{ext}", stem.len())),
        start: stem.parse().ok()?,
        fps,
    })
}

/// FFmpeg's arguments for one pass: every frame through `blackdetect` and
/// `freezedetect`, the sound (`sound`, else the video's own, when it has
/// one) through `astats`, nothing written; progress on stdout.
pub fn pass_args(picture: &PictureInput, sound: Option<&Path>) -> Vec<String> {
    let mut args: Vec<String> = [
        "-hide_banner",
        "-nostats",
        "-progress",
        "pipe:1",
        "-loglevel",
        "info",
    ]
    .map(String::from)
    .to_vec();
    match picture {
        PictureInput::Video(video) => args.extend([
            "-threads".to_string(),
            DECODE_THREADS.to_string(),
            "-i".to_string(),
            video.to_string_lossy().into_owned(),
        ]),
        PictureInput::Sequence {
            pattern,
            start,
            fps,
        } => args.extend([
            "-framerate".to_string(),
            format!("{fps}"),
            "-start_number".to_string(),
            start.to_string(),
            "-i".to_string(),
            pattern.to_string_lossy().into_owned(),
        ]),
    }
    let audio_map = match sound {
        Some(wav) => {
            args.extend(["-i".to_string(), wav.to_string_lossy().into_owned()]);
            "1:a:0"
        }
        // `?`: an OBS video without sound is reported, not an error.
        None => "0:a:0?",
    };
    args.extend([
        "-map".to_string(),
        "0:v:0".to_string(),
        "-map".to_string(),
        audio_map.to_string(),
        "-vf".to_string(),
        format!(
            "blackdetect=d={BLACK_MIN_SECS}:pix_th={BLACK_PIXEL_SHARE},freezedetect=n={FREEZE_NOISE_DB}dB:d={HOLD_MIN_SECS}"
        ),
        "-af".to_string(),
        "astats=measure_perchannel=none".to_string(),
        "-f".to_string(),
        "null".to_string(),
        "-".to_string(),
    ]);
    args
}

/// Seconds decoded so far, from a `-progress` line (`out_time_us=1500000`).
pub fn progress_secs(line: &str) -> Option<f64> {
    let us = line
        .strip_prefix("out_time_us=")?
        .trim()
        .parse::<i64>()
        .ok()?;
    (us >= 0).then(|| us as f64 / 1_000_000.0)
}

/// Frames decoded so far, from a `-progress` line (`frame=1500`).
pub fn progress_frames(line: &str) -> Option<u64> {
    line.strip_prefix("frame=")?.trim().parse().ok()
}

/// The plan findings for a take of `frames` frames at `fps`, or its
/// `duration_secs` when the frames weren't counted. Without a plan, one note
/// that these weren't checked.
pub fn plan_findings(
    plan: Option<&TakePlan>,
    fps: Option<f64>,
    frames: Option<u64>,
    duration_secs: Option<f64>,
) -> Vec<Finding> {
    let Some(plan) = plan else {
        return vec![Finding::new(Check::NoPlan, Cause::Unknown, false)];
    };
    let rate = fps.filter(|f| f.is_finite() && *f > 0.0);
    let mut out = Vec::new();
    if let (Some(rate), Some(frames)) = (rate, frames)
        && let Some(m) = take_plan::length_mismatch(plan, rate, frames)
    {
        out.push(Finding {
            frames: Some(m.actual_frames),
            planned_frames: Some(m.planned_frames),
            secs: Some(m.actual_frames as f64 / rate),
            ..Finding::new(Check::Length, Cause::Capture, true)
        });
    }
    let video_secs = match (rate, frames) {
        (Some(rate), Some(frames)) => Some(frames as f64 / rate),
        _ => duration_secs,
    };
    if let Some(video_secs) = video_secs {
        let outside = take_plan::kills_outside(plan, video_secs);
        if let Some(&first) = outside.first() {
            out.push(Finding {
                at_secs: Some(first),
                secs: Some(video_secs),
                count: Some(outside.len() as u32),
                ..Finding::new(Check::KillOutside, Cause::Capture, true)
            });
        }
    }
    out
}

/// The rate a take's frames are counted at against its plan: a sequence's
/// own, a video's from its file (an OBS take records at OBS's rate, not the
/// batch's capture rate the take's file holds), else the capture rate.
fn frame_rate(
    picture: &PictureInput,
    video_fps: Option<f64>,
    capture_fps: Option<f64>,
) -> Option<f64> {
    match picture {
        PictureInput::Sequence { fps, .. } => Some(*fps),
        PictureInput::Video(_) => video_fps
            .filter(|f| f.is_finite() && *f > 0.0)
            .or(capture_fps),
    }
}

/// A take's plan and the rate it was captured at, from the file beside it.
fn recorded_plan(take_folder: &Path) -> (Option<TakePlan>, Option<f64>) {
    match super::take_meta::read_for_take(take_folder) {
        Some(meta) => (
            meta.plan,
            Some(f64::from(meta.capture_fps)).filter(|f| *f > 0.0),
        ),
        None => (None, None),
    }
}

fn sound_stamp(wav: &Path) -> SourceStamp {
    take_frames::source_stamp(&StreamMedia::Video(wav.to_path_buf()))
}

/// The cached results for a stream, when they were made from what the take
/// holds now.
pub fn read_cached(take_folder: &Path, stream: &str) -> Option<TakeChecks> {
    let media = take_frames::stream_media(&take_folder.join(stream))?;
    let wav = super::scanner::take_wav(take_folder);
    read_cached_for(take_folder, stream, &media, wav.as_deref())
}

fn read_cached_for(
    take_folder: &Path,
    stream: &str,
    media: &StreamMedia,
    wav: Option<&Path>,
) -> Option<TakeChecks> {
    let path = take_frames::cache_dir(take_folder, stream).join(CHECKS_FILE);
    let checks: TakeChecks = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    let usable = checks.version == CHECKS_VERSION
        && checks.source == take_frames::source_stamp(media)
        && checks.sound == wav.map(sound_stamp)
        && checks.plan == recorded_plan(take_folder).0;
    usable.then_some(checks)
}

/// Checks a take's stream and saves the results beside it, or reads back
/// the results already made from what it holds now.
/// `progress(done_secs, total_secs)` while FFmpeg runs.
/// `fps` is a frame sequence's rate when the take doesn't record its own.
pub async fn make_checks(
    ffmpeg: &Path,
    take_folder: &Path,
    stream: &str,
    fps: Option<f64>,
    cancel: Arc<AtomicBool>,
    mut progress: impl FnMut(f64, f64),
) -> Result<(TakeChecks, bool), String> {
    if !take_frames::is_plain_stream_name(stream) {
        return Err(crate::messages::take_frames_bad_stream_name(stream));
    }
    let media = take_frames::stream_media(&take_folder.join(stream))
        .ok_or_else(|| crate::messages::take_frames_no_stream(stream, take_folder.display()))?;
    let wav = super::scanner::take_wav(take_folder);
    if let Some(checks) = read_cached_for(take_folder, stream, &media, wav.as_deref()) {
        return Ok((checks, true));
    }

    let (picture, total, video_fps) = match &media {
        StreamMedia::Video(video) => {
            let (secs, video_fps) = take_frames::probe_video(ffmpeg, video, &cancel).await?;
            (PictureInput::Video(video.clone()), Some(secs), video_fps)
        }
        StreamMedia::Sequence(files) => {
            let rate = super::take_meta::recorded_fps(take_folder)
                .map(f64::from)
                .or(fps)
                .filter(|f| *f > 0.0)
                .ok_or_else(|| crate::messages::take_checks_no_rate(take_folder.display()))?;
            let input = sequence_input(files, rate)
                .ok_or_else(|| crate::messages::take_checks_unnumbered(take_folder.display()))?;
            (input, Some(files.len() as f64 / rate), None)
        }
    };

    // Stamped before the pass: a take still being written when it starts
    // stamps smaller than it ends up, so its results are made again.
    let source = take_frames::source_stamp(&media);
    let sound = wav.as_deref().map(sound_stamp);
    let total_secs = total.unwrap_or(0.0);
    let mut decoded_frames: Option<u64> = None;
    let run = take_frames::run_ffmpeg_watched(
        ffmpeg,
        &pass_args(&picture, wav.as_deref()),
        &cancel,
        CHECK_TIMEOUT,
        |line| {
            if let Some(done) = progress_secs(line) {
                progress(done.min(total_secs), total_secs);
            }
            if let Some(n) = progress_frames(line) {
                decoded_frames = Some(n);
            }
        },
    )
    .await
    .map_err(|e| {
        if e == crate::messages::take_frames_timed_out(CHECK_TIMEOUT.as_secs()) {
            crate::messages::take_checks_timed_out(CHECK_TIMEOUT.as_secs() / 60)
        } else {
            e
        }
    })?;
    if !run.success {
        return Err(crate::messages::combine_failed(
            run.stderr.lines().last().unwrap_or("").trim(),
        ));
    }
    let report = parse_report(&run.stderr, total);
    let frames = match &media {
        StreamMedia::Sequence(files) => Some(files.len() as u64),
        StreamMedia::Video(_) => decoded_frames,
    };
    let (plan, plan_fps) = recorded_plan(take_folder);
    let mut found = findings(&report);
    found.extend(plan_findings(
        plan.as_ref(),
        frame_rate(&picture, video_fps, plan_fps),
        frames,
        total,
    ));
    let checks = TakeChecks {
        version: CHECKS_VERSION,
        stream: stream.to_string(),
        source,
        sound,
        duration_secs: total,
        frames,
        plan,
        peak_db: report
            .sound
            .and_then(|s| s.peak_db)
            .filter(|v| v.is_finite()),
        rms_db: report
            .sound
            .and_then(|s| s.rms_db)
            .filter(|v| v.is_finite()),
        findings: found,
    };

    let dir = take_frames::cache_dir(take_folder, stream);
    std::fs::create_dir_all(&dir).map_err(|e| crate::messages::labeled(dir.display(), e))?;
    let path = dir.join(CHECKS_FILE);
    let part = dir.join(format!("{CHECKS_FILE}.part"));
    let text = serde_json::to_string_pretty(&checks).map_err(|e| e.to_string())?;
    std::fs::write(&part, text).map_err(|e| crate::messages::labeled(part.display(), e))?;
    std::fs::rename(&part, &path).map_err(|e| crate::messages::labeled(path.display(), e))?;
    Ok((checks, false))
}

#[cfg(test)]
#[path = "take_checks_tests.rs"]
mod tests;
