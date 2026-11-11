//! A take's picture without a player (#1069): a few frames spread across one
//! stream of a take, made once and cached beside it, for the Render tab's
//! thumbnail, hover preview and contact sheet.
//!
//! The pieces are kept apart so the take checks (#1084) can reuse them for
//! their own pass over the same take:
//!
//! - [`stream_media`] says what a stream folder holds: a frame sequence
//!   (HLAE's BMP or TGA files) or one video (HLAE direct-to-video, OBS).
//! - [`sequence_picks`] / [`video_picks`] spread `n` frames across it.
//! - [`run_ffmpeg`] runs one FFmpeg command, polled and cancellable, and
//!   returns what it printed, so a check can run `signalstats` or `astats`
//!   through it and parse the report.
//! - [`cache_dir`] is the take's own folder for what these passes make
//!   (`<take>/dodstudio_preview/<stream>/`). It lives inside the take folder,
//!   so Delete Take (#644) sends it to the Recycle Bin with the take.
//!
//! Frame files are picked, not decoded: a frame sequence's chosen files are
//! only scaled down to JPEG; a video is seeked (`-ss` before `-i`), which
//! is near-instant for the intra-only codecs HLAE writes (utvideo, ProRes)
//! and costs one GOP for OBS's H.264.

#![cfg(not(target_arch = "wasm32"))]

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt;
use tokio::process::Command;

use crate::hlcr::scanner;

/// The folder inside a take that holds every stream's cached frames.
pub const PREVIEW_DIR: &str = "dodstudio_preview";
/// The list of a stream's frames, written last so a half-made set is never
/// read back.
pub const MANIFEST_FILE: &str = "sheet.json";
/// Frames on a contact sheet. Odd, so the middle one (the row's thumbnail)
/// is the clip's exact middle; 3 × 3 on the sheet.
pub const SHEET_FRAMES: usize = 9;
/// Width of each cached frame, enough for the hover preview and the sheet.
pub const FRAME_WIDTH: u32 = 640;
/// Bumped when the cached set changes shape, so older sets are made again.
const MANIFEST_VERSION: u32 = 1;
/// Longest one FFmpeg call may run before it is stopped. One frame is well
/// under a second; this only catches a file FFmpeg hangs on.
const FFMPEG_TIMEOUT: Duration = Duration::from_secs(60);

/// Frame-sequence file types HLAE writes.
const SEQUENCE_EXTENSIONS: &[&str] = &["bmp", "tga"];

/// What one stream folder holds.
#[derive(Debug, Clone, PartialEq)]
pub enum StreamMedia {
    /// One file per frame, in frame order.
    Sequence(Vec<PathBuf>),
    /// One video file.
    Video(PathBuf),
}

/// What a stream folder holds, or `None` when it holds neither shape.
pub fn stream_media(stream_folder: &Path) -> Option<StreamMedia> {
    if let Some(video) = scanner::stream_video_path(stream_folder) {
        return Some(StreamMedia::Video(video));
    }
    let frames = sequence_files(stream_folder);
    (!frames.is_empty()).then_some(StreamMedia::Sequence(frames))
}

/// A frame sequence's files, sorted by name (HLAE numbers them `00000.bmp`
/// up, zero-padded, so name order is frame order).
pub fn sequence_files(folder: &Path) -> Vec<PathBuf> {
    let Ok(read_dir) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = read_dir
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .map(|x| x.to_string_lossy().to_lowercase())
                .is_some_and(|x| SEQUENCE_EXTENSIONS.contains(&x.as_str()))
        })
        .collect();
    files.sort_by_key(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()));
    files
}

/// Where `n` frames sit across a clip, as fractions of its length: the
/// middle of `n` equal parts, so the first and last frames stay clear of the
/// clip's very ends (often a black first frame or a cut).
pub fn spread(n: usize) -> Vec<f64> {
    (0..n).map(|i| (i as f64 + 0.5) / n as f64).collect()
}

