//! Putting a library build where the game reads it first, and taking it away
//! again (#1547).
//!
//! With `-addons`, which every Studio launch passes, the game reads
//! `dod_addon\maps\<map>.bsp` ahead of `dod\maps\<map>.bsp` (checked in game
//! on both engine builds), so a build copied there plays without touching the
//! installed map. The copy is Studio's for one game session:
//!
//! - **Only before a fresh game.** The engine remembers each map file's
//!   checksum from its first load and stops with "maps/<map>.bsp has been
//!   modified since starting the engine" when a later load in the same
//!   session finds another file (measured on both builds: a copy removed
//!   between two demos of one game). So a build goes in place only while no
//!   game runs, and comes out only once the game has closed; between those,
//!   one game plays one build of each map.
//! - **Never over a file Studio didn't put there.** A build is copied only
//!   into an empty slot, and a slot is emptied only while its bytes are still
//!   the library build's (or, with the library file gone, its checksum is
//!   still the build's). Anything else there is the user's and is left alone.
//! - **A record of what is in place**, `placed.json` in the library folder,
//!   written before each copy, so builds left by a Studio that closed or
//!   crashed are removed at its next start.
//!
//! Each launch's builds belong to a session; removing a session's builds
//! never touches a build a later launch took over.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use lock_recover::lock_or_recover;
use serde::{Deserialize, Serialize};

use super::bsp;
use super::build_library::{LIBRARY_FOLDER, build_path};
use crate::log_markdown;
use crate::messages::map_builds as msg;

/// The record of placed builds, in the library folder.
const RECORD_FILE: &str = "placed.json";
/// How often a release waiting on the game looks at the process list: it
/// only watches a process stay alive (CLAUDE.md, process lifecycles).
const GAME_POLL: Duration = Duration::from_millis(500);

/// One read-modify-write of a record at a time, across every thread here.
static RECORD_LOCK: Mutex<()> = Mutex::new(());
/// Makes two sessions started in the same nanosecond still differ.
static SESSION_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A build of a map, as the library names it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MapBuild {
    pub map_name: String,
    pub checksum: u32,
}

/// `<addon>\maps\<map>.bsp`: where the game looks first.
pub fn slot_path(addon: &Path, map_name: &str) -> PathBuf {
    addon.join("maps").join(format!("{map_name}.bsp"))
}

fn library(addon: &Path) -> PathBuf {
    addon.join(LIBRARY_FOLDER)
}

