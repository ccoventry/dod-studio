use super::*;

#[test]
fn seconds_parse_as_numbers() {
    assert_eq!(parse_time("610"), Ok(610.0));
    assert_eq!(parse_time("1234.5"), Ok(1234.5));
    assert_eq!(parse_time(" -30 "), Ok(-30.0));
    assert!(parse_time("inf").is_err());
    assert!(parse_time("NaN").is_err());
    assert!(parse_time("").is_err());
}

#[test]
fn minutes_and_hours_parse_as_the_demo_bar_shows_them() {
    assert_eq!(parse_time("10:10"), Ok(610.0));
    assert_eq!(parse_time("0:30"), Ok(30.0));
    assert_eq!(parse_time("-0:30"), Ok(-30.0));
    assert_eq!(parse_time("75:00"), Ok(4500.0));
    assert_eq!(parse_time("10:05.5"), Ok(605.5));
    assert_eq!(parse_time("1:02:03"), Ok(3723.0));
    assert_eq!(parse_time("0:00:05"), Ok(5.0));
}

#[test]
fn malformed_times_are_refused_with_the_forms_expected() {
    for bad in [
        "10:", ":10", "10:60", "1:60:00", "1:2:3:4", "a:10", "10:b", "1e1:00", "10:1e1", "10:-5",
        "--1:00", "+1:00", "10::10", "10:05.", "1.5:00",
    ] {
        let err = parse_time(bad).expect_err(bad);
        assert!(err.contains("m:ss (10:10)"), "{bad}: {err}");
    }
}

#[test]
fn the_console_s_split_at_colons_is_joined_back() {
    let words = |w: &[&str]| w.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let unquoted = words(&["10", ":", "10"]);
    assert_eq!(split_time(&unquoted), Some(("10:10".to_string(), &[][..])));
    let hours = words(&["1", ":", "02", ":", "03", "ds_spec_target", "13"]);
    let (time, rest) = split_time(&hours).unwrap();
    assert_eq!(time, "1:02:03");
    assert_eq!(follow_up(rest), Some("ds_spec_target 13".to_string()));
    let quoted = words(&["10:10"]);
    assert_eq!(split_time(&quoted).unwrap().0, "10:10");
    let plain = words(&["610", "ds_spec_target", "13"]);
    assert_eq!(split_time(&plain).unwrap().0, "610");
    // A trailing colon has nothing to join: it is left for the parser.
    let dangling = words(&["10", ":"]);
    assert_eq!(split_time(&dangling).unwrap().0, "10");
    assert_eq!(split_time(&[]), None);
}

#[test]
fn times_print_as_the_demo_bar_shows_them() {
    assert_eq!(clock_text(605.0), "10:05");
    assert_eq!(clock_text(-30.0), "-0:30");
    assert_eq!(clock_text(605.5), "10:05.50");
    assert_eq!(clock_text(3723.0), "1:02:03");
    assert_eq!(clock_text(59.999), "1:00");
}

fn look(now: f64, loading: bool) -> Option<Look> {
    Some(Look {
        active: true,
        loading,
        paused: false,
        now,
        scale: 1.0,
        before_frame: false,
    })
}

#[test]
fn a_queued_seek_waits_for_the_load_then_fires() {
    let mut w = Watch::default();
    // The player not up yet, then loading while it plays.
    assert_eq!(judge(&mut w, None, 0.01, 0.0), Verdict::Wait);
    assert_eq!(judge(&mut w, look(726.0, true), 0.01, 0.1), Verdict::Wait);
    assert_eq!(judge(&mut w, look(726.02, true), 0.02, 0.2), Verdict::Wait);
    // A long frame lets the clock run on further.
    assert_eq!(judge(&mut w, look(727.5, true), 1.0, 1.2), Verdict::Wait);
    assert_eq!(judge(&mut w, look(727.52, false), 0.02, 1.3), Verdict::Fire);
}

