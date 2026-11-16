use super::*;
use crate::test_support::Scratch;

fn attempt(stream: &str, started: u64, outcome: RenderOutcome, output: &str) -> RenderAttempt {
    RenderAttempt {
        started_unix_ms: started,
        stream: stream.to_string(),
        codec: "prores".to_string(),
        custom_codec_args: String::new(),
        fps: 120,
        outcome,
        output_path: output.to_string(),
        output_size_bytes: None,
        error: None,
    }
}

fn streams(names: &[&str]) -> Vec<String> {
    names.iter().map(|s| s.to_string()).collect()
}

const EVERY_FILE_EXISTS: fn(&str) -> bool = |_| true;

#[test]
fn the_setting_maps_to_what_happens_after_finish_and_before_a_job() {
    let policy = |after: &str, only| CleanupPolicy {
        after_finish: AfterFinish::from_setting(after),
        only_when_space_needed: only,
    };
    assert_eq!(policy("keep", false).on_finish(), None);
    assert_eq!(policy("keep", true).on_finish(), None);
    assert!(!policy("keep", true).makes_room());
    assert_eq!(
        policy("recycle", false).on_finish(),
        Some(AfterFinish::Recycle)
    );
    // The Recycle Bin frees nothing until emptied, so the option is ignored.
    assert_eq!(
        policy("recycle", true).on_finish(),
        Some(AfterFinish::Recycle)
    );
    assert!(!policy("recycle", true).makes_room());
    assert_eq!(
        policy("delete", false).on_finish(),
        Some(AfterFinish::Delete)
    );
    assert!(!policy("delete", false).makes_room());
    assert_eq!(policy("delete", true).on_finish(), None);
    assert!(policy("delete", true).makes_room());
}

#[test]
fn an_unknown_setting_value_keeps_takes() {
    assert_eq!(AfterFinish::from_setting(""), AfterFinish::Keep);
    assert_eq!(AfterFinish::from_setting("shred"), AfterFinish::Keep);
    assert_eq!(AfterFinish::from_setting("Delete"), AfterFinish::Keep);
}

#[test]
fn a_take_with_a_finished_render_on_disk_is_finished() {
    let history = vec![attempt("all", 10, RenderOutcome::Finished, "E:/out/a.mov")];
    assert_eq!(
        finished_at(&history, &streams(&["all"]), EVERY_FILE_EXISTS),
        Some(10)
    );
}

#[test]
fn never_rendered_failed_cancelled_and_unfinished_takes_are_not_finished() {
    let s = streams(&["all"]);
    assert_eq!(finished_at(&[], &s, EVERY_FILE_EXISTS), None);
    assert_eq!(finished_at(&[], &[], EVERY_FILE_EXISTS), None);
    for outcome in [
        RenderOutcome::Failed,
        RenderOutcome::Cancelled,
        RenderOutcome::Started,
    ] {
        let history = vec![attempt("all", 10, outcome, "E:/out/a.mov")];
        assert_eq!(
            finished_at(&history, &s, EVERY_FILE_EXISTS),
            None,
            "{outcome:?}"
        );
    }
}

#[test]
fn the_last_attempt_decides_not_an_earlier_finished_one() {
    let s = streams(&["all"]);
    let history = vec![
        attempt("all", 10, RenderOutcome::Finished, "E:/out/a.mov"),
        attempt("all", 20, RenderOutcome::Cancelled, ""),
    ];
    assert_eq!(finished_at(&history, &s, EVERY_FILE_EXISTS), None);
    // Order in the file doesn't matter, start time does.
    let history = vec![
        attempt("all", 30, RenderOutcome::Finished, "E:/out/b.mov"),
        attempt("all", 20, RenderOutcome::Failed, ""),
    ];
    assert_eq!(finished_at(&history, &s, EVERY_FILE_EXISTS), Some(30));
}