fn record_path(addon: &Path) -> PathBuf {
    library(addon).join(RECORD_FILE)
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Record {
    #[serde(default)]
    placed: Vec<PlacedBuild>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct PlacedBuild {
    map_name: String,
    checksum: u32,
    /// The launch whose game the build is in place for.
    session: u64,
}

/// The record, or an empty one when there is none. `None` (logged) when it
/// can't be read: callers then neither place nor remove anything, and never
/// write a new record over it, so what it lists is left in place rather than
/// deleted, or forgotten, on a guess.
fn load_record(addon: &Path) -> Option<Record> {
    let path = record_path(addon);
    let read = std::fs::read(&path).and_then(|bytes| {
        serde_json::from_slice(&bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    });
    match read {
        Ok(record) => Some(record),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Some(Record::default()),
        Err(e) => {
            log_markdown(&msg::record_unreadable(path.display(), e));
            None
        }
    }
}

/// Saves the record beside its final name first, so a crash mid-write never
/// leaves half a record. An empty record removes the file.
fn save_record(addon: &Path, record: &Record) -> std::io::Result<()> {
    let path = record_path(addon);
    if record.placed.is_empty() {
        return match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    std::fs::create_dir_all(library(addon))?;
    let json = serde_json::to_vec_pretty(record).map_err(std::io::Error::other)?;
    let part = path.with_extension("json.part");
    std::fs::write(&part, json)?;
    std::fs::rename(&part, &path)
}

/// A new launch's session id.
pub fn new_session() -> u64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64);
    now.wrapping_add(SESSION_COUNTER.fetch_add(1, Ordering::Relaxed))
}

/// Whether the file at `slot` is still the build Studio put there: its bytes
/// are the library file's (or the start of them, from a copy cut short), or,
/// with the library file gone, its checksum is the build's.
fn holds_build(slot: &Path, library_file: &Path, checksum: u32) -> bool {
    let Ok(placed) = std::fs::read(slot) else {
        return false;
    };
    match std::fs::read(library_file) {
        Ok(build) => placed.len() <= build.len() && build[..placed.len()] == placed[..],
        Err(_) => bsp::map_checksum(&placed).is_ok_and(|found| found == checksum),
    }
}

/// What putting one build in place did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaceOutcome {
    /// Copied into the empty slot.
    Placed,
    /// Studio had already put this build there; this launch takes it over.
    AlreadyInPlace,
    /// A file Studio didn't put there holds the slot; it is left alone.
    SlotNotOurs,
    /// The library doesn't hold the build.
    NotInLibrary,
    /// The copy, or the record before it, failed.
    Failed(String),
}

/// One launch's builds, removed together once its game has gone.
#[derive(Debug)]
pub struct Placement {
    addon: PathBuf,
    session: u64,
    builds: Vec<MapBuild>,
}

impl Placement {
    /// A launch that put nothing in place.
    pub fn none(addon: &Path) -> Self {
        Self {
            addon: addon.to_path_buf(),
            session: new_session(),
            builds: Vec::new(),
        }
    }

    /// Whether this launch put anything in place.
    pub fn is_empty(&self) -> bool {
        self.builds.is_empty()
    }

    /// The builds in place for this launch.
    pub fn builds(&self) -> &[MapBuild] {
        &self.builds
    }

    /// Removes this launch's builds now, for a game that never started or
    /// has closed. Returns how many came out.
    pub fn release(&self) -> usize {
        if self.is_empty() {
            return 0;
        }
        release_sessions(&self.addon, &[self.session])
    }

    /// Removes this launch's builds on a thread of their own once no game
    /// runs: after waiting up to `start_wait` for a game to show up (a
    /// launch whose game is still starting), then for every game to close.
    /// Any game, not only this launch's: one Studio cannot tell apart may
    /// have loaded the build too. A Studio that closes first leaves them to
    /// [`release_leftovers_after_games`] at its next start.
    pub fn release_after_game(self, start_wait: Duration) {
        if self.is_empty() {
            return;
        }
        let spawned = std::thread::Builder::new()
            .name("map_builds_release".into())
            .spawn(move || {
                wait_for_games_to_end(start_wait);
                self.release();
            });
        if let Err(e) = spawned {
            log_markdown(&msg::release_thread_failed(e));
        }
    }
}

/// Waits up to `start_wait` for a game to be running, then until none is.
fn wait_for_games_to_end(start_wait: Duration) {
    let mut sys = crate::sys::process::snapshot();
    let since = Instant::now();
    while !crate::sys::process::is_game_in(&sys) && since.elapsed() < start_wait {
        std::thread::sleep(GAME_POLL);
        crate::sys::process::refresh(&mut sys);
    }
    while crate::sys::process::is_game_in(&sys) {
        std::thread::sleep(GAME_POLL);
        crate::sys::process::refresh(&mut sys);
    }
}

/// Puts each of `wants` in place in `addon`'s maps folder for one new launch.
///
/// The caller makes sure no game runs (see the module doc). A build Studio
/// already put there for an earlier launch is taken over, another build of
/// the same map Studio put there is replaced, and a file Studio didn't put
/// there is left alone. Logs each step to the activity log. Returns the
/// launch's [`Placement`] and what happened to each want.
pub fn place_builds(addon: &Path, wants: &[MapBuild]) -> (Placement, Vec<PlaceOutcome>) {
    let session = new_session();
    let _guard = lock_or_recover(&RECORD_LOCK);
    let mut placement = Placement {
        addon: addon.to_path_buf(),
        session,
        builds: Vec::new(),
    };
    let Some(mut record) = load_record(addon) else {
        let unreadable = PlaceOutcome::Failed(msg::RECORD_UNREADABLE.to_string());
        return (placement, vec![unreadable; wants.len()]);
    };
    let outcomes = wants
        .iter()
        .map(|want| {
            let outcome = place_one(addon, &mut record, want, session);
            if matches!(outcome, PlaceOutcome::Placed | PlaceOutcome::AlreadyInPlace) {
                placement.builds.push(want.clone());
            }
            outcome
        })
        .collect();
    (placement, outcomes)
}

fn place_one(addon: &Path, record: &mut Record, want: &MapBuild, session: u64) -> PlaceOutcome {
    let library_file = build_path(&library(addon), &want.map_name, want.checksum);
    if !library_file.is_file() {
        log_markdown(&msg::not_in_library(&want.map_name, want.checksum));
        return PlaceOutcome::NotInLibrary;
    }
    let slot = slot_path(addon, &want.map_name);
    if slot.exists() {
        let ours = record
            .placed
            .iter()
            .position(|p| p.map_name == want.map_name)
            .filter(|&i| {
                let p = &record.placed[i];
                holds_build(
                    &slot,
                    &build_path(&library(addon), &p.map_name, p.checksum),
                    p.checksum,
                )
            });
        let Some(index) = ours else {
            log_markdown(&msg::slot_not_ours(&want.map_name, slot.display()));
            return PlaceOutcome::SlotNotOurs;
        };
        if record.placed[index].checksum == want.checksum {
            record.placed[index].session = session;
            if let Err(e) = save_record(addon, record) {
                log_markdown(&msg::record_unwritable(record_path(addon).display(), e));
            }
            log_markdown(&msg::kept_in_place(
                &want.map_name,
                want.checksum,
                slot.display(),
            ));
            return PlaceOutcome::AlreadyInPlace;
        }
        if let Some(e) = crate::shared::paths::remove_file_retrying(&slot) {
            log_markdown(&msg::place_failed(
                &want.map_name,
                want.checksum,
                slot.display(),
                &e,
            ));
            return PlaceOutcome::Failed(e.to_string());
        }
        record.placed.remove(index);
    }
    copy_into_slot(addon, record, want, session, &library_file, &slot)
}

/// Records the build, then copies it into the empty slot. The record comes
/// first, so a Studio that dies mid-copy still knows to remove what it left.
fn copy_into_slot(
    addon: &Path,
    record: &mut Record,
    want: &MapBuild,
    session: u64,
    library_file: &Path,
    slot: &Path,
) -> PlaceOutcome {
    let started = Instant::now();
    record.placed.push(PlacedBuild {
        map_name: want.map_name.clone(),
        checksum: want.checksum,
        session,
    });
    let fail = |record: &mut Record, e: &dyn std::fmt::Display| {
        record.placed.pop();
        if let Err(save) = save_record(addon, record) {
            log_markdown(&msg::record_unwritable(record_path(addon).display(), save));
        }
        log_markdown(&msg::place_failed(
            &want.map_name,
            want.checksum,
            slot.display(),
            e,
        ));
        PlaceOutcome::Failed(e.to_string())
    };
    if let Err(e) = save_record(addon, record) {
        log_markdown(&msg::record_unwritable(record_path(addon).display(), &e));
        record.placed.pop();
        return PlaceOutcome::Failed(e.to_string());
    }
    if let Err(e) = copy_new(library_file, slot) {
        return fail(record, &e);
    }
    log_markdown(&msg::placed(
        &want.map_name,
        want.checksum,
        slot.display(),
        started.elapsed().as_millis(),
    ));
    PlaceOutcome::Placed
}

/// Copies `from` to `to`, which must not exist: a file that appeared there
/// since the check is never overwritten. A copy cut short is removed.
fn copy_new(from: &Path, to: &Path) -> std::io::Result<()> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut source = std::fs::File::open(from)?;
    let mut target = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(to)?;
    let copied = std::io::copy(&mut source, &mut target).and_then(|_| target.sync_all());
    if copied.is_err() {
        drop(target);
        // Our own partial copy, created just above.
        let _ = std::fs::remove_file(to);
    }
    copied
}

