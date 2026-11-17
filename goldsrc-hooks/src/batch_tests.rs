use super::*;

fn file(lines: &[&str]) -> String {
    std::iter::once(HEADER)
        .chain(lines.iter().copied())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn a_batch_file_parses_into_sorted_demo_sections() {
    let plan = parse(&file(&[
        "// two demos",
        "demo\tC:/demos/a b.dem",
        "init\tmirv_movie_fps 60",
        "at\t310.5\techo late",
        "seek\t300\t305",
        "at\t300\techo first",
        "next\t320",
        "",
        "demo\tC:/demos/c.dem",
        "at\t10\techo c",
        "end\tsys_capture_done_path",
    ]))
    .unwrap();
    assert_eq!(plan.demos.len(), 2);
    let a = &plan.demos[0];
    assert_eq!(a.demo, "C:/demos/a b.dem");
    assert_eq!(a.init, ["mirv_movie_fps 60"]);
    let got: Vec<_> = a.entries.iter().map(|e| (e.at, e.action.clone())).collect();
    assert_eq!(
        got,
        [
            (300.0, Action::Seek(305.0)),
            (300.0, Action::Run("echo first".into())),
            (310.5, Action::Run("echo late".into())),
            (320.0, Action::Next),
        ]
    );
    assert_eq!(plan.end, ["sys_capture_done_path"]);
}

#[test]
fn a_bad_batch_file_says_which_line() {
    assert!(parse("hello").unwrap_err().contains("not a batch file"));
    assert!(parse(HEADER).unwrap_err().contains("no demos"));
    let e = parse(&file(&["at\t1\techo"])).unwrap_err();
    assert!(e.contains("line 2") && e.contains("before any"), "{e}");
    let e = parse(&file(&["demo\tx.dem", "at\tsoon\techo"])).unwrap_err();
    assert!(e.contains("line 3") && e.contains("not a time"), "{e}");
    let e = parse(&file(&["demo\tx.dem", "at\t1"])).unwrap_err();
    assert!(e.contains("needs 2"), "{e}");
    let e = parse(&file(&["demo\tx.dem", "jump\t1"])).unwrap_err();
    assert!(e.contains("unknown line"), "{e}");
    let e = parse(&file(&["demo\tx.dem", "seek\t5\t-1"])).unwrap_err();
    assert!(e.contains("0 or more"), "{e}");
}

fn entries(text: &[&str]) -> Vec<Entry> {
    let mut lines = vec!["demo\tx.dem"];
    lines.extend_from_slice(text);
    parse(&file(&lines)).unwrap().demos.remove(0).entries
}

#[test]
fn commands_run_once_in_order_and_a_seek_comes_after_them() {
    let e = entries(&[
        "at\t1\ta",
        "seek\t5\t20",
        "at\t5\tb",
        "at\t21\tc",
        "next\t30",
    ]);
    let mut cursor = Cursor { next: 0 };
    assert_eq!(cursor.due(&e, 0.5), (vec![], Then::Play));
    assert_eq!(cursor.due(&e, 1.0), (vec!["a".to_string()], Then::Play));
    assert_eq!(cursor.due(&e, 1.5), (vec![], Then::Play));
    // The command at the seek's own time still runs, before the seek.
    assert_eq!(
        cursor.due(&e, 5.0),
        (vec!["b".to_string()], Then::Seek(20.0))
    );
    assert_eq!(cursor.due(&e, 20.0), (vec![], Then::Play));
    assert_eq!(cursor.due(&e, 21.0), (vec!["c".to_string()], Then::Play));
    assert_eq!(cursor.due(&e, 31.0), (vec![], Then::Next));
}

#[test]
fn a_seek_that_is_not_ahead_plays_on() {
    let e = entries(&["seek\t5\t5.2", "seek\t6\t4", "next\t9"]);
    let mut cursor = Cursor { next: 0 };
    assert_eq!(cursor.due(&e, 5.0), (vec![], Then::Play));
    assert_eq!(cursor.due(&e, 6.0), (vec![], Then::Play));
}

#[test]
fn a_clock_that_goes_back_reruns_nothing() {
    let e = entries(&["at\t1\ta", "at\t2\tb", "next\t9"]);
    let mut cursor = Cursor { next: 0 };
    assert_eq!(cursor.due(&e, 2.0).0, ["a", "b"]);
    assert_eq!(cursor.due(&e, 0.5), (vec![], Then::Play));
    assert_eq!(cursor.due(&e, 2.0), (vec![], Then::Play));
}

/// An Original, then its Variant over the same stretch (#1085).
fn two_passes() -> Vec<Entry> {
    entries(&[
        "at\t10\toriginal start",
        "at\t17\toriginal stop",
        "back\t17.5\t5",
        "at\t17\tvariant stop",
        "at\t10\tvariant start",
        "next\t18",
    ])
}

#[test]
fn each_pass_sorts_on_its_own_and_back_ends_it() {
    let got: Vec<_> = two_passes().into_iter().map(|e| (e.at, e.action)).collect();
    assert_eq!(
        got,
        [
            (10.0, Action::Run("original start".into())),
            (17.0, Action::Run("original stop".into())),
            (17.5, Action::Back(5.0)),
            (10.0, Action::Run("variant start".into())),
            (17.0, Action::Run("variant stop".into())),
            (18.0, Action::Next),
        ]
    );
}

#[test]
fn a_back_goes_back_and_the_next_pass_runs_on_the_new_clock() {
    let e = two_passes();
    let mut cursor = Cursor { next: 0 };
    assert_eq!(cursor.due(&e, 10.0).0, ["original start"]);
    assert_eq!(cursor.due(&e, 17.0).0, ["original stop"]);
    // Every line of the next pass is due by this clock: none of them run.
    assert_eq!(cursor.due(&e, 17.6), (vec![], Then::Back(5.0)));
    // Landed at 5: the variant runs at its own times.
    assert_eq!(cursor.due(&e, 5.0), (vec![], Then::Play));
    assert_eq!(cursor.due(&e, 10.0).0, ["variant start"]);
    assert_eq!(cursor.due(&e, 17.0).0, ["variant stop"]);
    assert_eq!(cursor.due(&e, 18.0), (vec![], Then::Next));
}

#[test]
fn a_back_that_cannot_end_its_pass_is_refused() {
    let e = parse(&file(&["demo\tx.dem", "at\t20\tlate", "back\t17\t5"])).unwrap_err();
    assert!(e.contains("line 4") && e.contains("comes before"), "{e}");
    let e = parse(&file(&["demo\tx.dem", "next\t9", "back\t17\t5"])).unwrap_err();
    assert!(e.contains("line 4") && e.contains("never run"), "{e}");
    let e = parse(&file(&["demo\tx.dem", "back\t17"])).unwrap_err();
    assert!(e.contains("needs 2"), "{e}");
    // A `back` must land before its own time.
    let e = parse(&file(&["demo\tx.dem", "back\t17\t20"])).unwrap_err();
    assert!(e.contains("line 3") && e.contains("must go back"), "{e}");
    let e = parse(&file(&["demo\tx.dem", "back\t17\t17"])).unwrap_err();
    assert!(e.contains("must go back"), "{e}");
    // A `back` in another demo's section starts its own count.
    assert!(
        parse(&file(&[
            "demo\tx.dem",
            "next\t9",
            "demo\ty.dem",
            "back\t17\t5"
        ]))
        .is_ok()
    );
}

#[test]
fn a_back_is_a_clip_boundary() {
    assert!(holds_at(&Then::Back(5.0), &[], true));
    assert!(!holds_at(&Then::Back(5.0), &[], false));
}

#[test]
fn a_held_batch_gives_up_on_studio_only_after_it_has_been_gone_a_while() {
    assert!(!studio_gone(true, None), "Studio is reading");
    assert!(!studio_gone(true, Some(Duration::from_secs(2))), "a moment");
    assert!(studio_gone(true, Some(HELD_ALONE)));
    assert!(
        !studio_gone(false, Some(Duration::from_secs(600))),
        "no events pipe: no telling, so the hold stays"
    );
}

#[test]
fn a_hold_stops_only_at_a_clip_boundary() {
    let none: Vec<String> = vec![];
    let ff = vec!["sys_fast_forward".to_string()];
    assert!(holds_at(&Then::Seek(20.0), &none, true));
    assert!(holds_at(&Then::Next, &none, true));
    assert!(!holds_at(&Then::Play, &none, true), "mid-clip: plays on");
    assert!(
        !holds_at(&Then::Play, &["startmovie x".to_string()], true),
        "a clip's own lines: plays on"
    );
    assert!(!holds_at(&Then::Seek(20.0), &none, false));
    assert!(!holds_at(&Then::Next, &none, false));
    assert!(!holds_at(&Then::Play, &ff, false));
}

#[test]
fn a_gap_too_short_to_seek_over_is_a_boundary_too() {
    // Two clips close together: no seek, only the fast-forward between
    // them, after the first one's recording stopped.
    let ff = vec!["sys_fast_forward".to_string()];
    assert!(holds_at(&Then::Play, &ff, true));
}

#[test]
fn a_clip_waits_for_its_recording_until_studio_answers_leaves_or_times_out() {
    let a_moment = Duration::from_millis(80);
    assert_eq!(recording_wait(false, false, a_moment), RecordingWait::Wait);
    assert_eq!(recording_wait(true, false, a_moment), RecordingWait::Go);
    assert_eq!(
        recording_wait(true, true, RECORDING_WAIT_MAX),
        RecordingWait::Go,
        "a go-ahead that made it counts"
    );
    assert_eq!(
        recording_wait(false, true, a_moment),
        RecordingWait::StudioGone
    );
    assert_eq!(
        recording_wait(false, false, RECORDING_WAIT_MAX),
        RecordingWait::TimedOut
    );
}

#[test]
fn the_wait_line_is_the_batch_command_studio_writes() {
    assert_eq!(
        WAIT_FOR_RECORDING_LINE,
        "ds_capture_batch wait_for_recording"
    );
    let wait = format!("at\t4\t{WAIT_FOR_RECORDING_LINE}");
    let e = entries(&["at\t4\techo a", wait.as_str()]);
    let mut cursor = Cursor { next: 0 };
    let (lines, then) = cursor.due(&e, 4.0);
    assert_eq!(then, Then::Play);
    assert_eq!(lines, ["echo a", WAIT_FOR_RECORDING_LINE]);
}

#[test]
fn next_stops_the_frame_at_the_demo_end() {
    let e = entries(&["at\t1\ta", "next\t2", "at\t3\tafter"]);
    let mut cursor = Cursor { next: 0 };
    assert_eq!(cursor.due(&e, 5.0), (vec!["a".to_string()], Then::Next));
}
