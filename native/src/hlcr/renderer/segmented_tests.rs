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
        resumable_renders: true,
        live_thread_share: Default::default(),
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

/// What a render of `job` would decide before reserving room.
fn prepared(job: &RenderJob) -> Prepared {
    prepare(job, FPS).expect("a segmented take")
}

/// The plan and claimed folder a render of `job` into `output_folder` would
/// use.
async fn claim(job: &RenderJob, output_folder: &Path) -> (SegmentPlan, Option<SegmentFolder>) {
    let prepared = prepared(job);
    let folder = claim_folder(job, &prepared, output_folder).await;
    (prepared.plan, folder)
}

/// A segment folder's name.
fn name_of(folder: &SegmentFolder) -> String {
    folder
        .dir
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

#[tokio::test]
async fn a_take_of_three_segments_is_planned_in_three() {
    let root = Scratch::new("segmented_plan");
    let take = bare_take(&root);
    let job = job_for(
        clip_for(&take, None, false),
        config_for(&[&root], RenderCodec::ProRes422, "ffmpeg"),
    );
    let (plan, _) = claim(&job, &root).await;
    let spans: Vec<usize> = plan.segments().iter().map(|s| s.frames).collect();
    assert_eq!(spans, [30, 30, 10]);
}

#[tokio::test]
async fn claiming_throws_away_this_takes_other_settings_and_keeps_other_takes() {
    let root = Scratch::new("segmented_stale");
    let take = bare_take(&root);
    let export = root.join("export");
    let job = job_for(
        clip_for(&take, None, false),
        config_for(&[&export], RenderCodec::ProRes422, "ffmpeg"),
    );
    let (_, folder) = claim(&job, &export).await;
    let folder = folder.unwrap();
    let name = name_of(&folder);
    let take_part = name.split('_').next().unwrap().to_string();
    drop(folder);

    let segments_root = export.join(SEGMENTS_DIR);
    let stale = segments_root.join(format!("{take_part}_0000000000000000"));
    let other_take = segments_root.join("ffffffffffffffff_0000000000000000");
    for dir in [&stale, &other_take] {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("0000.seg"), b"old").unwrap();
    }
    let (_, folder) = claim(&job, &export).await;
    assert_eq!(folder.unwrap().dir, segments_root.join(&name));
    assert!(
        !stale.exists(),
        "segments of other settings are thrown away"
    );
    assert!(other_take.join("0000.seg").exists(), "another take's stay");
}

#[tokio::test]
async fn a_second_render_of_the_same_segments_renders_in_one_piece() {
    let root = Scratch::new("segmented_busy");
    let take = bare_take(&root);
    let job = job_for(
        clip_for(&take, None, false),
        config_for(&[&root], RenderCodec::ProRes422, "ffmpeg"),
    );
    let first = prepare(&job, FPS);
    assert!(first.is_some());
    assert!(prepare(&job, FPS).is_none());
    drop(first);
    assert!(
        prepare(&job, FPS).is_some(),
        "free again once the first render ends"
    );
}

/// The claim is taken before the render reserves its room (#1451 review):
/// another render of the take in one piece, started meanwhile, leaves the
/// resume's segments alone.
#[tokio::test]
async fn a_resumes_segments_are_safe_from_a_one_piece_sweep_while_it_reserves() {
    let root = Scratch::new("segmented_claimed_early");
    let take = bare_take(&root);
    let job = job_for(
        clip_for(&take, None, false),
        config_for(&[&root], RenderCodec::ProRes422, "ffmpeg"),
    );
    let (_, folder) = claim(&job, &root).await;
    let dir = folder.unwrap().dir.clone();
    std::fs::write(dir.join(segment_name(0)), b"a").unwrap();
    let resume = prepared(&job);
    remove_take_segments(&job).await;
    assert!(dir.join(segment_name(0)).exists());
    drop(resume);
    remove_take_segments(&job).await;
    assert!(!dir.exists(), "swept once nothing holds it");
}

