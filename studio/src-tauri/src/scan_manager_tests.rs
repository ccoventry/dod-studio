use super::*;
use crate::test_support::Scratch;

#[test]
fn a_skipped_demo_gets_a_plain_reason_from_its_contents() {
    let dir = Scratch::new("plain_scan_failure");
    let short = dir.path().join("short.dem");
    let text = dir.path().join("text.dem");
    let corrupt = dir.path().join("corrupt.dem");
    std::fs::write(&short, b"hello").unwrap();
    std::fs::write(&text, vec![b'x'; 2048]).unwrap();
    let mut demo = b"HLDEMO\0\0".to_vec();
    demo.resize(2048, 0xAB);
    std::fs::write(&corrupt, demo).unwrap();

    let r = "Failed to read demo header: failed to fill whole buffer";
    assert_eq!(
        plain_scan_failure(&short, r),
        crate::messages::SCAN_FAIL_TOO_SHORT
    );
    assert_eq!(
        plain_scan_failure(&text, "Failed to parse demo: x"),
        crate::messages::SCAN_FAIL_NOT_A_DEMO
    );
    assert_eq!(
        plain_scan_failure(&corrupt, "Failed to parse demo: x"),
        crate::messages::SCAN_FAIL_CORRUPT
    );
    let io = "Failed to read file: Access is denied. (os error 5)";
    assert_eq!(plain_scan_failure(&corrupt, io), io);
}

#[test]
fn skipped_demos_are_reported_in_file_order_with_their_reason() {
    let rows = skipped_demo_rows(vec![
        (5, "b.dem".into(), "Failed to parse demo: eof".into()),
        (2, "a.dem".into(), "Failed to read file: denied".into()),
    ]);
    assert_eq!(rows[0]["name"], "a.dem");
    assert_eq!(rows[0]["reason"], "Failed to read file: denied");
    assert_eq!(rows[1]["name"], "b.dem");
}

#[test]
fn the_scan_uses_the_worker_setting_within_its_range() {
    assert_eq!(scan_worker_count(5, 100), 5);
    assert_eq!(scan_worker_count(0, 100), 1);
    assert_eq!(scan_worker_count(20, 100), SCAN_WORKERS_MAX);
    // Never more threads than demos.
    assert_eq!(scan_worker_count(8, 3), 3);
    assert_eq!(scan_worker_count(4, 0), 1);
}

#[test]
fn the_scan_status_line_names_a_demo_only_while_one_is_being_parsed() {
    assert_eq!(
        scan_status_line(0, 1, 0, Some("a.dem")),
        "Reading demos: 0 of 1, a.dem"
    );
    assert_eq!(scan_status_line(1, 1, 0, None), "Reading demos: 1 of 1");
    assert_eq!(
        scan_status_line(3, 10, 2, Some("a.dem")),
        "Reading demos: 3 of 10 (2 from the analyzer cache), a.dem"
    );
}

#[test]
fn scan_progress_is_throttled_to_one_line_per_33_ms() {
    let last = std::sync::atomic::AtomicU32::new(u32::MAX);
    assert!(
        progress_emit_due(&last, 0),
        "the first line always goes out"
    );
    assert!(!progress_emit_due(&last, 20));
    assert!(progress_emit_due(&last, 33));
    assert!(!progress_emit_due(&last, 40));
}

#[test]
fn the_listing_line_counts_folders_and_demos() {
    assert_eq!(
        listing_status_line(12, 3),
        "Looking for demos: 12 folders read, 3 demos found"
    );
}

/// A drive-like tree: two demos to find, and deleted ones in the
/// Recycle Bin and System Volume Information.
fn drive_like(scratch: &Scratch) -> PathBuf {
    let root = scratch.path().to_path_buf();
    for dir in [
        "games/dod",
        "$RECYCLE.BIN/S-1-5-21",
        "System Volume Information",
    ] {
        std::fs::create_dir_all(root.join(dir)).unwrap();
    }
    for f in [
        "b.dem",
        "games/dod/a.dem",
        "$RECYCLE.BIN/S-1-5-21/$R1A2B3C.dem",
        "System Volume Information/c.dem",
    ] {
        std::fs::write(root.join(f), b"").unwrap();
    }
    root
}

