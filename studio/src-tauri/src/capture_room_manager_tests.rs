use super::*;

fn block(start: i32, stop: i32) -> CaptureBlock {
    CaptureBlock {
        demo_name: "d".into(),
        block_index: 0,
        drive_index: 0,
        take_folder: PathBuf::from("x"),
        take_key: String::new(),
        source_streak_indices: vec![],
        start_tick: start,
        end_tick: stop,
        record_start_tick: start,
        record_stop_tick: stop,
        ramped_gaps: 0,
        plan: None,
    }
}

#[test]
fn a_record_marker_finds_its_block_by_tick_past_a_skipped_demo() {
    let blocks = room_blocks(&[block(100, 200), block(300, 400), block(500, 600)], 3000);
    let start = |b: &RoomBlock| b.start_tick;
    let stop = |b: &RoomBlock| b.stop_tick;
    assert_eq!(resolve_block(&blocks, 0, Some(100), start), 0);
    // Block 1's demo was skipped: block 2's marker comes next.
    assert_eq!(resolve_block(&blocks, 1, Some(500), start), 2);
    assert_eq!(resolve_block(&blocks, 1, Some(600), stop), 2);
    // An earlier block's tick is never matched again.
    assert_eq!(resolve_block(&blocks, 2, Some(100), start), 2);
    // No tick, or one nothing names: the next in order.
    assert_eq!(resolve_block(&blocks, 1, None, start), 1);
    assert_eq!(resolve_block(&blocks, 1, Some(7), start), 1);
    // Past the end stays past the end, for the callers' `get`.
    assert_eq!(resolve_block(&blocks, 3, Some(100), start), 3);
}