#[tokio::test]
async fn a_claimed_folder_names_its_take() {
    let root = Scratch::new("segmented_take_file");
    let take = bare_take(&root);
    let job = job_for(
        clip_for(&take, None, false),
        config_for(&[&root], RenderCodec::ProRes422, "ffmpeg"),
    );
    let (_, folder) = claim(&job, &root).await;
    let written = std::fs::read_to_string(folder.unwrap().dir.join(TAKE_FILE)).unwrap();
    assert_eq!(Path::new(&written), take);
}

/// A stopped render's segments are found in whichever export folder holds
/// them, with the frames they cover, and the render asks for room there
/// first: the clip and the one segment still to render, sized from what the
/// finished ones weigh (2 bytes for 60 frames: a clip of 4 bytes with the
/// 1.5 margin, and 1 more for the last 10 frames), not from the estimate.
#[tokio::test]
async fn a_stopped_render_is_found_and_asks_for_room_on_its_own_drive_first() {
    let root = Scratch::new("segmented_stopped");
    let take = bare_take(&root);
    let (first, second) = (root.join("e1"), root.join("e2"));
    let job = job_for(
        clip_for(&take, None, false),
        config_for(&[&first, &second], RenderCodec::ProRes422, "ffmpeg"),
    );
    assert!(prepared(&job).stopped.is_none());
    let (_, folder) = claim(&job, &second).await;
    let folder = folder.unwrap();
    std::fs::write(folder.segment_file(0), b"a").unwrap();
    std::fs::write(folder.segment_file(1), b"b").unwrap();
    std::fs::write(folder.dir.join("0002.partial.seg"), b"c").unwrap();
    drop(folder);

    let resume = prepared(&job);
    assert_eq!(
        resume.stopped,
        Some(Stopped {
            export_dir: second.clone(),
            frames_on_disk: 60,
            bytes_on_disk: 2,
        })
    );
    let asks = resume.room_asks(&job.config.export_directories, 700);
    let shown: Vec<(Vec<PathBuf>, u64)> = asks.into_iter().map(|a| (a.dirs, a.amount)).collect();
    assert_eq!(
        shown,
        [
            (vec![second.clone()], 5),
            (vec![first.clone(), second.clone()], 1400),
        ]
    );
}

/// The room for the remaining segments was reserved on the drive the clip
/// goes to, so a stopped render's segments in another export folder are
/// thrown away and the render starts afresh in its own (#1451 review).
#[tokio::test]
async fn claiming_in_another_export_folder_throws_the_old_segments_away() {
    let root = Scratch::new("segmented_other_folder");
    let take = bare_take(&root);
    let (first, second) = (root.join("e1"), root.join("e2"));
    let job = job_for(
        clip_for(&take, None, false),
        config_for(&[&first, &second], RenderCodec::ProRes422, "ffmpeg"),
    );
    let (_, folder) = claim(&job, &second).await;
    let in_second = folder.as_ref().unwrap().dir.clone();
    std::fs::write(folder.unwrap().segment_file(0), b"a").unwrap();
    let (_, folder) = claim(&job, &first).await;
    let folder = folder.unwrap();
    assert!(folder.dir.starts_with(&first));
    assert!(!in_second.exists());
    assert!(
        !second.join(SEGMENTS_DIR).exists(),
        "its empty root goes too"
    );
}

#[test]
fn removing_a_folder_counts_its_finished_segments_and_drops_an_empty_root() {
    let root = Scratch::new("segmented_remove");
    let dir = root.join(SEGMENTS_DIR).join("take_settings");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(segment_name(0)), b"a").unwrap();
    std::fs::write(dir.join(segment_name(1)), b"b").unwrap();
    std::fs::write(dir.join("0002.partial.seg"), b"c").unwrap();
    std::fs::write(dir.join(TAKE_FILE), b"D:/t").unwrap();
    assert_eq!(remove_segment_dir(&dir), 2);
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

