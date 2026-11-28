//! Which build each demo will play on, and which library builds a launch
//! puts in place (#1547).
//!
//! The map check (`map_check::status_of`) compares a demo with the installed
//! map; this adds the two things a launch from Studio changes. A map file of
//! the user's own in `dod_addon\maps` is what the game reads first, so it is
//! the one compared. And where the installed map is missing or another build
//! while Studio's library holds the demo's, the demo plays right
//! (`MapStatus::FromLibrary`): Studio puts that build in place before it
//! starts the game.
//!
//! One game plays one build of each map (`build_placement`), so a batch whose
//! demos want different builds of one map plays them all on the build its
//! first such demo wants, and names the others in the activity log.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::build_library::{find_build, library_dir};
use super::build_placement::{MapBuild, Placement, is_studio_placement, place_builds, slot_path};
use super::map_check::{MapReference, MapStatus, map_reference, status_of};
use crate::log_markdown;
use crate::messages::map_builds as msg;
use crate::shared::paths::{addon_dir, mod_dir};

/// What will play for `reference` when Studio starts the game of the install
/// in `game_root` (the folder holding `hl.exe`): the map check against the
/// map the game reads first, with the library's build counted in.
pub fn status_for_launch(reference: &MapReference, game_root: &Path) -> MapStatus {
    let addon = addon_dir(game_root);
    if slot_path(&addon, &reference.map_name).is_file()
        && !is_studio_placement(&addon, &reference.map_name)
    {
        return status_of(reference, &addon.join("maps"));
    }
    let installed = status_of(reference, &mod_dir(game_root).join("maps"));
    with_library(installed, reference, &library_dir(game_root))
}

/// `status` (against the installed map), or `FromLibrary` when that map is
/// missing, unreadable or another build and `library` holds the demo's.
pub fn with_library(status: MapStatus, reference: &MapReference, library: &Path) -> MapStatus {
    let replaceable = matches!(
        status,
        MapStatus::WrongBuild { .. } | MapStatus::Missing | MapStatus::Unreadable { .. }
    );
    match reference.expected_checksum {
        Some(checksum)
            if replaceable && find_build(library, &reference.map_name, checksum).is_some() =>
        {
            MapStatus::FromLibrary { checksum }
        }
        _ => status,
    }
}

/// What a demo needs from a launch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Need {
    /// The installed map is its build (this one).
    InstalledIsRight(u32),
    /// The library's build, put in place.
    Library(u32),
    /// Nothing a launch can change: no build stated, or none to be had.
    NothingBetter,
}

impl Need {
    /// The need a map check's answer stands for.
    pub fn of(status: &MapStatus) -> Self {
        match status {
            MapStatus::Ok { checksum } => Self::InstalledIsRight(*checksum),
            MapStatus::FromLibrary { checksum } => Self::Library(*checksum),
            _ => Self::NothingBetter,
        }
    }
}

/// One demo of a launch, in play order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DemoMap {
    pub demo: PathBuf,
    pub map_name: String,
    pub need: Need,
}

/// A demo that won't play on its build, because an earlier demo of the same
/// launch needs another build of its map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeftOut {
    pub demo: PathBuf,
    pub map_name: String,
    pub wanted: u32,
    /// The library build in place instead, or `None` for the installed map.
    pub playing: Option<u32>,
}

/// The builds a launch puts in place, and the demos that won't get theirs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LaunchMaps {
    pub place: Vec<MapBuild>,
    pub left_out: Vec<LeftOut>,
}

/// Plans a launch that plays `demos` in this order: for each map, the first
/// demo that needs a particular build decides it (one game, one build of a
/// map), and every later demo of that map wanting another is left out.
pub fn plan(demos: &[DemoMap]) -> LaunchMaps {
    let mut chosen: HashMap<&str, Option<u32>> = HashMap::new();
    let mut maps = LaunchMaps::default();
    for demo in demos {
        let (wanted, from_library) = match demo.need {
            Need::InstalledIsRight(checksum) => (checksum, None),
            Need::Library(checksum) => (checksum, Some(checksum)),
            Need::NothingBetter => continue,
        };
        match chosen.get(demo.map_name.as_str()) {
            None => {
                chosen.insert(&demo.map_name, from_library);
                if let Some(checksum) = from_library {
                    maps.place.push(MapBuild {
                        map_name: demo.map_name.clone(),
                        checksum,
                    });
                }
            }
            Some(playing) if *playing == from_library => {}
            Some(playing) => maps.left_out.push(LeftOut {
                demo: demo.demo.clone(),
                map_name: demo.map_name.clone(),
                wanted,
                playing: *playing,
            }),
        }
    }
    maps
}

