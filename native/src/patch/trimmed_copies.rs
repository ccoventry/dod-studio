//! Trimmed copies of demos the configured engine can't play (#207).
//!
//! Before a capture batch or a Launch Preview plays a demo, Studio checks the
//! most entities any of its snapshots holds (the scan's
//! `peak_packet_entities`, from the analyzer cache) against the engine's
//! `MAX_PACKET_ENTITIES` (`sys::packet_limit`: 256 pre-Anniversary, 1024 on
//! the 25th Anniversary build). A demo over it would close the game the moment
//! it got there, so the game plays a copy trimmed under the limit instead
//! (`patch::entity_trim`), and the original is never touched.
//!
//! Copies live in Studio's own local folder, `%LOCALAPPDATA%\dod-studio\
//! trimmed_demos\<key>\<stem>_trimmed.dem`, never in the game's folders. The
//! key is the demo's size-and-header key (`utils::demo_hasher`), the trim's
//! target and [`TRIM_VERSION`], so a copy is made once per demo and reused by
//! every later batch and preview, and a different file under the same name
//! gets its own. The newest [`KEEP_COPIES`] stay, and never fewer than the
//! batch being started plays; older ones are removed once its copies are all
//! ready ([`prune_keeping`]).
//!
//! A new copy gets the original's analyzer cache entry, so the in-game DoD
//! Studio window lists its highlights at once instead of asking Studio to
//! read the copy again. The entry keeps the original's file info, so Studio's
//! list of analysed demos shows the demo once, by its own name.

use std::path::{Path, PathBuf};

use crate::patch::entity_trim::{
    MapProps, Outcome, Step, Tier, TrimError, TrimRequest, Trimmed, target_for, trim_demo,
};

/// Under `%LOCALAPPDATA%\dod-studio`.
pub const COPIES_DIR: &str = "trimmed_demos";
/// Bumped when the trim changes what it writes, so older copies are made
/// again rather than reused.
pub const TRIM_VERSION: u32 = 1;
/// Copies kept, newest first by when a batch or preview last used them. Each
/// is about the size of its original (40-50 MB for a WSOD25 HLTV half).
pub const KEEP_COPIES: usize = 8;
/// Written beside each copy every time it is used, for [`KEEP_COPIES`].
const USED_MARKER: &str = "last_used";
/// Each copy's name: the original's stem with this after it.
const COPY_SUFFIX: &str = "_trimmed";

/// The engine a demo is going to: its limit, and where its maps are.
#[derive(Debug, Clone)]
pub struct Engine {
    pub limit: u32,
    pub maps_dir: Option<PathBuf>,
}

impl Engine {
    /// The engine beside `game_exe` (`hl.exe`). A `hw.dll` that can't be read
    /// counts as pre-Anniversary, as the Highlights page's warning does.
    pub fn of(game_exe: &Path) -> Self {
        Self {
            limit: crate::sys::packet_limit::packet_entity_limit(game_exe)
                .unwrap_or(crate::sys::packet_limit::PRE_ANNIVERSARY_PACKET_ENTITIES),
            maps_dir: crate::patch::map_check::maps_dir_for_exe(game_exe),
        }
    }

    /// The engine's name in a log line.
    pub fn name(&self) -> &'static str {
        if self.limit <= crate::sys::packet_limit::PRE_ANNIVERSARY_PACKET_ENTITIES {
            "pre-Anniversary"
        } else {
            "25th Anniversary"
        }
    }
}

/// What the game should play for a demo.
#[derive(Debug, Clone, PartialEq)]
pub enum Playable {
    /// The original: no snapshot is over the engine's limit.
    Original,
    /// A trimmed copy at `path`. `made` is false when an earlier batch or
    /// preview made it.
    Copy { path: PathBuf, made: bool },
}

/// Where `demo`'s trimmed copy for `limit` goes under `root`, or `None` when
/// the demo can't be read for its key.
pub fn copy_path(root: &Path, demo: &Path, limit: u32) -> Option<PathBuf> {
    let key = crate::utils::demo_hasher::demo_key_text(demo)?;
    let stem = demo.file_stem()?.to_string_lossy();
    Some(
        root.join(format!("v{TRIM_VERSION}-{}-{key}", target_for(limit)))
            .join(format!("{stem}{COPY_SUFFIX}.dem")),
    )
}