/// Folders under `root`, each with one segment.
fn segment_folders(root: &Path, names: &[&str]) {
    for name in names {
        std::fs::create_dir_all(root.join(name)).unwrap();
        std::fs::write(root.join(name).join("0000.seg"), b"x").unwrap();
    }
}

#[test]
fn only_the_picked_folders_are_removed() {
    let dir = Scratch::new("segments_stale");
    let root = dir.join(SEGMENTS_DIR);
    segment_folders(&root, &["take_old", "take_new", "other_x"]);
    remove_unclaimed(std::slice::from_ref(&root), |name, _| {
        (name == "take_old").then_some("test")
    });
    assert!(!root.join("take_old").exists());
    assert!(root.join("take_new").join("0000.seg").exists());
    assert!(root.join("other_x").exists());
}

#[test]
fn a_folder_a_render_is_using_is_never_swept() {
    let dir = Scratch::new("segments_in_use");
    let root = dir.join(SEGMENTS_DIR);
    let name = "segments_in_use_test_folder";
    segment_folders(&root, &[name]);
    lock_in_use().insert(name.to_string());
    remove_unclaimed(std::slice::from_ref(&root), |_, _| Some("test"));
    lock_in_use().remove(name);
    assert!(root.join(name).join("0000.seg").exists());
}

/// A take on a drive that isn't there now (unplugged, offline) may still
/// exist: its segments stay.
#[test]
fn a_take_on_a_missing_drive_is_not_taken_for_gone() {
    let Some(letter) = ('D'..='Z')
        .rev()
        .find(|l| !Path::new(&format!("{l}:\\")).is_dir())
    else {
        return;
    };
    let root = Scratch::new("segmented_missing_drive");
    let dir = root.join("folder");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join(TAKE_FILE),
        format!("{letter}:\\takes\\demo\\take0000"),
    )
    .unwrap();
    assert!(!take_is_gone(&dir));
    std::fs::write(
        dir.join(TAKE_FILE),
        root.join("nope").to_string_lossy().as_bytes(),
    )
    .unwrap();
    assert!(take_is_gone(&dir));
}

#[cfg(windows)]
#[test]
fn a_paths_root_is_its_drive_or_share() {
    assert_eq!(root_of(Path::new("D:\\a\\b")), Some(PathBuf::from("D:\\")));
    assert_eq!(
        root_of(Path::new("\\\\host\\share\\a")),
        Some(PathBuf::from("\\\\host\\share\\"))
    );
    assert_eq!(root_of(Path::new("a\\b")), None);
}

/// FFmpeg 6.1 and 7.0 print no `frame=` for a stream-copy join (#1451
/// review): the container's own count decides, and a file neither can count
/// is kept, not failed.
#[test]
fn a_join_is_checked_by_its_containers_count_without_a_frame_line() {
    assert_eq!(check_frames(Some(1000), None, 1000), FrameCheck::Whole);
    assert_eq!(check_frames(Some(300), None, 1000), FrameCheck::Short(300));
    assert_eq!(check_frames(None, Some(300), 1000), FrameCheck::Short(300));
    // The container wins over a progress line that lost the last frames.
    assert_eq!(check_frames(Some(1000), Some(990), 1000), FrameCheck::Whole);
    assert_eq!(check_frames(None, None, 1000), FrameCheck::Unknown);
}