#[test]
fn listing_finds_demos_in_name_order_and_skips_system_folders() {
    let scratch = Scratch::new("list_scan_demos");
    let root = drive_like(&scratch);
    let mut list = Vec::new();
    let mut last = (0, 0);
    let listing = list_scan_demos(
        std::slice::from_ref(&root),
        true,
        &mut list,
        &std::sync::atomic::AtomicBool::new(false),
        &mut |folders, demos| last = (folders, demos),
    );
    assert!(listing.listed);
    assert!(!listing.cap.hit());
    assert_eq!(list, [root.join("games/dod/a.dem"), root.join("b.dem")]);
    // The root, games and games/dod; the two system folders not counted.
    assert_eq!(last, (3, 2));
}

#[test]
fn cancel_stops_the_listing_and_says_so() {
    let scratch = Scratch::new("list_scan_cancel");
    let root = drive_like(&scratch);
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let mut list = Vec::new();
    let mut calls = 0;
    let listing = list_scan_demos(
        std::slice::from_ref(&root),
        true,
        &mut list,
        &cancel,
        &mut |_, _| {
            calls += 1;
            // The first call is the start line; cancel on the next.
            if calls == 2 {
                cancel.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        },
    );
    assert!(!listing.listed);
    assert!(!listing.cap.hit(), "a cancel is not a cap");
    assert_eq!(calls, 2, "nothing more is read once cancelled");

    // Cancelled before it starts: nothing is listed.
    let mut list = Vec::new();
    assert!(
        !list_scan_demos(
            std::slice::from_ref(&root),
            true,
            &mut list,
            &cancel,
            &mut |_, _| {}
        )
        .listed
    );
    assert!(list.is_empty());
}

/// Lists `dirs` with nothing cancelled; the demos relative to `root`.
fn list_rel(root: &Path, dirs: &[PathBuf], subfolders: bool) -> (Vec<String>, Listing) {
    let mut list = Vec::new();
    let listing = list_scan_demos(
        dirs,
        subfolders,
        &mut list,
        &std::sync::atomic::AtomicBool::new(false),
        &mut |_, _| {},
    );
    let mut rel: Vec<String> = list
        .iter()
        .map(|p| {
            p.strip_prefix(root)
                .unwrap_or(p)
                .to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, "/")
        })
        .collect();
    rel.sort();
    (rel, listing)
}

#[test]
fn without_subfolders_only_the_folders_own_demos_are_listed() {
    let scratch = Scratch::new("list_no_subfolders");
    let root = drive_like(&scratch);
    std::fs::write(root.join("UPPER.DEM"), b"").unwrap();
    let (demos, listing) = list_rel(&root, std::slice::from_ref(&root), false);
    assert_eq!(demos, ["UPPER.DEM", "b.dem"]);
    assert!(listing.listed);
    assert!(!listing.cap.hit(), "no subfolders is a choice, not a cap");
}

#[test]
fn an_upper_case_dem_is_listed() {
    let scratch = Scratch::new("list_upper_dem");
    let root = scratch.path().to_path_buf();
    std::fs::create_dir_all(root.join("sub")).unwrap();
    std::fs::write(root.join("sub/MATCH.DEM"), b"").unwrap();
    std::fs::write(root.join("Final.Dem"), b"").unwrap();
    let (demos, _) = list_rel(&root, std::slice::from_ref(&root), true);
    assert_eq!(demos, ["Final.Dem", "sub/MATCH.DEM"]);
}

#[test]
fn the_depth_cap_stops_the_walk_and_says_so() {
    let scratch = Scratch::new("list_depth_cap");
    let root = scratch.path().to_path_buf();
    // d1/.../d6 is SCAN_MAX_DEPTH (6) levels down: its demo is not read.
    let mut deep = root.clone();
    for level in 1..=SCAN_MAX_DEPTH {
        deep = deep.join(format!("d{level}"));
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(deep.join(format!("at{level}.dem")), b"").unwrap();
    }
    let (demos, listing) = list_rel(&root, std::slice::from_ref(&root), true);
    assert_eq!(demos.len(), SCAN_MAX_DEPTH - 1, "{demos:?}");
    assert!(demos.iter().all(|d| !d.ends_with("at6.dem")));
    assert!(listing.listed);
    assert!(listing.cap.depth);
    assert!(!listing.cap.entries);
    assert_eq!(listing.cap.max_depth, 6);
    assert_eq!(
        scan_cap_log_line(&listing.cap),
        "Scan: some demos may be missing: folders more than 6 levels down were not read. Pick a folder further down to scan the rest."
    );
}