/// Each demo's map and need, in order, for the install in `game_root`. A
/// demo whose header can't be read is skipped: it can't name a map. Each
/// map file is read once however many demos want it.
pub fn demo_maps(demos: &[PathBuf], game_root: &Path) -> Vec<DemoMap> {
    let mut seen: HashMap<(String, Option<u32>), Need> = HashMap::new();
    demos
        .iter()
        .filter_map(|demo| {
            let reference = map_reference(demo).ok()?;
            let key = (reference.map_name.clone(), reference.expected_checksum);
            let need = *seen
                .entry(key)
                .or_insert_with(|| Need::of(&status_for_launch(&reference, game_root)));
            Some(DemoMap {
                demo: demo.clone(),
                map_name: reference.map_name,
                need,
            })
        })
        .collect()
}

/// Plans `demos` for a fresh game of the install in `game_root`, logs the
/// demos left out and the builds used, and puts the builds in place. Puts
/// nothing in place while a game runs: it may have loaded another build of
/// the map already, and loading a second is a fatal error.
pub fn place_for_launch(demos: &[PathBuf], game_root: &Path) -> Placement {
    let maps = plan_and_log(demos, game_root);
    let addon = addon_dir(game_root);
    if maps.place.is_empty() {
        return Placement::none(&addon);
    }
    if crate::sys::process::is_running(&[crate::sys::process::GAME_EXE]) {
        log_markdown(&msg::game_running(maps.place.len()));
        return Placement::none(&addon);
    }
    place_builds(&addon, &maps.place).0
}

/// [`plan`] for `demos`, with the left-out demos and the builds the launch
/// uses written to the activity log.
pub fn plan_and_log(demos: &[PathBuf], game_root: &Path) -> LaunchMaps {
    let maps = plan(&demo_maps(demos, game_root));
    for left in &maps.left_out {
        log_markdown(&msg::left_out(
            left.demo.display(),
            &left.map_name,
            left.wanted,
            left.playing,
        ));
    }
    if !maps.place.is_empty() {
        let names: Vec<String> = maps
            .place
            .iter()
            .map(|b| format!("{} {}", b.map_name, msg::build(b.checksum)))
            .collect();
        log_markdown(&msg::launch_places(maps.place.len(), names.join(", ")));
    }
    maps
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::maps::bsp;
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
        let slot = slot_path(&addon_dir(&root), "dod_x");
        std::fs::create_dir_all(slot.parent().unwrap()).unwrap();
        std::fs::write(&slot, test_map_bytes(9)).unwrap();
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
    fn two_demos_needing_different_builds_of_one_map_play_on_the_first_ones() {
        let maps = plan(&[
            demo("old.dem", "dod_x", Need::Library(2)),
            demo("new.dem", "dod_x", Need::InstalledIsRight(1)),
            demo("older.dem", "dod_x", Need::Library(3)),
        ]);
        assert_eq!(
            maps.place,
            vec![MapBuild {
                map_name: "dod_x".into(),
                checksum: 2
            }]
        );
        assert_eq!(
            maps.left_out,
            vec![
                LeftOut {
                    demo: "new.dem".into(),
                    map_name: "dod_x".into(),
                    wanted: 1,
                    playing: Some(2),
                },
                LeftOut {
                    demo: "older.dem".into(),
                    map_name: "dod_x".into(),
                    wanted: 3,
                    playing: Some(2),
                },
            ]
        );
    }

    #[test]
    fn a_first_demo_on_the_installed_build_keeps_it_for_the_batch() {
        let maps = plan(&[
            demo("new.dem", "dod_x", Need::InstalledIsRight(1)),
            demo("old.dem", "dod_x", Need::Library(2)),
        ]);
        assert!(maps.place.is_empty());
        assert_eq!(maps.left_out.len(), 1);
        assert_eq!(maps.left_out[0].playing, None);
    }

    #[test]
    fn demos_are_read_for_their_maps_in_order() {
        let root = game_root("demo_maps");
        let mut header = vec![0u8; crate::patch::DEMO_HEADER_SIZE];
        header[..8].copy_from_slice(b"HLDEMO\0\0");
        header[16..21].copy_from_slice(b"dod_x");
        header[536..540].copy_from_slice(&checksum(2).to_le_bytes());
        let old = root.join("old.dem");
        std::fs::write(&old, &header).unwrap();
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
}
