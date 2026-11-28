use super::*;
use crate::maps::build_library::{add_build, test_map_bytes};
use crate::test_support::Scratch;

fn checksum(seed: u8) -> u32 {
    bsp::map_checksum(&test_map_bytes(seed)).unwrap()
}

fn demo(name: &str, map: &str, need: Need) -> DemoMap {
    DemoMap {
        demo: PathBuf::from(name),
        map_name: map.into(),
        need,
    }
}

fn reference(map: &str, checksum: u32) -> MapReference {
    MapReference {
        map_name: map.into(),
        expected_checksum: Some(checksum),
    }
}

/// An install root with `dod\maps\dod_x.bsp` (build 1) and a library
/// holding build 2.
fn game_root(tag: &str) -> Scratch {
    let root = Scratch::new(format_args!("build_plan_{tag}"));
    let maps = mod_dir(&root).join("maps");
    std::fs::create_dir_all(&maps).unwrap();
    std::fs::write(maps.join("dod_x.bsp"), test_map_bytes(1)).unwrap();
    let source = root.join("old.bsp");
    std::fs::write(&source, test_map_bytes(2)).unwrap();
    add_build(&library_dir(&root), "dod_x", &source, &mut |_, _| {}).unwrap();
    root
}

/// A demo file of `dod_x` whose header says build `checksum`.
fn write_demo(root: &Path, name: &str, checksum: u32) -> PathBuf {
    let mut header = vec![0u8; crate::patch::DEMO_HEADER_SIZE];
    header[..8].copy_from_slice(b"HLDEMO\0\0");
    header[16..21].copy_from_slice(b"dod_x");
    header[536..540].copy_from_slice(&checksum.to_le_bytes());
    let path = root.join(name);
    std::fs::write(&path, &header).unwrap();
    path
}

fn slot(root: &Path) -> PathBuf {
    slot_path(&addon_dir(root), "dod_x")
}

#[test]
fn a_wrong_build_the_library_holds_plays_from_the_library() {
    let root = game_root("from_library");
    assert_eq!(
        status_for_launch(&reference("dod_x", checksum(2)), &root),
        MapStatus::FromLibrary {
            checksum: checksum(2)
        }
    );
    assert_eq!(
        status_for_launch(&reference("dod_x", checksum(1)), &root),
        MapStatus::Ok {
            checksum: checksum(1)
        }
    );
    assert_eq!(
        status_for_launch(&reference("dod_x", checksum(3)), &root),
        MapStatus::WrongBuild {
            expected: checksum(3),
            found: checksum(1)
        },
        "no build of its own: still a different version"
    );
}

#[test]
fn a_missing_map_the_library_holds_plays_from_the_library() {
    let root = game_root("missing");
    std::fs::remove_file(mod_dir(&root).join("maps").join("dod_x.bsp")).unwrap();
    assert_eq!(
        status_for_launch(&reference("dod_x", checksum(2)), &root),
        MapStatus::FromLibrary {
            checksum: checksum(2)
        }
    );
}

#[test]
fn the_users_own_map_in_the_addon_folder_is_the_one_compared() {
    let root = game_root("users_addon");
    std::fs::create_dir_all(slot(&root).parent().unwrap()).unwrap();
    std::fs::write(slot(&root), test_map_bytes(9)).unwrap();
    assert_eq!(users_slot_file(&root, "dod_x"), Some(slot(&root)));
    assert_eq!(
        status_for_launch(&reference("dod_x", checksum(2)), &root),
        MapStatus::WrongBuild {
            expected: checksum(2),
            found: checksum(9)
        },
        "Studio can't put its build over the user's file"
    );
    assert_eq!(
        status_for_launch(&reference("dod_x", checksum(9)), &root),
        MapStatus::Ok {
            checksum: checksum(9)
        }
    );
}

#[test]
fn a_build_studio_placed_is_not_mistaken_for_the_users() {
    let root = game_root("ours_in_addon");
    let (placement, _) = place_builds(
        &addon_dir(&root),
        &[MapBuild {
            map_name: "dod_x".into(),
            checksum: checksum(2),
        }],
    );
    assert_eq!(users_slot_file(&root, "dod_x"), None);
    assert_eq!(
        status_for_launch(&reference("dod_x", checksum(1)), &root),
        MapStatus::Ok {
            checksum: checksum(1)
        },
        "compared with the installed map under it"
    );
    placement.release();
}

