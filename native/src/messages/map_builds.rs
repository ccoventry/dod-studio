// messages/map_builds.rs
// Studio's map builds (`maps::build_library`, `maps::build_placement`,
// `maps::build_plan`, #1547): errors, activity-log lines, and the warnings
// the user is shown as toasts (the `*_warning` functions). Each function
// returns the text only.

use std::fmt::Display;

/// A build checksum as the 8 hex digits the library files are named by.
pub fn build(checksum: u32) -> String {
    format!("{checksum:08x}")
}

pub fn not_a_map_name(name: &str) -> String {
    format!("`{name}` is not a usable map name")
}

pub fn could_not_read(path: impl Display, err: impl Display) -> String {
    format!("could not read {path}: {err}")
}

pub fn not_a_map(path: impl Display, why: impl Display) -> String {
    format!("{path} is not a map file Studio can read: {why}")
}

pub fn could_not_write(path: impl Display, err: impl Display) -> String {
    format!("could not write {path}: {err}")
}

pub fn added(map: &str, checksum: u32, from: impl Display, to: impl Display, ms: u128) -> String {
    format!(
        "- Map builds: added build {} of {map} from {from} to {to} in {ms} ms",
        build(checksum)
    )
}

pub fn add_failed(map: &str, err: impl Display) -> String {
    format!("- Map builds: could not add a build of {map}: {err}")
}

pub fn already_had(map: &str, checksum: u32, path: impl Display) -> String {
    format!(
        "- Map builds: build {} of {map} is already in the library ({path})",
        build(checksum)
    )
}

pub fn placed(map: &str, checksum: u32, slot: impl Display, ms: u128) -> String {
    format!(
        "- Map builds: put build {} of {map} in place at {slot} for this game ({ms} ms)",
        build(checksum)
    )
}

pub fn kept_in_place(map: &str, checksum: u32, slot: impl Display) -> String {
    format!(
        "- Map builds: build {} of {map} is already in place at {slot}",
        build(checksum)
    )
}

pub fn slot_not_ours(map: &str, slot: impl Display) -> String {
    format!(
        "- Map builds: {slot} is not a file Studio put there, so it is left alone and {map} plays on it (move it away to let Studio use its builds)"
    )
}

pub fn not_in_library(map: &str, checksum: u32) -> String {
    format!(
        "- Map builds: build {} of {map} is not in the library any more; the installed map plays",
        build(checksum)
    )
}

pub fn game_running(count: usize) -> String {
    format!(
        "- Map builds: {count} map build(s) not put in place, because a game is running or starting: a game keeps the first build of a map it loads until it closes"
    )
}

pub fn open_game_keeps_its_maps(count: usize) -> String {
    format!(
        "- Map builds: {count} demo(s) won't play on their own build, because they play in the open game, which keeps the builds it has loaded"
    )
}

pub fn place_failed(map: &str, checksum: u32, slot: impl Display, err: impl Display) -> String {
    format!(
        "- Map builds: could not put build {} of {map} in place at {slot}: {err}",
        build(checksum)
    )
}

pub fn removed(map: &str, checksum: u32, slot: impl Display) -> String {
    format!(
        "- Map builds: removed build {} of {map} from {slot}, the installed map plays again",
        build(checksum)
    )
}

pub fn changed_since_placed(slot: impl Display) -> String {
    format!("- Map builds: {slot} changed since Studio put a build there, so it is left alone")
}

pub fn remove_failed(slot: impl Display, err: impl Display) -> String {
    format!("- Map builds: could not remove {slot}: {err}; Studio tries again at its next start")
}

pub const RECORD_UNREADABLE: &str = "the record of placed builds can't be read";

pub fn record_busy(path: impl Display, secs: u64) -> String {
    format!("the record of placed builds {path} stayed locked by another DoD Studio for {secs} s")
}

pub fn lock_failed(why: impl Display) -> String {
    format!("- Map builds: nothing put in place: {why}")
}

pub fn held_by_another_studio(map: &str, slot: impl Display) -> String {
    format!(
        "- Map builds: {slot} holds another build of {map} another running DoD Studio placed for its game, so it is left alone"
    )
}

pub fn part_sweep_skipped(addon: impl Display) -> String {
    format!(
        "- Map builds: another DoD Studio is changing the map builds in {addon}; unfinished copies are left for the next start"
    )
}

pub fn library_parts_swept(count: usize, library: impl Display) -> String {
    format!("- Map builds: removed {count} unfinished add(s) from {library}")
}

pub fn library_file_wrong(map: &str, file: impl Display) -> String {
    format!(
        "- Map builds: {file} is not the build its name says, so {map} plays on the installed map"
    )
}

pub fn parts_swept(count: usize, addon: impl Display) -> String {
    format!("- Map builds: removed {count} unfinished copy(ies) from {addon}\\maps")
}

pub fn open_game_demos(map: &str, demos: &[String]) -> String {
    format!(
        "- Map builds: in the running game these demos of {map} won't play on their build: {}",
        demos.join(", ")
    )
}

pub fn stale_build_demos(map: &str, demos: &[String]) -> String {
    format!(
        "- Map builds: an old build of {map} is still in dod_addon\\maps, so these demos won't play on the installed build: {}",
        demos.join(", ")
    )
}

