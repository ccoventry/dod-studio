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
