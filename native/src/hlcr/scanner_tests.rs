use super::*;
use crate::test_support::Scratch;

fn scratch_dir(name: &str) -> Scratch {
    Scratch::new(format_args!("scanner_test_{name}"))
}

fn write_frames(take: &Path, stream: &str) {
    let folder = take.join(stream);
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("00000.bmp"), b"bmp").unwrap();
}

#[test]
fn test_renderable_take_needs_both_wav_and_frames() {
    let take = scratch_dir("complete");
    std::fs::write(take.join("sound.wav"), b"wav").unwrap();
    write_frames(&take, "all");
    assert!(is_renderable_take(&take));
}

#[test]
fn test_take_without_wav_is_not_renderable() {
    // The realistic partial-capture case: frames landed, audio never flushed.
    let take = scratch_dir("no_wav");
    write_frames(&take, "all");
    assert!(!is_renderable_take(&take));
}

#[test]
fn test_take_without_frames_is_not_renderable() {
    let take = scratch_dir("no_frames");
    std::fs::write(take.join("sound.wav"), b"wav").unwrap();
    assert!(!is_renderable_take(&take));
}

#[test]
fn test_subfolder_without_first_frame_does_not_count() {
    // A frame folder is identified by 00000.bmp specifically — an empty or
    // partially-written stream folder must not qualify.
    let take = scratch_dir("empty_stream");
    std::fs::write(take.join("sound.wav"), b"wav").unwrap();
    std::fs::create_dir_all(take.join("all")).unwrap();
    assert!(!is_renderable_take(&take));
}

#[test]
fn test_missing_take_folder_is_not_renderable() {
    let missing = Scratch::absent("scanner_test_does_not_exist");
    assert!(!is_renderable_take(&missing));
}

/// A minimal but structurally real AVI header: RIFF/hdrl/avih plus an
/// optional strl/strh video stream header. Only the two frame-count fields
/// carry meaningful values; the rest is zero padding of the right width.
fn build_avi(avih_total_frames: u32, vids_stream_length: Option<u32>) -> Vec<u8> {
    fn chunk(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(id);
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(body);
        if body.len() % 2 == 1 {
            out.push(0); // chunks are word-aligned
        }
        out
    }

    let mut avih = vec![0u8; 56];
    avih[16..20].copy_from_slice(&avih_total_frames.to_le_bytes());

    let mut hdrl = b"hdrl".to_vec();
    hdrl.extend_from_slice(&chunk(b"avih", &avih));

    if let Some(len) = vids_stream_length {
        let mut strh = vec![0u8; 56];
        strh[0..4].copy_from_slice(b"vids");
        strh[32..36].copy_from_slice(&len.to_le_bytes());
        let mut strl = b"strl".to_vec();
        strl.extend_from_slice(&chunk(b"strh", &strh));
        hdrl.extend_from_slice(&chunk(b"LIST", &strl));
    }

    let mut body = b"AVI ".to_vec();
    body.extend_from_slice(&chunk(b"LIST", &hdrl));
    chunk(b"RIFF", &body)
}

fn write_video(take: &Path, stream: &str, avi: &[u8]) {
    let folder = take.join(stream);
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join(VIDEO_FILE), avi).unwrap();
}

fn scratch(name: &str) -> Scratch {
    Scratch::new(format_args!("scan_{name}"))
}

/// A stream folder holding a video with the given bytes, named `video.<ext>`.
fn write_named_video(take: &Path, stream: &str, ext: &str, body: &[u8]) -> PathBuf {
    let folder = take.join(stream);
    std::fs::create_dir_all(&folder).unwrap();
    let p = folder.join(format!("video.{ext}"));
    std::fs::write(&p, body).unwrap();
    p
}

/// Enough of an MP4 to carry the audio-handler signature the scan looks
/// for: an `hdlr` box declaring `soun`, padded the way a real one is.
fn mp4_with_audio() -> Vec<u8> {
    let mut v = b"\x00\x00\x00\x20ftypisom".to_vec();
    v.extend_from_slice(b"hdlr");
    v.extend_from_slice(&[0u8; 8]); // version/flags + pre_defined
    v.extend_from_slice(b"soun");
    v.extend_from_slice(&[0u8; 64]);
    v
}

fn mp4_without_audio() -> Vec<u8> {
    let mut v = b"\x00\x00\x00\x20ftypisom".to_vec();
    v.extend_from_slice(b"hdlr");
    v.extend_from_slice(&[0u8; 8]);
    v.extend_from_slice(b"vide");
    v.extend_from_slice(&[0u8; 64]);
    v
}

