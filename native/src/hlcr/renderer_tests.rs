use super::*;
use crate::hlcr::config::RenderCodec;
use crate::test_support::Scratch;

fn scratch(name: &str) -> Scratch {
    Scratch::new(format_args!("renderer_test_{name}"))
}

fn sized_clip(width: u32, height: u32, frame_count: usize, clip_type: &str) -> ClipData {
    ClipData {
        take_folder: String::new(),
        clip_type: clip_type.to_string(),
        img_folder: "all".to_string(),
        wav_file: None,
        base_name: "t".to_string(),
        frame_count,
        width,
        height,
        date: String::new(),
        video_file: None,
        alpha_folder: None,
        clip_name: None,
    }
}

/// #774: a reservation on C:\a comes off what C:\b has, so with room for
/// one job on C: the second goes to D:, and to nowhere once D: is full.
#[cfg(windows)]
#[test]
fn reservations_on_one_folder_reduce_its_drives_other_folders() {
    let mut ledger = HashMap::new();
    let free = |p: &Path| Some(if p.starts_with(r"C:\") { 15 } else { 12 });
    let c_only = [PathBuf::from(r"C:\a"), PathBuf::from(r"C:\b")];
    let with_d = [
        PathBuf::from(r"C:\a"),
        PathBuf::from(r"C:\b"),
        PathBuf::from(r"D:\x"),
    ];

    let picked = |r: Reserved| r.picked.map(|p| p.0);
    let first = try_reserve(&mut ledger, &c_only, 10, |_| 0, free);
    assert_eq!(picked(first), Some(PathBuf::from(r"C:\a")));
    assert_eq!(
        picked(try_reserve(&mut ledger, &c_only, 10, |_| 0, free)),
        None
    );
    let second = try_reserve(&mut ledger, &with_d, 10, |_| 0, free);
    assert_eq!(picked(second), Some(PathBuf::from(r"D:\x")));
    assert_eq!(
        picked(try_reserve(&mut ledger, &with_d, 10, |_| 0, free)),
        None
    );
    assert_eq!(ledger.get(&ledger_key(Path::new(r"C:\a"))), Some(&10));
    assert_eq!(ledger.get(&ledger_key(Path::new(r"D:\x"))), Some(&10));
}

/// A folder whose free space can't be read is reserved on, as it was when
/// "unknown" was the largest number; a known one still has to have room.
#[test]
fn an_unreadable_export_folder_still_takes_a_reservation() {
    let mut ledger = HashMap::new();
    let dirs = [PathBuf::from("no-drive-a")];
    let hit = try_reserve(&mut ledger, &dirs, 10, |_| 5, |_| None);
    assert_eq!(hit.picked, Some((PathBuf::from("no-drive-a"), 0, None)));
    assert_eq!(ledger.get(&ledger_key(&dirs[0])), Some(&10));
    // Reservations never make an unreadable folder refuse.
    let again = try_reserve(&mut ledger, &dirs, 10, |_| 5, |_| None);
    assert!(again.picked.is_some());
    let mut ledger = HashMap::new();
    let short = try_reserve(&mut ledger, &dirs, 10, |_| 5, |_| Some(15));
    assert_eq!(short.picked, None);
    assert_eq!(short.passed_over, vec![(PathBuf::from("no-drive-a"), 15)]);
    let fits = try_reserve(&mut ledger, &dirs, 10, |_| 5, |_| Some(16));
    assert!(fits.picked.is_some());
}

/// #1430: the Windows drive (here C:) keeps its larger margin, so a job
/// that fits there by the usual margin goes on to D: instead.
#[cfg(windows)]
#[test]
fn a_windows_drive_folder_below_its_keep_free_margin_is_passed_over() {
    const GB: u64 = 1024 * 1024 * 1024;
    let mut ledger = HashMap::new();
    let dirs = [PathBuf::from(r"C:\Exports"), PathBuf::from(r"D:\Exports")];
    let margin_of = |p: &Path| {
        if p.starts_with(r"C:\") {
            50 * GB
        } else {
            EXPORT_SAFETY_MARGIN_BYTES
        }
    };
    let free = |p: &Path| {
        Some(if p.starts_with(r"C:\") {
            40 * GB
        } else {
            100 * GB
        })
    };

    let reserved = try_reserve(&mut ledger, &dirs, 10 * GB, margin_of, free);
    assert_eq!(reserved.picked.map(|p| p.0), Some(dirs[1].clone()));
    assert_eq!(reserved.passed_over, vec![(dirs[0].clone(), 40 * GB)]);
    // It had room by the usual margin, so the skip is the setting's.
    assert_eq!(
        held_back_for_windows_drive(&reserved.passed_over, 10 * GB, margin_of),
        Some(&(dirs[0].clone(), 40 * GB))
    );
}

/// With the Windows drive above its margin, it is used first as before.
#[cfg(windows)]
#[test]
fn a_windows_drive_folder_with_room_past_its_margin_is_still_used_first() {
    const GB: u64 = 1024 * 1024 * 1024;
    let mut ledger = HashMap::new();
    let dirs = [PathBuf::from(r"C:\Exports"), PathBuf::from(r"D:\Exports")];
    let margin_of = |p: &Path| {
        if p.starts_with(r"C:\") {
            50 * GB
        } else {
            EXPORT_SAFETY_MARGIN_BYTES
        }
    };
    let reserved = try_reserve(&mut ledger, &dirs, 10 * GB, margin_of, |_| Some(61 * GB));
    assert_eq!(reserved.picked.map(|p| p.0), Some(dirs[0].clone()));
    assert!(reserved.passed_over.is_empty());
}

/// A folder passed over because it was full by the usual margin too is
/// not blamed on the setting.
#[test]
fn a_folder_full_by_the_usual_margin_is_not_held_back_by_the_setting() {
    const GB: u64 = 1024 * 1024 * 1024;
    let passed = [(PathBuf::from("windows"), 5 * GB)];
    assert_eq!(
        held_back_for_windows_drive(&passed, 10 * GB, |_| 50 * GB),
        None
    );
    // Room by the usual margin, but the folder keeps only that margin.
    let passed = [(PathBuf::from("other"), 20 * GB)];
    assert_eq!(
        held_back_for_windows_drive(&passed, 10 * GB, |_| EXPORT_SAFETY_MARGIN_BYTES),
        None
    );
}

/// #120: the first clip of a kind reserves the raw estimate; once one
/// has finished, later ones reserve its real rate plus the margin,
/// never more than raw, and only for the same codec and frame size.
#[test]
fn reservations_learn_from_finished_renders() {
    // Frame sizes no other test uses, since the rates are session-wide.
    let first = sized_clip(1234, 567, 100, "single");
    let raw = 1234 * 567 * 3 * 100;
    assert_eq!(
        job_reservation_estimate(&first, RenderCodec::H264Software, ""),
        raw
    );

    record_output_size(&first, RenderCodec::H264Software, "", 1_000_000);
    let later = sized_clip(1234, 567, 200, "single");
    assert_eq!(
        job_reservation_estimate(&later, RenderCodec::H264Software, ""),
        (1_000_000.0 / 100.0 * LEARNED_RATE_MARGIN * 200.0) as u64
    );
    // A smaller clip never lowers the rate that's kept.
    record_output_size(&first, RenderCodec::H264Software, "", 10);
    assert_eq!(
        job_reservation_estimate(&later, RenderCodec::H264Software, ""),
        (1_000_000.0 / 100.0 * LEARNED_RATE_MARGIN * 200.0) as u64
    );
    // Another codec, frame size, custom args or the HUD stream: raw.
    assert_eq!(
        job_reservation_estimate(&later, RenderCodec::ProRes, ""),
        raw * 2
    );
    let hud = sized_clip(1234, 567, 200, "hud_only");
    assert_eq!(
        job_reservation_estimate(&hud, RenderCodec::H264Software, ""),
        raw * 2
    );
    record_output_size(&first, RenderCodec::Custom, "-c:v mpeg4", 1_000);
    assert_eq!(
        job_reservation_estimate(&later, RenderCodec::Custom, "-c:v ffv1"),
        raw * 2
    );
    // Never above raw, however much a render wrote.
    let big = sized_clip(1234, 568, 10, "single");
    record_output_size(&big, RenderCodec::DnxHr, "", u64::MAX / 4);
    assert_eq!(
        job_reservation_estimate(&big, RenderCodec::DnxHr, ""),
        1234 * 568 * 3 * 10
    );
}

/// Each ProRes profile learns its own rate; until it has one, it borrows
/// another profile's through their size ratio. Other codecs never do.
#[test]
fn a_prores_profile_borrows_another_profiles_rate_until_it_has_its_own() {
    // A frame size no other test uses, since the rates are session-wide.
    let clip = sized_clip(1236, 571, 100, "single");
    let raw = 1236 * 571 * 3 * 100;
    assert_eq!(
        job_reservation_estimate(&clip, RenderCodec::ProResLt, ""),
        raw
    );
    // HQ wrote 1000 bytes a frame.
    record_output_size(&clip, RenderCodec::ProRes, "", 100_000);
    let expect = |ratio: f64| (1000.0 * ratio * LEARNED_RATE_MARGIN * 100.0).ceil() as u64;
    assert_eq!(
        job_reservation_estimate(&clip, RenderCodec::ProRes, ""),
        expect(1.0)
    );
    assert_eq!(
        job_reservation_estimate(&clip, RenderCodec::ProRes422, ""),
        expect(0.67)
    );
    assert_eq!(
        job_reservation_estimate(&clip, RenderCodec::ProResLt, ""),
        expect(0.47)
    );
    assert_eq!(
        job_reservation_estimate(&clip, RenderCodec::ProRes4444, ""),
        expect(1.5)
    );
    // LT's own measured rate wins over the borrowed one.
    record_output_size(&clip, RenderCodec::ProResLt, "", 30_000);
    assert_eq!(
        job_reservation_estimate(&clip, RenderCodec::ProResLt, ""),
        (300.0 * LEARNED_RATE_MARGIN * 100.0).ceil() as u64
    );
    // Not for another codec, nor the HUD stream.
    assert_eq!(job_reservation_estimate(&clip, RenderCodec::DnxHr, ""), raw);
    let hud = sized_clip(1236, 571, 100, "hud_only");
    assert_eq!(
        job_reservation_estimate(&hud, RenderCodec::ProRes422, ""),
        raw
    );
}

/// Renders a generated 2560x1440 Ut Video take with every ProRes choice
/// through `run_render_job` and reads back each file's profile with
/// ffprobe. Needs FFmpeg: `DOD_FFMPEG` is the path to ffmpeg.exe, with
/// ffprobe.exe beside it. Prints each file's size next to HQ's.
#[tokio::test]
#[ignore = "needs FFmpeg: set DOD_FFMPEG to ffmpeg.exe's path"]
async fn prores_choices_render_with_their_profile() {
    let ffmpeg = PathBuf::from(
        std::env::var("DOD_FFMPEG").expect("DOD_FFMPEG must name ffmpeg.exe for this test"),
    );
    assert!(ffmpeg.is_file(), "no FFmpeg at {}", ffmpeg.display());
    let ffprobe = ffmpeg.with_file_name(if cfg!(windows) {
        "ffprobe.exe"
    } else {
        "ffprobe"
    });
    let root = scratch("prores_profiles");
    let take_folder = root.join("demo").join("take0000");
    let stream = take_folder.join("all");
    std::fs::create_dir_all(&stream).unwrap();
    let export_dir = root.join("export");
    std::fs::create_dir_all(&export_dir).unwrap();
    let frames = 150usize;
    let made = std::process::Command::new(&ffmpeg)
        .args([
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
        ])
        .arg(format!(
            "testsrc2=size=2560x1440:rate=300:duration={}",
            frames as f64 / 300.0
        ))
        .args(["-pix_fmt", "rgb24", "-c:v", "utvideo"])
        .arg(stream.join("video.avi"))
        .status()
        .unwrap();
    assert!(made.success());
    let made = std::process::Command::new(&ffmpeg)
        .args([
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
        ])
        .arg(format!(
            "sine=frequency=440:duration={}",
            frames as f64 / 300.0
        ))
        .arg(take_folder.join("sound.wav"))
        .status()
        .unwrap();
    assert!(made.success());

    let mut sizes = Vec::new();
    for (codec, profile) in [
        (RenderCodec::ProRes, "HQ"),
        (RenderCodec::ProRes422, "Standard"),
        (RenderCodec::ProResLt, "LT"),
        (RenderCodec::ProRes4444, "4444"),
    ] {
        let clip = ClipData {
            clip_name: Some(codec.to_str_id().to_string()),
            take_folder: take_folder.to_string_lossy().into_owned(),
            clip_type: "single".to_string(),
            img_folder: "all".to_string(),
            wav_file: Some("sound.wav".to_string()),
            base_name: "t".to_string(),
            frame_count: frames,
            width: 2560,
            height: 1440,
            date: "-".to_string(),
            video_file: Some("video.avi".to_string()),
            alpha_folder: None,
        };
        let config = RenderConfig {
            ffmpeg_path: ffmpeg.to_string_lossy().into_owned(),
            target_codec: codec,
            ..source_copy_config(&export_dir)
        };
        let (tx, rx) = mpsc::channel();
        run_render_job(
            "0".to_string(),
            clip,
            config,
            tx,
            Arc::new(AtomicBool::new(false)),
            Arc::new(Mutex::new(HashMap::new())),
        )
        .await;
        let updates = drain(&rx);
        assert_eq!(last_finished(&updates), Some((true, None)), "{:?}", updates);
        let out = updates
            .iter()
            .find_map(|u| match u {
                RenderUpdate::OutputPath(_, p) => Some(p.clone()),
                _ => None,
            })
            .unwrap();
        let probe = std::process::Command::new(&ffprobe)
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=codec_name,profile,pix_fmt",
                "-of",
                "default=noprint_wrappers=1",
            ])
            .arg(&out)
            .output()
            .unwrap();
        let report = String::from_utf8_lossy(&probe.stdout);
        assert!(
            report.lines().any(|l| l == format!("profile={profile}")),
            "{codec:?}: {report}"
        );
        assert!(report.contains("codec_name=prores"), "{codec:?}: {report}");
        sizes.push((codec, std::fs::metadata(&out).unwrap().len()));
    }
    let hq = sizes[0].1 as f64;
    for (codec, size) in &sizes {
        println!(
            "{:?}: {} bytes, {:.2}x HQ (default ratio {})",
            codec,
            size,
            *size as f64 / hq,
            codec.prores_profile().unwrap().2
        );
    }
    let _ = std::fs::remove_dir_all(&root);
}

