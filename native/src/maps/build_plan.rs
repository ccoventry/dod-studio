//! Which build each demo will play on, and which library builds a launch
//! puts in place (#1547).
//!
//! The map check (`map_check::status_of`) compares a demo with the installed
//! map; this adds the two things a launch from Studio changes. A map file of
//! the user's own in `dod_addon\maps` is what the game reads first, so it is
//! the one compared. And where the installed map is missing or another build
//! while Studio's library holds the demo's, the demo plays right when Studio
//! starts the game for it (`MapStatus::FromLibrary`): Studio puts that build
//! in place first.
//!
//! One game plays one build of each map (`build_placement`). A batch whose
//! demos want different builds of one map keeps the installed build when
//! any of them wants it, so a demo that played right never plays wrong; only
//! when none does, the first library build wanted is put in place. The
//! others are left out and the user is told which. A game already running
//! (an open game a batch or preview reuses, or one in the way of a fresh
//! launch) keeps what it has loaded: its demos are compared against what it
//! reads, and the user is told about each that won't play on its build.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use super::bsp;
use super::build_library::{find_build, library_dir};
pub use super::build_placement::GameState;
use super::build_placement::{
    MapBuild, PlaceOutcome, Placement, place_builds, placed_build, release_stale, slot_path,
};
use super::map_check::{MapReference, MapStatus, map_path, map_reference, status_of};
use crate::log_markdown;
use crate::messages::map_builds as msg;
use crate::shared::paths::{addon_dir, mod_dir};

/// Whether the unreadable-record warning was shown already this run: once
/// is enough, the log has every time.
static RECORD_WARNED: AtomicBool = AtomicBool::new(false);

/// What will play for `reference` when Studio starts a fresh game of the
/// install in `game_root` (the folder holding `hl.exe`): the map check
/// against the map the game reads first, with the library's build counted
/// in. A game already running may read something else
/// ([`open_game_warnings`]).
pub fn status_for_launch(reference: &MapReference, game_root: &Path) -> MapStatus {
    if let Some(slot) = users_slot_file(game_root, &reference.map_name) {
        return status_of(reference, slot.parent().unwrap_or(&slot));
    }
    let installed = status_of(reference, &mod_dir(game_root).join("maps"));
    with_library(installed, reference, &library_dir(game_root))
}

