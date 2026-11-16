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
//! Without `-addons` on the command line the addon folder is still tried,
//! after `<game>`'s other folders: the engine may switch it on from its own
//! settings instead (not checked), and a demo found only there can only have
//! been played from there.
//!
//! Not searched: `<game>_hd`, `<game>_<language>` and the `hl.exe` folder,
//! which the engine also reads but which don't hold demos in practice.
//!
//! A name with a drive (`D:/folder/demo`) is a full path: the engine itself
//! can't open one, `demo_other_drives` makes it, and it opens that file. A
//! relative name with `..` (`../other/demo`) is joined to each search path
//! like any other, as the engine does.
//!
//! Readers on the engine thread that ask every frame (the DoD Studio
//! window's tabs, map cfgs, spray lambdas, Review) go through [`playing`],
//! which locates each play once (#1474).

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Plays so far: [`forget`] counts one, and a remembered file from an
/// earlier play is stale on every thread.
static PLAYS: AtomicU64 = AtomicU64::new(0);

thread_local! {
    /// What [`playing`] last located. Every caller runs on the engine
    /// thread; a caller on another thread keeps an entry of its own, which
    /// [`PLAYS`] still makes stale at the next play.
    static PLAYING: RefCell<Option<Located>> = const { RefCell::new(None) };
}

/// One play's demo, as [`playing`] found it.
#[derive(Debug, Clone, PartialEq)]
struct Located {
    play: u64,
    name: String,
    path: PathBuf,
    /// False when no candidate existed and `path` is the fallback.
    found: bool,
}

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
        if !addons {
            folders.push(format!("{g}_addon"));
        }
        if g.eq_ignore_ascii_case(FALLBACK_GAME) {
            break;
        }
    }
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

/// The file the demo `name` (as `playdemo`/`viewdemo` was given it) opened,
/// remembered until the next play, so callers that ask every frame cost no
/// file-system lookups; the hook log names it once per play. When no
/// candidate exists, the path in the game folder, so the caller's own error
/// names a file. Callers that keep something per demo key it on this path,
/// so a replay of the same name from another folder is read afresh.
pub(crate) fn playing(name: &str) -> PathBuf {
    located(name).path
}

/// [`playing`]'s file, or `None` when no candidate exists: for a caller
/// that hands the path on (Review) rather than reporting its own error.
pub(crate) fn playing_found(name: &str) -> Option<PathBuf> {
    let found = located(name);
    found.found.then_some(found.path)
}

fn located(name: &str) -> Located {
    PLAYING.with(|cache| {
        remembered(
            &mut cache.borrow_mut(),
            PLAYS.load(Ordering::Acquire),
            name,
            locate,
            || in_game_folder(name),
            report,
        )
    })
}

/// The file `name` would open, found now and not logged: for lists of
/// demos (the Demos tab's rows) rather than the one playing.
// The Demos tab is x86-only.
#[cfg_attr(not(target_arch = "x86"), allow(dead_code))]
pub(crate) fn file_for(name: &str) -> PathBuf {
    locate(name).unwrap_or_else(|_| in_game_folder(name))
}

/// Where each row of a demo list opens, found once per row: the Demos tab
/// asks for every row on each filter keystroke, and a search is up to six
/// file-system lookups (#1474).
#[derive(Debug, Default)]
pub(crate) struct RowFiles(std::collections::HashMap<String, PathBuf>);

impl RowFiles {
    /// The file row `name` opens: remembered, else `find`'s answer.
    #[cfg_attr(not(target_arch = "x86"), allow(dead_code))]
    pub(crate) fn file(&mut self, name: &str, find: impl FnOnce(&str) -> PathBuf) -> PathBuf {
        self.0
            .entry(name.to_string())
            .or_insert_with(|| find(name))
            .clone()
    }

    /// Forgets one row, whose remembered file has gone (moved meanwhile).
    #[cfg_attr(not(target_arch = "x86"), allow(dead_code))]
    pub(crate) fn drop_row(&mut self, name: &str) {
        self.0.remove(name);
    }

    /// Forgets every row: the list is filled again.
    #[cfg_attr(not(target_arch = "x86"), allow(dead_code))]
    pub(crate) fn clear(&mut self) {
        self.0.clear();
    }
}

/// Where a demo found nowhere is reported missing: the game folder, where
/// every reader looked before #1474.
fn in_game_folder(name: &str) -> PathBuf {
    crate::streaks::demo_path(&crate::texture_hires::game_dir(), name)
}

/// Makes what [`playing`] remembers stale: a new play may open another
/// file under the same name (a copy put in `dod_addon` meanwhile). Called by
/// `demo_reload` on every `playdemo`/`viewdemo`.
pub(crate) fn forget() {
    PLAYS.fetch_add(1, Ordering::AcqRel);
}

/// [`playing`]'s lookup: `cache` when it holds `name` for this `play`, else
/// `find`'s answer (or `fallback` when it found nothing), stored and passed
/// to `log`.
fn remembered(
    cache: &mut Option<Located>,
    play: u64,
    name: &str,
    find: impl FnOnce(&str) -> Result<PathBuf, Vec<PathBuf>>,
    fallback: impl FnOnce() -> PathBuf,
    log: impl FnOnce(&str),
) -> Located {
    if let Some(known) = cache.as_ref()
        && known.play == play
        && known.name == name
    {
        return known.clone();
    }
    let (path, found) = match find(name) {
        Ok(path) => {
            log(&format!("{name} opens {}", path.display()));
            (path, true)
        }
        Err(tried) => {
            let tried: Vec<String> = tried.iter().map(|p| p.display().to_string()).collect();
            log(&format!("can't find {name}; tried {}", tried.join(", ")));
            (fallback(), false)
        }
    };
    let known = Located {
        play,
        name: name.to_string(),
        path,
        found,
    };
    *cache = Some(known.clone());
    known
}