/// The folder copies live in.
pub fn copies_root() -> PathBuf {
    crate::shared::paths::get_local_appdata_dir().join(COPIES_DIR)
}

/// The most entities one of `demo`'s snapshots holds, from its analyzer cache
/// entry. `None` when it has none (not scanned, or scanned before the count
/// existed).
pub fn cached_peak(demo: &Path) -> Option<u32> {
    analysis::cache::load(&crate::analyzer_cache_root(), demo)?
        .1
        .demo_info
        .peak_packet_entities
}

/// What the game should play for `demo` on `engine`: the original, or a
/// trimmed copy, made now if there is none yet. Reports each step of a trim
/// through `progress`. Writes the copy, its analyzer cache entry and the
/// "last used" marker, and logs what it did to the activity log.
///
/// `Err` when the demo is over the limit and can't be trimmed safely; the
/// caller must not play it then.
pub fn prepare_playable(
    demo: &Path,
    engine: &Engine,
    cancel: &dyn Fn() -> bool,
    progress: &mut dyn FnMut(Step),
) -> Result<Playable, TrimError> {
    let peak = cached_peak(demo);
    if peak.is_some_and(|p| p <= engine.limit) {
        return Ok(Playable::Original);
    }
    let root = copies_root();
    let Some(path) = copy_path(&root, demo, engine.limit) else {
        return Err(TrimError::Unreadable(format!(
            "{} could not be opened",
            demo.display()
        )));
    };
    if path.is_file() {
        // A copy made while the original had no current cache entry gets one
        // now, if the original has one since.
        let cache = crate::analyzer_cache_root();
        if !analysis::cache::is_fresh(&cache, &path) {
            seed_analysis(&cache, demo, &path, None);
        }
        mark_used(&path);
        crate::log_markdown(&format!(
            "✂️ **Trimmed copy** — `{}` has more entities in one snapshot than the {} engine takes ({}); \
             the game plays the trimmed copy made earlier, `{}`. The original is unchanged.",
            file_name(demo),
            engine.name(),
            engine.limit,
            path.display()
        ));
        return Ok(Playable::Copy { path, made: false });
    }
    let bytes = std::fs::read(demo)
        .map_err(|e| TrimError::Unreadable(format!("{}: {e}", demo.display())))?;
    let map = map_props(demo, engine);
    let outcome = trim_demo(
        &bytes,
        TrimRequest {
            limit: engine.limit,
            target: target_for(engine.limit),
            map: map.as_ref(),
            cancel,
            progress,
        },
    )?;
    drop(bytes);
    let trimmed = match outcome {
        Outcome::Unneeded { .. } => return Ok(Playable::Original),
        Outcome::Trimmed(t) => t,
    };
    save_copy(&path, &trimmed.bytes)?;
    seed_analysis(
        &crate::analyzer_cache_root(),
        demo,
        &path,
        Some(trimmed.peak_after),
    );
    mark_used(&path);
    crate::log_markdown(&made_message(demo, engine, &trimmed, &path));
    Ok(Playable::Copy { path, made: true })
}

fn file_name(demo: &Path) -> String {
    demo.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| demo.display().to_string())
}

/// The demo's map, from the engine's maps folder; `None` (logged) when it
/// isn't there or doesn't read, which leaves the decorative props out.
fn map_props(demo: &Path, engine: &Engine) -> Option<MapProps> {
    let name = crate::patch::map_check::map_reference(demo).ok()?.map_name;
    if !crate::patch::map_check::is_safe_map_name(&name) {
        return None;
    }
    let path = crate::patch::map_check::map_path(engine.maps_dir.as_ref()?, &name);
    match MapProps::from_file(&path) {
        Ok(props) => Some(props),
        Err(e) => {
            crate::log_markdown(&format!(
                "⚠️ **Trimmed copy** — couldn't read the map for `{}` ({e}); its decorations are left out of the trim",
                file_name(demo)
            ));
            None
        }
    }
}

