use super::*;
use crate::obs::fake_obs::{self, Reply};
use serde_json::json;

fn stats(out_total: i64, out_skip: i64, r_total: i64, r_skip: i64) -> Value {
    json!({
        "outputTotalFrames": out_total,
        "outputSkippedFrames": out_skip,
        "renderTotalFrames": r_total,
        "renderSkippedFrames": r_skip,
        "averageFrameRenderTime": 1.5,
    })
}

#[test]
fn the_tracker_counts_only_while_obs_records() {
    let mut t = RecordingTracker::default();
    t.on_poll(0.0, Some(false), Some(&stats(100, 0, 1000, 0)));
    t.on_poll(1.0, Some(true), Some(&stats(100, 0, 1100, 0)));
    t.on_poll(2.0, Some(true), Some(&stats(220, 12, 1220, 3)));
    t.on_poll(3.0, Some(true), Some(&stats(340, 30, 1340, 3)));
    // After the stop the counters move on, and must not be counted.
    t.on_poll(4.0, Some(false), Some(&stats(340, 30, 1460, 40)));
    let s = t.finish();
    assert!(s.connected && s.saw_recording);
    assert_eq!(
        s.counters,
        BenchCounters {
            output_total: 240,
            output_skipped: 30,
            render_total: 240,
            render_skipped: 3,
        }
    );
    assert_eq!(s.recorded_secs, 2.0);
    assert_eq!(s.peak_render_ms, 1.5);
}

#[test]
fn a_tracker_that_never_saw_obs_says_so() {
    let mut t = RecordingTracker::default();
    t.on_poll(0.0, None, None);
    let s = t.finish();
    assert!(!s.connected && !s.saw_recording);
}

#[test]
fn counters_going_down_read_as_nothing() {
    let base = BenchCounters {
        output_total: 500,
        output_skipped: 5,
        render_total: 500,
        render_skipped: 5,
    };
    assert_eq!(
        BenchCounters::default().since(base),
        BenchCounters::default()
    );
    assert_eq!(BenchCounters::default().skipped_share(), 0.0);
}

#[test]
fn the_watch_reads_a_recording_from_obs() {
    let polls = Arc::new(std::sync::Mutex::new(0u32));
    let p = Arc::clone(&polls);
    let fake = fake_obs::serve(move |kind, _| match kind {
        "GetRecordStatus" => {
            let n = *p.lock().unwrap();
            Reply::ok(json!({ "outputActive": (1..=4).contains(&n) }))
        }
        "GetStats" => {
            let mut n = p.lock().unwrap();
            *n += 1;
            let k = i64::from(*n);
            Reply::ok(stats(k * 120, k * 6, k * 120, 0))
        }
        _ => Reply::ok(json!({})),
    });
    let watch = StatsWatch::start(fake.url.clone(), String::new());
    let deadline = Instant::now() + Duration::from_secs(10);
    while *polls.lock().unwrap() < 7 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    let s = watch.finish();
    assert!(s.connected && s.saw_recording);
    // Recording on the polls whose GetStats answers 2..=5 (each
    // GetRecordStatus reads the count before that poll's GetStats raises
    // it): 3 intervals of 120 frames, 6 of them skipped each.
    assert_eq!(s.counters.output_total, 360);
    assert_eq!(s.counters.output_skipped, 18);
    assert!((s.counters.skipped_share() - 0.05).abs() < 1e-9);
}

#[test]
fn ydif_lines_and_repeats() {
    let text = "frame:0 pts:0 pts_time:0\nlavfi.signalstats.YDIF=0\nframe:1\nlavfi.signalstats.YDIF=2.5\nlavfi.signalstats.YDIF=0.01\nnoise\n";
    let ydif: Vec<f64> = text.lines().filter_map(parse_ydif_line).collect();
    assert_eq!(ydif, vec![0.0, 2.5, 0.01]);
    // Moving, two repeats, moving, three repeats, moving.
    let ydif = [0.0, 2.5, 0.01, 0.02, 3.0, 0.1, 0.1, 0.1, 4.0];
    let rc = |counted, repeated, longest, still| RepeatCount {
        counted,
        repeated,
        longest,
        still,
    };
    assert_eq!(count_repeats(&ydif, 0, 3), rc(8, 5, 3, 0));
    // Runs longer than the cap are a still picture.
    assert_eq!(count_repeats(&ydif, 0, 2), rc(5, 2, 2, 3));
    assert_eq!(count_repeats(&ydif, 5, 3), rc(4, 3, 3, 0));
    assert_eq!(count_repeats(&[], 0, 3), RepeatCount::default());
    // Beside a frame that barely moved, or at the clip's end: still.
    let slow = [0.0, 3.0, 0.2, 0.01, 3.0, 0.01];
    assert_eq!(count_repeats(&slow, 0, 3), rc(3, 0, 0, 2));
    assert_eq!(max_repeat_run(120.0), 12);
    assert_eq!(max_repeat_run(10.0), 2);
}

