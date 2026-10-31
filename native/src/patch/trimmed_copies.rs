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
//! gets its own. Beside each copy, a marker names the map `.bsp` it was made
//! with ([`map_tag`]): when the map changes (or appears, or goes), the copy
//! is made again, since the trim's drop set depends on it. The newest
//! [`KEEP_COPIES`] stay ([`prune`]), and a copy a running batch plays is never
//! removed, however many it plays ([`Pins`]). Before a copy is saved, its
//! drive must have room for it with [`FREE_SPACE_HEADROOM_BYTES`](crate::shared::movie_copy::FREE_SPACE_HEADROOM_BYTES) to spare.
//!
//! A new copy gets the original's analyzer cache entry, so the in-game DoD
//! Studio window lists its highlights at once instead of asking Studio to
//! read the copy again. The entry keeps the original's file info, so Studio's
//! list of analysed demos shows the demo once, by its own name.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::Instant;

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
/// Written beside each copy: the [`map_tag`] of the map it was made with.
const MAP_MARKER: &str = "map";
/// [`map_tag`] when the map's `.bsp` isn't there to read.
const NO_MAP_TAG: &str = "none";
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

/// Where [`prepare_playable`] is, for its status line, with how far through
/// the step it is (0.0-1.0).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Progress {
    /// Reading a demo with no peak in the analyzer cache, to learn whether it
    /// needs a copy at all: most such demos don't.
    Counting(f32),
    /// Reading a demo known to need a copy.
    Reading(f32),
    /// Writing the copy.
    Writing,
    /// Reading the copy back to check it.
    Checking(f32),
}

/// The [`Progress`] of a trim's `step`, for a demo whose cached peak is
/// `peak` (`None` when the cache has none, so the read decides).
fn progress_of(peak: Option<u32>, step: Step) -> Progress {
    match step {
        Step::Reading(f) if peak.is_none() => Progress::Counting(f),
        Step::Reading(f) => Progress::Reading(f),
        Step::Writing => Progress::Writing,
        Step::Checking(f) => Progress::Checking(f),
    }
}

/// Copy folders that running batches play, one entry per [`Pins`] holding
/// one. [`prune`] never removes them.
static PINNED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

fn pinned() -> MutexGuard<'static, Vec<PathBuf>> {
    // The list stays whole if a holder panicked: each push or remove is one call.
    PINNED.lock().unwrap_or_else(|p| p.into_inner())
}

/// Keeps copies from being pruned while it lives. A batch holds one from the
/// moment each of its copies is ready until its game is done with them, so a
/// Launch Preview started meanwhile can't remove a copy the batch has yet to
/// play, however many it plays.
#[derive(Debug, Default)]
pub struct Pins(Vec<PathBuf>);

impl Pins {
    /// Pins `copy`'s folder until this is dropped.
    pub fn add(&mut self, copy: &Path) {
        if let Some(dir) = copy.parent() {
            pinned().push(dir.to_path_buf());
            self.0.push(dir.to_path_buf());
        }
    }
}

impl Drop for Pins {
    fn drop(&mut self) {
        let mut all = pinned();
        for dir in self.0.drain(..) {
            if let Some(at) = all.iter().position(|d| *d == dir) {
                all.swap_remove(at);
            }
        }
    }
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
/// trimmed copy, made now if there is none yet or the one there was made
/// with another map. Reports each step through `progress`. Writes the copy,
/// its analyzer cache entry and its markers, and logs what it did to the
/// activity log.
///
/// `Err` when the demo is over the limit and can't be trimmed safely, or its
/// copy has no room on the drive; the caller must not play it then.
pub fn prepare_playable(
    demo: &Path,
    engine: &Engine,
    cancel: &dyn Fn() -> bool,
    progress: &mut dyn FnMut(Progress),
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
    let map = read_map(demo, engine);
    match copy_state(&path, &map.tag) {
        CopyState::Current => {
            reuse_copy(demo, engine, &path);
            return Ok(Playable::Copy { path, made: false });
        }
        CopyState::MadeWithAnotherMap => {
            crate::log_markdown(&crate::messages::trimmed_copy_map_changed(&file_name(demo)));
        }
        CopyState::Missing => {}
    }
    let started = Instant::now();
    let bytes = std::fs::read(demo)
        .map_err(|e| TrimError::Unreadable(format!("{}: {e}", demo.display())))?;
    let props = map.props(demo);
    let outcome = trim_demo(
        &bytes,
        TrimRequest {
            limit: engine.limit,
            target: target_for(engine.limit),
            map: props.as_ref(),
            cancel,
            progress: &mut |step| progress(progress_of(peak, step)),
        },
    )?;
    drop(bytes);
    let trimmed = match outcome {
        Outcome::Unneeded { .. } => return Ok(Playable::Original),
        Outcome::Trimmed(t) => t,
    };
    check_room(&path, trimmed.bytes.len() as u64)?;
    save_copy(&path, &trimmed.bytes)?;
    save_map_marker(&path, &map.tag);
    seed_analysis(
        &crate::analyzer_cache_root(),
        demo,
        &path,
        Some(trimmed.peak_after),
    );
    mark_used(&path);
    crate::log_markdown(&made_message(
        demo,
        engine,
        &trimmed,
        &path,
        started.elapsed(),
    ));
    Ok(Playable::Copy { path, made: true })
}

/// Plays the copy already at `path`: seeds its cache entry if it has none
/// yet, stamps it used, and logs it.
fn reuse_copy(demo: &Path, engine: &Engine, path: &Path) {
    // A copy made while the original had no current cache entry gets one
    // now, if the original has one since.
    let cache = crate::analyzer_cache_root();
    if !analysis::cache::is_fresh(&cache, path) {
        seed_analysis(&cache, demo, path, None);
    }
    mark_used(path);
    crate::log_markdown(&format!(
        "✂️ **Trimmed copy** — `{}` has more entities in one snapshot than the {} engine takes ({}); \
         the game plays the trimmed copy made earlier, `{}`. The original is unchanged.",
        file_name(demo),
        engine.name(),
        engine.limit,
        path.display()
    ));
}

fn file_name(demo: &Path) -> String {
    demo.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| demo.display().to_string())
}

