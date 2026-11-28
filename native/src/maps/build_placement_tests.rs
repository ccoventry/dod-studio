use super::*;
use crate::maps::build_library::{add_build, test_map_bytes};
use crate::test_support::Scratch;

struct Install {
    _dir: Scratch,
    addon: PathBuf,
}

/// An addon folder whose library holds builds 1 and 2 of `dod_x`.
fn install(tag: &str) -> Install {
    let dir = Scratch::new(format_args!("build_placement_{tag}"));
    let addon = dir.join("dod_addon");
    for seed in [1, 2] {
        let source = dir.join(format!("{seed}.bsp"));
        std::fs::write(&source, test_map_bytes(seed)).unwrap();
        add_build(&library(&addon), "dod_x", &source, &mut |_, _| {}).unwrap();
    }
    Install { _dir: dir, addon }
}

fn build(seed: u8) -> MapBuild {
    MapBuild {
        map_name: "dod_x".into(),
        checksum: bsp::map_checksum(&test_map_bytes(seed)).unwrap(),
    }
}

fn slot(addon: &Path) -> PathBuf {
    slot_path(addon, "dod_x")
}

#[test]
fn a_build_goes_into_the_empty_slot_and_comes_out_on_release() {
    let i = install("place");
    let (placement, outcomes) = place_builds(&i.addon, &[build(1)]);
    assert_eq!(outcomes, vec![PlaceOutcome::Placed]);
    assert_eq!(std::fs::read(slot(&i.addon)).unwrap(), test_map_bytes(1));
    assert!(
        !part_path(&i.addon, "dod_x").exists(),
        "the copy's own name is gone"
    );
    assert_eq!(placed_build(&i.addon, "dod_x"), Some(build(1).checksum));

    assert_eq!(placement.release(), 1);
    assert!(!slot(&i.addon).exists());
    assert!(!record_path(&i.addon).exists(), "nothing left to remember");
}

#[test]
fn a_file_studio_did_not_place_is_never_overwritten_or_removed() {
    let i = install("theirs");
    std::fs::create_dir_all(slot(&i.addon).parent().unwrap()).unwrap();
    std::fs::write(slot(&i.addon), test_map_bytes(9)).unwrap();

    let (placement, outcomes) = place_builds(&i.addon, &[build(1)]);
    assert_eq!(
        outcomes,
        vec![PlaceOutcome::SlotNotOurs {
            slot: slot(&i.addon)
        }]
    );
    assert!(placement.is_empty());
    assert!(!is_studio_placement(&i.addon, "dod_x"));
    assert_eq!(placement.release(), 0);
    assert_eq!(std::fs::read(slot(&i.addon)).unwrap(), test_map_bytes(9));
}

#[test]
fn a_file_that_appears_in_the_slot_during_the_copy_is_not_overwritten() {
    let dir = Scratch::new("build_placement_link_race");
    let from = dir.join("build.bsp");
    std::fs::write(&from, test_map_bytes(1)).unwrap();
    let slot = dir.join("maps").join("dod_x.bsp");
    std::fs::create_dir_all(slot.parent().unwrap()).unwrap();
    std::fs::write(&slot, b"the user's").unwrap();
    let part = dir.join("maps").join("dod_x.bsp.dodstudio-part");

    assert_eq!(link_copy(&from, &part, &slot).unwrap(), Linked::SlotTaken);
    assert_eq!(std::fs::read(&slot).unwrap(), b"the user's");
    assert!(!part.exists());
}

#[test]
fn a_placed_build_the_user_replaced_is_left_alone_on_release() {
    let i = install("replaced");
    let (placement, _) = place_builds(&i.addon, &[build(1)]);
    std::fs::write(slot(&i.addon), test_map_bytes(9)).unwrap();

    assert_eq!(placement.release(), 0);
    assert_eq!(std::fs::read(slot(&i.addon)).unwrap(), test_map_bytes(9));
    assert!(
        placed_sessions(&i.addon).is_empty(),
        "forgotten, not retried"
    );
}

#[test]
fn a_short_or_empty_file_in_the_slot_is_not_taken_for_studios() {
    // Copies land whole, so a prefix of the build is someone else's file.
    let i = install("short");
    let (placement, _) = place_builds(&i.addon, &[build(1)]);
    for short in [Vec::new(), test_map_bytes(1)[..10].to_vec()] {
        std::fs::write(slot(&i.addon), &short).unwrap();
        assert!(!is_studio_placement(&i.addon, "dod_x"));
    }
    assert_eq!(placement.release(), 0);
    assert!(slot(&i.addon).exists());
}