/// A segment folder path of at most 242 characters (`MAX_SEGMENT_DIR_UNITS`)
/// keeps `\\NNNN.partial.seg` within 259, for FFmpeg builds before 5.1, and
/// the join's working folder within Windows' 258 (#1451 review: an export
/// path over about 204 characters failed every join with os error 267). The
/// segments root and folder add 54 characters, so an export folder of up to
/// 188 holds one.
#[test]
fn an_export_folder_of_up_to_188_characters_holds_a_segment_folder() {
    let root = Scratch::new("segmented_long_path");
    let take = bare_take(&root);
    let job = job_for(
        clip_for(&take, None, false),
        config_for(&[&root], RenderCodec::ProRes422, "ffmpeg"),
    );
    let key = prepared(&job).key;
    let added = PathBuf::from("x")
        .join(SEGMENTS_DIR)
        .join(key.folder_name())
        .as_os_str()
        .len()
        - 1;
    assert_eq!(added, 54);
    assert_eq!(MAX_SEGMENT_DIR_UNITS + 1 + "0000.partial.seg".len(), 259);
    let export_of = |len: usize| PathBuf::from("D:".to_string() + &"a".repeat(len - 2));
    assert!(segments_fit_under(&export_of(
        MAX_SEGMENT_DIR_UNITS - added
    )));
    assert!(segments_fit_under(&export_of(188)));
    assert!(!segments_fit_under(&export_of(189)));
}

/// Only export folders that can hold a segment folder are asked for
/// segment room (#1451 review): a short one is preferred over a long one
/// first in the list, and with none, nothing is asked (one piece).
#[test]
fn segment_room_is_asked_only_of_folders_short_enough() {
    let root = Scratch::new("segmented_long_asks");
    let take = bare_take(&root);
    let short = root.join("e");
    let long = PathBuf::from(format!("D:/{}", "a".repeat(200)));
    let job = job_for(
        clip_for(&take, None, false),
        config_for(&[&long, &short], RenderCodec::ProRes422, "ffmpeg"),
    );
    let asks = prepared(&job).room_asks(&job.config.export_directories, 700);
    let shown: Vec<(Vec<PathBuf>, u64)> = asks.into_iter().map(|a| (a.dirs, a.amount)).collect();
    assert_eq!(shown, [(vec![short], 1400)]);
    assert!(
        prepared(&job)
            .room_asks(std::slice::from_ref(&long), 700)
            .is_empty()
    );
}

/// "Resumable renders" off (#1451): no segments.
#[tokio::test]
async fn with_resumable_renders_off_a_take_is_not_segmented() {
    let root = Scratch::new("segmented_off");
    let take = bare_take(&root);
    let mut config = config_for(&[&root], RenderCodec::ProRes422, "ffmpeg");
    config.resumable_renders = false;
    let off = job_for(clip_for(&take, None, false), config);
    assert!(prepare(&off, FPS).is_none());
}

/// What `reserve_room` reserved for `job`, and whether it is segmented.
async fn reserved_for(job: &RenderJob) -> (u64, bool) {
    let ledger = Arc::new(Mutex::new(HashMap::new()));
    let (_, _guard, segments) = super::super::reserve_room(job, &ledger, FPS)
        .await
        .expect("room on the test drive");
    let reserved = ledger.lock().unwrap().values().sum();
    (reserved, segments.is_some())
}

/// Off, a long take reserves the clip alone and renders in one piece, and
/// its stopped segments are removed, so turning the setting off to save
/// space gets the space back; on, it reserves for the segments too.
#[tokio::test]
async fn with_resumable_renders_off_room_is_reserved_for_the_clip_alone() {
    let root = Scratch::new("segmented_off_room");
    let take = bare_take(&root);
    let export = root.join("export");
    std::fs::create_dir_all(&export).unwrap();
    let mut config = config_for(&[&export], RenderCodec::ProRes422, "ffmpeg");
    let clip = clip_for(&take, None, false);
    let one_piece =
        crate::hlcr::renderer::job_reservation_estimate(&clip, RenderCodec::ProRes422, "");
    let on = job_for(clip.clone(), config.clone());
    assert_eq!(reserved_for(&on).await, (2 * one_piece, true));

    let (_, folder) = claim(&on, &export).await;
    let dir = folder.unwrap().dir.clone();
    std::fs::write(dir.join(segment_name(0)), b"a").unwrap();
    config.resumable_renders = false;
    let off = job_for(clip, config);
    assert_eq!(reserved_for(&off).await, (one_piece, false));
    assert!(!dir.exists(), "the stopped segments are removed");
}

