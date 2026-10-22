//! Joining rendered clips into one video (#107), the first piece of the
//! Rough Cut page idea (#585), which will reuse it as its export step.
//!
//! When every clip has the same video codec, size, frame rate and pixel
//! format, and the same audio, they are joined without re-encoding (FFmpeg's
//! concat demuxer with `-c copy`): seconds, and no quality lost. Otherwise
//! every clip is scaled and padded to the first one's size and frame rate
//! and the whole video is re-encoded to H.264 and AAC; a clip with no sound
//! gets silence for its length.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use serde::Serialize;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::Command;

/// What `ffmpeg -i` says about one clip.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct ClipInfo {
    pub path: String,
    pub duration_secs: f64,
    pub video_codec: String,
    pub pix_fmt: String,
    pub width: u32,
    pub height: u32,
    /// As FFmpeg prints it ("60", "29.97").
    pub fps: String,
    pub audio_codec: Option<String>,
    pub sample_rate: Option<u32>,
    pub channels: Option<String>,
}

/// How a set of clips would be joined.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CombinePlan {
    pub clips: Vec<ClipInfo>,
    /// True when the clips can be joined without re-encoding.
    pub stream_copy: bool,
    pub total_secs: f64,
}

/// Reads one clip's streams from `ffmpeg -i <clip>`'s report.
pub async fn probe(ffmpeg: &Path, clip: &Path) -> Result<ClipInfo, String> {
    let mut cmd = Command::new(ffmpeg);
    #[cfg(target_os = "windows")]
    cmd.creation_flags(0x08000000);
    cmd.kill_on_drop(true)
        .args(["-hide_banner", "-i"])
        .arg(clip)
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    // `ffmpeg -i` with no output exits 1 after printing the report; that is
    // expected, so only the text matters.
    let out = cmd
        .output()
        .await
        .map_err(|e| crate::messages::labeled("FFmpeg", e))?;
    let report = String::from_utf8_lossy(&out.stderr);
    parse_report(&clip.to_string_lossy(), &report)
        .ok_or_else(|| crate::messages::combine_unreadable_clip(&clip.to_string_lossy()))
}

/// Picks the first video and first audio stream out of an `ffmpeg -i` report.
pub fn parse_report(path: &str, report: &str) -> Option<ClipInfo> {
    let mut info = ClipInfo {
        path: path.to_string(),
        ..ClipInfo::default()
    };
    let mut have_video = false;
    for line in report.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("Duration: ") {
            info.duration_secs = parse_clock(rest.split(',').next().unwrap_or(""))?;
        } else if let Some(at) = line.find(": Video: ").filter(|_| !have_video) {
            let fields: Vec<&str> = split_fields(&line[at + 9..]);
            info.video_codec = first_word(fields.first()?);
            info.pix_fmt = first_word(fields.get(1).copied().unwrap_or(""))
                .trim_end_matches('(')
                .to_string();
            let (w, h) = fields.iter().find_map(|f| size_of(f))?;
            info.width = w;
            info.height = h;
            info.fps = fields
                .iter()
                .find_map(|f| f.trim().strip_suffix(" fps"))
                .unwrap_or("")
                .trim()
                .to_string();
            have_video = true;
        } else if let Some(at) = line
            .find(": Audio: ")
            .filter(|_| info.audio_codec.is_none())
        {
            let fields: Vec<&str> = split_fields(&line[at + 9..]);
            info.audio_codec = Some(first_word(fields.first()?));
            info.sample_rate = fields
                .iter()
                .find_map(|f| f.trim().strip_suffix(" Hz"))
                .and_then(|r| r.trim().parse().ok());
            info.channels = fields
                .iter()
                .position(|f| f.trim().ends_with(" Hz"))
                .and_then(|i| fields.get(i + 1))
                .map(|c| c.trim().to_string());
        }
    }
    have_video.then_some(info)
}