#[test]
fn a_build_the_library_lacks_is_not_placed() {
    let i = install("missing");
    let (placement, outcomes) = place_builds(&i.addon, &[build(7)]);
    assert_eq!(outcomes, vec![PlaceOutcome::NotInLibrary]);
    assert!(placement.is_empty());
    assert!(!slot(&i.addon).exists());
}

#[test]
fn a_later_launch_takes_over_the_same_build_and_an_earlier_release_leaves_it() {
    let i = install("takeover");
    let (first, _) = place_builds(&i.addon, &[build(1)]);
    let (second, outcomes) = place_builds(&i.addon, &[build(1)]);
    assert_eq!(outcomes, vec![PlaceOutcome::AlreadyInPlace]);

    assert_eq!(first.release(), 0, "the second launch's game needs it");
    assert!(slot(&i.addon).exists());
    assert_eq!(second.release(), 1);
    assert!(!slot(&i.addon).exists());
}

#[test]
fn another_build_studio_placed_is_replaced_for_a_new_launch() {
    let i = install("swap");
    let (first, _) = place_builds(&i.addon, &[build(1)]);
    let (second, outcomes) = place_builds(&i.addon, &[build(2)]);
    assert_eq!(outcomes, vec![PlaceOutcome::Placed]);
    assert_eq!(std::fs::read(slot(&i.addon)).unwrap(), test_map_bytes(2));
    assert_eq!(first.release(), 0);
    assert_eq!(second.release(), 1);
}

#[test]
fn a_slot_the_user_emptied_is_placed_again_and_only_the_new_launch_owns_it() {
    let i = install("emptied");
    let (first, _) = place_builds(&i.addon, &[build(1)]);
    std::fs::remove_file(slot(&i.addon)).unwrap();

    let (second, outcomes) = place_builds(&i.addon, &[build(1)]);
    assert_eq!(outcomes, vec![PlaceOutcome::Placed]);
    assert_eq!(
        placed_sessions(&i.addon).len(),
        1,
        "the stale entry is gone"
    );
    assert_eq!(first.release(), 0, "the old launch's release leaves it");
    assert!(slot(&i.addon).exists());
    assert_eq!(second.release(), 1);
}

#[test]
fn leftovers_of_a_closed_studio_are_found_and_removed_with_their_half_copies() {
    let i = install("leftovers");
    // A Studio that placed a build and never released it, and one that died
    // while copying the next.
    let (placement, _) = place_builds(&i.addon, &[build(1)]);
    std::mem::forget(placement);
    std::fs::write(part_path(&i.addon, "dod_x"), b"half").unwrap();

    let sessions = placed_sessions(&i.addon);
    assert_eq!(sessions.len(), 1);
    assert_eq!(release_sessions(&i.addon, &sessions), 1);
    assert!(!slot(&i.addon).exists());
    assert!(!part_path(&i.addon, "dod_x").exists());
}

#[test]
fn with_the_library_file_gone_a_placed_build_is_still_known_by_its_checksum() {
    let i = install("library_gone");
    let (placement, _) = place_builds(&i.addon, &[build(1)]);
    std::fs::remove_file(build_path(&library(&i.addon), "dod_x", build(1).checksum)).unwrap();
    assert_eq!(placement.release(), 1);
}

#[test]
fn an_unreadable_record_deletes_nothing_and_says_so() {
    let i = install("bad_record");
    let (placement, _) = place_builds(&i.addon, &[build(1)]);
    std::fs::write(record_path(&i.addon), b"{ not json").unwrap();
    assert_eq!(placement.release(), 0);
    assert!(slot(&i.addon).exists());
    assert_eq!(
        std::fs::read(record_path(&i.addon)).unwrap(),
        b"{ not json",
        "never written over"
    );
    let (again, outcomes) = place_builds(&i.addon, &[build(2)]);
    assert!(again.is_empty());
    assert_eq!(
        outcomes,
        vec![PlaceOutcome::RecordUnreadable {
            record: record_path(&i.addon)
        }]
    );
}

#[test]
fn the_record_stays_locked_while_another_studio_holds_it() {
    let i = install("locked");
    let held = open_unshared(&library(&i.addon).join(LOCK_FILE)).unwrap();
    assert!(lock_record_within(&i.addon, Duration::from_millis(200)).is_err());
    drop(held);
    assert!(lock_record_within(&i.addon, Duration::from_millis(200)).is_ok());
}

/// A process list that answers from a script, one answer per look.
fn scripted(answers: &[bool]) -> impl FnMut() -> bool + '_ {
    let mut at = 0;
    move || {
        let answer = answers[at.min(answers.len() - 1)];
        at += 1;
        answer
    }
}