pub fn release_thread_failed(err: impl Display) -> String {
    format!(
        "- Map builds: could not start the wait for the game to close ({err}); Studio removes its builds at its next start"
    )
}

pub fn record_unreadable(path: impl Display, err: impl Display) -> String {
    format!("- Map builds: could not read the record of placed builds {path}: {err}")
}

pub fn record_unwritable(path: impl Display, err: impl Display) -> String {
    format!("- Map builds: could not save the record of placed builds {path}: {err}")
}

pub fn leftovers_found(count: usize, root: impl Display) -> String {
    format!(
        "- Map builds: {count} build(s) left in place in {root} by an earlier session; removed once no game runs"
    )
}

pub fn left_at_exit(count: usize) -> String {
    format!(
        "- Map builds: DoD Studio is closing with {count} launch(es) of map builds in place while a game runs; the next start removes them once no game runs"
    )
}

pub fn released_at_exit(count: usize) -> String {
    format!("- Map builds: DoD Studio is closing; removed {count} map build(s) it had in place")
}

pub fn left_out(demo: impl Display, map: &str, wanted: u32, playing: Option<u32>) -> String {
    let playing = playing.map_or_else(
        || "the installed build".to_string(),
        |c| format!("build {}", build(c)),
    );
    format!(
        "- Map builds: {demo} wants build {} of {map}, but this game plays {playing} of it (one game plays one build of a map)",
        build(wanted)
    )
}

pub fn launch_places(count: usize, maps: impl Display) -> String {
    format!("- Map builds: this game plays {count} map(s) on a build from Studio's library: {maps}")
}

// ── Shown to the user (toasts, the batch's warnings) ───────────────────────

/// Demos of one map a launch can't play on their own build.
pub fn left_out_warning(map: &str, demos: &[String]) -> String {
    format!(
        "{} of {map} won't play on the version {} recorded on: {}. A game plays one version of a map, so capture {} in a batch of {} own.",
        count_demos(demos.len()),
        if demos.len() == 1 {
            "it was"
        } else {
            "they were"
        },
        names_preview(demos),
        if demos.len() == 1 { "it" } else { "them" },
        if demos.len() == 1 { "its" } else { "their" },
    )
}

/// Demos of one map that play in an open game holding another build of it.
pub fn open_game_warning(map: &str, demos: &[String]) -> String {
    format!(
        "{} of {map} will play on another version in the open game: {}. Close the game to play {} on the version {} recorded on.",
        count_demos(demos.len()),
        names_preview(demos),
        if demos.len() == 1 { "it" } else { "them" },
        if demos.len() == 1 {
            "it was"
        } else {
            "they were"
        },
    )
}

/// Demos that want the installed build while an old one is still in place.
pub fn stale_build_warning(map: &str, demos: &[String]) -> String {
    format!(
        "An old version of {map} DoD Studio put in dod_addon\\maps is still there, so {} will play on it: {}. Close every game and start again, or remove dod_addon\\maps\\{map}.bsp.",
        if demos.len() == 1 {
            "1 demo".to_string()
        } else {
            format!("{} demos", demos.len())
        },
        names_preview(demos),
    )
}

pub fn library_file_wrong_warning(map: &str, file: impl Display) -> String {
    format!(
        "{map}: {file} is not the version its name says, so the installed version plays. Add the old version again."
    )
}
/// A user's own file in `dod_addon\maps` takes the place of Studio's build.
pub fn slot_taken_warning(map: &str, slot: impl Display) -> String {
    format!(
        "{map}: the game plays {slot}, a file DoD Studio didn't put there. Move it away to play the demos on the version they were recorded on."
    )
}

/// Another running DoD Studio keeps another version in place for its game.
pub fn held_by_another_studio_warning(map: &str) -> String {
    format!(
        "{map}: the game plays the version another DoD Studio that is running put in dod_addon\\maps for its game, not the one these demos were recorded on. Try again once that game has closed."
    )
}

pub fn not_in_library_warning(map: &str, checksum: u32) -> String {
    format!(
        "{map}: version {} is no longer in DoD Studio's map builds, so the installed version plays.",
        build(checksum)
    )
}

pub fn place_failed_warning(map: &str, err: impl Display) -> String {
    format!(
        "{map}: DoD Studio could not put the old version in place ({err}), so the installed version plays."
    )
}

pub fn record_unreadable_warning(path: impl Display) -> String {
    format!(
        "DoD Studio can't read its record of old map versions in place ({path}), so it puts none in place. Close the game, then delete that file and any map in dod_addon\\maps that DoD Studio put there."
    )
}

/// How many names a warning lists before "and N more"; the log has them all.
const NAMES_SHOWN: usize = 3;

/// The first few of `names`, then how many more.
pub fn names_preview(names: &[String]) -> String {
    let shown = names
        .iter()
        .take(NAMES_SHOWN)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    match names.len().saturating_sub(NAMES_SHOWN) {
        0 => shown,
        more => format!("{shown} and {more} more"),
    }
}

fn count_demos(n: usize) -> String {
    if n == 1 {
        "1 demo".to_string()
    } else {
        format!("{n} demos")
    }
}
