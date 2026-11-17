use super::*;
use crate::obs::log_tail::parse_marker;
use std::cell::Cell;

/// Done folders whose answers are set by the test, counting each read.
#[derive(Default)]
struct Folders {
    done: bool,
    trigger: bool,
    reads: Cell<u32>,
}

impl DoneFolders for Folders {
    fn done_marker(&self) -> bool {
        self.reads.set(self.reads.get() + 1);
        self.done
    }
    fn exit_trigger(&self) -> bool {
        self.reads.set(self.reads.get() + 1);
        self.trigger
    }
}

fn ago(secs: u64) -> Instant {
    Instant::now() - Duration::from_secs(secs)
}

/// An error box closes even a game the player had open, and then the
/// message says DoD Studio closed it; a launched game's message is the
/// box's summary alone (#1231).
#[test]
fn an_error_box_in_a_borrowed_game_says_the_game_was_closed() {
    let summary = "The game stopped on one of its own internal checks.";
    assert_eq!(dialog_failure_reason(summary, false), summary);
    let borrowed = dialog_failure_reason(summary, true);
    assert!(borrowed.starts_with(summary), "{borrowed}");
    assert!(
        borrowed.contains("closed the game you had open"),
        "{borrowed}"
    );
}

/// A batch that has shown nothing unusual gets the floor. Five minutes is
/// the number that has to clear an unfocused game's stalled fast-forward.
#[test]
fn quiet_batch_waits_the_floor() {
    assert_eq!(marker_stall_deadline(Duration::ZERO), MARKER_STALL_FLOOR);
    assert_eq!(
        marker_stall_deadline(Duration::from_secs(30)),
        MARKER_STALL_FLOOR
    );
}

/// A demo whose breadcrumbs are genuinely further apart than the floor
/// must not be killed for being slow — it has already demonstrated the gap
/// is legitimate.
#[test]
fn adapts_to_a_batch_with_long_legitimate_gaps() {
    let observed = Duration::from_secs(240);
    assert_eq!(
        marker_stall_deadline(observed),
        Duration::from_secs(720),
        "three times the longest gap already survived"
    );
}

/// The adaptive term only ever widens the window. Narrowing it would make
/// a fast batch trip on its first slow stretch.
#[test]
fn never_shortens_below_the_floor() {
    for secs in [0, 1, 10, 99, 100, 101] {
        assert!(marker_stall_deadline(Duration::from_secs(secs)) >= MARKER_STALL_FLOOR);
    }
}

#[test]
fn a_batch_end_names_the_pipe_whenever_its_marker_arrived() {
    // Marker and exit-trigger folder both there by the same check.
    assert_eq!(
        completion_source(true, false, false, true),
        "BATCH_COMPLETE from the game events pipe"
    );
    assert_eq!(
        completion_source(false, true, false, false),
        "BATCH_COMPLETE marker"
    );
    // No pipe (an old hook DLL, or GOLDSRC_HOOKS_EVENTS=0): the folder.
    assert_eq!(
        completion_source(false, false, false, true),
        "done marker: false, exit trigger: true"
    );
}

#[test]
fn obs_mode_polls_every_16_ms_and_the_rest_every_half_second() {
    assert_eq!(poll_interval(true), Duration::from_millis(16));
    assert_eq!(poll_interval(false), Duration::from_millis(500));
}

#[test]
fn a_check_is_due_the_first_time_and_then_once_its_interval_passed() {
    let every = Duration::from_secs(1);
    assert!(is_due(None, every));
    assert!(!is_due(Some(Instant::now()), every));
    assert!(is_due(Some(ago(1)), every));
}

#[test]
fn only_batch_complete_markers_say_the_batch_is_done() {
    let marker = |line: &str, via_pipe: bool| {
        let mut m = parse_marker(line).unwrap();
        m.via_pipe = via_pipe;
        m
    };
    let done_log = marker("[dod-studio] BATCH_COMPLETE", false);
    let done_pipe = marker("[dod-studio] BATCH_COMPLETE", true);
    let other = marker("[dod-studio] STOP_RECORD - Tick 5", true);

    let mut seen = CompleteMarkers::default();
    seen.on_marker(&other, true);
    seen.on_marker(&done_log, false);
    assert_eq!(
        seen,
        CompleteMarkers::default(),
        "the log's, outside OBS mode"
    );
    seen.on_marker(&done_log, true);
    assert_eq!(
        seen,
        CompleteMarkers {
            obs: true,
            pipe: false
        }
    );

    let mut seen = CompleteMarkers::default();
    seen.on_marker(&done_pipe, false);
    assert_eq!(
        seen,
        CompleteMarkers {
            obs: false,
            pipe: true
        }
    );
    seen.on_marker(&done_pipe, true);
    assert_eq!(
        seen,
        CompleteMarkers {
            obs: true,
            pipe: true
        }
    );
}

