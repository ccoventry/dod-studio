//! `ds_map_run_cfgs`: a cfg per map that runs before each demo's map loads,
//! for settings that differ by map, such as brightness and gamma.
//!
//! ## Why the game's own map cfgs don't do it
//!
//! A server runs `<map>.cfg` (both builds) and, on the 25th Anniversary
//! build, `maps/<map>_load.cfg` and `maps/<map>_unload.cfg` when it loads a
//! map. Demo playback has no server, so none of them run: on POST,
//! `viewdemo` of an anzio demo tried neither (no "couldn't exec" line),
//! while `map dod_anzio` ran `maps/dod_anzio_load.cfg` from `dod_addon`.
//!
//! Studio's own commands for a demo can't stand in for them either: a
//! batch runs a demo's init commands once its map has loaded, and `gamma`
//! set after the load reaches the map only in part. The 25th Anniversary
//! engine rebuilds the lightmaps on a gamma change (`lightmap_gamma.rs`), as
//! the pre-Anniversary one does with `lightmap_rebuild.rs` (#662), but
//! the textures were uploaded through the old table, so a map came out 4 to
//! 16 luma darker than with the same values set before it loaded (measured
//! on POST, `local/review-images/map-lighting`).
//!
//! ## What this does
//!
//! `demo_reload` wraps `playdemo` and `viewdemo`; before the engine's own
//! handler runs, [`before_demo`] reads the map name from the demo's header
//! and queues, in this order:
//!
//! - `exec dodstudio_maps/all_maps.cfg`: the values every map starts from,
//!   so a value one map's cfg sets doesn't carry over to the next map;
//! - `exec dodstudio_maps/<map>.cfg`: that map's own values.
//!
//! Each runs only when the file exists, in `dod_addon` (with `-addons`) or
//! `dod`, the two folders `exec` looks in, so a map with no cfg prints no
//! "couldn't exec" line. The lines are queued behind the `viewdemo` line on
//! the engine's command buffer, which runs to the end in the same frame;
//! the demo's map loads on a later frame, once the player reads the demo's
//! first messages, so the values are in place when the map builds.
//!
//! Not covered: a demo named on the game's command line (`+viewdemo`) runs
//! before the wrap is installed, and a demo that changes map partway plays
//! its second map with the first map's values. Studio splits multi-map
//! demos (#834), and a batch loads every demo with `viewdemo`.

// Only the 32-bit build plays demos; a host build compiles the rules for
// the tests alone.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::io::Read;
use std::path::{Path, PathBuf};

use crate::names::console_name;
use crate::setting::Setting;

pub const NAME: &str = console_name!("map_run_cfgs");

/// `ds_map_run_cfgs`, or its default when the cvar could not be registered.
/// On by default: with no files in the folder it does nothing.
pub static SETTING: Setting = Setting::new(NAME, true);

/// The folder, in `dod_addon` or `dod`, that holds the cfgs.
pub const FOLDER: &str = "dodstudio_maps";

/// The cfg that runs before every map's own.
pub const ALL_MAPS: &str = "all_maps";

/// Where a `.dem` header keeps the map name: after the `HLDEMO` magic and
/// the two protocol numbers.
const MAP_NAME_AT: usize = 16;
/// The map name field's size (`char mapname[260]`).
const MAP_NAME_LEN: usize = 260;
/// Every demo starts with this, NUL-padded to 8 bytes.
const MAGIC: &[u8] = b"HLDEMO\0";

/// Called by `demo_reload`'s wrappers with the name `playdemo`/`viewdemo`
/// was given, before the engine's own handler. Queues the cfgs for the
/// demo's map (see the module doc) and logs which ran. Main thread only.
pub(crate) fn before_demo(demo: &str) {
    if !SETTING.on() {
        return;
    }
    let game_dir = crate::texture_hires::game_dir();
    let path = crate::streaks::demo_path(&game_dir, demo);
    let Some(map) = map_of(&path) else {
        report(&format!(
            "{NAME}: no map name in {}'s header, so no map cfg ran",
            path.display()
        ));
        return;
    };
    let folders = search_folders(&game_dir);
    let mut ran = Vec::new();
    for cfg in cfgs_for(&map) {
        if !folders.iter().any(|dir| dir.join(&cfg).is_file()) {
            continue;
        }
        if queue_exec(&cfg) {
            ran.push(cfg);
        } else {
            report(&format!("{NAME}: the engine refused exec {cfg}"));
        }
    }
    if !ran.is_empty() {
        report(&format!("{NAME}: {map}: ran {}", ran.join(", ")));
    }
}