fn report(message: &str) {
    // Safety: `report` only appends to the hook log; any thread may call it.
    unsafe { crate::debug::report(&format!("demo_locate: {message}")) };
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
    fn without_addons_the_addon_folder_comes_last() {
        let found = candidates(&root(), "dod", false, "match1.dem");
        assert_eq!(
            names(&found),
            [
                "dod/match1.dem",
                "dod_downloads/match1.dem",
                "dod_addon/match1.dem",
                "valve/match1.dem",
                "valve_downloads/match1.dem",
                "valve_addon/match1.dem",
            ]
        );
    }

    #[test]
    fn a_valve_game_is_searched_once() {
        let found = candidates(&root(), "valve", false, "a");
        assert_eq!(
            names(&found),
            ["valve/a.dem", "valve_downloads/a.dem", "valve_addon/a.dem"]
        );
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

    fn addon_copy(name: &str) -> Result<PathBuf, Vec<PathBuf>> {
        first_existing(candidates(&root(), "dod", true, name), |p| {
            p.starts_with(root().join("dod_addon"))
        })
    }

    fn game_folder_copy() -> PathBuf {
        root().join("dod").join("match1.dem")
    }

    #[test]
    fn the_playing_demo_is_located_once_and_logged_once() {
        let mut cache = None;
        let mut lines = Vec::new();
        let first = remembered(&mut cache, 1, "match1", addon_copy, game_folder_copy, |l| {
            lines.push(l.to_string())
        });
        assert_eq!(first.path, root().join("dod_addon").join("match1.dem"));
        assert!(first.found);
        let again = remembered(
            &mut cache,
            1,
            "match1",
            |_| panic!("looked up again"),
            game_folder_copy,
            |l| lines.push(l.to_string()),
        );
        assert_eq!(again, first);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("dod_addon"), "{}", lines[0]);
    }

    #[test]
    fn another_demo_is_located_afresh() {
        let mut cache = None;
        remembered(
            &mut cache,
            1,
            "match1",
            addon_copy,
            game_folder_copy,
            |_| {},
        );
        let other = remembered(
            &mut cache,
            1,
            "match2",
            addon_copy,
            game_folder_copy,
            |_| {},
        );
        assert_eq!(other.path, root().join("dod_addon").join("match2.dem"));
    }

    #[test]
    fn the_same_name_played_again_is_located_afresh() {
        // Played from dod first, then a copy appears in dod_addon.
        let mut cache = None;
        let only_dod = |n: &str| {
            first_existing(candidates(&root(), "dod", true, n), |p| {
                p.starts_with(root().join("dod"))
            })
        };
        let first = remembered(&mut cache, 1, "match1", only_dod, game_folder_copy, |_| {});
        assert_eq!(first.path, root().join("dod").join("match1.dem"));
        let next = remembered(
            &mut cache,
            2,
            "match1",
            addon_copy,
            game_folder_copy,
            |_| {},
        );
        assert_eq!(next.path, root().join("dod_addon").join("match1.dem"));
    }

    #[test]
    fn a_rows_file_is_found_once_until_forgotten() {
        let mut rows = RowFiles::default();
        let first = rows.file("match1.dem", |n| root().join("dod_addon").join(n));
        let again = rows.file("match1.dem", |_| panic!("looked up again"));
        assert_eq!(again, first);
        rows.drop_row("match1.dem");
        let moved = rows.file("match1.dem", |n| root().join("dod").join(n));
        assert_eq!(moved, root().join("dod").join("match1.dem"));
        rows.clear();
        let refilled = rows.file("match1.dem", |n| root().join("dod_addon").join(n));
        assert_eq!(refilled, first);
    }

    #[test]
    fn forget_makes_every_remembered_play_stale() {
        let before = PLAYS.load(Ordering::Acquire);
        forget();
        assert!(PLAYS.load(Ordering::Acquire) > before);
    }

    #[test]
    fn a_full_path_is_played_from_where_it_is() {
        let mut cache = None;
        let found = remembered(
            &mut cache,
            1,
            "D:/demos/match1",
            |n| first_existing(candidates(&root(), "dod", true, n), |_| true),
            game_folder_copy,
            |_| {},
        );
        assert_eq!(found.path, PathBuf::from("D:/demos/match1.dem"));
    }

    #[test]
    fn a_demo_nowhere_falls_back_to_the_game_folder_and_says_so() {
        let mut cache = None;
        let mut lines = Vec::new();
        let found = remembered(
            &mut cache,
            1,
            "match1",
            |n| first_existing(candidates(&root(), "dod", true, n), |_| false),
            game_folder_copy,
            |l| lines.push(l.to_string()),
        );
        assert_eq!(found.path, game_folder_copy());
        assert!(!found.found);
        assert!(lines[0].starts_with("can't find match1"), "{}", lines[0]);
    }
}