#[test]
fn a_batch_places_each_library_build_once() {
    let maps = plan(&[
        demo("a.dem", "dod_x", Need::Library(2)),
        demo("b.dem", "dod_x", Need::Library(2)),
        demo("c.dem", "dod_y", Need::InstalledIsRight(5)),
        demo("d.dem", "dod_z", Need::NothingBetter),
    ]);
    assert_eq!(
        maps.place,
        vec![MapBuild {
            map_name: "dod_x".into(),
            checksum: 2
        }]
    );
    assert!(maps.left_out.is_empty());
}

#[test]
fn a_demo_on_the_installed_build_keeps_it_for_the_batch_wherever_it_is() {
    // A demo that plays right on the installed map never plays wrong for an
    // old demo's sake, even when the old one comes first.
    let maps = plan(&[
        demo("old.dem", "dod_x", Need::Library(2)),
        demo("new.dem", "dod_x", Need::InstalledIsRight(1)),
        demo("older.dem", "dod_x", Need::Library(3)),
    ]);
    assert!(maps.place.is_empty());
    assert_eq!(
        maps.left_out,
        vec![
            LeftOut {
                demo: "old.dem".into(),
                map_name: "dod_x".into(),
                wanted: 2,
                playing: None,
            },
            LeftOut {
                demo: "older.dem".into(),
                map_name: "dod_x".into(),
                wanted: 3,
                playing: None,
            },
        ]
    );
}

#[test]
fn with_no_demo_on_the_installed_build_the_first_old_build_wanted_is_used() {
    let maps = plan(&[
        demo("old.dem", "dod_x", Need::Library(2)),
        demo("older.dem", "dod_x", Need::Library(3)),
        demo("other.dem", "dod_y", Need::InstalledIsRight(5)),
    ]);
    assert_eq!(
        maps.place,
        vec![MapBuild {
            map_name: "dod_x".into(),
            checksum: 2
        }]
    );
    assert_eq!(maps.left_out.len(), 1);
    assert_eq!(maps.left_out[0].playing, Some(2));
}

#[test]
fn demos_are_read_for_their_maps_in_order() {
    let root = game_root("demo_maps");
    let old = write_demo(&root, "old.dem", checksum(2));
    let junk = root.join("junk.dem");
    std::fs::write(&junk, b"no").unwrap();

    assert_eq!(
        demo_maps(&[junk, old.clone()], &root),
        vec![DemoMap {
            demo: old,
            map_name: "dod_x".into(),
            need: Need::Library(checksum(2)),
        }]
    );
}

#[test]
fn a_launch_puts_the_build_in_place_and_says_nothing() {
    let root = game_root("launch");
    let old = write_demo(&root, "old.dem", checksum(2));
    let launch = place_for_launch_when(&[old], &root, GameState::NoneRunning);
    assert!(launch.warnings.is_empty(), "{:?}", launch.warnings);
    assert_eq!(std::fs::read(slot(&root)).unwrap(), test_map_bytes(2));
    assert_eq!(launch.placement.release(), 1);
}

#[test]
fn a_launch_while_a_game_runs_places_nothing_and_says_which_demos_play_wrong() {
    let root = game_root("game_running");
    let old = write_demo(&root, "old.dem", checksum(2));
    let other = write_demo(&root, "other_old.dem", checksum(2));
    let launch = place_for_launch_when(&[old, other], &root, GameState::Running);
    assert!(launch.placement.is_empty());
    assert!(!slot(&root).exists());
    assert_eq!(
        launch.warnings,
        vec![msg::open_game_warning(
            "dod_x",
            &["old.dem".to_string(), "other_old.dem".to_string()]
        )],
        "one warning per map, not one per demo"
    );
}

#[test]
fn a_mixed_launch_warns_about_the_demos_left_out() {
    let root = game_root("mixed");
    let old = write_demo(&root, "old.dem", checksum(2));
    let new = write_demo(&root, "new.dem", checksum(1));
    let launch = place_for_launch_when(&[old, new], &root, GameState::NoneRunning);
    assert!(launch.placement.is_empty(), "the installed build stays");
    assert_eq!(
        launch.warnings,
        vec![msg::left_out_warning("dod_x", &["old.dem".to_string()])]
    );
}