/// The cfgs to look for before a demo of `map`, in the order they run, as
/// `exec` names them.
pub(crate) fn cfgs_for(map: &str) -> [String; 2] {
    [
        format!("{FOLDER}/{ALL_MAPS}.cfg"),
        format!("{FOLDER}/{map}.cfg"),
    ]
}

/// The map name in a demo's header, or `None` for a file that can't be read,
/// isn't a demo, or names a map that isn't a plain file name.
pub(crate) fn map_of(path: &Path) -> Option<String> {
    let mut header = [0u8; MAP_NAME_AT + MAP_NAME_LEN];
    std::fs::File::open(path)
        .ok()?
        .read_exact(&mut header)
        .ok()?;
    map_in_header(&header)
}

/// The map name in a demo header's bytes; see [`map_of`].
pub(crate) fn map_in_header(header: &[u8]) -> Option<String> {
    if !header.starts_with(MAGIC) {
        return None;
    }
    let field = header.get(MAP_NAME_AT..MAP_NAME_AT + MAP_NAME_LEN)?;
    let end = field.iter().position(|&b| b == 0).unwrap_or(field.len());
    let name = std::str::from_utf8(&field[..end]).ok()?;
    is_plain_map_name(name).then(|| name.to_string())
}

/// Whether `name` can go into an `exec` line and a file name as it is:
/// letters, digits, `_`, `-` and `.`, not empty and not `.`-only. Rules out
/// a path, a quote or a `;` that would start a second command.
pub(crate) fn is_plain_map_name(name: &str) -> bool {
    !name.is_empty()
        && !name.chars().all(|c| c == '.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

/// The folders `exec` reads from, first match wins: `dod_addon` (only with
/// `-addons`), then `dod`.
pub(crate) fn search_folders(game_dir: &Path) -> Vec<PathBuf> {
    let mut folders = Vec::with_capacity(2);
    if crate::texture_hires::addons_on() {
        folders.push(crate::game_folder::game_root().join(crate::game_folder::addon_name()));
    }
    folders.push(game_dir.to_path_buf());
    folders
}

/// Appends `exec <cfg>` to the engine's command buffer. False when the
/// engine's function table isn't captured yet.
fn queue_exec(cfg: &str) -> bool {
    std::ffi::CString::new(format!("exec {cfg}\n"))
        .is_ok_and(|line| crate::engine::client_cmd(&line))
}

fn report(message: &str) {
    // Safety: `debug::report` only appends to the hook log.
    unsafe { crate::debug::report(message) };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header_with(map: &[u8]) -> Vec<u8> {
        let mut header = vec![0u8; MAP_NAME_AT + MAP_NAME_LEN];
        header[..MAGIC.len()].copy_from_slice(MAGIC);
        header[MAP_NAME_AT..MAP_NAME_AT + map.len()].copy_from_slice(map);
        header
    }

    #[test]
    fn a_demo_header_gives_its_map_name() {
        assert_eq!(
            map_in_header(&header_with(b"dod_anzio")).as_deref(),
            Some("dod_anzio")
        );
    }

    #[test]
    fn a_file_without_the_demo_magic_has_no_map() {
        let mut header = header_with(b"dod_anzio");
        header[0] = b'X';
        assert_eq!(map_in_header(&header), None);
    }

    #[test]
    fn a_short_header_has_no_map() {
        assert_eq!(map_in_header(&header_with(b"dod_anzio")[..100]), None);
    }

    #[test]
    fn a_map_name_that_could_break_the_exec_line_is_refused() {
        for bad in [
            &b"dod_anzio;quit"[..],
            b"../dod_anzio",
            b"dod anzio",
            b"dod\"anzio",
            b"..",
            b"",
        ] {
            assert_eq!(map_in_header(&header_with(bad)), None, "{bad:?}");
        }
    }

    #[test]
    fn map_names_with_dashes_and_dots_are_kept() {
        assert_eq!(
            map_in_header(&header_with(b"dod_thunder2_b5a-v1.1")).as_deref(),
            Some("dod_thunder2_b5a-v1.1")
        );
    }

    #[test]
    fn the_all_maps_cfg_runs_before_the_maps_own() {
        assert_eq!(
            cfgs_for("dod_anzio"),
            [
                "dodstudio_maps/all_maps.cfg".to_string(),
                "dodstudio_maps/dod_anzio.cfg".to_string()
            ]
        );
    }

    #[test]
    fn the_fixture_demo_names_its_map() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("test-fixtures")
            .join("ci_fixture.dem");
        let map = map_of(&fixture).expect("ci_fixture.dem has a map name");
        assert!(map.starts_with("dod_"), "{map}");
    }
}