/// Off, a deleted take's segments are removed too (#1451 review).
#[tokio::test]
async fn with_resumable_renders_off_a_gone_takes_segments_are_removed() {
    let root = Scratch::new("segmented_off_orphans");
    let take = bare_take(&root);
    let export = root.join("export");
    let mut config = config_for(&[&export], RenderCodec::ProRes422, "ffmpeg");
    config.resumable_renders = false;
    let job = job_for(clip_for(&take, None, false), config);
    let segments_root = export.join(SEGMENTS_DIR);
    segment_folders(&segments_root, &["gone_while_off"]);
    std::fs::write(
        segments_root.join("gone_while_off").join(TAKE_FILE),
        root.join("demo")
            .join("take0099")
            .to_string_lossy()
            .as_bytes(),
    )
    .unwrap();
    remove_orphans(&job).await;
    assert!(!segments_root.join("gone_while_off").exists());
}

/// A take deleted with its segments still in an export folder (#1451
/// review): the next render removes them; another take's stay, and so do
/// segments whose folder names no take.
#[tokio::test]
async fn segments_of_a_take_that_is_gone_are_removed_by_the_next_render() {
    let root = Scratch::new("segmented_orphans");
    let take = bare_take(&root);
    let export = root.join("export");
    let job = job_for(
        clip_for(&take, None, false),
        config_for(&[&export], RenderCodec::ProRes422, "ffmpeg"),
    );
    let segments_root = export.join(SEGMENTS_DIR);
    segment_folders(&segments_root, &["gone_a", "here_b", "unnamed_c"]);
    let gone = root.join("demo").join("take0099");
    std::fs::write(
        segments_root.join("gone_a").join(TAKE_FILE),
        gone.to_string_lossy().as_bytes(),
    )
    .unwrap();
    std::fs::write(
        segments_root.join("here_b").join(TAKE_FILE),
        take.to_string_lossy().as_bytes(),
    )
    .unwrap();
    remove_orphans(&job).await;
    assert!(!segments_root.join("gone_a").exists());
    assert!(segments_root.join("here_b").exists());
    assert!(segments_root.join("unnamed_c").exists());
}

/// A take rendered in one piece (now short of room, or under a codec that
/// isn't segmented) leaves no segments of a stopped render behind (#1451
/// review); another take's stay.
#[tokio::test]
async fn rendering_in_one_piece_removes_this_takes_segments() {
    let root = Scratch::new("segmented_one_piece");
    let take = bare_take(&root);
    let export = root.join("export");
    let job = job_for(
        clip_for(&take, None, false),
        config_for(&[&export], RenderCodec::ProRes422, "ffmpeg"),
    );
    let take_part = super::super::segment_plan::take_key(&job.clip);
    let segments_root = export.join(SEGMENTS_DIR);
    let this_take = format!("{take_part}_0000000000000000");
    let other_take = "ffffffffffffffff_0000000000000000";
    segment_folders(&segments_root, &[&this_take, other_take]);
    remove_take_segments(&job).await;
    assert!(!segments_root.join(&this_take).exists());
    assert!(segments_root.join(other_take).exists());
}

/// A take captured again in place at the same length has the same frame
/// count and wav size; its wav's modified time still tells its segments
/// from the old take's.
#[test]
fn a_take_written_again_at_the_same_length_gets_a_new_key() {
    let root = Scratch::new("segmented_rewritten");
    let take = bare_take(&root);
    let job = job_for(
        clip_for(&take, None, false),
        config_for(&[&root], RenderCodec::ProRes422, "ffmpeg"),
    );
    let before = prepared(&job).key;
    let wav = std::fs::File::options()
        .write(true)
        .open(take.join("sound.wav"))
        .unwrap();
    wav.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000))
        .unwrap();
    drop(wav);
    let after = prepared(&job).key;
    assert_eq!(after.take, before.take);
    assert_ne!(after.settings, before.settings);
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
    let (plan, folder) = claim(&job, &export).await;
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

