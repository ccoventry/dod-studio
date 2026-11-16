//! Tests of segmented renders (#1451). The ones marked `#[ignore]` run a
//! real FFmpeg (`DOD_FFMPEG` names ffmpeg.exe) on small takes made by it, at
//! 3 fps so a 70-frame take is three segments of 30, 30 and 10 frames.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, mpsc};

use super::*;
use crate::hlcr::config::{RenderCodec, RenderConfig};
use crate::hlcr::renderer::ffmpeg_args::{FfmpegArgs, encode_settings};
use crate::hlcr::renderer::{RenderJob, RenderUpdate, run_render_job};
use crate::hlcr::scanner::ClipData;
use crate::test_support::Scratch;

const FRAMES: usize = 70;
const FPS: u32 = 3;

fn clip_for(take_folder: &Path, video: Option<&str>, hud: bool) -> ClipData {
    ClipData {
        clip_name: Some(if hud { "long hud" } else { "long take" }.to_string()),
        take_folder: take_folder.to_string_lossy().into_owned(),
        clip_type: if hud { "hud_only" } else { "single" }.to_string(),
        img_folder: if hud { "hudcolor" } else { "all" }.to_string(),
        wav_file: Some("sound.wav".to_string()),
        base_name: "t".to_string(),
        frame_count: FRAMES,
        width: 64,
        height: 48,
        date: "-".to_string(),
        video_file: video.map(str::to_string),
        alpha_folder: hud.then(|| "hudalpha".to_string()),
    }
}

fn config_for(export_dirs: &[&Path], codec: RenderCodec, ffmpeg: &str) -> RenderConfig {
    RenderConfig {
        ffmpeg_path: ffmpeg.to_string(),
        source_folder: String::new(),
        export_directories: export_dirs.iter().map(|d| d.to_path_buf()).collect(),
        fps: FPS,
        target_codec: codec,
        custom_codec_args: String::new(),
        max_concurrent_renders: 1,
        clip_tag: Some("v1/s/take0000/1/all".to_string()),
    }
}

fn job_for(clip: ClipData, config: RenderConfig) -> RenderJob {
    let (tx, _rx) = mpsc::channel();
    RenderJob::new(
        "9".to_string(),
        clip,
        config,
        tx,
        Arc::new(AtomicBool::new(false)),
    )
}

/// A take folder whose wav is long enough for its frames, with no frames:
/// enough for the plan and the segment key.
fn bare_take(root: &Path) -> PathBuf {
    let take = root.join("demo").join("take0000");
    std::fs::create_dir_all(take.join("all")).unwrap();
    write_wav_header(&take.join("sound.wav"), 30.0);
    take
}

/// A PCM wav header claiming `seconds` of 8 kHz mono sound, with the bytes.
fn write_wav_header(path: &Path, seconds: f64) {
    let data_len = (seconds * 16000.0) as u32;
    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1u16.to_le_bytes()); // mono
    wav.extend_from_slice(&8000u32.to_le_bytes());
    wav.extend_from_slice(&16000u32.to_le_bytes()); // byte rate
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    wav.resize(wav.len() + data_len as usize, 0);
    std::fs::write(path, wav).unwrap();
}

/// The plan, encoder and claimed folder a render of `job` would use.
fn claim(job: &RenderJob, output_folder: &Path) -> (SegmentPlan, Option<SegmentFolder>) {
    let plan = super::super::segment_plan::plan(&take_facts(job, FPS)).expect("a segmented take");
    let encode = encode_settings(job.is_hud, &job.config).unwrap();
    let folder = claim_folder(job, &plan, &encode, output_folder);
    (plan, folder)
}

#[test]
fn a_take_of_three_segments_is_planned_in_three() {
    let root = Scratch::new("segmented_plan");
    let take = bare_take(&root);
    let job = job_for(
        clip_for(&take, None, false),
        config_for(&[&root], RenderCodec::ProRes422, "ffmpeg"),
    );
    let (plan, _) = claim(&job, &root);
    let spans: Vec<usize> = plan.segments().iter().map(|s| s.frames).collect();
    assert_eq!(spans, [30, 30, 10]);
}

#[test]
fn claiming_throws_away_this_takes_other_settings_and_keeps_other_takes() {
    let root = Scratch::new("segmented_stale");
    let take = bare_take(&root);
    let export = root.join("export");
    let job = job_for(
        clip_for(&take, None, false),
        config_for(&[&export], RenderCodec::ProRes422, "ffmpeg"),
    );
    let (_, folder) = claim(&job, &export);
    let folder = folder.unwrap();
    let name = folder
        .dir
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let take_part = name.split('_').next().unwrap().to_string();
    drop(folder);

    let segments_root = export.join(SEGMENTS_DIR);
    let stale = segments_root.join(format!("{take_part}_0000000000000000"));
    let other_take = segments_root.join("ffffffffffffffff_0000000000000000");
    for dir in [&stale, &other_take] {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("0000.seg"), b"old").unwrap();
    }
    let (_, folder) = claim(&job, &export);
    assert_eq!(folder.unwrap().dir, segments_root.join(&name));
    assert!(
        !stale.exists(),
        "segments of other settings are thrown away"
    );
    assert!(other_take.join("0000.seg").exists(), "another take's stay");
}