#[test]
fn a_take_whose_clip_file_is_gone_is_not_finished() {
    let history = vec![attempt("all", 10, RenderOutcome::Finished, "E:/out/a.mov")];
    assert_eq!(finished_at(&history, &streams(&["all"]), |_| false), None);
    let no_path = vec![attempt("all", 10, RenderOutcome::Finished, "")];
    assert_eq!(
        finished_at(&no_path, &streams(&["all"]), EVERY_FILE_EXISTS),
        None
    );
}

#[test]
fn a_hud_pair_needs_both_halves_finished() {
    let s = streams(&["all", "hud:hudcolor"]);
    let one = vec![attempt("all", 10, RenderOutcome::Finished, "E:/out/a.mov")];
    assert_eq!(finished_at(&one, &s, EVERY_FILE_EXISTS), None);
    let both = vec![
        attempt("all", 10, RenderOutcome::Finished, "E:/out/a.mov"),
        attempt(
            "hud:hudcolor",
            12,
            RenderOutcome::Finished,
            "E:/out/a_hud.mov",
        ),
    ];
    assert_eq!(finished_at(&both, &s, EVERY_FILE_EXISTS), Some(12));
}

#[test]
fn folder_keys_ignore_case_slashes_and_a_trailing_separator() {
    assert_eq!(
        folder_key("D:/Captures/S1/b0/take0000/"),
        folder_key("d:\\captures\\s1\\b0\\take0000")
    );
}

fn drives(list: &[(&str, u64)]) -> Vec<(String, u64)> {
    list.iter().map(|(v, f)| (v.to_string(), *f)).collect()
}

fn vols(list: &[&str]) -> Vec<String> {
    list.iter().map(|v| v.to_string()).collect()
}

#[test]
fn each_drive_keeps_its_headroom_and_a_piece_needs_one_drive() {
    let mut ledger = RoomLedger::new(drives(&[("c:", 30), ("d:", 5)]), 10, vec![25]);
    // 20 usable on C:, none on D: (5 is below its headroom).
    assert_eq!(
        ledger.shortfall(),
        Some(Shortfall {
            piece_bytes: 25,
            best_drive_bytes: 20
        })
    );
    // Freeing on D: first fills its headroom.
    ledger.add("d:", 25);
    assert!(!ledger.has_room(), "D: has 20 usable, still short of 25");
    ledger.add("d:", 5);
    assert!(ledger.has_room());
}

#[test]
fn space_on_a_drive_the_job_does_not_use_does_not_count() {
    let mut ledger = RoomLedger::new(drives(&[("c:", 0)]), 0, vec![10]);
    assert!(!ledger.counts("e:"));
    ledger.add("e:", 100);
    assert!(!ledger.has_room());
}

/// The case #716's review found: 60% of a job free on each of two drives
/// used to add up to enough, and nothing was deleted.
#[test]
fn two_drives_with_sixty_percent_each_do_not_hold_one_job() {
    let ledger = RoomLedger::new(drives(&[("d:", 60), ("e:", 60)]), 0, vec![100]);
    assert_eq!(
        ledger.shortfall(),
        Some(Shortfall {
            piece_bytes: 100,
            best_drive_bytes: 60
        })
    );
}

#[test]
fn room_is_made_on_one_drive_by_its_oldest_takes() {
    let ledger = RoomLedger::new(drives(&[("d:", 60), ("e:", 60)]), 0, vec![100]);
    // Oldest first: D: 30, E: 45, D: 20. Making room on D: takes 50 (both
    // of its takes), on E: 45, so E:'s take goes and D:'s stay.
    let volumes = vols(&["d:", "e:", "d:"]);
    let sizes = [30, 45, 20];
    let chosen = choose_deletions(&ledger, &volumes, |i| Some(sizes[i])).unwrap();
    assert_eq!(chosen, vec![1]);

    // Only D: has takes: both of them go, E:'s free space doesn't help.
    let volumes = vols(&["d:", "d:"]);
    let sizes = [30, 20];
    let chosen = choose_deletions(&ledger, &volumes, |i| Some(sizes[i])).unwrap();
    assert_eq!(chosen, vec![0, 1]);
}