/// The user's own `dod_addon\maps\<map>.bsp`, when one is there: the game
/// plays it, and Studio never puts a build over it.
pub fn users_slot_file(game_root: &Path, map_name: &str) -> Option<PathBuf> {
    let addon = addon_dir(game_root);
    let slot = slot_path(&addon, map_name);
    (slot.is_file() && placed_build(&addon, map_name).is_none()).then_some(slot)
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

/// The build a game running now reads for `map_name`: Studio's build in the
/// slot, else the user's file there, else the installed map. `None` when
/// there is none or it can't be read.
pub fn build_running_game_reads(game_root: &Path, map_name: &str) -> Option<u32> {
    let addon = addon_dir(game_root);
    if let Some(checksum) = placed_build(&addon, map_name) {
        return Some(checksum);
    }
    let slot = slot_path(&addon, map_name);
    let file = if slot.is_file() {
        slot
    } else {
        map_path(&mod_dir(game_root).join("maps"), map_name)
    };
    bsp::map_checksum_of_file(&file).ok()
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

    /// The build the demo was recorded on, when a launch can give it one.
    pub fn wanted(&self) -> Option<u32> {
        match self {
            Self::InstalledIsRight(checksum) | Self::Library(checksum) => Some(*checksum),
            Self::NothingBetter => None,
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

impl DemoMap {
    /// The demo's file name, for the user.
    pub fn name(&self) -> String {
        file_name(&self.demo)
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// A demo that won't play on its build, because another demo of the same
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

/// Plans a launch that plays `demos` in this order. For each map: the
/// installed build when any demo wants it (a demo that plays right on it
/// never plays wrong for another's sake), else the first library build
/// wanted. Every demo wanting another build of that map is left out.
pub fn plan(demos: &[DemoMap]) -> LaunchMaps {
    let wants_installed: HashSet<&str> = demos
        .iter()
        .filter(|d| matches!(d.need, Need::InstalledIsRight(_)))
        .map(|d| d.map_name.as_str())
        .collect();
    let mut chosen: HashMap<&str, Option<u32>> =
        wants_installed.iter().map(|m| (*m, None)).collect();
    let mut maps = LaunchMaps::default();
    for demo in demos {
        let (wanted, from_library) = match demo.need {
            Need::InstalledIsRight(checksum) => (checksum, None),
            Need::Library(checksum) => (checksum, Some(checksum)),
            Need::NothingBetter => continue,
        };
        let playing = *chosen.entry(&demo.map_name).or_insert_with(|| {
            if let Some(checksum) = from_library {
                maps.place.push(MapBuild {
                    map_name: demo.map_name.clone(),
                    checksum,
                });
            }
            from_library
        });
        if playing != from_library {
            maps.left_out.push(LeftOut {
                demo: demo.demo.clone(),
                map_name: demo.map_name.clone(),
                wanted,
                playing,
            });
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

/// [`plan`] for `demos` on the install in `game_root`, with nothing put in
/// place: what the confirm before a batch shows.
pub fn plan_for(demos: &[PathBuf], game_root: &Path) -> LaunchMaps {
    plan(&demo_maps(demos, game_root))
}

/// What a launch did with its maps: the builds in place for its game, and
/// the warnings for the user (demos that won't play on their build, builds
/// that couldn't be put in place). Each warning is in the activity log too.
#[derive(Debug)]
pub struct LaunchOutcome {
    pub placement: Placement,
    pub warnings: Vec<String>,
}

/// Plans `demos` for a fresh game of the install in `game_root` and puts the
/// builds in place, unless a game runs or starts already: it may have loaded
/// another build of a map, and loading a second is a fatal error.
pub fn place_for_launch(demos: &[PathBuf], game_root: &Path) -> LaunchOutcome {
    place_for_launch_when(demos, game_root, GameState::now())
}

/// [`place_for_launch`] with `games` the state of the games now.
pub fn place_for_launch_when(
    demos: &[PathBuf],
    game_root: &Path,
    games: GameState,
) -> LaunchOutcome {
    let addon = addon_dir(game_root);
    if games == GameState::NoneRunning {
        // A build an earlier launch left (a failed removal, a game closed
        // and started again in a moment) would play this launch's demos of
        // that map on the old build without a word.
        release_stale(&addon);
    }
    let demo_maps = demo_maps(demos, game_root);
    let maps = plan(&demo_maps);
    let mut warnings = left_out_warnings(&maps.left_out);
    if games == GameState::Running {
        if !maps.place.is_empty() {
            log_markdown(&msg::game_running(maps.place.len()));
        }
        warnings.extend(running_game_warnings(&demo_maps, game_root));
        return LaunchOutcome {
            placement: Placement::none(&addon),
            warnings,
        };
    }
    let (placement, outcomes) = if maps.place.is_empty() {
        (Placement::none(&addon), Vec::new())
    } else {
        place_builds(&addon, &maps.place)
    };
    log_in_place(&maps.place, &outcomes);
    warnings.extend(outcome_warnings(&maps.place, &outcomes));
    warnings.extend(stale_build_warnings(&demo_maps, &maps, &addon));
    LaunchOutcome {
        placement,
        warnings,
    }
}

/// The demos that want the installed build of a map while a build Studio
/// placed is still in its slot (one `release_stale` could not remove: its
/// delete failed, or another running Studio placed it), one warning per map.
fn stale_build_warnings(demos: &[DemoMap], maps: &LaunchMaps, addon: &Path) -> Vec<String> {
    let placing: HashSet<&str> = maps.place.iter().map(|b| b.map_name.as_str()).collect();
    let mut stale: HashMap<&str, bool> = HashMap::new();
    let affected = demos.iter().filter(|demo| {
        matches!(demo.need, Need::InstalledIsRight(_))
            && !placing.contains(demo.map_name.as_str())
            && *stale
                .entry(&demo.map_name)
                .or_insert_with(|| placed_build(addon, &demo.map_name).is_some())
    });
    by_map(affected)
        .into_iter()
        .map(|(map, names)| {
            log_markdown(&msg::stale_build_demos(map, &names));
            msg::stale_build_warning(map, &names)
        })
        .collect()
}

/// `demos`' file names grouped by map, maps in the order first seen.
fn by_map<'a>(demos: impl Iterator<Item = &'a DemoMap>) -> Vec<(&'a str, Vec<String>)> {
    let mut grouped: Vec<(&str, Vec<String>)> = Vec::new();
    for demo in demos {
        match grouped.iter_mut().find(|(map, _)| *map == demo.map_name) {
            Some((_, names)) => names.push(demo.name()),
            None => grouped.push((&demo.map_name, vec![demo.name()])),
        }
    }
    grouped
}

/// The warnings for `demos` played in a game that runs already (an open game
/// a batch or preview reuses): each demo a fresh launch would play on its
/// build, but this game won't, because it reads another build of the map
/// (an old build Studio put in place for it, or the installed one).
pub fn open_game_warnings(demos: &[PathBuf], game_root: &Path) -> Vec<String> {
    running_game_warnings(&demo_maps(demos, game_root), game_root)
}

fn running_game_warnings(demos: &[DemoMap], game_root: &Path) -> Vec<String> {
    let mut reads: HashMap<&str, Option<u32>> = HashMap::new();
    let wrong: Vec<&DemoMap> = demos
        .iter()
        .filter(|demo| {
            let Some(wanted) = demo.need.wanted() else {
                return false;
            };
            let read = *reads
                .entry(&demo.map_name)
                .or_insert_with(|| build_running_game_reads(game_root, &demo.map_name));
            read != Some(wanted)
        })
        .collect();
    if !wrong.is_empty() {
        log_markdown(&msg::open_game_keeps_its_maps(wrong.len()));
    }
    by_map(wrong.into_iter())
        .into_iter()
        .map(|(map, names)| {
            log_markdown(&msg::open_game_demos(map, &names));
            msg::open_game_warning(map, &names)
        })
        .collect()
}

/// One warning per map with demos left out, naming them; each also logged.
fn left_out_warnings(left_out: &[LeftOut]) -> Vec<String> {
    let mut by_map: Vec<(&str, Vec<String>)> = Vec::new();
    for left in left_out {
        log_markdown(&msg::left_out(
            left.demo.display(),
            &left.map_name,
            left.wanted,
            left.playing,
        ));
        let name = file_name(&left.demo);
        match by_map.iter_mut().find(|(map, _)| *map == left.map_name) {
            Some((_, demos)) => demos.push(name),
            None => by_map.push((&left.map_name, vec![name])),
        }
    }
    by_map
        .into_iter()
        .map(|(map, demos)| msg::left_out_warning(map, &demos))
        .collect()
}

/// The activity-log line for the builds that are in place, from what
/// `place_builds` did.
fn log_in_place(wants: &[MapBuild], outcomes: &[PlaceOutcome]) {
    let names: Vec<String> = wants
        .iter()
        .zip(outcomes)
        .filter(|(_, outcome)| outcome.is_in_place())
        .map(|(b, _)| format!("{} {}", b.map_name, msg::build(b.checksum)))
        .collect();
    if !names.is_empty() {
        log_markdown(&msg::launch_places(names.len(), names.join(", ")));
    }
}

/// A warning for each build not put in place, and the unreadable record
/// once per run.
fn outcome_warnings(wants: &[MapBuild], outcomes: &[PlaceOutcome]) -> Vec<String> {
    wants
        .iter()
        .zip(outcomes)
        .filter_map(|(want, outcome)| match outcome {
            PlaceOutcome::Placed | PlaceOutcome::AlreadyInPlace => None,
            PlaceOutcome::SlotNotOurs { slot } => {
                Some(msg::slot_taken_warning(&want.map_name, slot.display()))
            }
            PlaceOutcome::NotInLibrary => {
                Some(msg::not_in_library_warning(&want.map_name, want.checksum))
            }
            PlaceOutcome::HeldByAnotherStudio { .. } => {
                Some(msg::held_by_another_studio_warning(&want.map_name))
            }
            PlaceOutcome::LibraryFileWrong { file } => Some(msg::library_file_wrong_warning(
                &want.map_name,
                file.display(),
            )),
            PlaceOutcome::RecordUnreadable { record } => (!RECORD_WARNED
                .swap(true, Ordering::Relaxed))
            .then(|| msg::record_unreadable_warning(record.display())),
            PlaceOutcome::Failed(why) => Some(msg::place_failed_warning(&want.map_name, why)),
        })
        .collect()
}

#[cfg(test)]
#[path = "build_plan_tests.rs"]
mod tests;