/// The OBS shape: one stream folder, a video with its audio already in it,
/// and no `sound.wav` anywhere. This is the case the predicate was widened
/// for, and both the capture side and Render Studio go through it.
#[test]
fn a_video_take_with_audio_is_renderable_without_a_wav() {
    let root = scratch("obs_take");
    let take = root.join("dodstudio_chain_01_b0").join("take0000");
    write_named_video(&take, "all", "mp4", &mp4_with_audio());
    assert!(
        collect_wav_files(&take).is_empty(),
        "no wav, by construction"
    );
    assert!(is_renderable_take(&take));
    // And through the take* nesting, which is how the capture side asks.
    assert!(is_renderable_take(&root.join("dodstudio_chain_01_b0")));
    let _ = std::fs::remove_dir_all(&root);
}

/// A muted OBS source produces a video that is valid in every respect and
/// silent. Admitting it would render a clip with no sound and no error.
#[test]
fn a_silent_video_take_is_not_renderable() {
    let root = scratch("silent");
    let take = root.join("dodstudio_chain_01_b0").join("take0000");
    write_named_video(&take, "all", "mp4", &mp4_without_audio());
    assert!(
        !is_renderable_take(&take),
        "a video with no audio track and no wav must not pass"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// OBS writes whatever container its settings name, and the file keeps the
/// extension it was written with rather than being renamed to `.avi`.
#[test]
fn video_takes_are_found_in_any_supported_container() {
    for ext in ["mp4", "mkv", "mov", "avi"] {
        let root = scratch(&format!("ext_{ext}"));
        let take = root.join("b0").join("take0000");
        write_named_video(&take, "all", ext, &mp4_with_audio());
        assert!(
            !collect_image_folders(&take).is_empty(),
            "a .{ext} video should mark its folder as a stream"
        );
        assert!(
            is_renderable_take(&take),
            ".{ext} take should be renderable"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}

/// The wav path must keep working exactly as before — a BMP sequence beside
/// a `sound.wav` is still the common case and is not audited for audio.
#[test]
fn the_wav_shape_still_passes_untouched() {
    let root = scratch("wav");
    let take = root.join("b0").join("take0000");
    let stream = take.join("all");
    std::fs::create_dir_all(&stream).unwrap();
    std::fs::write(stream.join("00000.bmp"), b"x").unwrap();
    std::fs::write(take.join("sound.wav"), b"x").unwrap();
    assert!(is_renderable_take(&take));
    let _ = std::fs::remove_dir_all(&root);
}

/// An empty folder, or one holding only our own metadata, is not a take.
#[test]
fn an_empty_folder_is_not_renderable() {
    let root = scratch("empty");
    assert!(!is_renderable_take(&root));
    let _ = std::fs::remove_dir_all(&root);
}

/// The audio signature must not be matched by an unrelated occurrence of
/// the tag somewhere in the payload — hence the bounded window.
#[test]
fn a_stray_soun_in_the_payload_is_not_an_audio_track() {
    let root = scratch("stray");
    let take = root.join("b0").join("take0000");
    let mut body = b"\x00\x00\x00\x20ftypisom".to_vec();
    body.extend_from_slice(b"hdlr");
    body.extend_from_slice(&[0u8; 8]);
    body.extend_from_slice(b"vide");
    body.extend_from_slice(&[0u8; 256]); // well past the window
    body.extend_from_slice(b"soun");
    let p = write_named_video(&take, "all", "mp4", &body);
    assert!(
        !video_has_audio(&p),
        "`soun` far from any hdlr is not an audio handler"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn test_stream_header_length_wins_over_avih_total_frames() {
    // Measured on a real capture: FFmpeg's AVI muxer wrote 1067 into
    // avih.dwTotalFrames while the video strh.dwLength read 1218, and 1218
    // is what ffprobe and the file's own 10.15s-at-120fps duration agree
    // on. Preferring avih would silently under-report render progress.
    let avi = build_avi(1067, Some(1218));
    let dir = scratch_dir("avi_disagreement");
    let path = dir.join(VIDEO_FILE);
    std::fs::write(&path, &avi).unwrap();
    assert_eq!(avi_frame_count(&path), Some(1218));
}

/// `build_avi`'s file with the video `strh`'s scale, rate and start set.
fn avi_with_rate(scale: u32, rate: u32, start: u32) -> Vec<u8> {
    let mut avi = build_avi(10, Some(10));
    let strh = avi.windows(4).position(|w| w == b"vids").unwrap();
    avi[strh + 20..strh + 24].copy_from_slice(&scale.to_le_bytes());
    avi[strh + 24..strh + 28].copy_from_slice(&rate.to_le_bytes());
    avi[strh + 28..strh + 32].copy_from_slice(&start.to_le_bytes());
    avi
}

#[test]
fn an_avi_frame_rate_is_read_from_its_video_stream_header() {
    let dir = scratch_dir("avi_rate");
    let path = dir.join(VIDEO_FILE);
    std::fs::write(&path, avi_with_rate(1, 300, 0)).unwrap();
    assert_eq!(avi_frame_rate(&path), Some((300, 1)));
    std::fs::write(&path, avi_with_rate(1001, 60000, 0)).unwrap();
    assert_eq!(avi_frame_rate(&path), Some((60000, 1001)));
}

#[test]
fn an_avi_with_no_usable_rate_has_none() {
    let dir = scratch_dir("avi_no_rate");
    let path = dir.join(VIDEO_FILE);
    std::fs::write(&path, avi_with_rate(0, 300, 0)).unwrap();
    assert_eq!(avi_frame_rate(&path), None);
    // A stream that starts later than 0 can't be seeked by frame number.
    std::fs::write(&path, avi_with_rate(1, 300, 5)).unwrap();
    assert_eq!(avi_frame_rate(&path), None);
    std::fs::write(&path, build_avi(10, None)).unwrap();
    assert_eq!(avi_frame_rate(&path), None);
}

#[test]
fn test_avih_used_when_there_is_no_video_stream_header() {
    let avi = build_avi(900, None);
    let dir = scratch_dir("avi_no_strh");
    let path = dir.join(VIDEO_FILE);
    std::fs::write(&path, &avi).unwrap();
    assert_eq!(avi_frame_count(&path), Some(900));
}

#[test]
fn test_unfinalised_avi_reports_zero_rather_than_guessing() {
    // A capture killed mid-write leaves both counts at 0. Zero is the
    // honest answer — the renderer already treats it as "no percentage".
    let avi = build_avi(0, Some(0));
    let dir = scratch_dir("avi_unfinalised");
    let path = dir.join(VIDEO_FILE);
    std::fs::write(&path, &avi).unwrap();
    assert_eq!(avi_frame_count(&path), Some(0));
}

#[test]
fn test_non_avi_file_is_not_parsed_as_one() {
    let dir = scratch_dir("avi_garbage");
    let path = dir.join(VIDEO_FILE);
    std::fs::write(&path, b"this is not a RIFF file at all").unwrap();
    assert_eq!(avi_frame_count(&path), None);
}

#[test]
fn test_video_take_reports_frames_from_the_container() {
    // The end-to-end shape: a video take used to scan as 0 frames, which
    // left Render Studio showing no progress percentage for it.
    let take = scratch_dir("video_frame_count");
    std::fs::write(take.join("sound.wav"), b"wav").unwrap();
    write_video(&take, "all", &build_avi(0, Some(1218)));

    let clips = scan(&take);
    assert_eq!(clips.len(), 1);
    assert_eq!(clips[0].frame_count, 1218);
    assert_eq!(clips[0].video_file.as_deref(), Some(VIDEO_FILE));
}

#[test]
fn test_bmp_take_still_counts_bitmaps() {
    let take = scratch_dir("bmp_frame_count");
    std::fs::write(take.join("sound.wav"), b"wav").unwrap();
    let folder = take.join("all");
    std::fs::create_dir_all(&folder).unwrap();
    for i in 0..3 {
        std::fs::write(folder.join(format!("{:05}.bmp", i)), b"bmp").unwrap();
    }

    let clips = scan(&take);
    assert_eq!(clips[0].frame_count, 3);
}

/// Collect what `scan_folder_background` emits for a scratch tree.
fn scan(root: &Path) -> Vec<ClipData> {
    let (tx, rx) = mpsc::channel();
    let (status_tx, _status_rx) = mpsc::channel();
    scan_folder_background(vec![root.to_path_buf()], tx, status_tx);
    rx.into_iter().collect()
}

#[test]
fn test_hud_pair_carries_both_folder_names_as_written() {
    // HLAE names these streams `hudColor`/`hudAlpha` in the
    // `mirv_movie_ffmpeg` command. The pair must still be recognised (the
    // lookup keys are lowercased), and both halves must be reported with
    // the spelling that is actually on disk rather than a literal.
    let take = scratch_dir("hud_pair_case");
    std::fs::write(take.join("sound.wav"), b"wav").unwrap();
    write_frames(&take, "all");
    write_frames(&take, "hudColor");
    write_frames(&take, "hudAlpha");

    let clips = scan(&take);
    let hud = clips
        .iter()
        .find(|c| c.clip_type == "hud_only")
        .expect("hud pair was not bundled");
    assert_eq!(hud.img_folder, "hudColor");
    assert_eq!(hud.alpha_folder.as_deref(), Some("hudAlpha"));

    // Bundling consumed all three folders, so nothing is emitted twice.
    assert_eq!(clips.len(), 2, "expected exactly the all + hud pair");
    let all = clips
        .iter()
        .find(|c| c.clip_type == "single")
        .expect("all stream missing");
    assert_eq!(all.img_folder, "all");
    assert_eq!(all.alpha_folder, None);
}

#[test]
fn test_video_take_hud_pair_names_the_video_in_both_halves() {
    // The FFmpeg-capture shape: one video per stream folder instead of a
    // numbered sequence. `alpha_folder` is what lets the renderer find the
    // second video without reconstructing the folder name.
    let take = scratch_dir("hud_pair_video");
    std::fs::write(take.join("sound.wav"), b"wav").unwrap();
    for stream in ["all", "hudcolor", "hudalpha"] {
        let folder = take.join(stream);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join(VIDEO_FILE), b"avi").unwrap();
    }

    let clips = scan(&take);
    let hud = clips
        .iter()
        .find(|c| c.clip_type == "hud_only")
        .expect("hud pair was not bundled");
    assert_eq!(hud.video_file.as_deref(), Some(VIDEO_FILE));
    assert_eq!(hud.alpha_folder.as_deref(), Some("hudalpha"));
}

#[test]
fn test_non_hud_clip_has_no_alpha_partner() {
    let take = scratch_dir("solo_stream");
    std::fs::write(take.join("sound.wav"), b"wav").unwrap();
    write_frames(&take, "all");

    let clips = scan(&take);
    assert_eq!(clips.len(), 1);
    assert_eq!(clips[0].alpha_folder, None);
}

#[test]
fn test_renderable_take_nested_under_hlae_take_number_folder() {
    // HLAE's mirv_movie plugin auto-numbers each recording into a
    // take0000, take0001, ... subfolder under the directory we point
    // mirv_movie_filename at, to avoid overwriting a previous take
    // written to the same path — confirmed on a real capture where the
    // block folder itself was empty except for exactly this layout.
    let block_folder = scratch_dir("nested_under_take_number");
    let take0000 = block_folder.join("take0000");
    std::fs::create_dir_all(&take0000).unwrap();
    std::fs::write(take0000.join("sound.wav"), b"wav").unwrap();
    write_frames(&take0000, "all");
    assert!(is_renderable_take(&block_folder));
}

/// The gap issue #82 exists to close: an OBS take — no `sound.wav`, its
/// audio already muxed into the video — used to be silently dropped by
/// the scanner's hard `wav_files.is_empty()` gate even though
/// `is_renderable_take` already recognised the shape.
#[test]
fn scan_finds_an_obs_take_with_no_wav() {
    let root = scratch("obs_scan");
    let take = root.join("dodstudio_chain_01_b0").join("take0000");
    write_named_video(&take, "all", "mp4", &mp4_with_audio());

    let clips = scan(&take);
    assert_eq!(
        clips.len(),
        1,
        "an OBS-shaped take must be scanned into exactly one clip"
    );
    assert_eq!(
        clips[0].wav_file, None,
        "no wav exists for an OBS take to name"
    );
    assert_eq!(clips[0].video_file.as_deref(), Some("video.mp4"));
    assert_eq!(clips[0].img_folder, "all");
    let _ = std::fs::remove_dir_all(&root);
}

/// Without a wav to derive a name from, the base name still has to be
/// distinctive per take — demo/take, same as the wav path's own fallback
/// for a literal `sound.wav`, plus a marker so it reads as an OBS clip.
#[test]
fn scan_names_an_obs_take_from_demo_and_take_not_a_wav() {
    let root = scratch("obs_naming");
    let take = root
        .join("some_demo")
        .join("dodstudio_chain_02_b1")
        .join("take0000");
    write_named_video(&take, "all", "mp4", &mp4_with_audio());

    let clips = scan(&take);
    assert_eq!(clips.len(), 1);
    assert!(
        clips[0].base_name.contains("dodstudio_chain_02_b1"),
        "{}",
        clips[0].base_name
    );
    assert!(
        clips[0].base_name.ends_with("-obs"),
        "{}",
        clips[0].base_name
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// A muted OBS source must stay invisible to the scanner too, not just to
/// `is_renderable_take` — otherwise the scanner would queue a render job
/// for a clip with no audio and no error.
#[test]
fn scan_does_not_find_a_silent_obs_take() {
    let root = scratch("obs_silent_scan");
    let take = root.join("dodstudio_chain_01_b0").join("take0000");
    write_named_video(&take, "all", "mp4", &mp4_without_audio());

    let clips = scan(&take);
    assert!(
        clips.is_empty(),
        "a silent video with no wav must not become a render job"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// The wav-and-frames shape must still scan to `Some(wav)`, unaffected by
/// `ClipData.wav_file` becoming optional.
#[test]
fn scan_still_carries_the_wav_name_for_the_ordinary_shape() {
    let take = scratch_dir("wav_shape_still_some");
    std::fs::write(take.join("sound.wav"), b"wav").unwrap();
    write_frames(&take, "all");

    let clips = scan(&take);
    assert_eq!(clips[0].wav_file.as_deref(), Some("sound.wav"));
}

/// `take_shape_is_renderable` only asks whether *some* stream in the take
/// has audio, which is enough to admit the take as a whole — it is not
/// enough to admit every stream inside it. With no take-level wav, a
/// stream whose own video is silent must be skipped individually rather
/// than scanned with `wav_file: None`, or it would render with no audio
/// and no error.
#[test]
fn a_silent_stream_beside_an_audible_one_is_not_scanned_even_though_the_take_is() {
    let root = scratch("mixed_streams");
    let take = root.join("dodstudio_chain_01_b0").join("take0000");
    write_named_video(&take, "all", "mp4", &mp4_with_audio());
    write_named_video(&take, "second", "mp4", &mp4_without_audio());

    let clips = scan(&take);
    assert_eq!(
        clips.len(),
        1,
        "only the audible stream should be scanned: {:?}",
        clips.iter().map(|c| &c.img_folder).collect::<Vec<_>>()
    );
    assert_eq!(clips[0].img_folder, "all");
    let _ = std::fs::remove_dir_all(&root);
}

/// The HUD-bundling branch must apply the same per-stream audio rule the
/// "process remaining folders" loop does — a silent `all` stream must not
/// be admitted just because bundling happened to trigger, and a HUD/alpha
/// composite (which never carries its own audio) must not be scanned at
/// all without a real wav, since `run_render_job` always rejects it.
#[test]
fn a_hud_bundle_with_no_wav_and_a_silent_all_stream_is_not_scanned() {
    let root = scratch("hud_bundle_silent_all");
    let take = root.join("dodstudio_chain_01_b0").join("take0000");
    // "all" is silent, but some other stream carries audio, so the take
    // as a whole still passes `take_shape_is_renderable`.
    write_named_video(&take, "all", "mp4", &mp4_without_audio());
    write_named_video(&take, "hudcolor", "mp4", &mp4_with_audio());
    write_named_video(&take, "hudalpha", "mp4", &mp4_without_audio());

    let clips = scan(&take);
    assert!(
        clips.is_empty(),
        "a silent `all` stream must not be bundled just because the take passes overall: {:?}",
        clips.iter().map(|c| &c.img_folder).collect::<Vec<_>>()
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// An audible `all` stream with no take-level wav can still be admitted
/// as the plain "single" clip — but the paired `hud_only` clip must not
/// be, since HUD compositing always needs a real wav and would only ever
/// fail at render time.
#[test]
fn a_hud_bundle_with_no_wav_admits_all_but_not_the_hud_pair() {
    let root = scratch("hud_bundle_no_wav");
    let take = root.join("dodstudio_chain_01_b0").join("take0000");
    write_named_video(&take, "all", "mp4", &mp4_with_audio());
    write_named_video(&take, "hudcolor", "mp4", &mp4_without_audio());
    write_named_video(&take, "hudalpha", "mp4", &mp4_without_audio());

    let clips = scan(&take);
    assert_eq!(
        clips.len(),
        1,
        "expected only the `all` single clip: {:?}",
        clips
            .iter()
            .map(|c| (&c.img_folder, &c.clip_type))
            .collect::<Vec<_>>()
    );
    assert_eq!(clips[0].clip_type, "single");
    assert_eq!(clips[0].img_folder, "all");
    let _ = std::fs::remove_dir_all(&root);
}