fn source_copy_config(export_dir: &Path) -> RenderConfig {
    RenderConfig {
        ffmpeg_path: "ffmpeg".to_string(),
        source_folder: String::new(),
        export_directories: vec![export_dir.to_path_buf()],
        fps: 300,
        target_codec: RenderCodec::SourceCopy,
        custom_codec_args: String::new(),
        max_concurrent_renders: 1,
        clip_tag: None,
    }
}

fn drain(rx: &mpsc::Receiver<RenderUpdate>) -> Vec<RenderUpdate> {
    let mut out = Vec::new();
    while let Ok(u) = rx.try_recv() {
        out.push(u);
    }
    out
}

fn last_finished(updates: &[RenderUpdate]) -> Option<(bool, Option<String>)> {
    updates.iter().rev().find_map(|u| match u {
        RenderUpdate::Finished(_, success, err) => Some((*success, err.clone())),
        _ => None,
    })
}

/// The core of issue #82's "skip" path: an OBS take with no separate wav
/// (audio already muxed into the video) is routed into the export pool
/// as a plain copy — no FFmpeg spawn, original file left in place.
#[tokio::test]
async fn skip_copies_the_source_video_into_the_export_pool() {
    let root = scratch("skip_ok");
    let take_folder = root.join("take");
    let stream = take_folder.join("all");
    std::fs::create_dir_all(&stream).unwrap();
    std::fs::write(stream.join("video.mp4"), b"fake obs video bytes").unwrap();
    let export_dir = root.join("export");
    std::fs::create_dir_all(&export_dir).unwrap();

    let clip = ClipData {
        clip_name: None,
        take_folder: take_folder.to_string_lossy().into_owned(),
        clip_type: "single".to_string(),
        img_folder: "all".to_string(),
        wav_file: None,
        base_name: "demo-take-obs".to_string(),
        frame_count: 0,
        width: 0,
        height: 0,
        date: "-".to_string(),
        video_file: Some("video.mp4".to_string()),
        alpha_folder: None,
    };

    let (tx, rx) = mpsc::channel();
    run_render_job(
        "0".to_string(),
        clip,
        source_copy_config(&export_dir),
        tx,
        Arc::new(AtomicBool::new(false)),
        Arc::new(Mutex::new(HashMap::new())),
    )
    .await;

    let updates = drain(&rx);
    assert_eq!(last_finished(&updates), Some((true, None)), "{:?}", updates);
    let output_path = updates
        .iter()
        .find_map(|u| match u {
            RenderUpdate::OutputPath(_, p) => Some(p.clone()),
            _ => None,
        })
        .expect("OutputPath was not sent");
    assert!(output_path.ends_with(".mp4"), "{}", output_path);
    assert_eq!(
        std::fs::read(&output_path).unwrap(),
        b"fake obs video bytes"
    );
    // A copy, not a move — the original take is untouched.
    assert!(stream.join("video.mp4").exists());
    let _ = std::fs::remove_dir_all(&root);
}

