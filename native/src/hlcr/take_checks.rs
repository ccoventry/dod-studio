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
//! A video shorter than its sound is #1105's partial take
//! (`take_length.rs`), shown on the row already, so it isn't repeated here.
//!
//! Every finding says whether re-capturing helps ([`Cause::Capture`]) or the
//! demo itself is the reason ([`Cause::Demo`]), because the fix differs.

#![cfg(not(target_arch = "wasm32"))]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::hlcr::take_frames::{self, SourceStamp, StreamMedia};

/// The checks' results, beside the stream's pictures.
pub const CHECKS_FILE: &str = "checks.json";
/// Bumped when what is checked or how changes, so older results are made
/// again.
const CHECKS_VERSION: u32 = 1;

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

/// Whether re-capturing helps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cause {
    /// Re-capturing helps.
    Capture,
    /// The demo itself; re-capturing won't change it.
    Demo,
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
        PictureInput::Video(video) => {
            args.extend(["-i".to_string(), video.to_string_lossy().into_owned()])
        }
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
        && checks.sound == wav.map(sound_stamp);
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

    let (picture, total) = match &media {
        StreamMedia::Video(video) => {
            let (secs, _) = take_frames::probe_video(ffmpeg, video, &cancel).await?;
            (PictureInput::Video(video.clone()), Some(secs))
        }
        StreamMedia::Sequence(files) => {
            let rate = super::take_meta::recorded_fps(take_folder)
                .map(f64::from)
                .or(fps)
                .filter(|f| *f > 0.0)
                .ok_or_else(|| crate::messages::take_checks_no_rate(take_folder.display()))?;
            let input = sequence_input(files, rate)
                .ok_or_else(|| crate::messages::take_checks_unnumbered(take_folder.display()))?;
            (input, Some(files.len() as f64 / rate))
        }
    };

    let total_secs = total.unwrap_or(0.0);
    let run = take_frames::run_ffmpeg_watched(
        ffmpeg,
        &pass_args(&picture, wav.as_deref()),
        &cancel,
        CHECK_TIMEOUT,
        |line| {
            if let Some(done) = progress_secs(line) {
                progress(done.min(total_secs), total_secs);
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
    let checks = TakeChecks {
        version: CHECKS_VERSION,
        stream: stream.to_string(),
        source: take_frames::source_stamp(&media),
        sound: wav.as_deref().map(sound_stamp),
        duration_secs: total,
        peak_db: report
            .sound
            .and_then(|s| s.peak_db)
            .filter(|v| v.is_finite()),
        rms_db: report
            .sound
            .and_then(|s| s.rms_db)
            .filter(|v| v.is_finite()),
        findings: findings(&report),
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
mod tests {
    use super::*;
    use crate::test_support::Scratch;
    use std::sync::atomic::Ordering;

    /// Trimmed from a real run over a 19.33 s, 300 fps HLAE take.
    const REAL_REPORT: &str = "\
[freezedetect @ 000002186fda3380] lavfi.freezedetect.freeze_start: 7.426667
[freezedetect @ 000002186fda3380] lavfi.freezedetect.freeze_duration: 0.34
[freezedetect @ 000002186fda3380] lavfi.freezedetect.freeze_end: 7.766667
[freezedetect @ 000002186fda3380] lavfi.freezedetect.freeze_start: 12.74
[freezedetect @ 000002186fda3380] lavfi.freezedetect.freeze_duration: 0.273333
[freezedetect @ 000002186fda3380] lavfi.freezedetect.freeze_end: 13.016667
[Parsed_astats_0 @ 000002186fda14c0] Overall
[Parsed_astats_0 @ 000002186fda14c0] DC offset: 0.000804
[Parsed_astats_0 @ 000002186fda14c0] Peak level dB: -5.542792
[Parsed_astats_0 @ 000002186fda14c0] RMS level dB: -25.064306
[Parsed_astats_0 @ 000002186fda14c0] RMS peak dB: -15.215879
[Parsed_astats_0 @ 000002186fda14c0] Peak count: 2.000000
[Parsed_astats_0 @ 000002186fda14c0] Abs Peak count: 1.000000
[Parsed_astats_0 @ 000002186fda14c0] Number of samples: 426302
";

    #[test]
    fn a_good_take_has_only_the_demos_brief_holds() {
        let report = parse_report(REAL_REPORT, Some(19.33));
        assert_eq!(report.frozen.len(), 2);
        let sound = report.sound.unwrap();
        assert_eq!(sound.peak_db, Some(-5.542792));
        assert_eq!(sound.rms_db, Some(-25.064306));
        assert_eq!(sound.peak_count, Some(2.0));
        assert_eq!(sound.samples, Some(426302.0));
        let found = findings(&report);
        assert_eq!(found.len(), 1, "{found:?}");
        let holds = &found[0];
        assert_eq!(holds.check, Check::Holds);
        assert_eq!(holds.cause, Cause::Demo);
        assert!(!holds.warn);
        assert_eq!(holds.count, Some(2));
        assert_eq!(holds.at_secs, Some(7.426667));
        assert_eq!(holds.secs, Some(0.34));
    }

    #[test]
    fn a_black_stretch_is_a_capture_problem() {
        let report = parse_report(
            "[blackdetect @ 0000] black_start:2 black_end:5.5 black_duration:3.5\n\
             [blackdetect @ 0000] black_start:9 black_end:9.2 black_duration:0.2\n\
             [Parsed_astats_0 @ 0] Overall\n[Parsed_astats_0 @ 0] Peak level dB: -6\n",
            Some(10.0),
        );
        assert_eq!(report.black.len(), 2);
        let found = findings(&report);
        assert_eq!(
            found.len(),
            1,
            "a 0.2 s flash isn't a black take: {found:?}"
        );
        assert_eq!(found[0].check, Check::Black);
        assert_eq!(found[0].cause, Cause::Capture);
        assert_eq!((found[0].at_secs, found[0].secs), (Some(2.0), Some(3.5)));
    }

    #[test]
    fn a_black_stretch_is_not_also_reported_frozen() {
        // A black picture never moves either: one finding, as black.
        let report = parse_report(
            "[blackdetect @ 0] black_start:2 black_end:5.5 black_duration:3.5\n\
             [freezedetect @ 0] lavfi.freezedetect.freeze_start: 2\n\
             [freezedetect @ 0] lavfi.freezedetect.freeze_duration: 3.5\n\
             [freezedetect @ 0] lavfi.freezedetect.freeze_start: 7\n\
             [freezedetect @ 0] lavfi.freezedetect.freeze_duration: 2.5\n\
             [Parsed_astats_0 @ 0] Overall\n[Parsed_astats_0 @ 0] Peak level dB: -6\n",
            Some(10.0),
        );
        let found: Vec<(Check, Option<f64>)> = findings(&report)
            .iter()
            .map(|f| (f.check, f.at_secs))
            .collect();
        assert_eq!(
            found,
            vec![(Check::Black, Some(2.0)), (Check::Frozen, Some(7.0))]
        );
    }

    #[test]
    fn a_freeze_still_going_at_the_end_runs_to_the_end() {
        let report = parse_report(
            "[freezedetect @ 0] lavfi.freezedetect.freeze_start: 4\n\
             [Parsed_astats_0 @ 0] Overall\n[Parsed_astats_0 @ 0] Peak level dB: -6\n",
            Some(10.0),
        );
        assert_eq!(
            report.frozen,
            vec![Run {
                start: 4.0,
                secs: 6.0
            }]
        );
        let found = findings(&report);
        assert_eq!(found[0].check, Check::Frozen);
        assert_eq!(found[0].cause, Cause::Demo);
        assert!(found[0].warn);
        // A short hold counts once only, as a hold, never as frozen too.
        assert!(found.iter().all(|f| f.check != Check::Holds));
    }

    #[test]
    fn sound_missing_silent_or_clipping_is_a_warning() {
        let none = findings(&parse_report("", Some(5.0)));
        assert_eq!(none[0].check, Check::NoSound);

        let silent = parse_report(
            "[Parsed_astats_0 @ 0] Overall\n[Parsed_astats_0 @ 0] Peak level dB: -inf\n",
            None,
        );
        assert_eq!(findings(&silent)[0].check, Check::Silent);

        let clip = |count: &str| {
            findings(&parse_report(
                &format!(
                    "[Parsed_astats_0 @ 0] Overall\n[Parsed_astats_0 @ 0] Peak level dB: 0.000000\n\
                     [Parsed_astats_0 @ 0] Peak count: {count}\n[Parsed_astats_0 @ 0] Abs Peak count: 9\n\
                     [Parsed_astats_0 @ 0] Number of samples: 100000\n"
                ),
                None,
            ))
        };
        let hard = clip("29000");
        assert_eq!(hard[0].check, Check::Clipping);
        assert!((hard[0].share.unwrap() - 0.29).abs() < 1e-9);
        // One loud shot touching full scale is not clipping.
        assert!(clip("3").is_empty());
    }

    #[test]
    fn per_channel_lines_before_overall_are_ignored() {
        let report = parse_report(
            "[Parsed_astats_0 @ 0] Channel: 1\n[Parsed_astats_0 @ 0] Peak level dB: -70\n\
             [Parsed_astats_0 @ 0] Overall\n[Parsed_astats_0 @ 0] Peak level dB: -3\n",
            None,
        );
        assert_eq!(report.sound.unwrap().peak_db, Some(-3.0));
    }

    #[test]
    fn a_sequence_input_follows_hlaes_numbering() {
        let files = vec![
            PathBuf::from("D:/t/all/00000.bmp"),
            PathBuf::from("D:/t/all/00001.bmp"),
        ];
        assert_eq!(
            sequence_input(&files, 300.0),
            Some(PictureInput::Sequence {
                pattern: PathBuf::from("D:/t/all/%05d.bmp"),
                start: 0,
                fps: 300.0
            })
        );
        assert_eq!(
            sequence_input(&[PathBuf::from("D:/t/shot_1.tga")], 300.0),
            None
        );
        assert_eq!(sequence_input(&files, 0.0), None);
        assert_eq!(sequence_input(&[], 300.0), None);
    }

    #[test]
    fn the_pass_reads_hlaes_wav_or_the_videos_own_sound() {
        let video = PictureInput::Video(PathBuf::from("v.avi"));
        let hlae = pass_args(&video, Some(Path::new("sound.wav")));
        assert!(hlae.windows(2).any(|w| w == ["-map", "1:a:0"]));
        assert!(hlae.windows(2).any(|w| w == ["-i", "sound.wav"]));
        let obs = pass_args(&video, None);
        assert!(obs.windows(2).any(|w| w == ["-map", "0:a:0?"]));
        assert_eq!(obs.iter().filter(|a| *a == "-i").count(), 1);
        assert!(obs.iter().any(|a| a.starts_with("blackdetect=d=0.5")));
        assert_eq!(obs.last().unwrap(), "-");
    }

    #[test]
    fn progress_lines_give_seconds_done() {
        assert_eq!(progress_secs("out_time_us=1500000"), Some(1.5));
        assert_eq!(progress_secs("out_time_us=N/A"), None);
        assert_eq!(progress_secs("out_time_us=-9223372036854775807"), None);
        assert_eq!(progress_secs("frame=10"), None);
    }

    /// FFmpeg for the tests that need it: `DODSTUDIO_TEST_FFMPEG`, else
    /// `ffmpeg` on PATH. `None` when neither runs.
    fn test_ffmpeg() -> Option<PathBuf> {
        let candidate = std::env::var_os("DODSTUDIO_TEST_FFMPEG")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("ffmpeg"));
        std::process::Command::new(&candidate)
            .arg("-version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .ok()
            .filter(|s| s.success())
            .map(|_| candidate)
    }

    /// A take made with FFmpeg: 6 s of moving test picture with 2 s of black
    /// in it and a 2.5 s still at the end, and a silent `sound.wav`. Skipped
    /// (passes) where no FFmpeg runs.
    #[tokio::test]
    async fn checks_a_real_take_and_caches_the_result() {
        let Some(ffmpeg) = test_ffmpeg() else {
            eprintln!("no FFmpeg: set DODSTUDIO_TEST_FFMPEG to run this test");
            return;
        };
        let take = Scratch::new("take_checks_real");
        let take = take.path();
        std::fs::create_dir_all(take.join("all")).unwrap();
        let make = |args: &[&str]| {
            let ok = std::process::Command::new(&ffmpeg)
                .args(["-hide_banner", "-loglevel", "error", "-y"])
                .args(args)
                .status()
                .unwrap()
                .success();
            assert!(ok, "{args:?}");
        };
        let video = take.join("all").join("video.avi");
        make(&[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x180:rate=30:duration=6",
            "-vf",
            "drawbox=enable='between(t,1,3)':color=black:t=fill,\
             trim=end=3.5,tpad=stop_mode=clone:stop_duration=2.5",
            "-c:v",
            "mjpeg",
            "-q:v",
            "2",
            video.to_str().unwrap(),
        ]);
        make(&[
            "-f",
            "lavfi",
            "-i",
            "anullsrc=r=22050:cl=stereo",
            "-t",
            "6",
            take.join("sound.wav").to_str().unwrap(),
        ]);

        let cancel = Arc::new(AtomicBool::new(false));
        let mut ticks = Vec::new();
        let (checks, cached) = make_checks(&ffmpeg, take, "all", None, cancel.clone(), |d, t| {
            ticks.push((d, t))
        })
        .await
        .unwrap();
        assert!(!cached);
        assert!(!ticks.is_empty());
        let kinds: Vec<Check> = checks.findings.iter().map(|f| f.check).collect();
        assert!(kinds.contains(&Check::Black), "{checks:?}");
        assert!(kinds.contains(&Check::Frozen), "{checks:?}");
        assert!(kinds.contains(&Check::Silent), "{checks:?}");
        let black = checks
            .findings
            .iter()
            .find(|f| f.check == Check::Black)
            .unwrap();
        assert!((black.at_secs.unwrap() - 1.0).abs() < 0.1, "{black:?}");
        // The black stretch doesn't move either; it is reported once, as black.
        let frozen: Vec<_> = checks
            .findings
            .iter()
            .filter(|f| f.check == Check::Frozen)
            .collect();
        assert_eq!(frozen.len(), 1, "{checks:?}");
        assert!((frozen[0].at_secs.unwrap() - 3.5).abs() < 0.2, "{frozen:?}");

        let (again, cached) = make_checks(&ffmpeg, take, "all", None, cancel.clone(), |_, _| {})
            .await
            .unwrap();
        assert!(cached);
        assert_eq!(again, checks);

        // A new sound.wav makes the results stale.
        make(&[
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=22050",
            "-t",
            "6",
            take.join("sound.wav").to_str().unwrap(),
        ]);
        assert_eq!(read_cached(take, "all"), None);

        cancel.store(true, Ordering::SeqCst);
        let err = make_checks(&ffmpeg, take, "all", None, cancel, |_, _| {})
            .await
            .unwrap_err();
        assert!(take_frames::is_cancelled(&err), "{err}");
    }

    /// The live check: `DS_TAKE_CHECKS_TAKES` lists take folders (HLAE's
    /// `take0000`, `;`-separated) to check, each printed with its findings
    /// and how long the pass took. Needs FFmpeg (`DODSTUDIO_TEST_FFMPEG`).
    #[tokio::test]
    #[ignore = "needs real takes: set DS_TAKE_CHECKS_TAKES and DODSTUDIO_TEST_FFMPEG"]
    async fn real_takes() {
        let ffmpeg = test_ffmpeg().expect("no FFmpeg: set DODSTUDIO_TEST_FFMPEG");
        let list = std::env::var("DS_TAKE_CHECKS_TAKES").expect("DS_TAKE_CHECKS_TAKES is not set");
        for take in list.split(';').filter(|s| !s.is_empty()) {
            let started = std::time::Instant::now();
            let result = make_checks(
                &ffmpeg,
                Path::new(take),
                "all",
                None,
                Arc::new(AtomicBool::new(false)),
                |_, _| {},
            )
            .await;
            println!("{take} ({:.1} s):", started.elapsed().as_secs_f64());
            match result {
                Ok((checks, cached)) => {
                    println!(
                        "  cached {cached}, {:?} s, peak {:?} dB, rms {:?} dB",
                        checks.duration_secs, checks.peak_db, checks.rms_db
                    );
                    for f in &checks.findings {
                        println!("  {f:?}");
                    }
                }
                Err(e) => println!("  error: {e}"),
            }
        }
    }
}