#[test]
fn a_folder_picked_twice_or_inside_another_is_listed_once() {
    let scratch = Scratch::new("list_twice");
    let root = drive_like(&scratch);
    let dod = root.join("games/dod");
    let upper = PathBuf::from(root.to_string_lossy().to_uppercase());
    let (demos, _) = list_rel(&root, &[root.clone(), dod, upper], true);
    assert_eq!(demos, ["b.dem", "games/dod/a.dem"]);
}

#[test]
fn visited_folders_key_each_folder_by_where_it_really_is() {
    // `L:/link` is a junction to `D:/demos`: both roots key the same,
    // so the second is not walked, and a folder below either is one.
    let (mut visited, walk) = VisitedFolders::with_keys(vec![
        (PathBuf::from("D:/demos"), "d:/demos".into()),
        (PathBuf::from("L:/link"), "d:/demos".into()),
        (PathBuf::from("D:/demos/sub"), "d:/demos/sub".into()),
    ]);
    assert_eq!(
        walk,
        [PathBuf::from("D:/demos"), PathBuf::from("D:/demos/sub")]
    );
    // Another root, met inside the first: walked as a root of its own.
    assert!(!visited.first_visit(Path::new("D:/demos/sub")));
    assert!(visited.first_visit(Path::new("D:/demos/sub/2024")));
    assert!(!visited.first_visit(Path::new("D:/Demos/SUB/2024")));
    assert!(visited.first_visit(Path::new("D:/demos/other")));
    assert!(!visited.first_visit(Path::new("D:/demos/other/")));
}

/// A junction loop (`sub/loop` → the root) and the same folder picked
/// through a junction: every demo once, and the walk ends.
#[cfg(windows)]
#[test]
fn a_junction_loop_or_a_junction_root_lists_every_demo_once() {
    let scratch = Scratch::new("list_junction");
    let root = drive_like(&scratch);
    let link = root.join("games/loop");
    native::sys::junction::create(&link, &root).unwrap();
    let (demos, listing) = list_rel(&root, &[root.clone(), link.clone()], true);
    let _ = native::sys::junction::remove(&link);
    assert_eq!(demos, ["b.dem", "games/dod/a.dem"]);
    assert!(!listing.cap.hit());
}

#[test]
fn a_known_unchanged_demo_is_skipped_and_a_changed_one_is_not() {
    let scratch = Scratch::new("skip_known");
    let a = scratch.path().join("a.dem");
    let b = scratch.path().join("b.dem");
    let c = scratch.path().join("c.dem");
    std::fs::write(&a, b"HLDEMO aaaa").unwrap();
    std::fs::write(&b, b"HLDEMO bbbb").unwrap();
    std::fs::write(&c, b"HLDEMO cccc").unwrap();
    let key_b_before = demo_file_key(&b).unwrap();
    std::fs::write(&b, b"HLDEMO bbbb, re-recorded").unwrap();

    let known = vec![
        // Same file, different case and slashes: still a match.
        KnownDemo {
            path: a.to_string_lossy().to_uppercase().replace('\\', "/"),
            file_key: demo_file_key(&a).unwrap(),
        },
        // Changed on disk since it was queued: scanned again.
        KnownDemo {
            path: b.to_string_lossy().into_owned(),
            file_key: key_b_before,
        },
    ];
    let mut reported = Vec::new();
    let (keep, keys, unchanged, copies) =
        skip_known(vec![a, b.clone(), c.clone()], &known, &mut |done, total| {
            reported.push((done, total))
        });

    // Each key read is reported, so the queue's line moves while a big
    // folder's keys are read.
    assert_eq!(reported, [(1, 3), (2, 3), (3, 3)]);
    assert_eq!(unchanged, 1);
    assert_eq!(keep, vec![b.clone(), c.clone()]);
    assert_eq!(keys, vec![demo_file_key(&b), demo_file_key(&c)]);
    assert!(copies.is_empty());
}