#[test]
fn the_hooks_frame_times_file() {
    // goldsrc-hooks frame_times.rs writes this: a header, then one
    // microsecond count per frame. 990 frames at 4 ms, 10 at 20 ms.
    let mut text = String::from("# DoD Studio frame times\n# 1000 frames, ...\n");
    for _ in 0..990 {
        text.push_str("4000\n");
    }
    for _ in 0..10 {
        text.push_str("20000\n");
    }
    let g = GameFrames::from_file_text(&text, 120).unwrap();
    assert_eq!(g.frames, 1000);
    assert_eq!(g.low1_fps, 250.0);
    assert_eq!(g.low01_fps, 50.0);
    assert_eq!(g.max_ms, 20.0);
    // At 120 fps an OBS frame is 8.33 ms: only the 20 ms ones are slower.
    assert_eq!(g.slower_than_obs, 10);
    assert!((g.slower_share() - 0.01).abs() < 1e-9);
    // At 300 fps (3.33 ms) every frame is.
    assert_eq!(
        GameFrames::from_file_text(&text, 300)
            .unwrap()
            .slower_than_obs,
        1000
    );
    assert!(GameFrames::from_file_text("# nothing\n", 120).is_none());
}

#[test]
fn volumedetect_report() {
    let text = "[Parsed_volumedetect_0 @ 000] n_samples: 1234\n[Parsed_volumedetect_0 @ 000] mean_volume: -27.3 dB\n[Parsed_volumedetect_0 @ 000] max_volume: -4.0 dB\n";
    assert_eq!(parse_volumedetect(text), (Some(-27.3), Some(-4.0)));
    assert_eq!(parse_volumedetect("nothing"), (None, None));
}

fn file(repeated: u64, counted: u64) -> FileReport {
    FileReport {
        counted,
        repeated,
        has_audio: true,
        audio_peak_db: Some(-3.0),
        ..FileReport::default()
    }
}

fn recording(out_total: i64, skipped: i64, render_total: i64, missed: i64) -> RecordingStats {
    RecordingStats {
        connected: true,
        saw_recording: true,
        counters: BenchCounters {
            output_total: out_total,
            output_skipped: skipped,
            render_total,
            render_skipped: missed,
        },
        ..RecordingStats::default()
    }
}

#[test]
fn a_clean_clip_is_steady() {
    let v = judge(&recording(2400, 0, 2400, 0), &file(5, 2400));
    assert_eq!(
        (v.level, v.cause, v.sound),
        (Level::Steady, Cause::None, Sound::Ok)
    );
    assert!(v.counters_read);
}

#[test]
fn an_encoder_that_cant_keep_up_is_not_steady() {
    // 240 fps at 1440p on the old encoder settings (#645): 95% skipped.
    let v = judge(&recording(4800, 4560, 4800, 0), &file(0, 240));
    assert_eq!((v.level, v.cause), (Level::NotSteady, Cause::Encoder));
}

#[test]
fn rendering_lag_is_told_apart_from_encoding_lag() {
    let v = judge(&recording(2400, 0, 2400, 60), &file(0, 2400));
    assert_eq!((v.level, v.cause), (Level::Borderline, Cause::Rendering));
}

#[test]
fn repeats_the_counters_miss_are_the_games() {
    // fps_max at OBS's rate: 1.5-2.1% repeated, nothing skipped (#645).
    let v = judge(&recording(2400, 0, 2400, 0), &file(45, 2400));
    assert_eq!((v.level, v.cause), (Level::Borderline, Cause::Game));
    let v = judge(&recording(2400, 0, 2400, 0), &file(200, 2400));
    assert_eq!((v.level, v.cause), (Level::NotSteady, Cause::Game));
}

#[test]
fn the_cause_is_the_measure_furthest_past_steady() {
    // 3% skipped (6x its steady share) beats 4% repeated (4x).
    let v = judge(&recording(1000, 30, 1000, 0), &file(40, 1000));
    assert_eq!(v.cause, Cause::Encoder);
}