#[test]
fn a_queued_seek_is_cancelled_by_what_the_user_does() {
    // Settled: the first frames' jumps are let through.
    let loading = |w: &mut Watch| {
        assert_eq!(judge(w, look(368.5, true), 0.01, 0.0), Verdict::Wait);
        assert_eq!(judge(w, look(375.4, true), 0.01, 0.0), Verdict::Wait);
        for _ in 0..SETTLE_FRAMES {
            assert_eq!(judge(w, look(100.0, true), 0.01, 0.0), Verdict::Wait);
        }
    };
    let mut w = Watch::default();
    loading(&mut w);
    let paused = look(100.01, true).map(|l| Look { paused: true, ..l });
    assert_eq!(
        judge(&mut w, paused, 0.01, 0.1),
        Verdict::Cancel("you paused")
    );

    let mut w = Watch::default();
    loading(&mut w);
    let faster = look(100.01, true).map(|l| Look { scale: 2.0, ..l });
    assert_eq!(
        judge(&mut w, faster, 0.01, 0.1),
        Verdict::Cancel("you changed the speed")
    );

    let mut w = Watch::default();
    loading(&mut w);
    assert_eq!(
        judge(&mut w, look(40.0, true), 0.01, 0.1),
        Verdict::Cancel("you moved the demo's clock")
    );

    let mut w = Watch::default();
    loading(&mut w);
    assert_eq!(
        judge(&mut w, look(160.0, true), 0.01, 0.1),
        Verdict::Cancel("you moved the demo's clock")
    );

    let mut w = Watch::default();
    loading(&mut w);
    assert_eq!(
        judge(&mut w, None, 0.01, 0.1),
        Verdict::Cancel("you closed the demo")
    );
}

/// The player's own gap jump and `skip_gap` land just before a frame;
/// neither is the user.
#[test]
fn a_gap_jump_does_not_cancel_a_queued_seek() {
    let mut w = Watch::default();
    for _ in 0..=SETTLE_FRAMES {
        assert_eq!(judge(&mut w, look(8.5, true), 0.01, 0.0), Verdict::Wait);
    }
    let jumped = look(71.378, true).map(|l| Look {
        before_frame: true,
        ..l
    });
    assert_eq!(judge(&mut w, jumped, 0.01, 0.1), Verdict::Wait);
}

#[test]
fn a_demo_that_never_starts_or_loads_unseen_is_not_waited_on_forever() {
    let mut w = Watch::default();
    assert_eq!(judge(&mut w, None, 0.01, 10.0), Verdict::Wait);
    assert_eq!(
        judge(&mut w, None, 0.01, GIVE_UP_SECONDS + 1.0),
        Verdict::Cancel("the demo never started")
    );
    // Loaded between two looks: never seen loading, then fires.
    let mut w = Watch::default();
    for _ in 0..UNSEEN_LOAD_FRAMES {
        assert_eq!(judge(&mut w, look(50.0, false), 0.01, 0.1), Verdict::Wait);
    }
    assert_eq!(judge(&mut w, look(50.0, false), 0.01, 3.0), Verdict::Fire);
}

#[test]
fn a_seek_lands_inside_what_is_loaded() {
    assert_eq!(landing(100.0, 250.0, false, 50.0, 900.0), 250.0);
    assert_eq!(landing(100.0, -30.0, true, 50.0, 900.0), 70.0);
    assert_eq!(landing(100.0, 5000.0, false, 50.0, 900.0), 900.0);
    assert_eq!(landing(100.0, -500.0, true, 50.0, 900.0), 50.0);
    // An empty world reports 0..0 or worse; leave the engine to clamp.
    assert_eq!(landing(0.0, 42.0, false, 0.0, -1.0), 42.0);
}

/// The mark sits one frame before the landing, so `ReadDemoMessage`'s
/// "run every frame after the last one sent" runs the landing frame alone.
#[test]
fn a_clean_landing_runs_the_landing_frame_and_nothing_before_it() {
    assert_eq!(skip_mark(100, 5000), Some(4999));
    assert_eq!(skip_mark(5000, 100), Some(99));
    assert_eq!(skip_mark(0, 1), Some(0));
    assert_eq!(skip_mark(5000, 5000), None);
}