#[test]
fn the_wait_lasts_until_a_game_that_started_has_closed() {
    let cancel = AtomicBool::new(false);
    let mut looks = scripted(&[false, false, true, true, false]);
    let end = wait_for_games_to_end(Duration::from_secs(60), Duration::ZERO, &cancel, &mut looks);
    assert_eq!(end, WaitEnd::GamesClosed);
}

#[test]
fn the_wait_ends_when_no_game_shows_up_in_time() {
    let cancel = AtomicBool::new(false);
    let mut never = || false;
    let end = wait_for_games_to_end(Duration::ZERO, Duration::ZERO, &cancel, &mut never);
    assert_eq!(end, WaitEnd::GamesClosed);
}

#[test]
fn the_wait_stops_when_cancelled_while_a_game_runs() {
    let cancel = AtomicBool::new(true);
    let mut always = || true;
    let end = wait_for_games_to_end(Duration::ZERO, Duration::ZERO, &cancel, &mut always);
    assert_eq!(end, WaitEnd::Cancelled);
}

#[test]
fn a_release_after_the_game_with_nothing_placed_starts_no_thread() {
    // Nothing to wait for: returns at once, whatever the process list says.
    let i = install("none");
    Placement::none(&i.addon).release_after_game(Duration::from_secs(60));
    assert!(placed_sessions(&i.addon).is_empty());
}

#[test]
fn closing_studio_with_no_game_removes_its_builds_and_with_a_game_leaves_them() {
    let i = install("exit");
    let (placement, _) = place_builds(&i.addon, &[build(1)]);
    let active = vec![(i.addon.clone(), placement.session)];

    assert_eq!(release_or_leave(&active, true), 0);
    assert!(slot(&i.addon).exists(), "the game may have loaded it");
    assert_eq!(release_or_leave(&active, false), 1);
    assert!(!slot(&i.addon).exists());
    forget_active(&i.addon, placement.session);
}

/// A record entry for `dod_x` build 1 placed by the process `pid` started at
/// `started`, as another Studio would write it.
fn record_owned_by(i: &Install, pid: u32, started: u64) {
    let (placement, _) = place_builds(&i.addon, &[build(1)]);
    placement.game_seen();
    forget_active(&i.addon, placement.session);
    let mut record = load_record(&i.addon).unwrap();
    for placed in &mut record.placed {
        placed.owner_pid = pid;
        placed.owner_started = started;
    }
    save_record(&i.addon, &record).unwrap();
}

#[test]
fn a_build_a_running_studio_placed_is_never_taken_as_a_leftover() {
    let i = install("live_owner");
    // Another process that is running: the System process stands in for a
    // second Studio.
    let sys = crate::sys::process::snapshot();
    let other = sys
        .processes()
        .values()
        .find(|p| p.pid().as_u32() != std::process::id() && p.start_time() > 0)
        .map(|p| (p.pid().as_u32(), p.start_time()))
        .unwrap();
    record_owned_by(&i, other.0, other.1);
    assert!(closed_studios_sessions(&i.addon).is_empty());
    assert_eq!(release_stale(&i.addon), 0);
    assert!(slot(&i.addon).exists());
}

#[test]
fn a_build_a_closed_studio_placed_is_a_leftover() {
    let i = install("dead_owner");
    record_owned_by(&i, u32::MAX - 7, 1);
    assert_eq!(closed_studios_sessions(&i.addon).len(), 1);
    assert_eq!(release_stale(&i.addon), 1);
    assert!(!slot(&i.addon).exists());
}

#[test]
fn a_launch_is_protected_from_the_moment_its_build_is_placed() {
    // A second launch cleaning up before its own game, while the first
    // launch's call still runs (before any release thread exists).
    let i = install("starting");
    let (first, _) = place_builds(&i.addon, &[build(1)]);
    assert_eq!(release_stale(&i.addon), 0);
    assert!(slot(&i.addon).exists());
    assert_eq!(first.release(), 1);
    assert!(
        !lock_or_recover(&STARTING).contains(&first.session),
        "a release ends the start"
    );
}

#[test]
fn once_its_game_was_seen_a_launchs_build_is_stale_when_no_game_runs() {
    let i = install("seen");
    let (first, _) = place_builds(&i.addon, &[build(1)]);
    first.game_seen();
    assert_eq!(release_stale(&i.addon), 1);
    assert!(!slot(&i.addon).exists());
}