#[test]
fn a_build_kept_out_by_the_users_file_is_a_warning_not_only_a_log_line() {
    let root = game_root("taken_later");
    // Planned for the library build, then a file of the user's landed in the
    // slot before the copy (as between the map check and the launch).
    let wants = vec![MapBuild {
        map_name: "dod_x".into(),
        checksum: checksum(2),
    }];
    let outcomes = vec![PlaceOutcome::SlotNotOurs { slot: slot(&root) }];
    assert_eq!(
        outcome_warnings(&wants, &outcomes),
        vec![msg::slot_taken_warning("dod_x", slot(&root).display())]
    );
    assert_eq!(
        outcome_warnings(&wants, &[PlaceOutcome::NotInLibrary]),
        vec![msg::not_in_library_warning("dod_x", checksum(2))]
    );
}

#[test]
fn an_open_game_with_the_installed_map_warns_for_an_old_demo() {
    let root = game_root("open_installed");
    let old = write_demo(&root, "old.dem", checksum(2));
    let new = write_demo(&root, "new.dem", checksum(1));
    assert_eq!(
        open_game_warnings(&[old, new], &root),
        vec![msg::open_game_warning("dod_x", &["old.dem".to_string()])]
    );
}

#[test]
fn an_open_game_holding_an_old_build_warns_for_a_current_demo() {
    // The reverse case: Studio put the old build in place for this game, and
    // a demo on the installed build is sent to it.
    let root = game_root("open_old");
    let (placement, _) = place_builds(
        &addon_dir(&root),
        &[MapBuild {
            map_name: "dod_x".into(),
            checksum: checksum(2),
        }],
    );
    let old = write_demo(&root, "old.dem", checksum(2));
    let new = write_demo(&root, "new.dem", checksum(1));
    assert_eq!(build_running_game_reads(&root, "dod_x"), Some(checksum(2)));
    assert_eq!(
        open_game_warnings(&[old, new], &root),
        vec![msg::open_game_warning("dod_x", &["new.dem".to_string()])]
    );
    placement.release();
}

#[test]
fn a_build_left_in_place_is_taken_out_before_a_current_demos_launch() {
    // Force Relaunch closes the game and starts the next at once: a build
    // the last launch placed would play this current demo on the old map.
    let root = game_root("stale");
    let (left, _) = place_builds(
        &addon_dir(&root),
        &[MapBuild {
            map_name: "dod_x".into(),
            checksum: checksum(2),
        }],
    );
    std::mem::forget(left);
    let new = write_demo(&root, "new.dem", checksum(1));
    let launch = place_for_launch_when(&[new], &root, GameState::NoneRunning);
    assert!(!slot(&root).exists(), "the old build is out");
    assert!(launch.warnings.is_empty(), "{:?}", launch.warnings);
}

#[test]
fn a_running_game_with_nothing_to_place_still_says_which_demos_play_wrong() {
    let root = game_root("running_nothing_to_place");
    let (placement, _) = place_builds(
        &addon_dir(&root),
        &[MapBuild {
            map_name: "dod_x".into(),
            checksum: checksum(2),
        }],
    );
    let new = write_demo(&root, "new.dem", checksum(1));
    let launch = place_for_launch_when(&[new], &root, GameState::Running);
    assert!(slot(&root).exists(), "the running game may have loaded it");
    assert_eq!(
        launch.warnings,
        vec![msg::open_game_warning("dod_x", &["new.dem".to_string()])]
    );
    placement.release();
}

#[test]
fn a_long_list_of_demos_is_shortened_in_the_warning() {
    let names: Vec<String> = (1..=40).map(|n| format!("d{n}.dem")).collect();
    let warning = msg::open_game_warning("dod_x", &names);
    assert!(warning.starts_with("40 demos of dod_x"), "{warning}");
    assert!(
        warning.contains("d1.dem, d2.dem, d3.dem and 37 more"),
        "{warning}"
    );
    assert!(!warning.contains("d4.dem"));
}