/// A copy of a queued demo, and a second copy within the scan, are
/// reported without being kept for parsing. Within the scan the shorter
/// name is kept even though it sorts after its copy.
#[test]
fn identical_copies_are_skipped_before_parsing() {
    let scratch = Scratch::new("skip_copies");
    let queued = scratch.path().join("queued.dem");
    let copy = scratch.path().join("queued copy.dem");
    let fresh = scratch.path().join("fresh.dem");
    let fresh_copy = scratch.path().join("fresh - Copy.dem");
    std::fs::write(&queued, b"HLDEMO the queued one").unwrap();
    std::fs::write(&copy, b"HLDEMO the queued one").unwrap();
    std::fs::write(&fresh, b"HLDEMO a new one").unwrap();
    std::fs::write(&fresh_copy, b"HLDEMO a new one").unwrap();
    let known = vec![KnownDemo {
        path: queued.to_string_lossy().into_owned(),
        file_key: demo_file_key(&queued).unwrap(),
    }];

    let (keep, _, unchanged, copies) = skip_known(
        vec![
            // "fresh - Copy.dem" sorts first (' ' < '.'), as the scan's
            // list is.
            copy.clone(),
            fresh_copy.clone(),
            fresh.clone(),
            queued.clone(),
        ],
        &known,
        &mut |_, _| {},
    );

    assert_eq!(keep, vec![fresh.clone()]);
    assert_eq!(unchanged, 1);
    assert_eq!(
        copies,
        vec![
            ScanCopy {
                path: copy.to_string_lossy().into_owned(),
                same_as: queued.to_string_lossy().into_owned(),
                queued: true,
            },
            ScanCopy {
                path: fresh_copy.to_string_lossy().into_owned(),
                same_as: fresh.to_string_lossy().into_owned(),
                queued: false,
            },
        ]
    );
}

#[test]
fn a_moved_demo_is_found_by_its_key_in_a_subfolder() {
    let scratch = Scratch::new("locate_moved");
    let old = scratch.path().join("old");
    let new = scratch.path().join("archive").join("2026");
    std::fs::create_dir_all(&old).unwrap();
    std::fs::create_dir_all(&new).unwrap();
    let moved = old.join("match.dem");
    let kept = old.join("kept.dem");
    std::fs::write(&moved, b"HLDEMO the moved one").unwrap();
    std::fs::write(&kept, b"HLDEMO still here").unwrap();
    let key = demo_file_key(&moved).unwrap();
    std::fs::rename(&moved, new.join("renamed.dem")).unwrap();
    // Same size, different content: must not be taken for it.
    std::fs::write(new.join("decoy.dem"), b"HLDEMO the moved two").unwrap();

    let demos = vec![
        KnownDemo {
            path: moved.to_string_lossy().into_owned(),
            file_key: key,
        },
        KnownDemo {
            path: kept.to_string_lossy().into_owned(),
            file_key: demo_file_key(&kept).unwrap(),
        },
    ];
    let mut searched = Vec::new();
    let found = locate_missing_demos(&demos, &[scratch.path().to_path_buf()], &mut |n| {
        searched.push(n)
    });

    assert_eq!(found.len(), 1, "only the missing demo is reported");
    // The page is told once, before the walk, how many it looks for.
    assert_eq!(searched, vec![1]);
    assert_eq!(found[0].path, moved.to_string_lossy());
    assert_eq!(
        found[0].candidate.as_deref(),
        Some(new.join("renamed.dem").to_string_lossy().as_ref())
    );
}

/// A big folder listed first (a whole `dod/`) must not use up the entry
/// budget before a demo one folder down from its old place is reached:
/// the live failure that broke #477's test.
#[test]
fn a_nearby_move_is_found_even_behind_a_big_folder() {
    let scratch = Scratch::new("locate_budget");
    let big = scratch.path().join("big");
    let mut deep = big.clone();
    for level in 0..4 {
        deep = deep.join(format!("level{level}"));
        std::fs::create_dir_all(&deep).unwrap();
        for i in 0..20 {
            std::fs::write(deep.join(format!("f{i}.txt")), b"x").unwrap();
        }
    }
    let project = scratch.path().join("project");
    std::fs::create_dir_all(project.join("subfolder")).unwrap();
    let moved = project.join("match.dem");
    std::fs::write(&moved, b"HLDEMO moved one level down").unwrap();
    let key = demo_file_key(&moved).unwrap();
    std::fs::rename(&moved, project.join("subfolder").join("match.dem")).unwrap();

    let demos = vec![KnownDemo {
        path: moved.to_string_lossy().into_owned(),
        file_key: key,
    }];
    // 30 entries: the big tree has 84, the project folder 1, then its
    // subfolder 1. Depth-first from `big` spent them all first.
    let limits = LocateLimits {
        max_entries: 30,
        ..LOCATE_LIMITS
    };
    let found = locate_missing_demos_within(
        &demos,
        &[project.clone(), big],
        limits,
        &mut || std::time::Duration::ZERO,
        &mut |_| {},
    );
    assert_eq!(
        found[0].candidate.as_deref(),
        Some(
            project
                .join("subfolder")
                .join("match.dem")
                .to_string_lossy()
                .as_ref()
        )
    );
}