#[test]
fn a_second_render_of_the_same_segments_renders_in_one_piece() {
    let root = Scratch::new("segmented_busy");
    let take = bare_take(&root);
    let job = job_for(
        clip_for(&take, None, false),
        config_for(&[&root], RenderCodec::ProRes422, "ffmpeg"),
    );
    let (_, first) = claim(&job, &root);
    assert!(first.is_some());
    let (_, second) = claim(&job, &root);
    assert!(second.is_none());
    drop(first);
    let (_, third) = claim(&job, &root);
    assert!(third.is_some(), "free again once the first render ends");
}

/// A stopped render's segments are found in whichever export folder holds
/// them, even when this render's clip goes to another.
#[test]
fn segments_in_another_export_folder_are_found() {
    let root = Scratch::new("segmented_other_folder");
    let take = bare_take(&root);
    let (first, second) = (root.join("e1"), root.join("e2"));
    let job = job_for(
        clip_for(&take, None, false),
        config_for(&[&first, &second], RenderCodec::ProRes422, "ffmpeg"),
    );
    let (_, folder) = claim(&job, &second);
    let in_second = folder.unwrap().dir.clone();
    assert!(in_second.starts_with(&second));
    let (_, folder) = claim(&job, &first);
    assert_eq!(folder.unwrap().dir, in_second);
}

#[test]
fn removing_a_folder_counts_its_finished_segments_and_drops_an_empty_root() {
    let root = Scratch::new("segmented_remove");
    let take = bare_take(&root);
    let job = job_for(
        clip_for(&take, None, false),
        config_for(&[&root], RenderCodec::ProRes422, "ffmpeg"),
    );
    let (_, folder) = claim(&job, &root);
    let folder = folder.unwrap();
    std::fs::write(folder.segment_file(0), b"a").unwrap();
    std::fs::write(folder.segment_file(1), b"b").unwrap();
    std::fs::write(folder.dir.join("0002.partial.seg"), b"c").unwrap();
    assert_eq!(folder.remove(), 2);
    assert!(!root.join(SEGMENTS_DIR).exists());
}

#[test]
fn segment_numbers_run_together_counted_from_one() {
    assert_eq!(segment_ranges(&[]), "");
    assert_eq!(segment_ranges(&[0]), "1");
    assert_eq!(segment_ranges(&[0, 1, 2, 3]), "1-4");
    assert_eq!(segment_ranges(&[0, 1, 3, 5, 6]), "1-2, 4, 6-7");
}

#[test]
fn segments_are_numbered_with_four_digits_and_no_video_extension() {
    assert_eq!(segment_name(3), "0003.seg");
    assert_eq!(segment_name(12345), "12345.seg");
}

#[test]
fn only_the_picked_folders_are_removed() {
    let dir = crate::test_support::Scratch::new("segments_stale");
    let root = dir.join(SEGMENTS_DIR);
    for name in ["take_old", "take_new", "other_x"] {
        std::fs::create_dir_all(root.join(name)).unwrap();
        std::fs::write(root.join(name).join("0000.seg"), b"x").unwrap();
    }
    remove_other_settings(std::slice::from_ref(&root), |f| f == "take_old");
    assert!(!root.join("take_old").exists());
    assert!(root.join("take_new").join("0000.seg").exists());
    assert!(root.join("other_x").exists());
}

// ── With a real FFmpeg ───────────────────────────────────────────────────────

fn real_ffmpeg() -> String {
    let ffmpeg = std::env::var("DOD_FFMPEG").expect("DOD_FFMPEG must name ffmpeg.exe");
    assert!(Path::new(&ffmpeg).is_file(), "no FFmpeg at {ffmpeg}");
    ffmpeg
}