/// The frame indexes to show from a sequence of `frame_count` frames: `n`
/// spread across it, fewer when the clip is shorter than `n` frames.
pub fn sequence_picks(frame_count: usize, n: usize) -> Vec<usize> {
    let mut picks: Vec<usize> = spread(n)
        .into_iter()
        .map(|f| ((f * frame_count as f64) as usize).min(frame_count.saturating_sub(1)))
        .collect();
    if frame_count == 0 {
        return Vec::new();
    }
    picks.dedup();
    picks
}

/// The times (seconds) to show from a video `duration_secs` long.
pub fn video_picks(duration_secs: f64, n: usize) -> Vec<f64> {
    if !duration_secs.is_finite() || duration_secs <= 0.0 {
        return Vec::new();
    }
    spread(n).into_iter().map(|f| f * duration_secs).collect()
}

/// Whether `stream` is a bare folder name, safe to join onto a take folder.
pub fn is_plain_stream_name(stream: &str) -> bool {
    !stream.is_empty()
        && stream != "."
        && stream != ".."
        && !stream.contains(['/', '\\', ':'])
        && !stream.eq_ignore_ascii_case(PREVIEW_DIR)
}

/// The folder a stream's cached frames go in.
pub fn cache_dir(take_folder: &Path, stream: &str) -> PathBuf {
    take_folder.join(PREVIEW_DIR).join(stream)
}

/// What the cached set was made from, so a take re-captured into the same
/// folder makes a new set instead of showing the old one.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SourceStamp {
    /// The video's size, or the last frame file's.
    pub len: u64,
    /// The video's (or last frame file's) last-write time, ms since 1970.
    pub modified_ms: u64,
    /// Frame files in a sequence; 0 for a video.
    pub frame_count: usize,
}

fn file_stamp(path: &Path, frame_count: usize) -> SourceStamp {
    let meta = std::fs::metadata(path).ok();
    SourceStamp {
        len: meta.as_ref().map(|m| m.len()).unwrap_or(0),
        modified_ms: meta
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0),
        frame_count,
    }
}

/// The stamp of what a stream holds now.
pub fn source_stamp(media: &StreamMedia) -> SourceStamp {
    match media {
        StreamMedia::Video(file) => file_stamp(file, 0),
        StreamMedia::Sequence(frames) => match frames.last() {
            Some(last) => file_stamp(last, frames.len()),
            None => SourceStamp::default(),
        },
    }
}

/// One cached frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SheetFrame {
    /// File name inside the stream's cache folder.
    pub file: String,
    /// Frame number in the clip, counting from 0, when known (always for a
    /// sequence; from the time and rate for a video).
    pub frame: Option<u64>,
    /// Time into the clip, in seconds, when known (a sequence needs the
    /// take's recorded rate for this).
    pub secs: Option<f64>,
}

/// A stream's cached frames, in clip order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sheet {
    pub version: u32,
    /// The stream folder's name.
    pub stream: String,
    /// `"video"` or `"sequence"`.
    pub kind: String,
    pub source: SourceStamp,
    /// The clip's length in seconds, when known.
    pub duration_secs: Option<f64>,
    /// Frames per second, when known.
    pub fps: Option<f64>,
    pub frames: Vec<SheetFrame>,
}

impl Sheet {
    /// The frame the row's thumbnail shows: the middle one.
    pub fn middle(&self) -> Option<&SheetFrame> {
        self.frames.get(self.frames.len() / 2)
    }
}

/// The cached set for a stream, when there is a complete one made from what
/// the stream holds now.
pub fn read_cached(take_folder: &Path, stream: &str) -> Option<Sheet> {
    let media = stream_media(&take_folder.join(stream))?;
    read_cached_for(take_folder, stream, &media)
}