#[test]
fn without_obs_counters_the_file_decides() {
    let v = judge(&RecordingStats::default(), &file(0, 2400));
    assert_eq!(v.level, Level::Steady);
    assert!(!v.counters_read);
}

#[test]
fn sound_is_checked() {
    let mut f = file(0, 100);
    f.has_audio = false;
    assert_eq!(judge(&recording(1, 0, 1, 0), &f).sound, Sound::Missing);
    let mut f = file(0, 100);
    f.audio_peak_db = Some(-91.0);
    assert_eq!(judge(&recording(1, 0, 1, 0), &f).sound, Sound::Silent);
    f.audio_peak_db = None;
    assert_eq!(judge(&recording(1, 0, 1, 0), &f).sound, Sound::Silent);
}

fn streak(kill_times: &[f32]) -> CaptureStreak {
    CaptureStreak {
        start_tick: 0,
        end_tick: 0,
        source_demo: String::new(),
        target_player: None,
        kill_count: kill_times.len(),
        timeline_string: String::new(),
        duration_string: String::new(),
        player_index: 0,
        kills: kill_times
            .iter()
            .enumerate()
            .map(|(n, t)| ((*t * 100.0) as i32 + n as i32, *t, "Garand".to_string()))
            .collect(),
        start_index: 0,
        end_index: kill_times.len().saturating_sub(1),
        total_demo_frames: 0,
        demo_fps: 100.0,
        viewdemo_times: Vec::new(),
        frame_times: Arc::new(Vec::new()),
        status: Default::default(),
        match_start_tick: None,
        source_key: None,
        spectated: false,
        steam_id: None,
        recorded_this_demo: true,
    }
}

#[test]
fn the_clip_has_the_most_kills_that_fit() {
    // ci_fixture.dem's highlights: 4 Garand kills over 78 s, then two
    // 2-kill ones, in a 308 s demo.
    let streaks = [
        streak(&[19.86, 58.68, 67.57, 97.92]),
        streak(&[196.81, 216.68]),
        streak(&[252.81, 292.55]),
    ];
    let c = pick_clip(&streaks, 20.0, 308.0).unwrap();
    assert_eq!((c.streak.start_index, c.streak.end_index), (1, 2));
    assert_eq!(c.streak.kill_count, 2);
    let span = 67.57 - 58.68;
    assert!((c.pre_roll - (20.0 - span) / 2.0).abs() < 1e-4);
    assert_eq!(c.pre_roll, c.post_roll);
    // A minute takes three of the first four.
    let c = pick_clip(&streaks, 60.0, 308.0).unwrap();
    assert_eq!((c.streak.start_index, c.streak.end_index), (1, 3));
}

#[test]
fn a_fixed_stretch_is_recorded_as_given() {
    let mut s = streak(&[58.68, 67.57, 97.92]);
    s.viewdemo_times = vec![362.19, 371.09, 401.44];
    let streaks = [s];
    // 355..375 on the demo player's clock holds the first two kills.
    let c = pick_clip_in(&streaks, 355.0, 375.0, 20.0).unwrap();
    assert_eq!((c.streak.start_index, c.streak.end_index), (0, 1));
    assert!((c.pre_roll - 7.19).abs() < 1e-3);
    assert!((c.post_roll - 3.91).abs() < 1e-3);
    // A minute widens it evenly to 335..395.
    let c = pick_clip_in(&streaks, 355.0, 375.0, 60.0).unwrap();
    assert!((c.pre_roll - 27.19).abs() < 1e-3);
    assert!((c.post_roll - 23.91).abs() < 1e-3);
    // No kill inside: nothing to build the clip on.
    assert!(pick_clip_in(&streaks, 380.0, 395.0, 15.0).is_none());
}

#[test]
fn a_clip_stays_inside_the_demo() {
    // The only kill is 3 s in: 10 s of roll before it doesn't fit.
    assert!(pick_clip(&[streak(&[3.0])], 20.0, 300.0).is_none());
    let c = pick_clip(&[streak(&[3.0]), streak(&[100.0])], 20.0, 300.0).unwrap();
    assert_eq!(c.streak.kills[c.streak.start_index].1, 100.0);
    assert_eq!(c.pre_roll, 10.0);
    assert!(pick_clip(&[streak(&[100.0])], 1.0, 300.0).is_none());
}