/// The longest gap between markers widens the stall deadline, except a
/// gap that spans a hold; a `go` restarts the count.
#[test]
fn the_marker_clock_skips_gaps_that_span_a_hold() {
    let t0 = ago(100);
    let mut clock = MarkerClock::default();
    clock.on_marker(t0, false);
    assert_eq!(clock.longest_gap, Duration::ZERO, "nothing to measure from");
    clock.on_marker(t0 + Duration::from_secs(10), false);
    assert_eq!(clock.longest_gap, Duration::from_secs(10));
    clock.on_marker(t0 + Duration::from_secs(70), true);
    assert_eq!(clock.longest_gap, Duration::from_secs(10), "held");
    assert_eq!(clock.last_marker_at, Some(t0 + Duration::from_secs(70)));
    clock.on_go(t0 + Duration::from_secs(90));
    assert_eq!(clock.last_marker_at, Some(t0 + Duration::from_secs(90)));
    clock.on_marker(t0 + Duration::from_secs(95), false);
    assert_eq!(clock.longest_gap, Duration::from_secs(10));
}

#[test]
fn a_stall_mid_batch_is_only_called_in_obs_mode_with_the_game_up_and_not_held() {
    let quiet = MarkerClock {
        last_marker_at: Some(ago(301)),
        longest_gap: Duration::from_secs(5),
    };
    let stall = stalled(true, false, &quiet, Some(ago(400)), true).expect("stalled");
    assert!(!stall.never_started);
    assert_eq!(Some(stall.since), quiet.last_marker_at);
    assert_eq!(stalled(false, false, &quiet, Some(ago(400)), true), None);
    assert_eq!(stalled(true, true, &quiet, Some(ago(400)), true), None);
    assert_eq!(stalled(true, false, &quiet, Some(ago(400)), false), None);

    // Not yet past the deadline, which a long gap already seen widens.
    let recent = MarkerClock {
        last_marker_at: Some(ago(200)),
        longest_gap: Duration::ZERO,
    };
    assert_eq!(stalled(true, false, &recent, Some(ago(400)), true), None);
    let slow_demo = MarkerClock {
        last_marker_at: Some(ago(400)),
        longest_gap: Duration::from_secs(200),
    };
    assert_eq!(stalled(true, false, &slow_demo, Some(ago(500)), true), None);
}

/// No marker at all counts from hl.exe coming up, and says the demo
/// never started; before hl.exe is seen there is nothing to count from.
#[test]
fn a_batch_with_no_markers_counts_from_the_game_coming_up() {
    let none = MarkerClock::default();
    let stall = stalled(true, false, &none, Some(ago(301)), true).expect("stalled");
    assert!(stall.never_started);
    assert_eq!(stalled(true, false, &none, None, true), None);
    assert_eq!(stalled(true, false, &none, Some(ago(10)), true), None);
}

/// Outside OBS mode only a batch with no marker at all is called
/// stalled, once the game has been up for `FIRST_MARKER_DEADLINE`
/// (#1214); not while held, nor with the game gone.
#[test]
fn outside_obs_mode_only_a_demo_that_never_started_is_a_stall() {
    let none = MarkerClock::default();
    let over = FIRST_MARKER_DEADLINE.as_secs() + 1;
    let stall = stalled(false, false, &none, Some(ago(over)), true).expect("stalled");
    assert!(stall.never_started);
    let under = FIRST_MARKER_DEADLINE.as_secs() - 1;
    assert_eq!(stalled(false, false, &none, Some(ago(under)), true), None);
    assert_eq!(stalled(false, false, &none, None, true), None);
    assert_eq!(stalled(false, true, &none, Some(ago(over)), true), None);
    assert_eq!(stalled(false, false, &none, Some(ago(over)), false), None);
    let started = MarkerClock {
        last_marker_at: Some(ago(3600)),
        longest_gap: Duration::ZERO,
    };
    assert_eq!(stalled(false, false, &started, Some(ago(4000)), true), None);
}