fn read_cached_for(take_folder: &Path, stream: &str, media: &StreamMedia) -> Option<Sheet> {
    let dir = cache_dir(take_folder, stream);
    let text = std::fs::read_to_string(dir.join(MANIFEST_FILE)).ok()?;
    let sheet: Sheet = serde_json::from_str(&text).ok()?;
    let usable = sheet.version == MANIFEST_VERSION
        && sheet.source == source_stamp(media)
        && !sheet.frames.is_empty()
        && sheet.frames.iter().all(|f| dir.join(&f.file).is_file());
    usable.then_some(sheet)
}

/// The error a cancelled pass returns.
pub fn cancelled_error() -> String {
    crate::messages::TAKE_FRAMES_CANCELLED.to_string()
}

/// Whether `err` is a cancelled pass.
pub fn is_cancelled(err: &str) -> bool {
    err == crate::messages::TAKE_FRAMES_CANCELLED
}

/// What a finished FFmpeg run said.
#[derive(Debug, Clone)]
pub struct FfmpegRun {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
#[cfg(target_os = "windows")]
const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;

/// Runs FFmpeg with `args` and returns its output. Polled, never awaited
/// blind: `cancel` (checked every cycle) and the timeout both kill it, and
/// `kill_on_drop` makes sure it can't outlive Studio.
pub async fn run_ffmpeg(
    ffmpeg: &Path,
    args: &[String],
    cancel: &AtomicBool,
    timeout: Duration,
) -> Result<FfmpegRun, String> {
    run_ffmpeg_watched(ffmpeg, args, cancel, timeout, |_| {}).await
}

/// [`run_ffmpeg`], handing each line FFmpeg writes to stdout to `on_line`
/// as it arrives (with `-progress pipe:1`, its progress report), from the
/// polling loop, so a long pass can show how far it is.
pub async fn run_ffmpeg_watched(
    ffmpeg: &Path,
    args: &[String],
    cancel: &AtomicBool,
    timeout: Duration,
    mut on_line: impl FnMut(&str),
) -> Result<FfmpegRun, String> {
    let mut cmd = Command::new(ffmpeg);
    // No window, and below-normal priority: these passes run in the
    // background, and must never take CPU from the game capturing, OBS or
    // a render (#1084).
    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS);
    cmd.kill_on_drop(true)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| crate::messages::labeled("FFmpeg", e))?;
    // Both pipes drain while FFmpeg runs, so a long report (signalstats
    // prints a line per frame) can't fill a pipe and stall it.
    let (line_tx, mut line_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let stdout = tokio::spawn(read_lines(child.stdout.take(), line_tx));
    let stderr = tokio::spawn(read_all(child.stderr.take()));
    let started = Instant::now();
    let mut out = String::new();
    let mut take_lines = |out: &mut String| {
        while let Ok(line) = line_rx.try_recv() {
            on_line(&line);
            out.push_str(&line);
            out.push('\n');
        }
    };
    let status = loop {
        take_lines(&mut out);
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill().await;
            return Err(cancelled_error());
        }
        if started.elapsed() > timeout {
            let _ = child.kill().await;
            return Err(crate::messages::take_frames_timed_out(timeout.as_secs()));
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(e) => return Err(crate::messages::labeled("FFmpeg", e)),
        }
        tokio::time::sleep(Duration::from_millis(16)).await;
    };
    // The pipe closes with the process; what it wrote last is read here.
    let _ = stdout.await;
    take_lines(&mut out);
    Ok(FfmpegRun {
        success: status.success(),
        stdout: out,
        stderr: stderr.await.unwrap_or_default(),
    })
}

async fn read_lines(
    pipe: Option<impl tokio::io::AsyncRead + Unpin>,
    tx: tokio::sync::mpsc::UnboundedSender<String>,
) {
    use tokio::io::AsyncBufReadExt;
    let Some(pipe) = pipe else { return };
    let mut lines = tokio::io::BufReader::new(pipe).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if tx.send(line).is_err() {
            return;
        }
    }
}