#[test]
fn a_take_that_never_settles_is_not_handed_over() {
    let cancel = AtomicBool::new(true);
    let dir = std::env::temp_dir().join("dod_test_rwc_settle_cancelled");
    let _ = std::fs::create_dir_all(&dir);
    std::fs::write(dir.join("video.avi"), b"frames").unwrap();
    assert_eq!(settle(&dir, &cancel), None);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A partial take (#1105) is kept, never rendered or deleted from here:
/// a hold that only its space could end gives up at once instead of
/// waiting for it, and says why.
#[test]
fn a_partial_take_is_never_counted_on_to_free_space() {
    const GB: u64 = 1024 * 1024 * 1024;
    let mut state = State {
        settling: 2,
        recording: Some(1),
        ..State::default()
    };
    state.take_settled(0, 100, Some(4 * GB), false);
    state.take_settled(1, 100, Some(5 * GB), true);
    assert_eq!(state.settling, 0);
    assert_eq!(state.recording, None);
    assert_eq!(state.recorded, vec![(0, 4 * GB)]);
    assert_eq!(state.partial, vec![(1, 5 * GB)]);

    // Held with only the partial take on the drive: the pending list
    // (built from `recorded`) has nothing that will free space.
    let inputs = RoomInputs {
        free_bytes: GB,
        headroom_bytes: GB,
        writing_bytes: 0,
        next_clip_bytes: 3 * GB,
        pending: &[],
        output_ratio: None,
    };
    assert!(matches!(decide(&inputs), RoomDecision::CannotFit { .. }));
    assert_eq!(
        partial_note(5 * GB, 1),
        format!(
            " ({} of the drive is 1 partial take kept for you to check, which Studio never deletes)",
            format_gb(5 * GB)
        )
    );
    assert_eq!(partial_note(0, 0), "");

    // A take that never settled is in neither list.
    state.settling = 1;
    state.take_settled(2, 100, None, false);
    assert_eq!((state.recorded.len(), state.partial.len()), (1, 1));
}

/// #1430: a clip on the Windows drive keeps "Keep free on the Windows
/// drive" free beside it (when that is more than the usual headroom); a
/// clip on another drive keeps the usual headroom.
#[cfg(windows)]
#[test]
fn a_clip_on_the_windows_drive_keeps_the_windows_drive_setting_free() {
    let headroom = native::sys::disk::MIN_DRIVE_HEADROOM_BYTES;
    let windows = PathBuf::from(std::env::var_os("SystemRoot").expect("SystemRoot"));
    let session = windows.join("no-such-session-1430").join("clip_b0");
    assert_eq!(
        block_headroom(&session),
        headroom.max(native::sys::windows_drive::keep_free_bytes())
    );
    let missing = (b'A'..=b'Z')
        .map(|c| format!("{}:\\", c as char))
        .find(|d| !Path::new(d).exists());
    if let Some(drive) = missing {
        assert_eq!(block_headroom(&Path::new(&drive).join("clip_b0")), headroom);
        assert_eq!(windows_drive_note(&Path::new(&drive).join("clip_b0")), "");
    }
}

/// #1430: room by the usual headroom but not past the Windows drive's
/// setting, with no take there to free, can never fit: the batch stops
/// with the reason instead of holding for space that won't come.
#[test]
fn a_windows_drive_clip_short_only_of_the_setting_with_nothing_to_free_cannot_fit() {
    const GB: u64 = 1024 * 1024 * 1024;
    let usual = RoomInputs {
        free_bytes: 40 * GB,
        headroom_bytes: 15 * GB,
        writing_bytes: 0,
        next_clip_bytes: 10 * GB,
        pending: &[],
        output_ratio: None,
    };
    assert_eq!(decide(&usual), RoomDecision::Continue);
    let windows = RoomInputs {
        headroom_bytes: 50 * GB,
        ..usual
    };
    assert!(matches!(decide(&windows), RoomDecision::CannotFit { .. }));
}

#[cfg(windows)]
#[test]
fn a_stopped_batch_names_the_drive_letter() {
    assert_eq!(drive_label(Path::new(r"F:\DICE\session\clip_b0")), "F:");
    assert_eq!(drive_label(Path::new("relative")), "relative");
}

/// #1438: take cleanup takes a rendered take's rows out of the list
/// before its files go. In that window the take (handed over long ago,
/// no rows) is still counted as freeing its space, so a hold that is
/// about to have room doesn't stop; once nothing is deleting it, the
/// same take frees nothing.
#[test]
fn a_take_losing_its_rows_to_cleanup_still_frees_its_space() {
    const GB: u64 = 1024 * 1024 * 1024;
    let facts = |deleting| TakeFacts {
        raw_bytes: 10 * GB,
        renders: &[],
        volume: "F",
        exports_here: false,
        deletes: true,
        since_handover: Some(Duration::from_secs(600)),
        since_finished: None,
        deleting,
    };
    let decide_with = |take: PendingTake| {
        decide(&RoomInputs {
            free_bytes: 16 * GB,
            headroom_bytes: 15 * GB,
            writing_bytes: 0,
            next_clip_bytes: 5 * GB,
            pending: &[take],
            output_ratio: None,
        })
    };
    assert_eq!(
        decide_with(pending_take(&facts(true))),
        RoomDecision::Hold {
            short_bytes: 4 * GB,
            reclaimable_bytes: 10 * GB
        }
    );
    assert_eq!(
        decide_with(pending_take(&facts(false))),
        RoomDecision::CannotFit {
            short_bytes: 4 * GB
        }
    );
}

/// A take whose renders all finished counts while cleanup can still
/// delete it, and not once it has stayed past `FINISHED_GRACE`.
#[test]
fn a_rendered_take_cleanup_keeps_stops_counting() {
    let finished = [TakeRender {
        status: "Finished".into(),
        output_path: String::new(),
        output_size_bytes: Some(1),
    }];
    let facts = |since_finished| TakeFacts {
        raw_bytes: 10,
        renders: &finished,
        volume: "F",
        exports_here: false,
        deletes: true,
        since_handover: None,
        since_finished,
        deleting: false,
    };
    assert!(pending_take(&facts(Some(Duration::from_secs(1)))).render_coming);
    assert!(!pending_take(&facts(Some(native::hlcr::capture_room::FINISHED_GRACE))).render_coming);
}

#[test]
fn each_block_gets_the_planners_share_by_its_record_span() {
    let blocks = room_blocks(&[block(0, 100), block(200, 500)], 4000);
    assert_eq!(blocks[0].ticks, 100);
    assert_eq!(blocks[0].planned_bytes, 1000);
    assert_eq!(blocks[1].planned_bytes, 3000);
    // A block with no span still counts one tick rather than dividing by 0.
    let blocks = room_blocks(&[block(5, 5)], 10);
    assert_eq!(blocks[0].ticks, 1);
    assert_eq!(blocks[0].planned_bytes, 10);
}