fn ffmpeg_ok(ffmpeg: &str, cwd: &Path, args: &[String]) {
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

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

/// A 70-frame take at 3 fps with a 24 s wav: BMP frames, or a utvideo AVI
/// (`video`); a HUD take has a colour and an alpha stream.
fn make_take(ffmpeg: &str, root: &Path, video: bool, hud: bool) -> PathBuf {
    let take = root.join("demo").join("take0000");
    let streams: &[(&str, &str)] = if hud {
        &[("hudcolor", "testsrc2"), ("hudalpha", "testsrc")]
    } else {
        &[("all", "testsrc2")]
    };
    for (folder, source) in streams {
        std::fs::create_dir_all(take.join(folder)).unwrap();
        let mut args = strings(&["-f", "lavfi", "-i"]);
        args.push(format!("{source}=size=64x48:rate={FPS}"));
        args.extend(strings(&["-frames:v", &FRAMES.to_string()]));
        if video {
            args.extend(strings(&["-c:v", "utvideo"]));
            args.push(format!("{folder}/video.avi"));
        } else {
            args.extend(strings(&["-start_number", "0"]));
            args.push(format!("{folder}/%05d.bmp"));
        }
        ffmpeg_ok(ffmpeg, &take, &args);
    }
    ffmpeg_ok(
        ffmpeg,
        &take,
        &strings(&[
            "-f",
            "lavfi",
            "-i",
            "aevalsrc=0.5*sin(2*PI*(200+30*t)*t):s=22050:d=24",
            "-c:a",
            "pcm_s16le",
            "sound.wav",
        ]),
    );
    take
}

/// Each video frame's hash, in order.
fn frame_hashes(ffmpeg: &str, file: &Path) -> Vec<String> {
    let out = std::process::Command::new(ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-i"])
        .arg(file)
        .args(["-map", "0:v", "-f", "framemd5", "-"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| l.rsplit(',').next().map(|h| h.trim().to_string()))
        .collect()
}

/// The sound as raw 16-bit samples.
fn sound_bytes(ffmpeg: &str, file: &Path) -> Vec<u8> {
    let out = std::process::Command::new(ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-i"])
        .arg(file)
        .args(["-map", "0:a", "-f", "s16le", "-"])
        .output()
        .unwrap();
    out.stdout
}

/// The same take rendered in one piece, as before segments.
fn one_piece(ffmpeg: &str, clip: &ClipData, config: &RenderConfig, out: &Path) {
    let encode = encode_settings(clip.clip_type == "hud_only", config).unwrap();
    let mut args = FfmpegArgs {
        clip,
        fps: FPS,
        encode: &encode,
        threads: 2,
        out_file: out,
        clip_tag: None,
    }
    .to_args();
    // Its own `-y -hide_banner` and progress pipe are harmless here.
    args.retain(|a| a != "pipe:1" && a != "-progress");
    ffmpeg_ok(ffmpeg, Path::new(&clip.take_folder), &args);
}

async fn render(clip: ClipData, config: RenderConfig) -> Vec<RenderUpdate> {
    let (tx, rx) = mpsc::channel();
    run_render_job(
        "9".to_string(),
        clip,
        config,
        tx,
        Arc::new(AtomicBool::new(false)),
        Arc::new(Mutex::new(HashMap::new())),
    )
    .await;
    rx.try_iter().collect()
}

/// The finished clip's path, after checking the job ended with success and
/// its percentage only ever rose.
fn finished_clip(updates: &[RenderUpdate]) -> PathBuf {
    let mut last_percent = 0;
    let mut output = None;
    for update in updates {
        match update {
            RenderUpdate::Progress(_, p) => {
                assert!(*p >= last_percent, "progress went back: {updates:?}");
                last_percent = *p;
            }
            RenderUpdate::OutputPath(_, path) => output = Some(PathBuf::from(path)),
            RenderUpdate::Finished(_, ok, err) => assert!(*ok, "{err:?}"),
            _ => {}
        }
    }
    assert_eq!(last_percent, 100);
    output.expect("no output path")
}

/// Every take shape and intra-frame codec a segment covers, joined: the
/// same frames as the one-piece render, and the same sound sample for
/// sample, running to the picture's end (one piece stops it at the last
/// whole audio packet, `-shortest`).
#[tokio::test]
#[ignore = "needs FFmpeg: set DOD_FFMPEG to ffmpeg.exe's path"]
async fn a_segmented_render_matches_the_one_piece_render() {
    let ffmpeg = real_ffmpeg();
    let cases = [
        ("bmp_prores", false, false, RenderCodec::ProRes422),
        ("avi_prores", true, false, RenderCodec::ProRes422),
        ("bmp_huffyuv", false, false, RenderCodec::HuffYuv),
        ("avi_huffyuv", true, false, RenderCodec::HuffYuv),
        ("bmp_hud", false, true, RenderCodec::ProRes422),
        ("avi_hud", true, true, RenderCodec::ProRes422),
    ];
    for (name, video, hud, codec) in cases {
        let root = Scratch::new(format_args!("segmented_render_{name}"));
        let take = make_take(&ffmpeg, &root, video, hud);
        let export = root.join("export");
        std::fs::create_dir_all(&export).unwrap();
        let clip = clip_for(&take, video.then_some("video.avi"), hud);
        let config = config_for(&[&export], codec, &ffmpeg);
        let reference = root.join(format!(
            "one_piece{}",
            encode_settings(hud, &config).unwrap().file_ext
        ));
        one_piece(&ffmpeg, &clip, &config, &reference);

        let clip_file = finished_clip(&render(clip, config).await);
        let joined = frame_hashes(&ffmpeg, &clip_file);
        assert_eq!(joined.len(), FRAMES, "{name}");
        assert_eq!(joined, frame_hashes(&ffmpeg, &reference), "{name}");
        let sound = sound_bytes(&ffmpeg, &clip_file);
        let reference_sound = sound_bytes(&ffmpeg, &reference);
        // 70 frames at 3 fps of 22,050 Hz mono, 2 bytes a sample.
        assert_eq!(sound.len(), 70 * 22050 / 3 * 2, "{name}");
        assert_eq!(
            &sound[..reference_sound.len()],
            &reference_sound[..],
            "{name}"
        );
        assert!(
            !export.join(SEGMENTS_DIR).exists(),
            "{name}: segments are removed after the join"
        );
        // MOV keeps the tag inside the file; AVI keeps none (#1442).
        if clip_file.extension().is_some_and(|e| e == "mov") {
            assert_eq!(
                crate::hlcr::clip_id::mov_tags::read_mov_tag(
                    &clip_file,
                    crate::hlcr::clip_id::METADATA_KEY
                )
                .as_deref(),
                Some("v1/s/take0000/1/all"),
                "{name}"
            );
        }
    }
}

/// A render stopped after its first segment resumes from it: the segment on
/// disk is joined as it is, not rendered again. It is made from another
/// picture here, so the clip shows which one was used.
#[tokio::test]
#[ignore = "needs FFmpeg: set DOD_FFMPEG to ffmpeg.exe's path"]
async fn a_stopped_render_resumes_from_its_finished_segments() {
    let ffmpeg = real_ffmpeg();
    let root = Scratch::new("segmented_resume");
    let take = make_take(&ffmpeg, &root, false, false);
    let export = root.join("export");
    std::fs::create_dir_all(&export).unwrap();
    let clip = clip_for(&take, None, false);
    let config = config_for(&[&export], RenderCodec::ProRes422, &ffmpeg);
    let reference = root.join("one_piece.mov");
    one_piece(&ffmpeg, &clip, &config, &reference);

    // What the stopped render left: segment 0 finished, segment 1 half
    // written under its partial name.
    let job = job_for(clip.clone(), config.clone());
    let (plan, folder) = claim(&job, &export);
    let folder = folder.unwrap();
    let encode = encode_settings(false, &config).unwrap();
    let other = make_take(&ffmpeg, &root.join("other"), false, false);
    let mut other_clip = clip_for(&other, None, false);
    other_clip.img_folder = "all".to_string();
    // A different picture: testsrc instead of testsrc2.
    ffmpeg_ok(
        &ffmpeg,
        &other,
        &strings(&[
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=64x48:rate=3",
            "-frames:v",
            "70",
            "-start_number",
            "0",
            "all/%05d.bmp",
        ]),
    );
    let segment_zero = folder.segment_file(0);
    let args = SegmentArgs {
        clip: &other_clip,
        plan: &plan,
        segment: plan.segments()[0],
        encode: &encode,
        threads: 2,
        out_file: &segment_zero,
    }
    .to_args();
    let args: Vec<String> = args
        .into_iter()
        .filter(|a| a != "-progress" && a != "pipe:1")
        .collect();
    ffmpeg_ok(&ffmpeg, &other, &args);
    std::fs::write(folder.dir.join("0001.partial.seg"), b"cut off").unwrap();
    drop(folder);

    let clip_file = finished_clip(&render(clip, config.clone()).await);
    let joined = frame_hashes(&ffmpeg, &clip_file);
    let fake = frame_hashes(&ffmpeg, &segment_zero_copy(&ffmpeg, &other, &plan, &encode));
    let expected: Vec<String> = fake
        .into_iter()
        .chain(frame_hashes(&ffmpeg, &reference).into_iter().skip(30))
        .collect();
    assert_eq!(joined, expected);
    assert!(!export.join(SEGMENTS_DIR).exists());
}

/// Segment 0 of `take` rendered again into the take's own folder, for its
/// frame hashes.
fn segment_zero_copy(
    ffmpeg: &str,
    take: &Path,
    plan: &SegmentPlan,
    encode: &super::super::ffmpeg_args::Encode,
) -> PathBuf {
    let out = take.join("segment_zero.mov");
    let clip = clip_for(take, None, false);
    let args: Vec<String> = SegmentArgs {
        clip: &clip,
        plan,
        segment: plan.segments()[0],
        encode,
        threads: 2,
        out_file: &out,
    }
    .to_args()
    .into_iter()
    .filter(|a| a != "-progress" && a != "pipe:1")
    .collect();
    ffmpeg_ok(ffmpeg, take, &args);
    out
}