async fn read_all(pipe: Option<impl tokio::io::AsyncRead + Unpin>) -> String {
    let mut bytes = Vec::new();
    if let Some(mut pipe) = pipe {
        let _ = pipe.read_to_end(&mut bytes).await;
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// A video's length in seconds and its frame rate, from `ffmpeg -i`'s report.
pub async fn probe_video(
    ffmpeg: &Path,
    video: &Path,
    cancel: &AtomicBool,
) -> Result<(f64, Option<f64>), String> {
    let args = vec![
        "-hide_banner".to_string(),
        "-i".to_string(),
        video.to_string_lossy().into_owned(),
    ];
    // `ffmpeg -i` with no output exits 1 after printing the report.
    let run = run_ffmpeg(ffmpeg, &args, cancel, FFMPEG_TIMEOUT).await?;
    let info = crate::hlcr::combine::parse_report(&video.to_string_lossy(), &run.stderr)
        .filter(|i| i.duration_secs > 0.0)
        .ok_or_else(|| crate::messages::take_frames_no_length(video.display()))?;
    Ok((info.duration_secs, info.fps.parse::<f64>().ok()))
}

/// Where one frame comes from.
#[derive(Debug, Clone, PartialEq)]
pub enum FrameInput<'a> {
    /// A frame file of a sequence.
    File(&'a Path),
    /// A time (seconds) in a video.
    VideoAt(&'a Path, f64),
}

/// FFmpeg's arguments for writing one frame, scaled to `width`, as a JPEG.
pub fn frame_args(input: &FrameInput, out: &Path, width: u32) -> Vec<String> {
    let mut args: Vec<String> = ["-hide_banner", "-loglevel", "error", "-y"]
        .map(String::from)
        .to_vec();
    match input {
        FrameInput::File(file) => {
            args.extend(["-i".to_string(), file.to_string_lossy().into_owned()]);
        }
        FrameInput::VideoAt(video, secs) => args.extend([
            "-ss".to_string(),
            format!("{secs:.3}"),
            "-i".to_string(),
            video.to_string_lossy().into_owned(),
        ]),
    }
    args.extend([
        "-map".to_string(),
        "0:v:0".to_string(),
        "-frames:v".to_string(),
        "1".to_string(),
        "-vf".to_string(),
        format!("scale={width}:-2"),
        "-q:v".to_string(),
        "4".to_string(),
        out.to_string_lossy().into_owned(),
    ]);
    args
}

/// Writes one frame from `input` to `out`.
pub async fn extract_frame(
    ffmpeg: &Path,
    input: &FrameInput<'_>,
    out: &Path,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let _ = std::fs::remove_file(out);
    let run = run_ffmpeg(
        ffmpeg,
        &frame_args(input, out, FRAME_WIDTH),
        cancel,
        FFMPEG_TIMEOUT,
    )
    .await?;
    if !run.success {
        return Err(crate::messages::combine_failed(run.stderr.trim()));
    }
    if !out.is_file() {
        let (at, source) = match input {
            FrameInput::File(f) => ("its only frame".to_string(), f.display().to_string()),
            FrameInput::VideoAt(v, s) => (format!("{s:.2} s"), v.display().to_string()),
        };
        return Err(crate::messages::take_frames_no_frame(&at, source));
    }
    Ok(())
}

/// The take's frames for `stream`: the cached set when it is current, else a
/// new one, `progress(done, total)` after each frame. `fps` is the take's
/// recorded rate, when known, for a sequence's times.
pub async fn make_sheet(
    ffmpeg: &Path,
    take_folder: &Path,
    stream: &str,
    fps: Option<f64>,
    cancel: Arc<AtomicBool>,
    mut progress: impl FnMut(usize, usize),
) -> Result<(Sheet, bool), String> {
    if !is_plain_stream_name(stream) {
        return Err(crate::messages::take_frames_bad_stream_name(stream));
    }
    let media = stream_media(&take_folder.join(stream))
        .ok_or_else(|| crate::messages::take_frames_no_stream(stream, take_folder.display()))?;
    if let Some(sheet) = read_cached_for(take_folder, stream, &media) {
        return Ok((sheet, true));
    }

    let dir = cache_dir(take_folder, stream);
    std::fs::create_dir_all(&dir).map_err(|e| crate::messages::labeled(dir.display(), e))?;
    let manifest = dir.join(MANIFEST_FILE);
    // Gone before any frame is rewritten, so a pass cut off halfway leaves
    // no list pointing at a mix of old and new frames.
    let _ = std::fs::remove_file(&manifest);

    let fps = fps.filter(|f| *f > 0.0);
    let mut sheet = Sheet {
        version: MANIFEST_VERSION,
        stream: stream.to_string(),
        kind: String::new(),
        source: source_stamp(&media),
        duration_secs: None,
        fps,
        frames: Vec::new(),
    };
    match &media {
        StreamMedia::Sequence(files) => {
            sheet.kind = "sequence".to_string();
            sheet.duration_secs = fps.map(|f| files.len() as f64 / f);
            let picks = sequence_picks(files.len(), SHEET_FRAMES);
            for (i, &index) in picks.iter().enumerate() {
                let file = format!("frame_{i}.jpg");
                extract_frame(
                    ffmpeg,
                    &FrameInput::File(&files[index]),
                    &dir.join(&file),
                    &cancel,
                )
                .await?;
                sheet.frames.push(SheetFrame {
                    file,
                    frame: Some(index as u64),
                    secs: fps.map(|f| index as f64 / f),
                });
                progress(i + 1, picks.len());
            }
        }
        StreamMedia::Video(video) => {
            sheet.kind = "video".to_string();
            let (duration, video_fps) = probe_video(ffmpeg, video, &cancel).await?;
            let rate = video_fps.or(fps);
            sheet.duration_secs = Some(duration);
            sheet.fps = rate;
            let picks = video_picks(duration, SHEET_FRAMES);
            for (i, &secs) in picks.iter().enumerate() {
                let file = format!("frame_{i}.jpg");
                extract_frame(
                    ffmpeg,
                    &FrameInput::VideoAt(video, secs),
                    &dir.join(&file),
                    &cancel,
                )
                .await?;
                sheet.frames.push(SheetFrame {
                    file,
                    frame: rate.map(|r| (secs * r).round() as u64),
                    secs: Some(secs),
                });
                progress(i + 1, picks.len());
            }
        }
    }

    let text = serde_json::to_string_pretty(&sheet).map_err(|e| e.to_string())?;
    let part = dir.join(format!("{MANIFEST_FILE}.part"));
    std::fs::write(&part, text).map_err(|e| crate::messages::labeled(part.display(), e))?;
    std::fs::rename(&part, &manifest)
        .map_err(|e| crate::messages::labeled(manifest.display(), e))?;
    Ok((sheet, false))
}

/// A cached frame as `data:image/jpeg;base64,...`, for the webview.
pub fn frame_data_uri(path: &Path) -> Result<String, String> {
    use base64::Engine as _;
    let bytes = std::fs::read(path).map_err(|e| crate::messages::labeled(path.display(), e))?;
    Ok(format!(
        "data:image/jpeg;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_are_spread_across_the_clip_with_the_thumbnail_in_the_middle() {
        assert_eq!(spread(3), vec![1.0 / 6.0, 0.5, 5.0 / 6.0]);
        let picks = sequence_picks(900, SHEET_FRAMES);
        assert_eq!(picks, vec![50, 150, 250, 350, 450, 550, 650, 750, 850]);
        assert_eq!(picks[picks.len() / 2], 450);
    }

    #[test]
    fn a_clip_shorter_than_the_sheet_shows_each_frame_once() {
        assert_eq!(sequence_picks(4, SHEET_FRAMES), vec![0, 1, 2, 3]);
        assert_eq!(sequence_picks(1, SHEET_FRAMES), vec![0]);
        assert!(sequence_picks(0, SHEET_FRAMES).is_empty());
    }

    #[test]
    fn video_times_stay_inside_the_clip() {
        let t = video_picks(18.0, SHEET_FRAMES);
        assert_eq!(t.len(), 9);
        assert!((t[0] - 1.0).abs() < 1e-9);
        assert!((t[4] - 9.0).abs() < 1e-9);
        assert!((t[8] - 17.0).abs() < 1e-9);
        assert!(video_picks(0.0, 9).is_empty());
        assert!(video_picks(f64::NAN, 9).is_empty());
    }

    #[test]
    fn only_a_bare_folder_name_is_a_stream() {
        assert!(is_plain_stream_name("all"));
        assert!(is_plain_stream_name("hudColor"));
        for bad in [
            "",
            ".",
            "..",
            "../x",
            "a/b",
            "a\\b",
            "C:",
            "dodstudio_preview",
        ] {
            assert!(!is_plain_stream_name(bad), "{bad}");
        }
    }

    #[test]
    fn the_cache_lives_inside_the_take_so_deleting_the_take_takes_it_too() {
        let take = Path::new("D:/caps/session_1/chain_01_b0/take0000");
        let dir = cache_dir(take, "all");
        assert!(dir.starts_with(take));
        assert_eq!(dir, take.join("dodstudio_preview").join("all"));
    }

    #[test]
    fn frame_args_seek_before_the_input_and_scale_down() {
        let out = Path::new("o.jpg");
        let v = Path::new("video.avi");
        let args = frame_args(&FrameInput::VideoAt(v, 2.5), out, 640);
        let ss = args.iter().position(|a| a == "-ss").unwrap();
        let i = args.iter().position(|a| a == "-i").unwrap();
        assert!(ss < i, "input seek, not output seek: {args:?}");
        assert_eq!(args[ss + 1], "2.500");
        assert!(args.contains(&"scale=640:-2".to_string()));
        let file = frame_args(&FrameInput::File(Path::new("00010.bmp")), out, 640);
        assert!(!file.contains(&"-ss".to_string()));
        assert_eq!(file.last().unwrap(), "o.jpg");
    }

    fn temp_take(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "dodstudio_take_frames_{name}_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("all")).unwrap();
        dir
    }

    #[test]
    fn a_sequence_lists_bmp_and_tga_frames_in_order() {
        let take = temp_take("seq");
        for n in ["00002.bmp", "00000.bmp", "00001.BMP", "notes.txt"] {
            std::fs::write(take.join("all").join(n), b"x").unwrap();
        }
        let Some(StreamMedia::Sequence(files)) = stream_media(&take.join("all")) else {
            panic!("not a sequence");
        };
        let names: Vec<_> = files
            .iter()
            .map(|f| f.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["00000.bmp", "00001.BMP", "00002.bmp"]);
        let _ = std::fs::remove_dir_all(&take);
    }

    #[test]
    fn a_cached_set_is_used_only_while_the_stream_is_unchanged() {
        let take = temp_take("cache");
        let video = take.join("all").join("video.avi");
        std::fs::write(&video, b"abc").unwrap();
        let media = stream_media(&take.join("all")).unwrap();
        let dir = cache_dir(&take, "all");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("frame_0.jpg"), b"jpg").unwrap();
        let sheet = Sheet {
            version: MANIFEST_VERSION,
            stream: "all".into(),
            kind: "video".into(),
            source: source_stamp(&media),
            duration_secs: Some(1.0),
            fps: Some(60.0),
            frames: vec![SheetFrame {
                file: "frame_0.jpg".into(),
                frame: Some(30),
                secs: Some(0.5),
            }],
        };
        std::fs::write(
            dir.join(MANIFEST_FILE),
            serde_json::to_string(&sheet).unwrap(),
        )
        .unwrap();
        assert_eq!(read_cached(&take, "all"), Some(sheet));

        // A re-capture into the same folder: the set is made again.
        std::fs::write(&video, b"abcdef").unwrap();
        assert_eq!(read_cached(&take, "all"), None);
        let _ = std::fs::remove_dir_all(&take);
    }

    /// FFmpeg for the tests that need a real video: `DODSTUDIO_TEST_FFMPEG`,
    /// else `ffmpeg` on PATH. `None` when neither runs.
    fn test_ffmpeg() -> Option<PathBuf> {
        let candidate = std::env::var_os("DODSTUDIO_TEST_FFMPEG")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("ffmpeg"));
        std::process::Command::new(&candidate)
            .arg("-version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .ok()
            .filter(|s| s.success())
            .map(|_| candidate)
    }

    /// Makes a 3 s, 30 fps test video and a 12-frame BMP sequence with
    /// FFmpeg, then builds both sheets. Skipped (passes) where no FFmpeg runs.
    #[tokio::test]
    async fn makes_a_sheet_from_a_real_video_and_a_real_sequence() {
        let Some(ffmpeg) = test_ffmpeg() else {
            eprintln!("no FFmpeg: set DODSTUDIO_TEST_FFMPEG to run this test");
            return;
        };
        let take = temp_take("real");
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
            "testsrc=size=320x180:rate=30:duration=3",
            "-c:v",
            "mjpeg",
            video.to_str().unwrap(),
        ]);
        std::fs::create_dir_all(take.join("hudcolor")).unwrap();
        make(&[
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=320x180:rate=30:duration=0.4",
            "-start_number",
            "0",
            take.join("hudcolor").join("%05d.bmp").to_str().unwrap(),
        ]);

        let cancel = Arc::new(AtomicBool::new(false));
        let mut steps = Vec::new();
        let (sheet, cached) = make_sheet(&ffmpeg, &take, "all", None, cancel.clone(), |d, t| {
            steps.push((d, t))
        })
        .await
        .unwrap();
        assert!(!cached);
        assert_eq!(sheet.kind, "video");
        assert_eq!(sheet.frames.len(), SHEET_FRAMES);
        assert_eq!(steps.last(), Some(&(SHEET_FRAMES, SHEET_FRAMES)));
        assert!((sheet.duration_secs.unwrap() - 3.0).abs() < 0.1);
        assert_eq!(sheet.fps, Some(30.0));
        let mid = sheet.middle().unwrap();
        assert!((mid.secs.unwrap() - 1.5).abs() < 0.1);
        assert!(
            frame_data_uri(&cache_dir(&take, "all").join(&mid.file))
                .unwrap()
                .starts_with("data:image/jpeg;base64,/9j/")
        );

        let (again, cached) = make_sheet(&ffmpeg, &take, "all", None, cancel.clone(), |_, _| {})
            .await
            .unwrap();
        assert!(cached);
        assert_eq!(again.source, sheet.source);
        assert_eq!(again.frames.len(), sheet.frames.len());
        assert_eq!(again.frames[8].frame, Some(85));

        let (seq, _) = make_sheet(
            &ffmpeg,
            &take,
            "hudcolor",
            Some(30.0),
            cancel.clone(),
            |_, _| {},
        )
        .await
        .unwrap();
        assert_eq!(seq.kind, "sequence");
        assert_eq!(seq.frames.len(), 9);
        assert_eq!(seq.middle().unwrap().frame, Some(6));
        assert!((seq.middle().unwrap().secs.unwrap() - 0.2).abs() < 1e-9);

        cancel.store(true, Ordering::SeqCst);
        std::fs::remove_file(cache_dir(&take, "all").join(MANIFEST_FILE)).unwrap();
        let err = make_sheet(&ffmpeg, &take, "all", None, cancel, |_, _| {})
            .await
            .unwrap_err();
        assert!(is_cancelled(&err), "{err}");
        let _ = std::fs::remove_dir_all(&take);
    }
}