/// No export folder: refused, with nothing written anywhere (it used to
/// land in the working directory).
#[tokio::test]
async fn a_render_with_no_export_folder_is_refused() {
    let root = scratch("no_export");
    let take_folder = root.join("take");
    let stream = take_folder.join("all");
    std::fs::create_dir_all(&stream).unwrap();
    std::fs::write(stream.join("video.mp4"), b"fake obs video bytes").unwrap();
    let clip = ClipData {
        clip_name: None,
        take_folder: take_folder.to_string_lossy().into_owned(),
        clip_type: "single".to_string(),
        img_folder: "all".to_string(),
        wav_file: None,
        base_name: "no-export-take".to_string(),
        frame_count: 0,
        width: 0,
        height: 0,
        date: "-".to_string(),
        video_file: Some("video.mp4".to_string()),
        alpha_folder: None,
    };
    let mut config = source_copy_config(&root);
    config.export_directories.clear();

    let (tx, rx) = mpsc::channel();
    run_render_job(
        "0".to_string(),
        clip,
        config,
        tx,
        Arc::new(AtomicBool::new(false)),
        Arc::new(Mutex::new(HashMap::new())),
    )
    .await;

    let updates = drain(&rx);
    assert_eq!(
        last_finished(&updates),
        Some((false, Some(NO_EXPORT_FOLDER.to_string()))),
        "{:?}",
        updates
    );
    assert!(
        !updates
            .iter()
            .any(|u| matches!(u, RenderUpdate::OutputPath(..)))
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// A clip name (#441) names the finished file, and a second render of the
/// same clip gets a suffix instead of overwriting the first.
#[tokio::test]
async fn a_clip_name_names_the_file_and_never_overwrites() {
    let root = scratch("clip_name");
    let take_folder = root.join("take");
    let stream = take_folder.join("all");
    std::fs::create_dir_all(&stream).unwrap();
    std::fs::write(stream.join("video.mp4"), b"v").unwrap();
    let export_dir = root.join("export");
    std::fs::create_dir_all(&export_dir).unwrap();

    let clip = ClipData {
        clip_name: Some("anzio_krod_4k".to_string()),
        take_folder: take_folder.to_string_lossy().into_owned(),
        clip_type: "single".to_string(),
        img_folder: "all".to_string(),
        wav_file: None,
        base_name: "demo-take-obs".to_string(),
        frame_count: 0,
        width: 0,
        height: 0,
        date: "-".to_string(),
        video_file: Some("video.mp4".to_string()),
        alpha_folder: None,
    };

    for _ in 0..2 {
        let (tx, rx) = mpsc::channel();
        run_render_job(
            "0".to_string(),
            clip.clone(),
            source_copy_config(&export_dir),
            tx,
            Arc::new(AtomicBool::new(false)),
            Arc::new(Mutex::new(HashMap::new())),
        )
        .await;
        assert_eq!(last_finished(&drain(&rx)), Some((true, None)));
    }
    assert!(export_dir.join("anzio_krod_4k.mp4").is_file());
    assert!(export_dir.join("anzio_krod_4k_1.mp4").is_file());
}

/// #1430: a clip name cut for the export folders at queue time can meet
/// a longer folder added mid-batch; the render cuts it again so the path
/// stays within the limit, keeping the wav and HUD suffixes.
#[test]
fn a_clip_name_is_cut_to_fit_a_longer_export_folder() {
    let short = Path::new("D:/clips");
    let long_name = "x".repeat(200);
    let long = PathBuf::from(format!("E:/{}", "f".repeat(100)));
    let clip = ClipData {
        clip_name: Some(long_name.clone()),
        ..sized_clip(1, 1, 1, "single")
    };
    let room = name_room(short, "-sound", true, ".mov");
    let stem = clip_file_stem(&clip, "-sound", true, room).unwrap();
    assert_eq!(stem, format!("{long_name}-sound_hud"));

    let room = name_room(&long, "-sound", true, ".mov");
    let stem = clip_file_stem(&clip, "-sound", true, room).unwrap();
    assert!(stem.ends_with("-sound_hud"));
    // The unfinished file's path, the longer of the two, is the one that
    // fits exactly (#1439).
    let path_units =
        long.to_string_lossy().len() + 1 + stem.len() + 3 + PARTIAL_MARK.len() + ".mov".len();
    assert_eq!(path_units, OUTPUT_PATH_LIMIT);

    // No room at all: the default name, not an empty one.
    let deep = PathBuf::from(format!("E:/{}", "f".repeat(300)));
    let room = name_room(&deep, "", false, ".mov");
    assert_eq!(clip_file_stem(&clip, "", false, room), None);
    assert_eq!(fit_name("ab. c", 3), "ab");
    assert_eq!(fit_name("é😀", 2), "é");
}

/// #1430 with #1439: the name a render reserves, and so the unfinished
/// `.partial` file it writes, is the cut name, and the unfinished file's
/// path stays within the limit even when `_N` is added for a clash.
#[test]
fn the_reserved_and_partial_names_are_the_cut_clip_name() {
    let root = scratch("reserve_cut_name");
    let used = root.to_string_lossy().encode_utf16().count();
    let folder = root.join("f".repeat(OUTPUT_PATH_LIMIT - used - 60));
    let clip = ClipData {
        clip_name: Some("x".repeat(200)),
        ..sized_clip(1, 1, 1, "single")
    };
    let (tx, _rx) = mpsc::channel();
    let job = RenderJob::new(
        "0".to_string(),
        clip,
        source_copy_config(&folder),
        tx,
        Arc::new(AtomicBool::new(false)),
    );
    let naming = OutputNaming::new(&job);
    let first = naming.reserve(&job, &folder, ".mov");
    // A second render of the same clip while the first is in flight.
    let second = naming.reserve(&job, &folder, ".mov");

    let room = name_room(&folder, &naming.wav_part, false, ".mov");
    let cut = "x".repeat(room);
    assert_eq!(first.out_file, folder.join(format!("{cut}.mov")));
    assert_eq!(
        first.partial,
        folder.join(format!("{cut}{PARTIAL_MARK}.mov"))
    );
    assert_eq!(second.out_file, folder.join(format!("{cut}_1.mov")));
    for reserved in [&first, &second] {
        let units = reserved.partial.to_string_lossy().encode_utf16().count();
        assert!(units <= OUTPUT_PATH_LIMIT, "{units} units");
    }
}

#[test]
fn a_clip_name_is_cleaned_for_windows() {
    assert_eq!(sanitize_file_stem("anzio: krod?  "), "anzio_ krod_");
    assert_eq!(sanitize_file_stem("a/b\\c|d."), "a_b_c_d");
    assert_eq!(sanitize_file_stem(" . "), "");
}

/// HUD/alpha compositing always needs the FFmpeg alpha-merge pass, so
/// skip cannot apply to it regardless of how the clip's audio arrived.
#[tokio::test]
async fn skip_is_refused_for_a_hud_clip() {
    let root = scratch("skip_hud_refused");
    let take_folder = root.join("take");
    let stream = take_folder.join("hudcolor");
    std::fs::create_dir_all(&stream).unwrap();
    std::fs::write(stream.join("video.mp4"), b"x").unwrap();
    let export_dir = root.join("export");
    std::fs::create_dir_all(&export_dir).unwrap();

    let clip = ClipData {
        clip_name: None,
        take_folder: take_folder.to_string_lossy().into_owned(),
        clip_type: "hud_only".to_string(),
        img_folder: "hudcolor".to_string(),
        wav_file: None,
        base_name: "demo-take-obs".to_string(),
        frame_count: 0,
        width: 0,
        height: 0,
        date: "-".to_string(),
        video_file: Some("video.mp4".to_string()),
        alpha_folder: Some("hudalpha".to_string()),
    };

    let (tx, rx) = mpsc::channel();
    run_render_job(
        "0".to_string(),
        clip,
        source_copy_config(&export_dir),
        tx,
        Arc::new(AtomicBool::new(false)),
        Arc::new(Mutex::new(HashMap::new())),
    )
    .await;

    let updates = drain(&rx);
    match last_finished(&updates) {
        Some((false, Some(_))) => {}
        other => panic!("expected a rejection, got {:?}", other),
    }
    let _ = std::fs::remove_dir_all(&root);
}

/// A clip with neither a wav nor a captured video has no audio source at
/// all — this must fail loudly rather than render a silent clip.
#[tokio::test]
async fn a_clip_with_no_audio_source_at_all_is_refused() {
    let root = scratch("no_audio_source");
    let take_folder = root.join("take");
    std::fs::create_dir_all(take_folder.join("all")).unwrap();
    let export_dir = root.join("export");
    std::fs::create_dir_all(&export_dir).unwrap();

    let clip = ClipData {
        clip_name: None,
        take_folder: take_folder.to_string_lossy().into_owned(),
        clip_type: "single".to_string(),
        img_folder: "all".to_string(),
        wav_file: None,
        base_name: "demo-take-obs".to_string(),
        frame_count: 0,
        width: 0,
        height: 0,
        date: "-".to_string(),
        video_file: None,
        alpha_folder: None,
    };

    let (tx, rx) = mpsc::channel();
    run_render_job(
        "0".to_string(),
        clip,
        source_copy_config(&export_dir),
        tx,
        Arc::new(AtomicBool::new(false)),
        Arc::new(Mutex::new(HashMap::new())),
    )
    .await;

    let updates = drain(&rx);
    match last_finished(&updates) {
        Some((false, Some(_))) => {}
        other => panic!("expected a rejection, got {:?}", other),
    }
    let _ = std::fs::remove_dir_all(&root);
}

/// Skip needs an actual captured video to copy — an OBS-shaped clip
/// missing its video file (moved, deleted) must fail rather than copy
/// nothing and report success.
#[tokio::test]
async fn skip_is_refused_when_the_source_video_is_missing() {
    let root = scratch("skip_missing_video");
    let take_folder = root.join("take");
    std::fs::create_dir_all(take_folder.join("all")).unwrap();
    let export_dir = root.join("export");
    std::fs::create_dir_all(&export_dir).unwrap();

    let clip = ClipData {
        clip_name: None,
        take_folder: take_folder.to_string_lossy().into_owned(),
        clip_type: "single".to_string(),
        img_folder: "all".to_string(),
        wav_file: None,
        base_name: "demo-take-obs".to_string(),
        frame_count: 0,
        width: 0,
        height: 0,
        date: "-".to_string(),
        // Claims a video that was never written.
        video_file: Some("video.mp4".to_string()),
        alpha_folder: None,
    };

    let (tx, rx) = mpsc::channel();
    run_render_job(
        "0".to_string(),
        clip,
        source_copy_config(&export_dir),
        tx,
        Arc::new(AtomicBool::new(false)),
        Arc::new(Mutex::new(HashMap::new())),
    )
    .await;

    let updates = drain(&rx);
    match last_finished(&updates) {
        Some((false, Some(_))) => {}
        other => panic!("expected a rejection, got {:?}", other),
    }
    let _ = std::fs::remove_dir_all(&root);
}

/// A clip with its own separate wav has something real to mux — skipping
/// it would silently discard that audio track rather than reflect it,
/// so `clip_is_skip_eligible` (shared with the Tauri per-job toggle gate)
/// must refuse it even if a caller forces `SourceCopy` anyway.
#[tokio::test]
async fn skip_is_refused_when_the_clip_still_has_a_separate_wav() {
    let root = scratch("skip_has_wav");
    let take_folder = root.join("take");
    let stream = take_folder.join("all");
    std::fs::create_dir_all(&stream).unwrap();
    std::fs::write(stream.join("video.mp4"), b"x").unwrap();
    std::fs::write(take_folder.join("sound.wav"), b"wav").unwrap();
    let export_dir = root.join("export");
    std::fs::create_dir_all(&export_dir).unwrap();

    let clip = ClipData {
        clip_name: None,
        take_folder: take_folder.to_string_lossy().into_owned(),
        clip_type: "single".to_string(),
        img_folder: "all".to_string(),
        wav_file: Some("sound.wav".to_string()),
        base_name: "demo-take".to_string(),
        frame_count: 0,
        width: 0,
        height: 0,
        date: "-".to_string(),
        video_file: Some("video.mp4".to_string()),
        alpha_folder: None,
    };

    let (tx, rx) = mpsc::channel();
    run_render_job(
        "0".to_string(),
        clip,
        source_copy_config(&export_dir),
        tx,
        Arc::new(AtomicBool::new(false)),
        Arc::new(Mutex::new(HashMap::new())),
    )
    .await;

    let updates = drain(&rx);
    match last_finished(&updates) {
        Some((false, Some(_))) => {}
        other => panic!("expected a rejection, got {:?}", other),
    }
    let _ = std::fs::remove_dir_all(&root);
}

/// #80's sibling gap on the Skip path itself: a plain `tokio::fs::copy`
/// has no way to honor a mid-copy cancel. `copy_cancellable` must remove
/// whatever partial file it started, not leave a truncated one sitting
/// under the pipeline's naming looking like a finished export.
#[tokio::test]
async fn copy_cancellable_removes_the_partial_file_when_cancelled() {
    let root = scratch("copy_cancel");
    let src = root.join("src.bin");
    std::fs::write(&src, vec![0u8; 1024]).unwrap();
    let dst = root.join("dst.bin");
    let cancel = AtomicBool::new(true);

    let cancelled = copy_cancellable(&src, &dst, &cancel).await.unwrap();
    assert!(cancelled);
    assert!(
        !dst.exists(),
        "a cancelled copy must not leave a partial file behind"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test]
async fn copy_cancellable_completes_normally_without_a_cancel() {
    let root = scratch("copy_ok");
    let src = root.join("src.bin");
    std::fs::write(&src, b"hello world").unwrap();
    let dst = root.join("dst.bin");
    let cancel = AtomicBool::new(false);

    let cancelled = copy_cancellable(&src, &dst, &cancel).await.unwrap();
    assert!(!cancelled);
    assert_eq!(std::fs::read(&dst).unwrap(), b"hello world");
    let _ = std::fs::remove_dir_all(&root);
}

/// A cancelled or failed render removes its partial file. A finished
/// clip is never at risk: its name is never a partial name (#1439).
#[tokio::test]
async fn remove_partial_output_removes_the_jobs_partial_file() {
    let root = scratch("partial_output");
    let written = root.join("clip.partial.mp4");
    std::fs::write(&written, b"48 bytes of a header").unwrap();
    remove_partial_output(&written).await;
    assert!(!written.exists(), "the job's partial file must go");
    // Already gone is fine.
    remove_partial_output(&written).await;
    let _ = std::fs::remove_dir_all(&root);
}

// ── Whole-job behaviour, pinned before #1177 split `run_render_job` ──────

/// A take folder `<root>/demo/take0000` with an `all` stream folder, and
/// an export folder beside it.
fn take_dirs(root: &Path) -> (PathBuf, PathBuf) {
    let take_folder = root.join("demo").join("take0000");
    std::fs::create_dir_all(take_folder.join("all")).unwrap();
    let export_dir = root.join("export");
    std::fs::create_dir_all(&export_dir).unwrap();
    (take_folder, export_dir)
}

fn take_clip(take_folder: &Path) -> ClipData {
    ClipData {
        clip_name: None,
        take_folder: take_folder.to_string_lossy().into_owned(),
        clip_type: "single".to_string(),
        img_folder: "all".to_string(),
        wav_file: None,
        base_name: "t".to_string(),
        frame_count: 0,
        width: 0,
        height: 0,
        date: "-".to_string(),
        video_file: None,
        alpha_folder: None,
    }
}

async fn run(clip: ClipData, config: RenderConfig, cancel: bool) -> Vec<RenderUpdate> {
    let (tx, rx) = mpsc::channel();
    run_render_job(
        "7".to_string(),
        clip,
        config,
        tx,
        Arc::new(AtomicBool::new(cancel)),
        Arc::new(Mutex::new(HashMap::new())),
    )
    .await;
    drain(&rx)
}

/// The updates as plain text, so a sequence compares in one assert.
fn describe(updates: &[RenderUpdate]) -> Vec<String> {
    updates
        .iter()
        .map(|u| match u {
            RenderUpdate::Progress(id, p) => format!("{id} progress {p}"),
            RenderUpdate::Speed(id, s) => format!("{id} speed {s}"),
            RenderUpdate::Status(id, s) => format!("{id} status {s}"),
            RenderUpdate::OutputPath(id, _) => format!("{id} output"),
            RenderUpdate::Finished(id, ok, err) => {
                format!("{id} finished {ok} {}", err.as_deref().unwrap_or("-"))
            }
        })
        .collect()
}

fn only_refusal(updates: &[RenderUpdate], text: &str) {
    assert_eq!(
        describe(updates),
        vec![format!("7 finished false {text}")],
        "{updates:?}"
    );
}

fn files_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// Each refusal before any work is one `Finished` with its own text, in
/// this order: FFmpeg, take folder, wav, HUD without a wav, no audio, Skip
/// not allowed.
#[tokio::test]
async fn refusals_before_any_work_send_one_finished_with_their_text() {
    let root = scratch("refusals");
    let (take_folder, export_dir) = take_dirs(&root);
    let h264 = |ffmpeg: &str| RenderConfig {
        ffmpeg_path: ffmpeg.to_string(),
        target_codec: RenderCodec::H264Software,
        ..source_copy_config(&export_dir)
    };
    let missing_ffmpeg = root.join("nope").join("ffmpeg.exe");

    let mut with_wav = take_clip(&take_folder);
    with_wav.wav_file = Some("sound.wav".to_string());
    let updates = run(
        with_wav.clone(),
        h264(&missing_ffmpeg.to_string_lossy()),
        false,
    )
    .await;
    only_refusal(
        &updates,
        &format!("FFmpeg not found at: {}", missing_ffmpeg.display()),
    );

    // Skip never runs FFmpeg, so a bad path doesn't stop it; the missing
    // take folder does.
    let gone = root.join("demo").join("take9999");
    let mut skip_cfg = source_copy_config(&export_dir);
    skip_cfg.ffmpeg_path = missing_ffmpeg.to_string_lossy().into_owned();
    let updates = run(take_clip(&gone), skip_cfg, false).await;
    only_refusal(
        &updates,
        &format!("Take folder not found: {}", gone.display()),
    );

    let updates = run(with_wav.clone(), h264("ffmpeg"), false).await;
    only_refusal(
        &updates,
        &format!(
            "Audio file not found: {}",
            take_folder.join("sound.wav").display()
        ),
    );

    let mut hud = take_clip(&take_folder);
    hud.clip_type = "hud_only".to_string();
    hud.video_file = Some("video.avi".to_string());
    let updates = run(hud, h264("ffmpeg"), false).await;
    only_refusal(
        &updates,
        "HUD/alpha clips need a separate audio track (sound.wav), but this take has none.",
    );

    let updates = run(take_clip(&take_folder), h264("ffmpeg"), false).await;
    only_refusal(
        &updates,
        "No audio source for this take: no sound.wav and no audio-bearing video.",
    );

    std::fs::write(take_folder.join("sound.wav"), b"wav").unwrap();
    let mut hud = with_wav.clone();
    hud.clip_type = "hud_only".to_string();
    hud.video_file = Some("video.avi".to_string());
    let updates = run(hud, source_copy_config(&export_dir), false).await;
    only_refusal(
        &updates,
        "Skip (keep original) isn't available for HUD/alpha clips — pick a codec to render them.",
    );
    let mut wav_and_video = with_wav.clone();
    wav_and_video.video_file = Some("video.avi".to_string());
    let updates = run(wav_and_video, source_copy_config(&export_dir), false).await;
    only_refusal(
        &updates,
        "Skip (keep original) isn't available for a take with its own separate audio track — pick a codec to render it.",
    );
    assert!(files_in(&export_dir).is_empty());
}

/// No export folder with room for the job: refused with the space it
/// needed, before anything is written.
#[tokio::test]
async fn a_job_too_big_for_every_export_folder_is_refused_with_the_space_it_needs() {
    let root = scratch("no_room");
    let (take_folder, export_dir) = take_dirs(&root);
    std::fs::write(take_folder.join("sound.wav"), b"wav").unwrap();
    let mut clip = take_clip(&take_folder);
    clip.wav_file = Some("sound.wav".to_string());
    // A frame size no other test uses, since learned rates are session-wide.
    (clip.width, clip.height, clip.frame_count) = (100_003, 100_000, 1_000_000);
    let config = RenderConfig {
        target_codec: RenderCodec::H264Software,
        ..source_copy_config(&export_dir)
    };
    let needed = 100_003u64 * 100_000 * 3 * 1_000_000 + EXPORT_SAFETY_MARGIN_BYTES;
    let updates = run(clip, config, false).await;
    only_refusal(
        &updates,
        &format!(
            "Not rendered: every export folder (1) has less than {:.1} GB free once the other renders running now are counted. Free some space or add another export folder in Configuration → Render Settings.",
            needed as f64 / (1024.0 * 1024.0 * 1024.0)
        ),
    );
}

/// Custom with no arguments is refused after the export folder is
/// picked, with nothing written into it.
#[tokio::test]
async fn custom_without_arguments_is_refused() {
    let root = scratch("custom_empty");
    let (take_folder, export_dir) = take_dirs(&root);
    std::fs::write(take_folder.join("sound.wav"), b"wav").unwrap();
    let mut clip = take_clip(&take_folder);
    clip.wav_file = Some("sound.wav".to_string());
    let config = RenderConfig {
        target_codec: RenderCodec::Custom,
        custom_codec_args: "   ".to_string(),
        ..source_copy_config(&export_dir)
    };
    let updates = run(clip, config, false).await;
    only_refusal(
        &updates,
        "Custom codec selected but no FFmpeg arguments were entered.",
    );
    assert!(files_in(&export_dir).is_empty());
}

/// An FFmpeg path that names a file which isn't a program: `Rendering`,
/// then the spawn error.
#[tokio::test]
async fn an_ffmpeg_that_cannot_start_reports_the_spawn_error() {
    let root = scratch("spawn_fail");
    let (take_folder, export_dir) = take_dirs(&root);
    std::fs::write(take_folder.join("sound.wav"), b"wav").unwrap();
    let not_a_program = root.join("ffmpeg.txt");
    std::fs::write(&not_a_program, b"not a program").unwrap();
    let mut clip = take_clip(&take_folder);
    clip.wav_file = Some("sound.wav".to_string());
    let config = RenderConfig {
        ffmpeg_path: not_a_program.to_string_lossy().into_owned(),
        target_codec: RenderCodec::H264Software,
        ..source_copy_config(&export_dir)
    };
    let updates = run(clip, config, false).await;
    let text = describe(&updates);
    assert_eq!(text.len(), 2, "{text:?}");
    assert_eq!(text[0], "7 status Rendering");
    assert!(
        text[1].starts_with("7 finished false Failed to spawn FFmpeg process: "),
        "{text:?}"
    );
}

/// A Skip job's whole update sequence, and the file name it gets without
/// a clip name: `<demo>_<take>_all_<4 hex digits>.<source extension>`.
#[tokio::test]
async fn skip_sends_its_updates_in_order_and_names_the_file_after_the_take() {
    let root = scratch("skip_sequence");
    let (take_folder, export_dir) = take_dirs(&root);
    std::fs::write(take_folder.join("all").join("video.mkv"), b"v").unwrap();
    let mut clip = take_clip(&take_folder);
    clip.video_file = Some("video.mkv".to_string());
    let updates = run(clip, source_copy_config(&export_dir), false).await;
    assert_eq!(
        describe(&updates),
        vec![
            "7 status Rendering",
            "7 status Finished",
            "7 progress 100",
            "7 output",
            "7 finished true -",
        ]
    );
    let names = files_in(&export_dir);
    assert_eq!(names.len(), 1);
    let name = &names[0];
    let hash = name
        .strip_prefix("demo_take0000_all_")
        .and_then(|rest| rest.strip_suffix(".mkv"))
        .unwrap_or_else(|| panic!("{name}"));
    assert!(
        hash.len() == 4 && hash.chars().all(|c| c.is_ascii_hexdigit()),
        "{name}"
    );
    let output = updates.iter().find_map(|u| match u {
        RenderUpdate::OutputPath(_, p) => Some(PathBuf::from(p)),
        _ => None,
    });
    assert_eq!(output, Some(export_dir.join(name)));
}

/// Cancel set before a Skip job starts: `Cancelled`, nothing copied.
#[tokio::test]
async fn skip_cancelled_before_it_starts_copies_nothing() {
    let root = scratch("skip_cancel_first");
    let (take_folder, export_dir) = take_dirs(&root);
    std::fs::write(take_folder.join("all").join("video.mp4"), b"v").unwrap();
    let mut clip = take_clip(&take_folder);
    clip.video_file = Some("video.mp4".to_string());
    let updates = run(clip, source_copy_config(&export_dir), true).await;
    assert_eq!(
        describe(&updates),
        vec!["7 status Cancelled", "7 finished false Cancelled by user"]
    );
    assert!(files_in(&export_dir).is_empty());
}

fn real_ffmpeg() -> PathBuf {
    let ffmpeg = PathBuf::from(
        std::env::var("DOD_FFMPEG").expect("DOD_FFMPEG must name ffmpeg.exe for this test"),
    );
    assert!(ffmpeg.is_file(), "no FFmpeg at {}", ffmpeg.display());
    ffmpeg
}

/// A 10-frame BMP take with a sound.wav, made by FFmpeg.
fn make_bmp_take(ffmpeg: &Path, take_folder: &Path) {
    let made = std::process::Command::new(ffmpeg)
        .args(["-y", "-hide_banner", "-loglevel", "error", "-f", "lavfi"])
        .args(["-i", "testsrc2=size=64x48:rate=30:duration=0.334"])
        .args(["-frames:v", "10", "-start_number", "0"])
        .arg(take_folder.join("all").join("%05d.bmp"))
        .status()
        .unwrap();
    assert!(made.success());
    let made = std::process::Command::new(ffmpeg)
        .args(["-y", "-hide_banner", "-loglevel", "error", "-f", "lavfi"])
        .args(["-i", "sine=frequency=440:duration=0.334"])
        .arg(take_folder.join("sound.wav"))
        .status()
        .unwrap();
    assert!(made.success());
}

/// Cancel set before FFmpeg's first progress line: killed on the first
/// poll, its partial file removed.
#[tokio::test]
#[ignore = "needs FFmpeg: set DOD_FFMPEG to ffmpeg.exe's path"]
async fn an_ffmpeg_render_cancelled_at_once_leaves_no_file() {
    let ffmpeg = real_ffmpeg();
    let root = scratch("ffmpeg_cancel");
    let (take_folder, export_dir) = take_dirs(&root);
    make_bmp_take(&ffmpeg, &take_folder);
    let mut clip = take_clip(&take_folder);
    clip.wav_file = Some("sound.wav".to_string());
    clip.frame_count = 10;
    let config = RenderConfig {
        ffmpeg_path: ffmpeg.to_string_lossy().into_owned(),
        target_codec: RenderCodec::H264Software,
        ..source_copy_config(&export_dir)
    };
    let updates = run(clip, config, true).await;
    assert_eq!(
        describe(&updates),
        vec![
            "7 status Rendering",
            "7 status Cancelled",
            "7 finished false Cancelled by user",
        ]
    );
    assert!(files_in(&export_dir).is_empty());
}

/// FFmpeg exiting with an error: `Error`, its exit code and log in the
/// message, its partial file removed.
#[tokio::test]
#[ignore = "needs FFmpeg: set DOD_FFMPEG to ffmpeg.exe's path"]
async fn an_ffmpeg_failure_reports_its_exit_code_and_log() {
    let ffmpeg = real_ffmpeg();
    let root = scratch("ffmpeg_fail");
    let (take_folder, export_dir) = take_dirs(&root);
    make_bmp_take(&ffmpeg, &take_folder);
    let mut clip = take_clip(&take_folder);
    clip.wav_file = Some("sound.wav".to_string());
    clip.frame_count = 10;
    let config = RenderConfig {
        ffmpeg_path: ffmpeg.to_string_lossy().into_owned(),
        target_codec: RenderCodec::Custom,
        custom_codec_args: "-c:v no_such_codec".to_string(),
        ..source_copy_config(&export_dir)
    };
    let text = describe(&run(clip, config, false).await);
    assert_eq!(text.len(), 3, "{text:?}");
    assert_eq!(text[0], "7 status Rendering");
    assert_eq!(text[1], "7 status Error");
    assert!(
        text[2].starts_with("7 finished false FFmpeg failed with exit code: ")
            && text[2].contains("no_such_codec"),
        "{text:?}"
    );
    assert!(files_in(&export_dir).is_empty());
}

/// A finished BMP render: `Rendering`, progress and speed lines while
/// FFmpeg reports, then `Finished`, 100, the output path, success.
#[tokio::test]
#[ignore = "needs FFmpeg: set DOD_FFMPEG to ffmpeg.exe's path"]
async fn an_ffmpeg_render_reports_progress_then_finishes() {
    let ffmpeg = real_ffmpeg();
    let root = scratch("ffmpeg_ok");
    let (take_folder, export_dir) = take_dirs(&root);
    make_bmp_take(&ffmpeg, &take_folder);
    let mut clip = take_clip(&take_folder);
    clip.wav_file = Some("sound.wav".to_string());
    clip.frame_count = 10;
    clip.clip_name = Some("bmp_take".to_string());
    let config = RenderConfig {
        ffmpeg_path: ffmpeg.to_string_lossy().into_owned(),
        target_codec: RenderCodec::H264Software,
        fps: 30,
        ..source_copy_config(&export_dir)
    };
    let text = describe(&run(clip, config, false).await);
    assert_eq!(text.first().map(String::as_str), Some("7 status Rendering"));
    assert_eq!(
        text[text.len() - 4..],
        [
            "7 status Finished",
            "7 progress 100",
            "7 output",
            "7 finished true -"
        ]
    );
    let middle = &text[1..text.len() - 4];
    assert!(middle.contains(&"7 progress 100".to_string()), "{text:?}");
    assert!(
        middle
            .iter()
            .all(|l| l.starts_with("7 progress ") || l.starts_with("7 speed ")),
        "{text:?}"
    );
    assert_eq!(files_in(&export_dir), vec!["bmp_take.mp4"]);
}

/// FFmpeg failing after it has started writing (a broken frame halfway,
/// `-xerror`): `Error` with its log, and the half-written file removed.
#[tokio::test]
#[ignore = "needs FFmpeg: set DOD_FFMPEG to ffmpeg.exe's path"]
async fn an_ffmpeg_failure_partway_removes_the_half_written_file() {
    let ffmpeg = real_ffmpeg();
    let root = scratch("ffmpeg_fail_partway");
    let (take_folder, export_dir) = take_dirs(&root);
    make_bmp_take(&ffmpeg, &take_folder);
    std::fs::write(take_folder.join("all").join("00005.bmp"), b"not a bmp").unwrap();
    let mut clip = take_clip(&take_folder);
    clip.wav_file = Some("sound.wav".to_string());
    clip.frame_count = 10;
    let config = RenderConfig {
        ffmpeg_path: ffmpeg.to_string_lossy().into_owned(),
        target_codec: RenderCodec::Custom,
        custom_codec_args: "-c:v ffv1 -xerror".to_string(),
        fps: 30,
        ..source_copy_config(&export_dir)
    };
    let text = describe(&run(clip, config, false).await);
    assert_eq!(text.first().map(String::as_str), Some("7 status Rendering"));
    assert_eq!(text[text.len() - 2], "7 status Error", "{text:?}");
    assert!(
        text[text.len() - 1].starts_with("7 finished false FFmpeg failed with exit code: "),
        "{text:?}"
    );
    assert!(files_in(&export_dir).is_empty());
}

/// Cancel partway through a render: the export drive stays reserved
/// while FFmpeg writes, the half-written file is removed, and the
/// reservation is released once the job ends.
#[tokio::test]
#[ignore = "needs FFmpeg: set DOD_FFMPEG to ffmpeg.exe's path"]
async fn an_ffmpeg_render_cancelled_partway_keeps_its_reservation_till_it_ends() {
    let ffmpeg = real_ffmpeg();
    let root = scratch("ffmpeg_cancel_partway");
    let (take_folder, export_dir) = take_dirs(&root);
    make_bmp_take(&ffmpeg, &take_folder);
    let mut clip = take_clip(&take_folder);
    clip.wav_file = Some("sound.wav".to_string());
    clip.frame_count = 10;
    // `realtime` slows the 10 frames to several seconds, so the job is
    // still running when it is looked at and cancelled.
    let config = RenderConfig {
        ffmpeg_path: ffmpeg.to_string_lossy().into_owned(),
        target_codec: RenderCodec::Custom,
        custom_codec_args: "-c:v ffv1 -vf realtime=speed=0.05".to_string(),
        fps: 30,
        ..source_copy_config(&export_dir)
    };
    let reserved = |ledger: &Arc<Mutex<HashMap<PathBuf, u64>>>| -> u64 {
        ledger.lock().unwrap().values().sum()
    };
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let ledger = Arc::new(Mutex::new(HashMap::new()));
    let job = tokio::spawn(run_render_job(
        "7".to_string(),
        clip,
        config,
        tx,
        cancel.clone(),
        ledger.clone(),
    ));

    let mut updates = Vec::new();
    let started = std::time::Instant::now();
    while files_in(&export_dir).is_empty() {
        assert!(
            started.elapsed() < std::time::Duration::from_secs(20),
            "FFmpeg never wrote its output: {:?}",
            describe(&updates)
        );
        assert!(!job.is_finished(), "{:?}", describe(&drain(&rx)));
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        updates.extend(drain(&rx));
    }
    assert!(reserved(&ledger) > 0, "nothing reserved while rendering");
    cancel.store(true, Ordering::Relaxed);
    tokio::time::timeout(std::time::Duration::from_secs(10), job)
        .await
        .expect("the job did not stop on Cancel")
        .unwrap();
    updates.extend(drain(&rx));

    let text = describe(&updates);
    assert_eq!(text.first().map(String::as_str), Some("7 status Rendering"));
    assert_eq!(
        text[text.len() - 2..],
        ["7 status Cancelled", "7 finished false Cancelled by user"],
        "{text:?}"
    );
    assert!(
        text[1..text.len() - 2]
            .iter()
            .all(|l| l.starts_with("7 progress ") || l.starts_with("7 speed ")),
        "{text:?}"
    );
    assert!(files_in(&export_dir).is_empty());
    assert_eq!(reserved(&ledger), 0);
}

/// A segmented render's asks (#1451): its own drive for a resume, twice
/// the clip anywhere, then the clip alone, each on drives with `free` room.
fn segmented_asks() -> Vec<RoomAsk> {
    let (a, b) = (PathBuf::from("no-drive-a"), PathBuf::from("no-drive-b"));
    vec![
        RoomAsk {
            dirs: vec![b.clone()],
            amount: 15,
        },
        RoomAsk {
            dirs: vec![a.clone(), b.clone()],
            amount: 20,
        },
        RoomAsk {
            dirs: vec![a, b],
            amount: 10,
        },
    ]
}

fn first_fit(free_a: u64, free_b: u64) -> (Option<usize>, Option<PathBuf>, HashMap<PathBuf, u64>) {
    let mut ledger = HashMap::new();
    let free = |p: &Path| {
        Some(if p.ends_with("no-drive-a") {
            free_a
        } else {
            free_b
        })
    };
    let (fitted, reserved) = reserve_first_fit(&mut ledger, &segmented_asks(), |_| 0, free);
    (fitted, reserved.picked.map(|p| p.0), ledger)
}

/// The resume's own drive is asked first, for the clip and the segments
/// still to render, even when another drive has room for a fresh start.
#[test]
fn a_resume_reserves_on_its_own_drive_first() {
    let (fitted, dir, ledger) = first_fit(100, 16);
    assert_eq!(fitted, Some(0));
    assert_eq!(dir, Some(PathBuf::from("no-drive-b")));
    assert_eq!(ledger.values().sum::<u64>(), 15, "only the ask that fitted");
}

#[test]
fn a_fresh_start_takes_twice_the_clip_on_any_drive() {
    let (fitted, dir, _) = first_fit(21, 5);
    assert_eq!(fitted, Some(1));
    assert_eq!(dir, Some(PathBuf::from("no-drive-a")));
}

/// Room for the clip but not for the segments too (#1451 review): the
/// render goes ahead in one piece, as it did before segments.
#[test]
fn room_for_the_clip_only_falls_back_to_one_piece() {
    let (fitted, dir, ledger) = first_fit(11, 5);
    assert_eq!(fitted, Some(2));
    assert_eq!(dir, Some(PathBuf::from("no-drive-a")));
    assert_eq!(ledger.values().sum::<u64>(), 10);
}

#[test]
fn no_room_even_for_the_clip_fits_nothing() {
    let (fitted, dir, ledger) = first_fit(9, 9);
    assert_eq!((fitted, dir), (None, None));
    assert!(ledger.is_empty());
}