/// Writes the copy beside its final name and renames it over, so a copy
/// that exists is always whole. A preview and a batch can make the same copy
/// at once: each writes its own partial file, and the one that renames second
/// takes the first one's copy (the trim is the same bytes every run).
fn save_copy(path: &Path, bytes: &[u8]) -> Result<(), TrimError> {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let fail = |e: std::io::Error| {
        TrimError::CheckFailed(format!("it could not be saved to {}: {e}", path.display()))
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(fail)?;
    }
    let partial = path.with_extension(format!(
        "dem.{}-{}.part",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::write(&partial, bytes).map_err(fail)?;
    match std::fs::rename(&partial, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(&partial);
            if path.is_file() { Ok(()) } else { Err(fail(e)) }
        }
    }
}

/// Gives the copy the original's analyzer cache entry under `root` (see the
/// module doc), with the copy's own peak when it is known. Best-effort:
/// without it, the game asks Studio to read the copy.
fn seed_analysis(root: &Path, original: &Path, copy: &Path, peak_after: Option<u32>) {
    let Some((file_info, mut analysis)) = analysis::cache::load(root, original) else {
        return;
    };
    analysis.demo_info.peak_packet_entities = peak_after;
    let _ = analysis::cache::store(root, copy, &file_info, &analysis);
}

/// Stamps the copy's folder as just used. Best-effort: a copy whose stamp
/// couldn't be written is only removed sooner.
fn mark_used(copy: &Path) {
    if let Some(dir) = copy.parent() {
        let _ = std::fs::write(dir.join(USED_MARKER), b"");
    }
}

/// Removes the copies past the newest [`KEEP_COPIES`], once a batch or a
/// preview has every copy it plays (`in_use` of them, each just marked used,
/// so the newest): pruning while a batch's copies are still being made would
/// remove its first ones once it plays more than [`KEEP_COPIES`]. Returns how
/// many it removed.
pub fn prune_keeping(root: &Path, in_use: usize) -> usize {
    prune(root, KEEP_COPIES.max(in_use))
}

/// Removes all but the `keep` most recently used copy folders under `root`.
/// Returns how many it removed.
pub fn prune(root: &Path, keep: usize) -> usize {
    let Ok(entries) = std::fs::read_dir(root) else {
        return 0;
    };
    let mut folders: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .map(|p| {
            let used = std::fs::metadata(p.join(USED_MARKER))
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            (used, p)
        })
        .collect();
    folders.sort_by_key(|(used, _)| std::cmp::Reverse(*used));
    folders
        .iter()
        .skip(keep)
        .filter(|(_, p)| std::fs::remove_dir_all(p).is_ok())
        .count()
}

/// The activity log's line for a copy just made.
fn made_message(demo: &Path, engine: &Engine, t: &Trimmed, path: &Path) -> String {
    format!(
        "✂️ **Trimmed copy** — `{}` has up to {} entities in one snapshot, and the {} engine closes at more than {}. \
         The game plays a copy with {} static entities left out of every snapshot (up to {} now): \
         {} invisible, {} sprites (fog and glows), {} decorations far from where anyone played. \
         Saved as `{}`; the original is unchanged.",
        file_name(demo),
        t.peak_before,
        engine.name(),
        engine.limit,
        t.dropped.len(),
        t.peak_after,
        t.count_of(Tier::Invisible),
        t.count_of(Tier::Sprite),
        t.count_of(Tier::Decoration),
        path.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Scratch;

    #[test]
    fn a_copy_is_named_for_its_demo_key_and_target() {
        let dir = Scratch::new("trimmed_copy_path");
        let demo = dir.join("wsod25_grp3_h1_hltv.dem");
        std::fs::write(&demo, b"HLDEMO\0\0 some bytes").unwrap();
        let path = copy_path(&dir.join("root"), &demo, 256).unwrap();
        let key = crate::utils::demo_hasher::demo_key_text(&demo).unwrap();
        assert_eq!(
            path,
            dir.join("root")
                .join(format!("v{TRIM_VERSION}-246-{key}"))
                .join("wsod25_grp3_h1_hltv_trimmed.dem")
        );
        assert_eq!(
            copy_path(&dir.join("root"), &dir.join("missing.dem"), 256),
            None
        );
    }

    #[test]
    fn a_different_file_under_the_same_name_gets_its_own_copy() {
        let dir = Scratch::new("trimmed_copy_key");
        let demo = dir.join("a.dem");
        std::fs::write(&demo, b"HLDEMO\0\0 first").unwrap();
        let first = copy_path(&dir, &demo, 256).unwrap();
        std::fs::write(&demo, b"HLDEMO\0\0 second, longer").unwrap();
        assert_ne!(copy_path(&dir, &demo, 256).unwrap(), first);
    }

    #[test]
    fn prune_keeps_the_most_recently_used_copies() {
        let dir = Scratch::new("trimmed_copy_prune");
        for (i, name) in ["old", "middle", "new"].iter().enumerate() {
            let folder = dir.join(name);
            std::fs::create_dir_all(&folder).unwrap();
            let marker = std::fs::File::create(folder.join(USED_MARKER)).unwrap();
            let when = std::time::SystemTime::UNIX_EPOCH
                + std::time::Duration::from_secs(1_000_000 + i as u64 * 1000);
            marker.set_modified(when).unwrap();
        }
        assert_eq!(prune(&dir, 2), 1);
        assert!(!dir.join("old").exists());
        assert!(dir.join("middle").exists() && dir.join("new").exists());
    }

    #[test]
    fn a_copy_saved_twice_is_whole_and_leaves_no_partial_file() {
        let dir = Scratch::new("trimmed_copy_save");
        let path = dir.join("key").join("a_trimmed.dem");
        save_copy(&path, b"HLDEMO copy").unwrap();
        save_copy(&path, b"HLDEMO copy").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"HLDEMO copy");
        let names: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from("a_trimmed.dem")]);
    }

    #[test]
    fn a_batch_playing_more_copies_than_are_kept_keeps_them_all() {
        let dir = Scratch::new("trimmed_copy_prune_batch");
        let batch = KEEP_COPIES + 2;
        for i in 0..batch {
            let folder = dir.join(format!("copy{i}"));
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(folder.join(USED_MARKER), b"").unwrap();
        }
        assert_eq!(prune_keeping(&dir, batch), 0);
        assert_eq!(prune_keeping(&dir, 1), 2, "the next preview prunes them");
    }

    #[test]
    fn a_copy_gets_the_originals_cache_entry_under_the_originals_name() {
        let dir = Scratch::new("trimmed_copy_seed");
        let root = dir.join("cache");
        let original = dir.join("lennon2_hltv.dem");
        let copy = dir.join("lennon2_hltv_trimmed.dem");
        std::fs::write(&original, b"HLDEMO\0\0 original").unwrap();
        std::fs::write(&copy, b"HLDEMO\0\0 copy").unwrap();
        let mut analysis = analysis::Analysis::default();
        analysis.demo_info.peak_packet_entities = Some(300);
        let info = analysis::cache::FileInfo::of(&original).unwrap();
        analysis::cache::store(&root, &original, &info, &analysis).unwrap();

        seed_analysis(&root, &original, &copy, Some(246));

        let (seeded_info, seeded) = analysis::cache::load(&root, &copy).expect("seeded");
        assert_eq!(seeded.demo_info.peak_packet_entities, Some(246));
        assert_eq!(seeded_info.path, info.path, "listed once, by the original");
    }

    #[test]
    fn a_copy_of_a_demo_with_no_cache_entry_gets_none() {
        let dir = Scratch::new("trimmed_copy_seed_none");
        let copy = dir.join("copy.dem");
        std::fs::write(&copy, b"HLDEMO").unwrap();
        seed_analysis(
            &dir.join("cache"),
            &dir.join("missing.dem"),
            &copy,
            Some(246),
        );
        assert!(analysis::cache::load(&dir.join("cache"), &copy).is_none());
    }

    #[test]
    fn the_engine_is_named_by_its_limit() {
        let pre = Engine {
            limit: 256,
            maps_dir: None,
        };
        let anniversary = Engine {
            limit: 1024,
            maps_dir: None,
        };
        assert_eq!(pre.name(), "pre-Anniversary");
        assert_eq!(anniversary.name(), "25th Anniversary");
    }
}