/// A demo's map as the trim sees it: the `.bsp`'s bytes, when the engine's
/// maps folder has it, and their [`map_tag`].
struct MapFile {
    tag: String,
    /// `Err` says why the `.bsp` couldn't be read; `Ok(None)`: no map to look for.
    bytes: Result<Option<Vec<u8>>, String>,
}

impl MapFile {
    /// The map's props for the trim; `None` (logged) when it isn't there or
    /// doesn't read, which leaves the decorative props out. Logged here, when
    /// a copy is made, not on every use of one already made.
    fn props(&self, demo: &Path) -> Option<MapProps> {
        let parsed = match &self.bytes {
            Ok(bytes) => MapProps::parse(bytes.as_deref()?),
            Err(why) => Err(why.clone()),
        };
        match parsed {
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
}

/// Reads `demo`'s map from the engine's maps folder. Read on every use, a
/// few milliseconds for a `.bsp`, so a changed map is always noticed.
fn read_map(demo: &Path, engine: &Engine) -> MapFile {
    let bytes = match map_file(demo, engine) {
        None => Ok(None),
        Some(path) => std::fs::read(&path)
            .map(Some)
            .map_err(|e| format!("{}: {e}", path.display())),
    };
    MapFile {
        tag: map_tag(bytes.as_ref().ok().and_then(|b| b.as_deref())),
        bytes,
    }
}

/// Where `demo`'s map's `.bsp` would be, or `None` when its map name can't be
/// read or used, or the engine has no maps folder.
fn map_file(demo: &Path, engine: &Engine) -> Option<PathBuf> {
    let name = crate::patch::map_check::map_reference(demo).ok()?.map_name;
    if !crate::patch::map_check::is_safe_map_name(&name) {
        return None;
    }
    Some(crate::patch::map_check::map_path(
        engine.maps_dir.as_ref()?,
        &name,
    ))
}

/// Names a map's `.bsp` by its contents (`None`: no map to read). The trim
/// leaves out decorations only when it has the map, and picks them by where
/// the map puts them, so a copy is only reused with the map it was made with.
pub fn map_tag(bsp: Option<&[u8]>) -> String {
    match bsp {
        Some(bytes) => format!("{:016x}", crate::utils::demo_hasher::fnv1a_hash(bytes)),
        None => NO_MAP_TAG.to_string(),
    }
}

/// Whether the copy at `path` can be played as it is.
#[derive(Debug, PartialEq)]
enum CopyState {
    Missing,
    /// Made with the map `map_tag` names (its marker says so).
    Current,
    /// Made with another map, or before copies carried a map marker: it is
    /// made again.
    MadeWithAnotherMap,
}

fn copy_state(path: &Path, map_tag: &str) -> CopyState {
    if !path.is_file() {
        return CopyState::Missing;
    }
    let marker = path.parent().map(|dir| dir.join(MAP_MARKER));
    match marker.and_then(|m| std::fs::read_to_string(m).ok()) {
        Some(tag) if tag == map_tag => CopyState::Current,
        _ => CopyState::MadeWithAnotherMap,
    }
}

/// Records which map the copy at `path` was made with. Best-effort: a copy
/// whose marker couldn't be written is only made again next time.
fn save_map_marker(path: &Path, map_tag: &str) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::write(dir.join(MAP_MARKER), map_tag);
    }
}