#[test]
fn it_says_so_when_no_one_drive_can_be_made_to_fit() {
    let ledger = RoomLedger::new(drives(&[("d:", 60), ("e:", 60)]), 0, vec![100]);
    // 30 more on each drive: 180 in all, but 90 at most on one.
    let volumes = vols(&["d:", "e:"]);
    let short = choose_deletions(&ledger, &volumes, |_| Some(30)).unwrap_err();
    assert_eq!(
        short,
        Shortfall {
            piece_bytes: 100,
            best_drive_bytes: 90
        }
    );
    // A take the delete refuses frees nothing.
    let short = choose_deletions(&ledger, &vols(&["d:"]), |_| None).unwrap_err();
    assert_eq!(short.best_drive_bytes, 60);
}

#[test]
fn a_job_that_fits_on_one_drive_needs_nothing_while_another_is_full() {
    let ledger = RoomLedger::new(drives(&[("d:", 0), ("e:", 150)]), 0, vec![100]);
    assert!(ledger.has_room());
    let chosen = choose_deletions(&ledger, &vols(&["d:", "e:"]), |_| {
        panic!("nothing is sized when the job fits")
    })
    .unwrap();
    assert!(chosen.is_empty());
}

#[test]
fn several_pieces_are_placed_largest_first_across_drives() {
    // Two blocks of 60 fit one on each 70-byte drive.
    assert!(RoomLedger::new(drives(&[("d:", 70), ("e:", 70)]), 0, vec![60, 60]).has_room());
    // 80 and 50 don't: 80 has no drive. Freeing 10 on D: places 80 there
    // and 50 on E:.
    let ledger = RoomLedger::new(drives(&[("d:", 70), ("e:", 70)]), 0, vec![50, 80]);
    assert!(!ledger.has_room());
    let chosen = choose_deletions(&ledger, &vols(&["e:", "d:"]), |i| Some([5, 10][i])).unwrap();
    assert_eq!(chosen, vec![1]);
}

#[test]
fn pieces_that_need_room_on_two_drives_use_both() {
    // Two blocks of 60, both drives full, one 70-byte take on each: no one
    // drive's takes are enough, so D: is used up and then E:'s take
    // finishes the job.
    let ledger = RoomLedger::new(drives(&[("d:", 0), ("e:", 0)]), 0, vec![60, 60]);
    let volumes = vols(&["d:", "e:"]);
    let sizes = [70, 70];
    let chosen = choose_deletions(&ledger, &volumes, |i| Some(sizes[i])).unwrap();
    assert_eq!(chosen, vec![0, 1]);
}

#[test]
fn folders_on_one_drive_are_one_volume() {
    if cfg!(windows) {
        let mut path = PathBuf::from("C:");
        path.push("captures");
        path.push("s1");
        let mut other = PathBuf::from("c:");
        other.push("clips");
        assert_eq!(volume_of(&path), volume_of(&other));
        assert!(!volume_of(&path).is_empty());
    }
    assert_eq!(volume_of(Path::new("relative")), "");
}

// ── Scanning a Destinations folder ───────────────────────────────────────

/// A frame-sequence take under `<capture>/<session>/<block>/take0000`,
/// with `history` recorded in its block folder. Returns the take folder.
fn take_with_history(capture: &Scratch, block: &str, history: Vec<RenderAttempt>) -> PathBuf {
    let take = capture
        .join("session_20261011_120000")
        .join(block)
        .join("take0000");
    std::fs::create_dir_all(take.join("all")).unwrap();
    std::fs::write(take.join("sound.wav"), vec![0u8; 100]).unwrap();
    std::fs::write(take.join("all").join("00000.bmp"), vec![0u8; 900]).unwrap();
    let mut meta = take_meta::SessionMeta::new("s", 120);
    meta.renders = history;
    take_meta::write(take.parent().unwrap(), &meta).unwrap();
    take
}