#[test]
fn a_stall_says_whether_the_demo_ever_started() {
    let (line, reason) = stall_texts(Duration::from_secs(301), true);
    assert_eq!(
        line,
        "[HLAE] No console markers for 5 min 1 s with hl.exe still running — treating the batch as stalled. Playback never produced a single marker, so the demo most likely failed to load."
    );
    assert!(
        reason.starts_with("the demo never started: the game came up, but no demo began"),
        "{reason}"
    );
    assert!(reason.contains("hook DLL is up to date"), "{reason}");
    assert!(
        reason.contains("demo file is still where Studio found it"),
        "{reason}"
    );
    let (line, reason) = stall_texts(Duration::from_secs(30), false);
    assert!(line.starts_with("[HLAE] No console markers for 30.0 s with hl.exe still running"));
    assert!(line.ends_with("a frozen engine all look like this from out here."));
    assert!(reason.starts_with("the batch stopped progressing"));
}

/// Done needs 11 whole seconds, then any one of the four signs; the
/// folders aren't read before then, nor after the first one found.
#[test]
fn a_batch_is_complete_after_its_first_ten_seconds_by_any_sign() {
    let folders = Folders {
        done: true,
        ..Default::default()
    };
    let none = CompleteMarkers::default();
    assert!(!batch_complete(
        Duration::from_millis(10_999),
        none,
        &folders
    ));
    assert_eq!(folders.reads.get(), 0, "too early to look");
    assert!(batch_complete(Duration::from_secs(11), none, &folders));
    assert_eq!(folders.reads.get(), 1, "the done marker was enough");

    let trigger = Folders {
        trigger: true,
        ..Default::default()
    };
    assert!(batch_complete(Duration::from_secs(11), none, &trigger));
    let nothing = Folders::default();
    assert!(!batch_complete(Duration::from_secs(60), none, &nothing));
    let obs = CompleteMarkers {
        obs: true,
        pipe: false,
    };
    let pipe = CompleteMarkers {
        obs: false,
        pipe: true,
    };
    assert!(batch_complete(Duration::from_secs(11), obs, &nothing));
    assert!(batch_complete(Duration::from_secs(11), pipe, &nothing));
}

#[test]
fn the_game_never_started_once_the_launcher_left_and_5_s_passed() {
    let nothing = Folders::default();
    let after = Duration::from_secs(6);
    assert!(game_never_started(true, false, after, &nothing));
    assert!(
        !game_never_started(false, false, after, &nothing),
        "launcher still up"
    );
    assert!(
        !game_never_started(true, true, after, &nothing),
        "the game was seen"
    );
    assert!(!game_never_started(
        true,
        false,
        Duration::from_millis(5_999),
        &nothing
    ));
    let done = Folders {
        done: true,
        ..Default::default()
    };
    assert!(!game_never_started(true, false, after, &done));
    let trigger = Folders {
        trigger: true,
        ..Default::default()
    };
    assert!(!game_never_started(true, false, after, &trigger));
}

#[test]
fn the_game_went_away_when_it_was_seen_and_nothing_says_done() {
    let nothing = Folders::default();
    let none = CompleteMarkers::default();
    assert!(game_went_away(true, false, none, &nothing));
    assert!(!game_went_away(false, false, none, &nothing), "never seen");
    assert!(!game_went_away(true, true, none, &nothing), "still up");
    assert_eq!(
        nothing.reads.get(),
        2,
        "read only on the one that got that far"
    );
    let pipe = CompleteMarkers {
        obs: false,
        pipe: true,
    };
    assert!(!game_went_away(true, false, pipe, &nothing));
    // The OBS-mode marker alone doesn't count here.
    let obs = CompleteMarkers {
        obs: true,
        pipe: false,
    };
    assert!(game_went_away(true, false, obs, &nothing));
    let done = Folders {
        done: true,
        ..Default::default()
    };
    assert!(!game_went_away(true, false, none, &done));
    let trigger = Folders {
        trigger: true,
        ..Default::default()
    };
    assert!(!game_went_away(true, false, none, &trigger));
}

#[test]
fn a_hold_is_sent_when_it_changes_and_retried_a_second_after_a_failure() {
    assert!(hold_send_due(true, true, false, None));
    assert!(hold_send_due(true, false, true, None));
    assert!(!hold_send_due(true, true, true, None), "already told");
    assert!(
        !hold_send_due(false, true, false, None),
        "a patched batch can't hold"
    );
    assert!(!hold_send_due(true, true, false, Some(Instant::now())));
    assert!(hold_send_due(true, true, false, Some(ago(1))));
}

#[test]
fn the_console_log_warning_names_the_file_and_the_grace() {
    let text = console_log_not_written_text(Path::new("C:/Games/qconsole.log"));
    assert!(text.starts_with(
        "⚠️ **The engine is not writing its console log.** `C:/Games/qconsole.log` is unchanged 1 min 0 s after the game started"
    ));
    assert!(text.ends_with("or HLAE may not be passing the flag through to the game."));
}