/// `Err` when the drive `copy` goes on hasn't `needed_bytes` free with
/// [`FREE_SPACE_HEADROOM_BYTES`](crate::shared::movie_copy::FREE_SPACE_HEADROOM_BYTES) to spare. A drive whose free space can't be
/// told is let through: the write itself still fails cleanly.
fn check_room(copy: &Path, needed_bytes: u64) -> Result<(), TrimError> {
    // The free-space query wants a folder that exists.
    let Some(folder) = copy.ancestors().skip(1).find(|p| p.is_dir()) else {
        return Ok(());
    };
    room_for(
        folder,
        crate::sys::disk::get_available_bytes(folder),
        needed_bytes,
    )
}

/// [`check_room`]'s rule, for `free` bytes on `folder`'s drive.
fn room_for(folder: &Path, free: Option<u64>, needed_bytes: u64) -> Result<(), TrimError> {
    if crate::shared::movie_copy::fits(needed_bytes, free) {
        return Ok(());
    }
    Err(TrimError::NoRoom {
        folder: folder.to_path_buf(),
        free_bytes: free.unwrap_or(0),
        needed_bytes,
    })
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
    // A write that fails partway (a full disk) leaves no partial file behind.
    if let Err(e) = std::fs::write(&partial, bytes) {
        let _ = std::fs::remove_file(&partial);
        return Err(fail(e));
    }
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

/// Removes all but the `keep` most recently used copy folders under `root`,
/// leaving any a running batch has pinned ([`Pins`]) whatever their age.
/// Returns how many it removed.
pub fn prune(root: &Path, keep: usize) -> usize {
    let Ok(entries) = std::fs::read_dir(root) else {
        return 0;
    };
    let folders: Vec<(std::time::SystemTime, PathBuf)> = entries
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
    let pinned = pinned().clone();
    folders_to_prune(folders, keep, &pinned)
        .iter()
        .filter(|p| std::fs::remove_dir_all(p).is_ok())
        .count()
}

/// [`prune`]'s rule: of `folders` (each with when it was last used), the ones
/// past the `keep` newest that aren't in `pinned`.
fn folders_to_prune(
    mut folders: Vec<(std::time::SystemTime, PathBuf)>,
    keep: usize,
    pinned: &[PathBuf],
) -> Vec<PathBuf> {
    folders.sort_by_key(|(used, _)| std::cmp::Reverse(*used));
    folders
        .into_iter()
        .skip(keep)
        .map(|(_, p)| p)
        .filter(|p| !pinned.contains(p))
        .collect()
}

/// The activity log's line for a copy just made, `took` after it started.
fn made_message(
    demo: &Path,
    engine: &Engine,
    t: &Trimmed,
    path: &Path,
    took: std::time::Duration,
) -> String {
    format!(
        "✂️ **Trimmed copy** — `{}` has up to {} entities in one snapshot, and the {} engine closes at more than {}. \
         The game plays a copy with {} static entities left out of every snapshot (up to {} now): \
         {} invisible, {} sprites (fog and glows), {} decorations far from where anyone played. \
         Saved as `{}` in {:.1} s; the original is unchanged.",
        file_name(demo),
        t.peak_before,
        engine.name(),
        engine.limit,
        t.dropped.len(),
        t.peak_after,
        t.count_of(Tier::Invisible),
        t.count_of(Tier::Sprite),
        t.count_of(Tier::Decoration),
        path.display(),
        took.as_secs_f64()
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

    /// `count` copy folders under `dir`, `copy0` the oldest used, each with a
    /// copy file; returns the copies' paths.
    fn copies_used_in_order(dir: &Path, count: usize) -> Vec<PathBuf> {
        (0..count)
            .map(|i| {
                let folder = dir.join(format!("copy{i}"));
                std::fs::create_dir_all(&folder).unwrap();
                let marker = std::fs::File::create(folder.join(USED_MARKER)).unwrap();
                let when = std::time::SystemTime::UNIX_EPOCH
                    + std::time::Duration::from_secs(1_000_000 + i as u64 * 1000);
                marker.set_modified(when).unwrap();
                folder.join("a_trimmed.dem")
            })
            .collect()
    }

    #[test]
    fn a_preview_during_a_batch_of_more_copies_than_are_kept_removes_none_of_the_batchs() {
        let dir = Scratch::new("trimmed_copy_prune_batch");
        let copies = copies_used_in_order(&dir, KEEP_COPIES + 2);
        let mut batch = Pins::default();
        for copy in &copies {
            batch.add(copy);
        }
        assert_eq!(prune(&dir, KEEP_COPIES), 0, "the batch's copies are pinned");
        assert!(copies.iter().all(|c| c.parent().unwrap().is_dir()));
        drop(batch);
        assert_eq!(prune(&dir, KEEP_COPIES), 2, "once it ends they prune");
        assert!(!dir.join("copy0").exists() && !dir.join("copy1").exists());
    }

    #[test]
    fn a_copy_pinned_twice_stays_pinned_until_both_let_go() {
        let dir = Scratch::new("trimmed_copy_pin_twice");
        let copy = &copies_used_in_order(&dir, 1)[0];
        let mut first = Pins::default();
        let mut second = Pins::default();
        first.add(copy);
        second.add(copy);
        drop(first);
        assert_eq!(prune(&dir, 0), 0);
        drop(second);
        assert_eq!(prune(&dir, 0), 1);
    }

    #[test]
    fn prune_skips_pinned_folders_past_the_newest() {
        let at = |s: u64| std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(s);
        let folders = vec![
            (at(1), PathBuf::from("old")),
            (at(3), PathBuf::from("new")),
            (at(2), PathBuf::from("pinned")),
        ];
        assert_eq!(
            folders_to_prune(folders, 1, &[PathBuf::from("pinned")]),
            vec![PathBuf::from("old")]
        );
    }

    #[test]
    fn a_copy_made_with_the_same_map_is_reused() {
        let dir = Scratch::new("trimmed_copy_state_current");
        let copy = dir.join("key").join("a_trimmed.dem");
        save_copy(&copy, b"HLDEMO copy").unwrap();
        let tag = map_tag(Some(b"BSP v30"));
        save_map_marker(&copy, &tag);
        assert_eq!(copy_state(&copy, &tag), CopyState::Current);
    }

    #[test]
    fn a_copy_is_made_again_when_its_map_changes_appears_or_goes() {
        let dir = Scratch::new("trimmed_copy_state_map");
        let copy = dir.join("key").join("a_trimmed.dem");
        save_copy(&copy, b"HLDEMO copy").unwrap();
        save_map_marker(&copy, &map_tag(Some(b"BSP v30")));
        assert_eq!(
            copy_state(&copy, &map_tag(Some(b"BSP v30, edited"))),
            CopyState::MadeWithAnotherMap
        );
        assert_eq!(
            copy_state(&copy, &map_tag(None)),
            CopyState::MadeWithAnotherMap
        );
        save_map_marker(&copy, &map_tag(None));
        assert_eq!(
            copy_state(&copy, &map_tag(Some(b"BSP v30"))),
            CopyState::MadeWithAnotherMap
        );
    }

    #[test]
    fn a_copy_with_no_map_marker_is_made_again() {
        let dir = Scratch::new("trimmed_copy_state_unmarked");
        let copy = dir.join("key").join("a_trimmed.dem");
        assert_eq!(copy_state(&copy, NO_MAP_TAG), CopyState::Missing);
        save_copy(&copy, b"HLDEMO copy").unwrap();
        assert_eq!(copy_state(&copy, NO_MAP_TAG), CopyState::MadeWithAnotherMap);
    }

    #[test]
    fn a_copy_that_would_leave_its_drive_too_full_is_refused() {
        let folder = Path::new("C:/copies");
        let copy = 50 * 1024 * 1024;
        let enough = copy + crate::shared::movie_copy::FREE_SPACE_HEADROOM_BYTES;
        assert_eq!(room_for(folder, Some(enough), copy), Ok(()));
        assert_eq!(
            room_for(folder, Some(enough - 1), copy),
            Err(TrimError::NoRoom {
                folder: folder.to_path_buf(),
                free_bytes: enough - 1,
                needed_bytes: copy,
            })
        );
        assert_eq!(room_for(folder, None, copy), Ok(()), "unknown: let through");
    }

    #[test]
    fn reading_a_demo_with_no_cached_peak_says_it_is_counting_not_copying() {
        assert_eq!(
            progress_of(None, Step::Reading(0.5)),
            Progress::Counting(0.5)
        );
        assert_eq!(
            progress_of(Some(300), Step::Reading(0.5)),
            Progress::Reading(0.5)
        );
        assert_eq!(progress_of(None, Step::Writing), Progress::Writing);
        assert_eq!(
            progress_of(None, Step::Checking(0.2)),
            Progress::Checking(0.2)
        );
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