/// A demo `levels` folders below the search folder, searched with the
/// real limits and a clock that never runs out.
fn search_levels_down(name: &str, levels: usize) -> MissingDemo {
    let scratch = Scratch::new(name);
    let original = scratch.path().join("match.dem");
    std::fs::write(&original, b"HLDEMO the one that moved").unwrap();
    let key = demo_file_key(&original).unwrap();
    let mut dir = scratch.path().to_path_buf();
    for level in 0..levels {
        dir = dir.join(format!("level{level}"));
    }
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::rename(&original, dir.join("match.dem")).unwrap();
    let demos = vec![KnownDemo {
        path: original.to_string_lossy().into_owned(),
        file_key: key,
    }];
    let mut found = locate_missing_demos_within(
        &demos,
        &[scratch.path().to_path_buf()],
        LOCATE_LIMITS,
        &mut || std::time::Duration::ZERO,
        &mut |_| {},
    );
    found.remove(0)
}

#[test]
fn a_demo_three_folders_down_is_found() {
    let found = search_levels_down("locate_depth3", 3);
    assert!(found.candidate.is_some(), "{found:?}");
    assert!(!found.search_stopped_early);
}

#[test]
fn a_demo_four_folders_down_is_not_found() {
    let found = search_levels_down("locate_depth4", 4);
    assert!(found.candidate.is_none(), "{found:?}");
}

#[test]
fn the_time_budget_stops_the_search_and_says_so() {
    let scratch = Scratch::new("locate_time");
    let original = scratch.path().join("match.dem");
    std::fs::write(&original, b"HLDEMO the one that moved").unwrap();
    let key = demo_file_key(&original).unwrap();
    std::fs::create_dir_all(scratch.path().join("sub")).unwrap();
    std::fs::rename(&original, scratch.path().join("sub").join("match.dem")).unwrap();
    let demos = vec![KnownDemo {
        path: original.to_string_lossy().into_owned(),
        file_key: key,
    }];
    // The clock starts past the budget: nothing is read.
    let found = locate_missing_demos_within(
        &demos,
        &[scratch.path().to_path_buf()],
        LOCATE_LIMITS,
        &mut || LOCATE_TIME_BUDGET,
        &mut |_| {},
    );
    assert!(found[0].candidate.is_none());
    assert!(found[0].search_stopped_early);
    // With time left, the same search finds it and does not say stopped.
    let found = locate_missing_demos_within(
        &demos,
        &[scratch.path().to_path_buf()],
        LOCATE_LIMITS,
        &mut || LOCATE_TIME_BUDGET / 2,
        &mut |_| {},
    );
    assert!(found[0].candidate.is_some());
    assert!(!found[0].search_stopped_early);
}

#[test]
fn a_demo_replaced_in_place_is_changed_and_an_untouched_one_is_not() {
    let scratch = Scratch::new("changed_demos");
    let same = scratch.path().join("same.dem");
    let replaced = scratch.path().join("replaced.dem");
    std::fs::write(&same, b"HLDEMO the scanned file").unwrap();
    std::fs::write(&replaced, b"HLDEMO the scanned file, too").unwrap();
    let demos = vec![
        KnownDemo {
            path: same.to_string_lossy().into_owned(),
            file_key: demo_file_key(&same).unwrap(),
        },
        KnownDemo {
            path: replaced.to_string_lossy().into_owned(),
            file_key: demo_file_key(&replaced).unwrap(),
        },
        KnownDemo {
            path: scratch
                .path()
                .join("gone.dem")
                .to_string_lossy()
                .into_owned(),
            file_key: "1-00".into(),
        },
    ];
    std::fs::write(&replaced, b"HLDEMO a different, shorter one").unwrap();
    assert_eq!(
        changed_demos_with_progress(&demos, |_, _| {}),
        vec![replaced.to_string_lossy().into_owned()]
    );
}

#[test]
fn the_changed_demo_check_counts_every_demo_it_reads() {
    let scratch = Scratch::new("changed_count");
    let same = scratch.path().join("same.dem");
    std::fs::write(&same, b"HLDEMO the scanned one").unwrap();
    let demos = vec![
        KnownDemo {
            path: same.to_string_lossy().into_owned(),
            file_key: demo_file_key(&same).unwrap(),
        },
        KnownDemo {
            path: scratch
                .path()
                .join("gone.dem")
                .to_string_lossy()
                .into_owned(),
            file_key: "1-00".into(),
        },
    ];
    let mut seen = Vec::new();
    let changed = changed_demos_with_progress(&demos, |done, total| seen.push((done, total)));
    assert!(changed.is_empty());
    assert_eq!(seen, vec![(1, 2), (2, 2)]);
}