fn rendered(capture: &Scratch, name: &str) -> String {
    let out = capture.join("exports");
    std::fs::create_dir_all(&out).unwrap();
    let file = out.join(name);
    std::fs::write(&file, b"clip").unwrap();
    file.to_string_lossy().into_owned()
}

#[test]
fn only_finished_idle_takes_are_found_oldest_first() {
    let capture = Scratch::new("take_cleanup_scan");
    let a = rendered(&capture, "a.mov");
    let b = rendered(&capture, "b.mov");
    let newer = take_with_history(
        &capture,
        "chain_01_b0",
        vec![attempt("all", 200, RenderOutcome::Finished, &a)],
    );
    let older = take_with_history(
        &capture,
        "chain_01_b1",
        vec![attempt("all", 100, RenderOutcome::Finished, &b)],
    );
    take_with_history(
        &capture,
        "chain_01_b2",
        vec![attempt("all", 50, RenderOutcome::Failed, "")],
    );
    take_with_history(&capture, "chain_01_b3", Vec::new());
    let gone = capture.join("exports").join("missing.mov");
    take_with_history(
        &capture,
        "chain_01_b4",
        vec![attempt(
            "all",
            10,
            RenderOutcome::Finished,
            &gone.to_string_lossy(),
        )],
    );
    let dirs = vec![capture.path().to_path_buf()];

    let found = finished_takes(&dirs, &HashSet::new());
    let folders: Vec<_> = found.iter().map(|t| t.take_folder.clone()).collect();
    assert_eq!(folders, vec![older.clone(), newer.clone()]);

    let busy = HashSet::from([folder_key(&older.to_string_lossy())]);
    let found = finished_takes(&dirs, &busy);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].take_folder, newer);
}

/// A HUD pair's take holds both halves on disk, so both must be finished
/// even when the Render tab only listed the main row.
#[test]
fn a_takes_streams_are_read_from_its_folder() {
    let capture = Scratch::new("take_cleanup_streams_on_disk");
    let take = take_with_history(&capture, "chain_01_b0", Vec::new());
    assert_eq!(streams_on_disk(&take), streams(&["all"]));
    for stream in ["hudcolor", "hudalpha"] {
        std::fs::create_dir_all(take.join(stream)).unwrap();
        std::fs::write(take.join(stream).join("00000.bmp"), b"b").unwrap();
    }
    let on_disk = streams_on_disk(&take);
    assert_eq!(on_disk, streams(&["all", "hud:hudcolor"]));
    let a = rendered(&capture, "a.mov");
    let main_only = vec![attempt("all", 10, RenderOutcome::Finished, &a)];
    assert_eq!(
        finished_at(&main_only, &on_disk, |p| Path::new(p).is_file()),
        None,
        "the HUD half was never rendered"
    );
}

fn need(capture: &Scratch, needed: u64) -> SpaceNeed {
    SpaceNeed {
        targets: vec![capture.path().to_path_buf()],
        pieces: vec![needed],
        fallback_pieces: None,
        headroom_bytes: 0,
    }
}

/// A long render asks room for its segments and clip together, else for the
/// clip alone (#1451): with no deletion making room for both, the cleanup
/// makes room for the clip, as it did before segments.
#[test]
fn room_too_short_for_the_segments_is_made_for_the_clip_alone() {
    let capture = Scratch::new("take_cleanup_plan_fallback");
    let a = rendered(&capture, "a.mov");
    let oldest = take_with_history(
        &capture,
        "chain_01_b0",
        vec![attempt("all", 100, RenderOutcome::Finished, &a)],
    );
    let dirs = vec![capture.path().to_path_buf()];
    let with_fallback = |peak: u64, one_piece: u64| SpaceNeed {
        pieces: vec![peak],
        fallback_pieces: Some(vec![one_piece]),
        ..need(&capture, peak)
    };
    // 500 free plus the one take's 1000 is 1500: short of 2400, enough for 1200.
    let plan = plan_room(
        &with_fallback(2400, 1200),
        &dirs,
        &HashSet::new(),
        |_| Some(500),
        |_| {},
    );
    let RoomPlan::Delete(chosen) = plan else {
        panic!("expected deletions for the clip alone, got {plan:?}");
    };
    assert_eq!(chosen.len(), 1);
    assert_eq!(chosen[0].take_folder, oldest);
    // Room for the clip already: nothing is deleted just for segments that
    // can't fit anyway.
    assert!(matches!(
        plan_room(
            &with_fallback(2400, 400),
            &dirs,
            &HashSet::new(),
            |_| Some(500),
            |_| {}
        ),
        RoomPlan::Enough
    ));
    // Short even for the clip: Short, as before.
    assert!(matches!(
        plan_room(
            &with_fallback(4000, 2000),
            &dirs,
            &HashSet::new(),
            |_| Some(500),
            |_| {}
        ),
        RoomPlan::Short { .. }
    ));
}

