// messages/map_builds.rs
// Studio's map builds (`maps::build_library`, `maps::build_placement`,
// `maps::build_plan`, #1547): errors and activity-log lines. Each function
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
        "- Map builds: {count} map build(s) not put in place, because a game is running: a game keeps the first build of a map it loads until it closes"
    )
}

pub fn open_game_keeps_its_maps(count: usize) -> String {
    format!(
        "- Map builds: {count} map build(s) not put in place, because the batch runs in the open game, which keeps the maps it has loaded; close the game to use them"
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

pub fn left_out(demo: impl Display, map: &str, wanted: u32, playing: Option<u32>) -> String {
    let playing = playing.map_or_else(|| "the installed one".to_string(), build);
    format!(
        "- Map builds: {demo} wants build {} of {map}, but an earlier demo of this batch needs {playing}, and one game plays one build of a map: capture it in a batch of its own",
        build(wanted)
    )
}

pub fn batch_places(count: usize, maps: impl Display) -> String {
    format!(
        "- Map builds: this batch plays {count} map(s) on a build from Studio's library: {maps}"
    )
}