#[test]
fn closing_studio_while_a_launch_is_starting_leaves_its_build() {
    let i = install("exit_starting");
    let (first, _) = place_builds(&i.addon, &[build(1)]);
    assert!(lock_or_recover(&STARTING).contains(&first.session));
    assert_eq!(exit_game_state(), GameState::Running);
    assert_eq!(first.release(), 1);
}

#[test]
fn a_release_with_no_game_to_wait_for_ends_the_start_at_once() {
    // A batch hands its builds over once its game has run.
    let i = install("batch_end");
    let (placement, _) = place_builds(&i.addon, &[build(1)]);
    let session = placement.session;
    placement.release_after_game(Duration::ZERO);
    assert!(!lock_or_recover(&STARTING).contains(&session));
}

#[test]
fn a_launch_whose_game_was_seen_is_no_longer_starting() {
    let i = install("game_seen");
    let (placement, _) = place_builds(&i.addon, &[build(1)]);
    placement.game_seen();
    assert!(!lock_or_recover(&STARTING).contains(&placement.session));
    assert_eq!(placement.release(), 1);
}

#[test]
fn another_build_a_running_studio_placed_is_never_replaced() {
    let i = install("held");
    let sys = crate::sys::process::snapshot();
    let other = sys
        .processes()
        .values()
        .find(|p| p.pid().as_u32() != std::process::id() && p.start_time() > 0)
        .map(|p| (p.pid().as_u32(), p.start_time()))
        .unwrap();
    record_owned_by(&i, other.0, other.1);
    let (placement, outcomes) = place_builds(&i.addon, &[build(2)]);
    assert_eq!(
        outcomes,
        vec![PlaceOutcome::HeldByAnotherStudio {
            slot: slot(&i.addon)
        }]
    );
    assert!(placement.is_empty());
    assert_eq!(std::fs::read(slot(&i.addon)).unwrap(), test_map_bytes(1));
}

#[test]
fn the_move_without_replace_never_overwrites() {
    let dir = Scratch::new("build_placement_move");
    let (from, to) = (dir.join("part"), dir.join("slot.bsp"));
    std::fs::write(&from, b"ours").unwrap();
    std::fs::write(&to, b"theirs").unwrap();
    let err = move_no_replace(&from, &to).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
    assert_eq!(std::fs::read(&to).unwrap(), b"theirs");
    std::fs::remove_file(&to).unwrap();
    move_no_replace(&from, &to).unwrap();
    assert_eq!(std::fs::read(&to).unwrap(), b"ours");
}

#[test]
fn the_start_up_sweep_removes_only_studios_unfinished_copies() {
    let i = install("sweep");
    let maps = i.addon.join("maps");
    std::fs::create_dir_all(&maps).unwrap();
    // As a copy writes it: `fs::copy` keeps the library file's (old) time.
    let library_file = build_path(&library(&i.addon), "dod_x", build(1).checksum);
    std::fs::copy(&library_file, part_path(&i.addon, "dod_x")).unwrap();
    std::fs::write(maps.join("dod_x.bsp"), b"the user's").unwrap();
    std::fs::write(maps.join("dod_x.txt"), b"text").unwrap();
    assert_eq!(
        sweep_part_files_within(&i.addon, Duration::from_millis(100)),
        1
    );
    assert!(maps.join("dod_x.bsp").exists() && maps.join("dod_x.txt").exists());
}

#[test]
fn the_sweep_leaves_a_copy_another_studio_is_writing() {
    // A copy holds the record's lock while it runs.
    let i = install("sweep_locked");
    std::fs::create_dir_all(i.addon.join("maps")).unwrap();
    let library_file = build_path(&library(&i.addon), "dod_x", build(1).checksum);
    std::fs::copy(&library_file, part_path(&i.addon, "dod_x")).unwrap();
    let held = open_unshared(&library(&i.addon).join(LOCK_FILE)).unwrap();
    assert_eq!(
        sweep_part_files_within(&i.addon, Duration::from_millis(100)),
        0
    );
    assert!(part_path(&i.addon, "dod_x").exists());
    drop(held);
}

#[test]
fn a_library_file_whose_bytes_are_another_build_is_not_placed() {
    let i = install("wrong_bytes");
    let file = build_path(&library(&i.addon), "dod_x", build(1).checksum);
    std::fs::write(&file, test_map_bytes(9)).unwrap();
    let (placement, outcomes) = place_builds(&i.addon, &[build(1)]);
    assert_eq!(outcomes, vec![PlaceOutcome::LibraryFileWrong { file }]);
    assert!(placement.is_empty());
    assert!(!slot(&i.addon).exists());
}