/// Removes the builds of `sessions` from `addon`'s maps folder: each one
/// still holding its build is deleted, each one changed since is left alone
/// and forgotten. A delete that fails stays in the record for the next
/// start. Returns how many came out.
pub fn release_sessions(addon: &Path, sessions: &[u64]) -> usize {
    let _guard = lock_or_recover(&RECORD_LOCK);
    let Some(mut record) = load_record(addon) else {
        return 0;
    };
    let mut removed = 0;
    let mut kept = Vec::with_capacity(record.placed.len());
    for placed in std::mem::take(&mut record.placed) {
        if !sessions.contains(&placed.session) {
            kept.push(placed);
            continue;
        }
        let slot = slot_path(addon, &placed.map_name);
        if !slot.exists() {
            continue;
        }
        let library_file = build_path(&library(addon), &placed.map_name, placed.checksum);
        if !holds_build(&slot, &library_file, placed.checksum) {
            log_markdown(&msg::changed_since_placed(slot.display()));
            continue;
        }
        match crate::shared::paths::remove_file_retrying(&slot) {
            None => {
                removed += 1;
                log_markdown(&msg::removed(
                    &placed.map_name,
                    placed.checksum,
                    slot.display(),
                ));
            }
            Some(e) => {
                log_markdown(&msg::remove_failed(slot.display(), e));
                kept.push(placed);
            }
        }
    }
    record.placed = kept;
    if let Err(e) = save_record(addon, &record) {
        log_markdown(&msg::record_unwritable(record_path(addon).display(), e));
    }
    removed
}