/// Each test take is 1000 bytes on disk (a 100-byte wav, a 900-byte frame).
const TAKE_BYTES: u64 = 1000;

#[test]
fn room_is_made_by_the_oldest_finished_takes_only_as_far_as_needed() {
    let capture = Scratch::new("take_cleanup_plan");
    let a = rendered(&capture, "a.mov");
    let b = rendered(&capture, "b.mov");
    let c = rendered(&capture, "c.mov");
    take_with_history(
        &capture,
        "chain_01_b0",
        vec![attempt("all", 300, RenderOutcome::Finished, &a)],
    );
    let oldest = take_with_history(
        &capture,
        "chain_01_b1",
        vec![attempt("all", 100, RenderOutcome::Finished, &b)],
    );
    let second = take_with_history(
        &capture,
        "chain_01_b2",
        vec![attempt("all", 200, RenderOutcome::Finished, &c)],
    );
    let dirs = vec![capture.path().to_path_buf()];

    // 500 free, 2000 needed: the two oldest takes (2000 bytes) bring it to 2500.
    let plan = plan_room(
        &need(&capture, 2000),
        &dirs,
        &HashSet::new(),
        |_| Some(500),
        |_| {},
    );
    let RoomPlan::Delete(chosen) = plan else {
        panic!("expected deletions, got {plan:?}");
    };
    let folders: Vec<_> = chosen.iter().map(|d| d.take_folder.clone()).collect();
    assert_eq!(folders, vec![oldest, second]);
    assert!(chosen.iter().all(|d| d.size_bytes == TAKE_BYTES));
}

#[test]
fn nothing_is_planned_when_there_is_room_or_free_space_is_unknown() {
    let capture = Scratch::new("take_cleanup_plan_enough");
    let a = rendered(&capture, "a.mov");
    take_with_history(
        &capture,
        "chain_01_b0",
        vec![attempt("all", 100, RenderOutcome::Finished, &a)],
    );
    let dirs = vec![capture.path().to_path_buf()];
    assert!(matches!(
        plan_room(
            &need(&capture, 100),
            &dirs,
            &HashSet::new(),
            |_| Some(100),
            |_| {}
        ),
        RoomPlan::Enough
    ));
    assert!(matches!(
        plan_room(
            &need(&capture, 100),
            &dirs,
            &HashSet::new(),
            |_| None,
            |_| {}
        ),
        RoomPlan::Unknown
    ));
    let no_targets = SpaceNeed {
        targets: Vec::new(),
        pieces: vec![100],
        fallback_pieces: None,
        headroom_bytes: 0,
    };
    assert!(matches!(
        plan_room(&no_targets, &dirs, &HashSet::new(), |_| Some(0), |_| {}),
        RoomPlan::Unknown
    ));
}