/// Splits a stream description at the commas that aren't inside brackets
/// (`yuv420p(tv, bt709, progressive)` is one field).
fn split_fields(text: &str) -> Vec<&str> {
    let mut fields = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (i, c) in text.char_indices() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            ',' if depth == 0 => {
                fields.push(&text[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    fields.push(&text[start..]);
    fields
}

fn first_word(field: &str) -> String {
    field
        .trim()
        .split(|c: char| c.is_whitespace() || c == '(')
        .next()
        .unwrap_or("")
        .to_string()
}

fn size_of(field: &str) -> Option<(u32, u32)> {
    let word = field.split_whitespace().next()?;
    let (w, h) = word.split_once('x')?;
    Some((w.parse().ok()?, h.parse().ok()?))
}

/// "00:01:05.25" in seconds.
fn parse_clock(text: &str) -> Option<f64> {
    let mut secs = 0.0;
    for part in text.trim().split(':') {
        secs = secs * 60.0 + part.parse::<f64>().ok()?;
    }
    Some(secs)
}

/// Whether `clips` can be joined without re-encoding: same container,
/// video codec, size, frame rate and pixel format, and the same audio (or
/// none on all of them).
pub fn can_stream_copy(clips: &[ClipInfo]) -> bool {
    let Some(first) = clips.first() else {
        return false;
    };
    let ext = |c: &ClipInfo| {
        Path::new(&c.path)
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
    };
    clips.iter().all(|c| {
        ext(c) == ext(first)
            && c.video_codec == first.video_codec
            && (c.width, c.height) == (first.width, first.height)
            && c.fps == first.fps
            && c.pix_fmt == first.pix_fmt
            && c.audio_codec == first.audio_codec
            && c.sample_rate == first.sample_rate
            && c.channels == first.channels
    })
}

/// Probes every clip and says how they would be joined.
pub async fn plan(ffmpeg: &Path, clips: &[PathBuf]) -> Result<CombinePlan, String> {
    let mut infos = Vec::with_capacity(clips.len());
    for clip in clips {
        infos.push(probe(ffmpeg, clip).await?);
    }
    Ok(CombinePlan {
        stream_copy: can_stream_copy(&infos),
        total_secs: infos.iter().map(|c| c.duration_secs).sum(),
        clips: infos,
    })
}

/// The concat demuxer's list file. Forward slashes, single quotes escaped
/// the way FFmpeg's tokenizer reads them.
pub fn concat_list(clips: &[ClipInfo]) -> String {
    clips
        .iter()
        .map(|c| {
            format!(
                "file '{}'\n",
                c.path.replace('\\', "/").replace('\'', "'\\''")
            )
        })
        .collect()
}

/// The arguments for a join, without the leading FFmpeg path. `list` is
/// where `concat_list` was written (stream copy only).
pub fn combine_args(plan: &CombinePlan, list: &Path, output: &Path) -> Vec<String> {
    let mut args: Vec<String> = vec!["-y".into(), "-hide_banner".into()];
    if plan.stream_copy {
        args.extend(["-f", "concat", "-safe", "0", "-i"].map(String::from));
        args.push(list.to_string_lossy().into_owned());
        args.extend(["-c", "copy"].map(String::from));
    } else {
        let first = &plan.clips[0];
        let fps = if first.fps.is_empty() {
            "60"
        } else {
            first.fps.as_str()
        };
        let mut filter = String::new();
        for (i, clip) in plan.clips.iter().enumerate() {
            args.push("-i".into());
            args.push(clip.path.clone());
            filter.push_str(&format!(
                "[{i}:v]scale={w}:{h}:force_original_aspect_ratio=decrease,pad={w}:{h}:(ow-iw)/2:(oh-ih)/2,setsar=1,fps={fps},format=yuv420p[v{i}];",
                w = first.width,
                h = first.height,
            ));
            if clip.audio_codec.is_some() {
                filter.push_str(&format!(
                    "[{i}:a]aresample=48000,aformat=channel_layouts=stereo[a{i}];"
                ));
            } else {
                filter.push_str(&format!(
                    "anullsrc=r=48000:cl=stereo,atrim=duration={:.3}[a{i}];",
                    clip.duration_secs
                ));
            }
        }
        for i in 0..plan.clips.len() {
            filter.push_str(&format!("[v{i}][a{i}]"));
        }
        filter.push_str(&format!("concat=n={}:v=1:a=1[v][a]", plan.clips.len()));
        args.extend(["-filter_complex".to_string(), filter]);
        args.extend(
            [
                "-map", "[v]", "-map", "[a]", "-c:v", "libx264", "-preset", "medium", "-crf", "18",
                "-c:a", "aac", "-b:a", "320k",
            ]
            .map(String::from),
        );
    }
    args.extend(["-progress", "pipe:1", "-loglevel", "error"].map(String::from));
    args.push(output.to_string_lossy().into_owned());
    args
}

/// Joins `plan`'s clips into `output`. `progress` gets the fraction done
/// (0..1), at most every 33 ms; `cancel` stops FFmpeg and removes the
/// half-written file.
pub async fn combine(
    ffmpeg: &Path,
    plan: &CombinePlan,
    output: &Path,
    cancel: Arc<AtomicBool>,
    mut progress: impl FnMut(f64),
) -> Result<(), String> {
    if plan.clips.len() < 2 {
        return Err(crate::messages::COMBINE_NEEDS_TWO.to_string());
    }
    if plan.clips.iter().any(|c| Path::new(&c.path) == output) {
        return Err(crate::messages::COMBINE_OUTPUT_IS_INPUT.to_string());
    }
    let list = std::env::temp_dir().join(format!("dodstudio_combine_{}.txt", std::process::id()));
    if plan.stream_copy {
        std::fs::write(&list, concat_list(&plan.clips))
            .map_err(|e| crate::messages::labeled(list.display(), e))?;
    }

    let mut cmd = Command::new(ffmpeg);
    #[cfg(target_os = "windows")]
    cmd.creation_flags(0x08000000);
    cmd.kill_on_drop(true)
        .args(combine_args(plan, &list, output))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| crate::messages::labeled("FFmpeg", e))?;

    // FFmpeg's -progress lines: out_time_us=<microseconds>.
    let done_us = Arc::new(AtomicU64::new(0));
    let stdout = child.stdout.take();
    let reader_done = Arc::clone(&done_us);
    tokio::spawn(async move {
        let Some(stdout) = stdout else { return };
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if let Some(us) = line
                .strip_prefix("out_time_us=")
                .and_then(|v| v.trim().parse().ok())
            {
                reader_done.store(us, Ordering::Relaxed);
            }
        }
    });
    let mut stderr = child.stderr.take();
    let errors = tokio::spawn(async move {
        let mut text = String::new();
        if let Some(stderr) = stderr.as_mut() {
            let _ = stderr.read_to_string(&mut text).await;
        }
        text
    });

    let total_us = (plan.total_secs * 1_000_000.0).max(1.0);
    let mut last = std::time::Instant::now() - std::time::Duration::from_secs(1);
    let status = loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill().await;
            let _ = std::fs::remove_file(output);
            let _ = std::fs::remove_file(&list);
            return Err(crate::messages::COMBINE_CANCELLED.to_string());
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(e) => return Err(crate::messages::labeled("FFmpeg", e)),
        }
        if last.elapsed() >= std::time::Duration::from_millis(33) {
            last = std::time::Instant::now();
            progress((done_us.load(Ordering::Relaxed) as f64 / total_us).min(1.0));
        }
        tokio::time::sleep(std::time::Duration::from_millis(16)).await;
    };
    let _ = std::fs::remove_file(&list);
    if status.success() {
        progress(1.0);
        Ok(())
    } else {
        let _ = std::fs::remove_file(output);
        let log = errors.await.unwrap_or_default();
        Err(crate::messages::combine_failed(log.trim()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const H264: &str = "Input #0, mov,mp4,m4a,3gp,3g2,mj2, from 'a.mp4':
  Duration: 00:00:05.03, start: 0.000000, bitrate: 12000 kb/s
  Stream #0:0[0x1](und): Video: h264 (High) (avc1 / 0x31637661), yuv420p(tv, bt709, progressive), 1920x1080 [SAR 1:1 DAR 16:9], 11800 kb/s, 60 fps, 60 tbr, 15360 tbn (default)
  Stream #0:1[0x2](und): Audio: aac (LC) (mp4a / 0x6134706D), 48000 Hz, stereo, fltp, 320 kb/s (default)
";
    const PRORES_SILENT: &str = "Input #0, mov,mp4,m4a,3gp,3g2,mj2, from 'b.mov':
  Duration: 00:01:02.50, start: 0.000000, bitrate: 220000 kb/s
  Stream #0:0[0x1]: Video: prores (HQ) (apch / 0x68637061), yuv422p10le(progressive), 1280x720, 219000 kb/s, 29.97 fps, 29.97 tbr, 30k tbn (default)
";

    #[test]
    fn reads_an_ffmpeg_report() {
        let a = parse_report("a.mp4", H264).unwrap();
        assert_eq!(
            a,
            ClipInfo {
                path: "a.mp4".into(),
                duration_secs: 5.03,
                video_codec: "h264".into(),
                pix_fmt: "yuv420p".into(),
                width: 1920,
                height: 1080,
                fps: "60".into(),
                audio_codec: Some("aac".into()),
                sample_rate: Some(48000),
                channels: Some("stereo".into()),
            }
        );
        let b = parse_report("b.mov", PRORES_SILENT).unwrap();
        assert_eq!(
            (b.video_codec.as_str(), b.pix_fmt.as_str()),
            ("prores", "yuv422p10le")
        );
        assert_eq!((b.width, b.height, b.fps.as_str()), (1280, 720, "29.97"));
        assert_eq!(b.duration_secs, 62.5);
        assert_eq!(b.audio_codec, None);
        assert_eq!(parse_report("x.txt", "x.txt: Invalid data found"), None);
    }

    #[test]
    fn copies_only_when_everything_matches() {
        let a = parse_report("C:/r/a.mp4", H264).unwrap();
        let a2 = ClipInfo {
            path: "C:/r/b.mp4".into(),
            ..a.clone()
        };
        assert!(can_stream_copy(&[a.clone(), a2.clone()]));
        let other_fps = ClipInfo {
            fps: "30".into(),
            ..a2.clone()
        };
        assert!(!can_stream_copy(&[a.clone(), other_fps]));
        let other_box = ClipInfo {
            path: "C:/r/b.mov".into(),
            ..a2
        };
        assert!(!can_stream_copy(&[a.clone(), other_box]));
        let b = parse_report("b.mov", PRORES_SILENT).unwrap();
        assert!(!can_stream_copy(&[a, b]));
        assert!(!can_stream_copy(&[]));
    }

    #[test]
    fn the_list_file_quotes_paths_ffmpeg_style() {
        let clip = ClipInfo {
            path: r"C:\clips\it's here.mp4".into(),
            ..ClipInfo::default()
        };
        assert_eq!(concat_list(&[clip]), "file 'C:/clips/it'\\''s here.mp4'\n");
    }

    #[test]
    fn a_re_encode_fits_every_clip_to_the_first_and_fills_silence() {
        let a = parse_report("a.mp4", H264).unwrap();
        let b = parse_report("b.mov", PRORES_SILENT).unwrap();
        let plan = CombinePlan {
            stream_copy: false,
            total_secs: 67.53,
            clips: vec![a, b],
        };
        let args = combine_args(&plan, Path::new("list.txt"), Path::new("out.mp4"));
        let filter = &args[args.iter().position(|a| a == "-filter_complex").unwrap() + 1];
        assert!(
            filter.contains(
                "[1:v]scale=1920:1080:force_original_aspect_ratio=decrease,pad=1920:1080"
            )
        );
        assert!(filter.contains("fps=60"));
        assert!(filter.contains("anullsrc=r=48000:cl=stereo,atrim=duration=62.500[a1]"));
        assert!(filter.ends_with("[v0][a0][v1][a1]concat=n=2:v=1:a=1[v][a]"));
        assert!(!args.contains(&"list.txt".to_string()));
        assert_eq!(args.last().unwrap(), "out.mp4");
    }

    /// Joins two short generated clips for real, both ways. Needs an FFmpeg:
    /// `DODSTUDIO_TEST_FFMPEG=<path to ffmpeg.exe>`.
    #[tokio::test]
    #[ignore]
    async fn joins_real_clips() {
        let ffmpeg =
            PathBuf::from(std::env::var("DODSTUDIO_TEST_FFMPEG").expect("DODSTUDIO_TEST_FFMPEG"));
        let dir = crate::test_support::Scratch::new("combine_real");
        let make = |name: &str, size: &str, rate: &str| {
            let out = dir.join(name);
            let status = std::process::Command::new(&ffmpeg)
                .args(["-y", "-loglevel", "error", "-f", "lavfi", "-i"])
                .arg(format!("testsrc=duration=1:size={size}:rate={rate}"))
                .args([
                    "-f",
                    "lavfi",
                    "-i",
                    "sine=duration=1",
                    "-shortest",
                    "-pix_fmt",
                    "yuv420p",
                ])
                .arg(&out)
                .status()
                .unwrap();
            assert!(status.success());
            out
        };
        let a = make("a.mp4", "320x240", "30");
        let b = make("b.mp4", "320x240", "30");
        let c = make("c.mp4", "160x120", "25");

        let same = plan(&ffmpeg, &[a.clone(), b]).await.unwrap();
        assert!(same.stream_copy);
        let out = dir.join("copy.mp4");
        combine(
            &ffmpeg,
            &same,
            &out,
            Arc::new(AtomicBool::new(false)),
            |_| {},
        )
        .await
        .unwrap();
        let joined = probe(&ffmpeg, &out).await.unwrap();
        assert!(
            (joined.duration_secs - 2.0).abs() < 0.2,
            "{}",
            joined.duration_secs
        );

        let mixed = plan(&ffmpeg, &[a, c]).await.unwrap();
        assert!(!mixed.stream_copy);
        let out = dir.join("encode.mp4");
        let mut last = 0.0;
        combine(
            &ffmpeg,
            &mixed,
            &out,
            Arc::new(AtomicBool::new(false)),
            |f| last = f,
        )
        .await
        .unwrap();
        assert_eq!(last, 1.0);
        let joined = probe(&ffmpeg, &out).await.unwrap();
        assert_eq!((joined.width, joined.height), (320, 240));
        assert!(
            (joined.duration_secs - 2.0).abs() < 0.2,
            "{}",
            joined.duration_secs
        );
    }
}