/// The sessions with a build in place in `addon` right now.
pub fn placed_sessions(addon: &Path) -> Vec<u64> {
    let _guard = lock_or_recover(&RECORD_LOCK);
    let sessions: HashSet<u64> = load_record(addon)
        .map(|record| record.placed.iter().map(|p| p.session).collect())
        .unwrap_or_default();
    sessions.into_iter().collect()
}

/// Whether the file at `addon`'s slot for `map_name` is a build Studio put
/// there, rather than one of the user's.
pub fn is_studio_placement(addon: &Path, map_name: &str) -> bool {
    let slot = slot_path(addon, map_name);
    let _guard = lock_or_recover(&RECORD_LOCK);
    let Some(record) = load_record(addon) else {
        return false;
    };
    record
        .placed
        .iter()
        .filter(|p| p.map_name == map_name)
        .any(|p| {
            holds_build(
                &slot,
                &build_path(&library(addon), &p.map_name, p.checksum),
                p.checksum,
            )
        })
}

/// At Studio's start: the builds an earlier Studio left in place (closed or
/// crashed before its game did) are removed on a thread of their own once no
/// game runs; a game still running may have loaded them. Only the builds in
/// place now: one a launch from this Studio takes over in the meantime stays.
pub fn release_leftovers_after_games(addon: &Path) {
    let sessions = placed_sessions(addon);
    if sessions.is_empty() {
        return;
    }
    log_markdown(&msg::leftovers_found(sessions.len(), addon.display()));
    let addon = addon.to_path_buf();
    let spawned = std::thread::Builder::new()
        .name("map_builds_leftovers".into())
        .spawn(move || {
            wait_for_games_to_end(Duration::ZERO);
            release_sessions(&addon, &sessions);
        });
    if let Err(e) = spawned {
        log_markdown(&msg::release_thread_failed(e));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::maps::build_library::{add_build, test_map_bytes};
    use crate::test_support::Scratch;

    struct Install {
        _dir: Scratch,
        addon: PathBuf,
    }

    /// An addon folder whose library holds builds 1 and 2 of `dod_x`.
    fn install(tag: &str) -> Install {
        let dir = Scratch::new(format_args!("build_placement_{tag}"));
        let addon = dir.join("dod_addon");
        for seed in [1, 2] {
            let source = dir.join(format!("{seed}.bsp"));
            std::fs::write(&source, test_map_bytes(seed)).unwrap();
            add_build(&library(&addon), "dod_x", &source, &mut |_, _| {}).unwrap();
        }
        Install { _dir: dir, addon }
    }

    fn build(seed: u8) -> MapBuild {
        MapBuild {
            map_name: "dod_x".into(),
            checksum: bsp::map_checksum(&test_map_bytes(seed)).unwrap(),
        }
    }

    fn slot(addon: &Path) -> PathBuf {
        slot_path(addon, "dod_x")
    }

    #[test]
    fn a_build_goes_into_the_empty_slot_and_comes_out_on_release() {
        let i = install("place");
        let (placement, outcomes) = place_builds(&i.addon, &[build(1)]);
        assert_eq!(outcomes, vec![PlaceOutcome::Placed]);
        assert_eq!(std::fs::read(slot(&i.addon)).unwrap(), test_map_bytes(1));
        assert!(is_studio_placement(&i.addon, "dod_x"));

        assert_eq!(placement.release(), 1);
        assert!(!slot(&i.addon).exists());
        assert!(!record_path(&i.addon).exists(), "nothing left to remember");
    }

    #[test]
    fn a_file_studio_did_not_place_is_never_overwritten_or_removed() {
        let i = install("theirs");
        std::fs::create_dir_all(slot(&i.addon).parent().unwrap()).unwrap();
        std::fs::write(slot(&i.addon), test_map_bytes(9)).unwrap();

        let (placement, outcomes) = place_builds(&i.addon, &[build(1)]);
        assert_eq!(outcomes, vec![PlaceOutcome::SlotNotOurs]);
        assert!(placement.is_empty());
        assert!(!is_studio_placement(&i.addon, "dod_x"));
        assert_eq!(placement.release(), 0);
        assert_eq!(std::fs::read(slot(&i.addon)).unwrap(), test_map_bytes(9));
    }

    #[test]
    fn a_placed_build_the_user_replaced_is_left_alone_on_release() {
        let i = install("replaced");
        let (placement, _) = place_builds(&i.addon, &[build(1)]);
        std::fs::write(slot(&i.addon), test_map_bytes(9)).unwrap();

        assert_eq!(placement.release(), 0);
        assert_eq!(std::fs::read(slot(&i.addon)).unwrap(), test_map_bytes(9));
        assert!(
            placed_sessions(&i.addon).is_empty(),
            "forgotten, not retried"
        );
    }

    #[test]
    fn a_build_the_library_lacks_is_not_placed() {
        let i = install("missing");
        let (placement, outcomes) = place_builds(&i.addon, &[build(7)]);
        assert_eq!(outcomes, vec![PlaceOutcome::NotInLibrary]);
        assert!(placement.is_empty());
        assert!(!slot(&i.addon).exists());
    }

    #[test]
    fn a_later_launch_takes_over_the_same_build_and_an_earlier_release_leaves_it() {
        let i = install("takeover");
        let (first, _) = place_builds(&i.addon, &[build(1)]);
        let (second, outcomes) = place_builds(&i.addon, &[build(1)]);
        assert_eq!(outcomes, vec![PlaceOutcome::AlreadyInPlace]);

        assert_eq!(first.release(), 0, "the second launch's game needs it");
        assert!(slot(&i.addon).exists());
        assert_eq!(second.release(), 1);
        assert!(!slot(&i.addon).exists());
    }

    #[test]
    fn another_build_studio_placed_is_replaced_for_a_new_launch() {
        let i = install("swap");
        let (first, _) = place_builds(&i.addon, &[build(1)]);
        let (second, outcomes) = place_builds(&i.addon, &[build(2)]);
        assert_eq!(outcomes, vec![PlaceOutcome::Placed]);
        assert_eq!(std::fs::read(slot(&i.addon)).unwrap(), test_map_bytes(2));
        assert_eq!(first.release(), 0);
        assert_eq!(second.release(), 1);
    }

    #[test]
    fn leftovers_of_a_closed_studio_are_found_and_removed() {
        let i = install("leftovers");
        // A Studio that placed a build and never released it.
        let (placement, _) = place_builds(&i.addon, &[build(1)]);
        std::mem::forget(placement);

        let sessions = placed_sessions(&i.addon);
        assert_eq!(sessions.len(), 1);
        assert_eq!(release_sessions(&i.addon, &sessions), 1);
        assert!(!slot(&i.addon).exists());
    }

    #[test]
    fn a_copy_cut_short_still_counts_as_studios_own() {
        let i = install("partial");
        let (placement, _) = place_builds(&i.addon, &[build(1)]);
        let full = test_map_bytes(1);
        std::fs::write(slot(&i.addon), &full[..full.len() / 2]).unwrap();
        assert_eq!(placement.release(), 1);
    }

    #[test]
    fn with_the_library_file_gone_a_placed_build_is_still_known_by_its_checksum() {
        let i = install("library_gone");
        let (placement, _) = place_builds(&i.addon, &[build(1)]);
        std::fs::remove_file(build_path(&library(&i.addon), "dod_x", build(1).checksum)).unwrap();
        assert_eq!(placement.release(), 1);
    }

    #[test]
    fn an_unreadable_record_deletes_nothing() {
        let i = install("bad_record");
        let (placement, _) = place_builds(&i.addon, &[build(1)]);
        std::fs::write(record_path(&i.addon), b"{ not json").unwrap();
        assert_eq!(placement.release(), 0);
        assert!(slot(&i.addon).exists());
        assert_eq!(
            std::fs::read(record_path(&i.addon)).unwrap(),
            b"{ not json",
            "never written over"
        );
        let (again, outcomes) = place_builds(&i.addon, &[build(2)]);
        assert!(again.is_empty());
        assert!(matches!(outcomes[..], [PlaceOutcome::Failed(_)]));
    }
}