/// A segment on disk under its finished name but damaged (#1451 review: a
/// zero-filled `0001.seg` joined into 300 of 1,000 frames and the job ended
/// Finished): the join fails, no clip is left and the segments are removed,
/// so the next render starts afresh.
#[tokio::test]
#[ignore = "needs FFmpeg: set DOD_FFMPEG to ffmpeg.exe's path"]
async fn a_damaged_segment_fails_the_join_instead_of_a_short_clip() {
    let ffmpeg = real_ffmpeg();
    let root = Scratch::new("segmented_damaged");
    let take = make_take(&ffmpeg, &root, false, false);
    let export = root.join("export");
    std::fs::create_dir_all(&export).unwrap();
    let clip = clip_for(&take, None, false);
    let config = config_for(&[&export], RenderCodec::ProRes422, &ffmpeg);

    let job = job_for(clip.clone(), config.clone());
    let (plan, folder) = claim(&job, &export).await;
    let folder = folder.unwrap();
    let encode = encode_settings(false, &config).unwrap();
    for segment in plan.segments() {
        let out = folder.segment_file(segment.index);
        if segment.index == 1 {
            std::fs::write(&out, vec![0u8; 64 * 1024]).unwrap();
            continue;
        }
        let args: Vec<String> = SegmentArgs {
            clip: &clip,
            plan: &plan,
            segment,
            encode: &encode,
            threads: 2,
            out_file: &out,
        }
        .to_args()
        .into_iter()
        .filter(|a| a != "-progress" && a != "pipe:1")
        .collect();
        ffmpeg_ok(&ffmpeg, &take, &args);
    }
    drop(folder);

    let updates = render(clip, config).await;
    let failed = updates.iter().any(|u| {
        matches!(u, RenderUpdate::Finished(_, false, Some(e)) if e.contains("segment") || e.contains("FFmpeg failed"))
    });
    assert!(failed, "{updates:?}");
    assert!(
        !updates
            .iter()
            .any(|u| matches!(u, RenderUpdate::OutputPath(..))),
        "{updates:?}"
    );
    let left: Vec<PathBuf> = std::fs::read_dir(&export)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect();
    assert!(
        left.is_empty(),
        "no clip and no segments are left: {left:?}"
    );
}

/// An export folder with a `#` in its path (#1451 review): FFmpeg reads the
/// concat list's path as a URL, so the join runs in the segment folder and
/// names everything by bare name.
#[tokio::test]
#[ignore = "needs FFmpeg: set DOD_FFMPEG to ffmpeg.exe's path"]
async fn an_export_folder_with_a_hash_in_its_name_joins() {
    let ffmpeg = real_ffmpeg();
    let root = Scratch::new("segmented_hash");
    let take = make_take(&ffmpeg, &root, false, false);
    let export = root.join("ex #1");
    std::fs::create_dir_all(&export).unwrap();
    let clip = clip_for(&take, None, false);
    let config = config_for(&[&export], RenderCodec::ProRes422, &ffmpeg);
    let updates = render(clip, config).await;
    assert!(
        updates
            .iter()
            .any(|u| matches!(u, RenderUpdate::Speed(_, s) if s.starts_with("Joining"))),
        "it was segmented: {updates:?}"
    );
    let clip_file = finished_clip(&updates);
    assert!(clip_file.starts_with(&export));
    assert_eq!(frame_hashes(&ffmpeg, &clip_file).len(), FRAMES);
    assert!(!export.join(SEGMENTS_DIR).exists());
}

