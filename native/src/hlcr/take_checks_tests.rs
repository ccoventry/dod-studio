use super::*;
use crate::hlcr::take_meta;
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
    // The video decodes on a few cores, not all of them.
    assert!(obs.windows(2).any(|w| w == ["-threads", "4"]));
}

#[test]
fn a_video_is_counted_at_its_own_rate_not_the_capture_rate() {
    // An OBS take: the batch captured "at 300", OBS recorded at 60.
    let video = PictureInput::Video(PathBuf::from("video.mp4"));
    assert_eq!(frame_rate(&video, Some(60.0), Some(300.0)), Some(60.0));
    assert_eq!(frame_rate(&video, None, Some(300.0)), Some(300.0));
    assert_eq!(frame_rate(&video, Some(f64::NAN), Some(300.0)), Some(300.0));
    let found = plan_findings(
        Some(&plan(19.0, &[5.0, 14.0])),
        frame_rate(&video, Some(60.0), Some(300.0)),
        Some(1140),
        Some(19.0),
    );
    assert!(found.is_empty(), "{found:?}");
    let sequence = PictureInput::Sequence {
        pattern: PathBuf::from("%05d.bmp"),
        start: 0,
        fps: 120.0,
    };
    assert_eq!(frame_rate(&sequence, None, Some(300.0)), Some(120.0));
}

#[test]
fn progress_lines_give_frames_done() {
    assert_eq!(progress_frames("frame=1500"), Some(1500));
    assert_eq!(progress_frames("frame=N/A"), None);
    assert_eq!(progress_frames("out_time_us=5"), None);
}

fn plan(length_secs: f64, kills: &[f64]) -> TakePlan {
    TakePlan {
        length_secs,
        kill_offsets_secs: kills.to_vec(),
        sped_up: Vec::new(),
    }
}

fn kinds(found: &[Finding]) -> Vec<Check> {
    found.iter().map(|f| f.check).collect()
}

#[test]
fn a_take_matching_its_plan_has_no_plan_findings() {
    let found = plan_findings(
        Some(&plan(19.0, &[5.0, 12.0])),
        Some(300.0),
        Some(5700),
        None,
    );
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn a_take_far_short_of_its_plan_is_a_capture_warning_with_both_counts() {
    let found = plan_findings(Some(&plan(19.0, &[5.0])), Some(300.0), Some(1500), None);
    let length = found.iter().find(|f| f.check == Check::Length).unwrap();
    assert_eq!(
        (length.frames, length.planned_frames),
        (Some(1500), Some(5700))
    );
    assert_eq!((length.cause, length.warn), (Cause::Capture, true));
}

#[test]
fn a_kill_past_the_video_is_one_warning_counting_them_all() {
    let found = plan_findings(
        Some(&plan(19.0, &[2.0, 12.0, 15.0])),
        Some(300.0),
        Some(2400),
        None,
    );
    let kill = found
        .iter()
        .find(|f| f.check == Check::KillOutside)
        .unwrap();
    assert_eq!((kill.count, kill.at_secs), (Some(2), Some(12.0)));
    assert_eq!(kill.secs, Some(8.0));
    assert!(kill.warn);
}

#[test]
fn without_a_frame_count_kills_are_held_to_the_decoded_length() {
    let found = plan_findings(Some(&plan(19.0, &[12.0])), None, None, Some(8.0));
    assert_eq!(kinds(&found), vec![Check::KillOutside]);
}

#[test]
fn a_take_without_a_plan_gets_one_unknown_note_never_a_warning() {
    let found = plan_findings(None, Some(300.0), Some(5700), Some(19.0));
    assert_eq!(kinds(&found), vec![Check::NoPlan]);
    assert_eq!((found[0].warn, found[0].cause), (false, Cause::Unknown));
}

#[test]
fn nothing_measured_means_no_plan_findings() {
    assert!(plan_findings(Some(&plan(19.0, &[30.0])), None, None, None).is_empty());
}

#[test]
fn results_made_for_another_plan_are_not_reused() {
    let take = Scratch::new("take_checks_plan_cache");
    let take = take.path();
    std::fs::create_dir_all(take.join("all")).unwrap();
    std::fs::write(take.join("all").join("video.avi"), b"video").unwrap();
    let media = take_frames::stream_media(&take.join("all")).unwrap();
    let checks = TakeChecks {
        version: CHECKS_VERSION,
        stream: "all".to_string(),
        source: take_frames::source_stamp(&media),
        sound: None,
        duration_secs: Some(1.0),
        frames: Some(30),
        plan: None,
        peak_db: None,
        rms_db: None,
        findings: Vec::new(),
    };
    let dir = take_frames::cache_dir(take, "all");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join(CHECKS_FILE),
        serde_json::to_string(&checks).unwrap(),
    )
    .unwrap();
    assert_eq!(read_cached(take, "all"), Some(checks));
    let meta = take_meta::SessionMeta {
        plan: Some(plan(1.0, &[])),
        ..take_meta::SessionMeta::new("s", 30)
    };
    take_meta::write(take, &meta).unwrap();
    assert_eq!(read_cached(take, "all"), None);
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

    // Without a plan the length and kills are unknown, and say so.
    assert!(kinds.contains(&Check::NoPlan), "{checks:?}");
    assert_eq!(checks.frames, Some(180), "{checks:?}");

    // A take that gets a plan is checked again against it: 6 s of video
    // against 20 planned, and one of two kills past its end.
    let plan = TakePlan {
        length_secs: 20.0,
        kill_offsets_secs: vec![2.0, 9.0],
        sped_up: Vec::new(),
    };
    let meta = take_meta::SessionMeta {
        plan: Some(plan),
        ..take_meta::SessionMeta::new("s", 30)
    };
    take_meta::write(take, &meta).unwrap();
    assert_eq!(read_cached(take, "all"), None);
    let (planned, cached) = make_checks(&ffmpeg, take, "all", None, cancel.clone(), |_, _| {})
        .await
        .unwrap();
    assert!(!cached);
    let length = planned
        .findings
        .iter()
        .find(|f| f.check == Check::Length)
        .unwrap_or_else(|| panic!("{planned:?}"));
    assert_eq!(
        (length.frames, length.planned_frames),
        (Some(180), Some(600))
    );
    let kills = planned
        .findings
        .iter()
        .find(|f| f.check == Check::KillOutside)
        .unwrap_or_else(|| panic!("{planned:?}"));
    assert_eq!((kills.count, kills.at_secs), (Some(1), Some(9.0)));
    assert!(!planned.findings.iter().any(|f| f.check == Check::NoPlan));

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