#[test]
fn the_changed_demo_count_is_throttled_but_always_ends_on_the_last() {
    use std::sync::atomic::AtomicU32;
    let last = AtomicU32::new(u32::MAX);
    assert!(changed_progress_due(&last, 0, 1, 10));
    assert!(!changed_progress_due(&last, 10, 2, 10));
    assert!(!changed_progress_due(&last, 32, 3, 10));
    assert!(changed_progress_due(&last, 33, 4, 10));
    // The last demo goes out however soon after the one before.
    assert!(changed_progress_due(&last, 34, 10, 10));
}

#[test]
fn a_missing_demo_with_no_key_or_no_match_is_reported_without_a_candidate() {
    let scratch = Scratch::new("locate_none");
    std::fs::write(scratch.path().join("other.dem"), b"HLDEMO other").unwrap();
    let gone = scratch.path().join("gone.dem");
    let demos = vec![
        KnownDemo {
            path: gone.to_string_lossy().into_owned(),
            file_key: String::new(),
        },
        KnownDemo {
            path: scratch
                .path()
                .join("gone2.dem")
                .to_string_lossy()
                .into_owned(),
            file_key: "999-0000000000000000".to_string(),
        },
    ];
    let mut searched = Vec::new();
    let found = locate_missing_demos(&demos, &[scratch.path().to_path_buf()], &mut |n| {
        searched.push(n)
    });
    assert_eq!(found.len(), 2);
    assert!(found.iter().all(|m| m.candidate.is_none()), "{found:?}");
    // Only the demo with a key can be looked for (#958).
    assert_eq!(searched, vec![1]);
}

#[test]
fn nothing_missing_means_no_search_to_report() {
    let scratch = Scratch::new("locate_present");
    let here = scratch.path().join("here.dem");
    std::fs::write(&here, b"HLDEMO here").unwrap();
    let demos = vec![KnownDemo {
        path: here.to_string_lossy().into_owned(),
        file_key: demo_file_key(&here).unwrap(),
    }];
    let mut searched = Vec::new();
    let found = locate_missing_demos(&demos, &[scratch.path().to_path_buf()], &mut |n| {
        searched.push(n)
    });
    assert!(found.is_empty());
    assert!(searched.is_empty());
}

#[test]
fn missing_scan_paths_names_only_the_paths_that_are_gone() {
    let scratch = Scratch::new("missing_scan_paths");
    let here = scratch.path().to_string_lossy().to_string();
    let gone = scratch.path().join("deleted").to_string_lossy().to_string();
    assert_eq!(missing_scan_paths(&[here, gone.clone()]), vec![gone]);
}

fn fixture_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-fixtures/ci_fixture.dem")
}

#[test]
fn a_scanned_demo_carries_the_current_highlights_version() {
    let path = fixture_path();
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let analysis = ::analysis::Analysis::try_from_bytes(&bytes).unwrap();
    let highlights = (
        100.0,
        Vec::new(),
        true,
        None,
        0,
        None,
        std::sync::Arc::new(Vec::new()),
    );
    let demo = serialized_demo(&path, "ci_fixture.dem".into(), None, highlights, &analysis);
    assert_eq!(
        demo.highlights_version,
        ::analysis::highlights::HIGHLIGHTS_VERSION
    );
    assert!(
        demo.highlights_version > 0,
        "0 means 'scanned before it was recorded'"
    );
    let json = serde_json::to_value(&demo).unwrap();
    assert_eq!(
        json["highlights_version"],
        ::analysis::highlights::HIGHLIGHTS_VERSION
    );
}

#[test]
fn a_demo_from_an_older_project_has_highlights_version_0() {
    let old = r#"{"path":"D:/demos/a.dem","name":"a.dem","tickrate":100.0,"is_pov":true,
        "local_player_index":null,"playback_frames":10,"streaks":[]}"#;
    let demo: SerializedDemo = serde_json::from_str(old).unwrap();
    assert_eq!(demo.highlights_version, 0);
}

#[test]
fn the_frontend_mirrors_the_highlights_version() {
    let js = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/scan_version.js"),
    )
    .unwrap();
    let line = format!(
        "export const HIGHLIGHTS_VERSION = {};",
        ::analysis::highlights::HIGHLIGHTS_VERSION
    );
    assert!(
        js.contains(&line),
        "studio/src/scan_version.js must say `{line}`"
    );
}
