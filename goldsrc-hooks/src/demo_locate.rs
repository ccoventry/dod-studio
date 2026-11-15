//! Which file a `viewdemo`/`playdemo` name opens (#1463), for the hook's own
//! reads of the playing demo (the look-ahead).
//!
//! The engine opens the demo through its file system with no path ID, so it
//! tries `<search path>/<name>` for every search path in turn and takes the
//! first that exists: `<game>_addon` (only with `-addons`, which every DoD
//! Studio launch passes), then `<game>`, then `<game>_downloads`, then the
//! `valve` fallback's own folders. Studio and test copies put demos in
//! `dod_addon`, so a reader that only looks in `dod` misses them.
//!
//! A name with a drive (`D:/folder/demo`) is a full path: the engine itself
//! can't open one, `demo_other_drives` makes it, and it opens that file. A
//! relative name with `..` (`../other/demo`) is joined to each search path
//! like any other, as the engine does.

use std::path::{Path, PathBuf};

/// The engine's `-game` fallback, searched after the game's own folders.
const FALLBACK_GAME: &str = "valve";

/// Every file `name` could open, in the order the engine tries them.
/// `root` is the folder holding `hl.exe`, `game` the `-game` folder's name.
pub(crate) fn candidates(root: &Path, game: &str, addons: bool, name: &str) -> Vec<PathBuf> {
    let file = crate::streaks::demo_path(Path::new(""), name);
    if file.has_root() || file.is_absolute() {
        return vec![file];
    }
    let mut folders = Vec::new();
    for g in [game, FALLBACK_GAME] {
        if addons {
            folders.push(format!("{g}_addon"));
        }
        folders.push(g.to_string());
        folders.push(format!("{g}_downloads"));
        if g.eq_ignore_ascii_case(FALLBACK_GAME) {
            break;
        }
    }
    folders.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    folders.iter().map(|f| root.join(f).join(&file)).collect()
}

/// The first of `candidates` for which `exists` holds, or, when none does,
/// every one tried.
pub(crate) fn first_existing(
    candidates: Vec<PathBuf>,
    exists: impl Fn(&Path) -> bool,
) -> Result<PathBuf, Vec<PathBuf>> {
    match candidates.iter().position(|p| exists(p)) {
        Some(i) => Ok(candidates.into_iter().nth(i).unwrap_or_default()),
        None => Err(candidates),
    }
}

/// The file the playing demo `name` (as `viewdemo` was given it) opened in
/// this game, or every path tried when none exists.
pub(crate) fn locate(name: &str) -> Result<PathBuf, Vec<PathBuf>> {
    first_existing(
        candidates(
            &crate::game_folder::game_root(),
            crate::game_folder::name(),
            crate::texture_hires::addons_on(),
            name,
        ),
        Path::is_file,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> PathBuf {
        PathBuf::from("C:/Games/Half-Life")
    }

    fn names(found: &[PathBuf]) -> Vec<String> {
        found
            .iter()
            .map(|p| {
                p.strip_prefix(root())
                    .unwrap_or(p)
                    .to_string_lossy()
                    .replace(std::path::MAIN_SEPARATOR, "/")
            })
            .collect()
    }

    #[test]
    fn with_addons_the_addon_folder_comes_first() {
        let found = candidates(&root(), "dod", true, "match1");
        assert_eq!(
            names(&found),
            [
                "dod_addon/match1.dem",
                "dod/match1.dem",
                "dod_downloads/match1.dem",
                "valve_addon/match1.dem",
                "valve/match1.dem",
                "valve_downloads/match1.dem",
            ]
        );
    }

    #[test]
    fn without_addons_the_addon_folder_is_not_searched() {
        let found = candidates(&root(), "dod", false, "match1.dem");
        assert_eq!(
            names(&found),
            [
                "dod/match1.dem",
                "dod_downloads/match1.dem",
                "valve/match1.dem",
                "valve_downloads/match1.dem",
            ]
        );
    }

    #[test]
    fn a_valve_game_is_searched_once() {
        let found = candidates(&root(), "valve", false, "a");
        assert_eq!(names(&found), ["valve/a.dem", "valve_downloads/a.dem"]);
    }

    #[test]
    fn a_full_path_is_the_one_file() {
        let found = candidates(&root(), "dod", true, "\"D:/demos/match1\"");
        assert_eq!(found, [PathBuf::from("D:/demos/match1.dem")]);
    }

    #[test]
    fn a_relative_path_is_joined_to_each_folder() {
        let found = candidates(&root(), "dod", true, "../movies/clip.dem");
        assert_eq!(names(&found)[0], "dod_addon/../movies/clip.dem");
        assert_eq!(names(&found)[1], "dod/../movies/clip.dem");
    }

    #[test]
    fn a_demo_only_in_the_addon_folder_is_found_there() {
        let found = first_existing(candidates(&root(), "dod", true, "match1"), |p| {
            p.starts_with(root().join("dod_addon"))
        });
        assert_eq!(found, Ok(root().join("dod_addon").join("match1.dem")));
    }

    #[test]
    fn the_addon_copy_wins_over_the_game_folders() {
        let found = first_existing(candidates(&root(), "dod", true, "match1"), |_| true);
        assert_eq!(found, Ok(root().join("dod_addon").join("match1.dem")));
    }

    #[test]
    fn a_demo_nowhere_lists_every_path_tried() {
        let tried = first_existing(candidates(&root(), "dod", true, "gone"), |_| false);
        assert_eq!(tried.map_err(|t| t.len()), Err(6));
    }
}