/// After the join the row's Speed cell shows the whole render's speed, as a
/// one-piece row keeps FFmpeg's last one, instead of going blank (#1512).
#[tokio::test]
#[ignore = "needs FFmpeg: set DOD_FFMPEG to ffmpeg.exe's path"]
async fn a_segmented_renders_speed_cell_ends_with_its_whole_speed() {
    let ffmpeg = real_ffmpeg();
    let root = Scratch::new("segmented_speed");
    let take = make_take(&ffmpeg, &root, false, false);
    let export = root.join("export");
    std::fs::create_dir_all(&export).unwrap();
    let clip = clip_for(&take, None, false);
    let config = config_for(&[&export], RenderCodec::ProRes422, &ffmpeg);
    let updates = render(clip, config).await;
    finished_clip(&updates);
    let speeds: Vec<&String> = updates
        .iter()
        .filter_map(|u| match u {
            RenderUpdate::Speed(_, s) => Some(s),
            _ => None,
        })
        .collect();
    let joining = speeds
        .iter()
        .position(|s| s.starts_with("Joining"))
        .expect("it was segmented");
    let last = speeds.last().unwrap();
    assert_eq!(
        speeds.len(),
        joining + 2,
        "one speed after the join: {speeds:?}"
    );
    assert!(
        last.ends_with("x)") && last.contains(" fps ("),
        "the whole render's speed: {last:?}"
    );
}

#[test]
fn the_whole_render_speed_is_frames_over_time_against_the_takes_rate() {
    let rate = FrameRate { num: 300, den: 1 };
    let took = std::time::Duration::from_secs(100);
    // 9,000 frames (30 s of take) in 100 s: 90 fps, 0.3x real time.
    assert_eq!(whole_render_speed(9000, took, rate), "90.00 fps (0.3x)");
    assert_eq!(
        whole_render_speed(10_500, std::time::Duration::from_secs(219), rate),
        "47.95 fps (0.16x)"
    );
    let ntsc = FrameRate {
        num: 30000,
        den: 1001,
    };
    assert_eq!(
        whole_render_speed(2997, std::time::Duration::from_secs(10), ntsc),
        "299.70 fps (10x)"
    );
}

#[test]
fn a_render_that_rendered_nothing_itself_shows_no_speed() {
    let rate = FrameRate { num: 300, den: 1 };
    assert_eq!(
        whole_render_speed(0, std::time::Duration::from_secs(5), rate),
        ""
    );
    assert_eq!(whole_render_speed(300, std::time::Duration::ZERO, rate), "");
}

/// A take whose last BMP can't be read (#1451 review): one piece renders
/// the frames there are, so the segmented render, whose last segment comes
/// out short, falls back to that instead of failing.
#[tokio::test]
#[ignore = "needs FFmpeg: set DOD_FFMPEG to ffmpeg.exe's path"]
async fn a_take_whose_last_frame_is_damaged_renders_in_one_piece() {
    let ffmpeg = real_ffmpeg();
    let root = Scratch::new("segmented_short_take");
    let take = make_take(&ffmpeg, &root, false, false);
    let last = take.join("all").join(format!("{:05}.bmp", FRAMES - 1));
    std::fs::write(&last, b"BM broken").unwrap();
    let export = root.join("export");
    std::fs::create_dir_all(&export).unwrap();
    let clip = clip_for(&take, None, false);
    let config = config_for(&[&export], RenderCodec::ProRes422, &ffmpeg);
    // The bar starts again with the one-piece render, so `finished_clip`'s
    // check that it only rises doesn't hold here.
    let updates = render(clip, config).await;
    assert!(
        updates
            .iter()
            .any(|u| matches!(u, RenderUpdate::Finished(_, true, _))),
        "{updates:?}"
    );
    let clip_file = updates
        .iter()
        .find_map(|u| match u {
            RenderUpdate::OutputPath(_, path) => Some(PathBuf::from(path)),
            _ => None,
        })
        .expect("no output path");
    assert_eq!(frame_hashes(&ffmpeg, &clip_file).len(), FRAMES - 1);
    assert!(!export.join(SEGMENTS_DIR).exists());
}