#[test]
fn nothing_is_planned_when_every_finished_take_is_still_not_enough() {
    let capture = Scratch::new("take_cleanup_plan_short");
    let a = rendered(&capture, "a.mov");
    let take = take_with_history(
        &capture,
        "chain_01_b0",
        vec![attempt("all", 100, RenderOutcome::Finished, &a)],
    );
    let dirs = vec![capture.path().to_path_buf()];
    let plan = plan_room(
        &need(&capture, 10_000),
        &dirs,
        &HashSet::new(),
        |_| Some(0),
        |_| {},
    );
    let RoomPlan::Short {
        needed_bytes,
        available_bytes,
        moved_takes,
    } = plan
    else {
        panic!("expected short, got {plan:?}");
    };
    assert_eq!((needed_bytes, available_bytes), (10_000, TAKE_BYTES));
    assert_eq!(moved_takes, 0);
    assert!(take.join("sound.wav").is_file(), "planning deletes nothing");
}

#[test]
fn a_finished_take_whose_clip_moved_is_kept_and_counted() {
    let s = streams(&["all"]);
    let finished = vec![attempt("all", 10, RenderOutcome::Finished, "E:/out/a.mov")];
    assert!(clip_moved(&finished, &s, |_| false));
    assert!(!clip_moved(&finished, &s, EVERY_FILE_EXISTS));
    let failed = vec![attempt("all", 10, RenderOutcome::Failed, "E:/out/a.mov")];
    assert!(!clip_moved(&failed, &s, |_| false));
    assert!(!clip_moved(&[], &s, |_| false));
    let no_path = vec![attempt("all", 10, RenderOutcome::Finished, "")];
    assert!(!clip_moved(&no_path, &s, |_| false));

    let capture = Scratch::new("take_cleanup_plan_moved");
    let gone = capture.join("exports").join("moved.mov");
    let take = take_with_history(
        &capture,
        "chain_01_b0",
        vec![attempt(
            "all",
            100,
            RenderOutcome::Finished,
            &gone.to_string_lossy(),
        )],
    );
    take_with_history(
        &capture,
        "chain_01_b1",
        vec![attempt("all", 50, RenderOutcome::Failed, "")],
    );
    let dirs = vec![capture.path().to_path_buf()];
    let plan = plan_room(
        &need(&capture, 500),
        &dirs,
        &HashSet::new(),
        |_| Some(0),
        |_| {},
    );
    let RoomPlan::Short {
        available_bytes,
        moved_takes,
        ..
    } = plan
    else {
        panic!("expected short, got {plan:?}");
    };
    assert_eq!((available_bytes, moved_takes), (0, 1));
    assert!(take.join("sound.wav").is_file());
}

#[test]
fn progress_is_silent_with_room_and_steps_through_a_scan_without() {
    let capture = Scratch::new("take_cleanup_plan_progress");
    let a = rendered(&capture, "a.mov");
    take_with_history(
        &capture,
        "chain_01_b0",
        vec![attempt("all", 100, RenderOutcome::Finished, &a)],
    );
    let dirs = vec![capture.path().to_path_buf()];
    let mut steps = Vec::new();
    plan_room(
        &need(&capture, 10),
        &dirs,
        &HashSet::new(),
        |_| Some(10),
        |s| steps.push(s),
    );
    assert!(steps.is_empty(), "{steps:?}");
    plan_room(
        &need(&capture, 900),
        &dirs,
        &HashSet::new(),
        |_| Some(0),
        |s| steps.push(s),
    );
    assert_eq!(
        steps,
        vec![
            RoomStep::Looking,
            RoomStep::Measuring { done: 0, total: 1 },
            RoomStep::Measuring { done: 1, total: 1 },
        ]
    );
}

#[test]
fn takes_outside_the_destinations_folders_are_never_planned() {
    let capture = Scratch::new("take_cleanup_plan_outside");
    let a = rendered(&capture, "a.mov");
    take_with_history(
        &capture,
        "chain_01_b0",
        vec![attempt("all", 100, RenderOutcome::Finished, &a)],
    );
    let other = Scratch::new("take_cleanup_plan_outside_other");
    let plan = plan_room(
        &need(&capture, 500),
        &[other.path().to_path_buf()],
        &HashSet::new(),
        |_| Some(0),
        |_| {},
    );
    assert!(matches!(plan, RoomPlan::Short { .. }), "{plan:?}");
}