/// A long forward seek moves the clock one step per frame, then lands on
/// the target exactly, however the clock ran in between.
#[test]
fn a_long_forward_seek_steps_then_lands() {
    assert_eq!(next_step(90.0, 1335.3), Step::To(95.0));
    assert_eq!(next_step(1331.0, 1335.3), Step::Land(1335.3));
    assert_eq!(next_step(1335.3, 1335.3), Step::Land(1335.3));
    // The clock ran past the target while stepping: land anyway.
    assert_eq!(next_step(1336.0, 1335.3), Step::Land(1335.3));
    let (mut now, mut frames) = (90.1, 0);
    while let Step::To(t) = next_step(now, 1335.3) {
        now = t + 0.01; // the clock also runs a little each frame
        frames += 1;
    }
    assert_eq!(frames, 248);
}

/// The player records the clock it sent at, which has run on a little
/// past what the step set.
#[test]
fn a_step_counts_as_sent_once_the_player_sent_at_or_after_it() {
    assert!(sent(95.004, 95.0));
    assert!(sent(95.0, 95.0));
    assert!(!sent(90.1, 95.0));
}

#[test]
fn only_long_forward_seeks_without_the_skip_go_in_steps() {
    assert!(stepped(90.0, 1335.3, false));
    assert!(!stepped(90.0, 94.0, false), "a short hop fits in one frame");
    assert!(
        !stepped(1335.3, 90.0, false),
        "backward runs nothing in between"
    );
    assert!(
        !stepped(90.0, 1335.3, true),
        "the skip jumps at once on purpose"
    );
}

/// The player records the clock it sent at: once the start frame is out,
/// it is far below where playback was.
#[test]
fn a_rewind_counts_as_sent_once_the_clock_it_sent_at_falls() {
    assert!(rewound(0.02, 1000.0));
    assert!(
        !rewound(1000.0, 1000.0),
        "still the frame before the rewind"
    );
    assert!(
        !rewound(1000.3, 1000.0),
        "a frame sent before the rewind took"
    );
}

/// A review moving on to another demo mid-rewind: the new demo gets none
/// of the old seek's steps or follow-up.
#[test]
fn loading_another_demo_drops_a_seek_still_on_its_way() {
    let mut seek = Pending::new(939.0, Some("spec_mode 4".into()), 300.0);
    seek.steps = 60;
    *PENDING.lock().unwrap() = Some(seek);
    assert_eq!(cancel_pending(), Some(939.0));
    assert!(!seeking());
    assert_eq!(cancel_pending(), None);
}

/// The float of #893 on `ktps8w1-m00cat_soul_lenn_h2` (Anniversary): the
/// clock at 8.52 s, the first playback frame (71.388 s) already sent.
#[test]
fn a_clock_stuck_behind_a_sent_frame_jumps_to_just_before_it() {
    let to = gap_landing(8.522, 71.388, 3, 3).expect("a gap");
    assert!((to - 71.378).abs() < 1e-3, "{to}");
    // Not sent yet: the player's own check still has its chance.
    assert_eq!(gap_landing(8.522, 71.388, 3, 2), None);
    // Normal playback: the next frame is a few milliseconds on.
    assert_eq!(gap_landing(226.663, 226.664, 14075, 14075), None);
    // A short gap the player plays through.
    assert_eq!(gap_landing(100.0, 101.5, 7, 7), None);
}

#[test]
fn the_words_after_the_time_are_one_command() {
    let words = |w: &[&str]| w.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(follow_up(&words(&[])), None);
    assert_eq!(
        follow_up(&words(&["ds_spec_target", "13"])),
        Some("ds_spec_target 13".to_string())
    );
}

/// No name may be the whole start of another: the console's autocomplete
/// swaps the shorter one for the longer when space is pressed.
#[test]
fn no_name_is_the_start_of_another() {
    let names = [SEEK_TO_NAME, SEEK_BY_NAME, SKIP_BETWEEN_NAME];
    for a in names {
        for b in names {
            assert!(a == b || !b.starts_with(a), "{a} is the start of {b}");
        }
    }
}

#[test]
fn the_builds_are_told_apart() {
    assert_ne!(BUILDS[0].time_date_stamp, BUILDS[1].time_date_stamp);
}
