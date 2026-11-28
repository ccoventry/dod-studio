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
//!   game runs or is starting (`hl.exe` or `hlae.exe`), and comes out only
//!   once none does; between those, one game plays one build of each map.
//! - **Never over a file Studio didn't put there.** A build is copied to a
//!   name of Studio's own beside the slot, then hard-linked into the slot,
//!   which fails if anything is there: the slot is never overwritten, and is
//!   never a half copy. It is emptied only while its bytes are still the
//!   library build's (or, with the library file gone, its checksum is still
//!   the build's). Anything else there is the user's and is left alone.
//! - **A record of what is in place**, `placed.json` in the library folder,
//!   written before each copy, so builds left by a Studio that closed or
//!   crashed are removed at its next start. Every change to it holds
//!   `placed.lock`, opened with no sharing, so two Studios on one install
//!   never interleave.
//!
//! Each launch's builds belong to a session; removing a session's builds
//! never touches a build a later launch took over.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use lock_recover::lock_or_recover;
use serde::{Deserialize, Serialize};

use super::bsp;
use super::build_library::{LIBRARY_FOLDER, build_path};
use crate::log_markdown;
use crate::messages::map_builds as msg;

/// The record of placed builds, in the library folder.
const RECORD_FILE: &str = "placed.json";
/// Held, unshared, across every change to the record.
const LOCK_FILE: &str = "placed.lock";
/// The suffix of Studio's copy beside the slot, before it is linked in.
const PART_SUFFIX: &str = ".dodstudio-part";
/// How long a change to the record waits for another Studio's.
const LOCK_WAIT: Duration = Duration::from_secs(5);
const LOCK_RETRY: Duration = Duration::from_millis(50);
/// How often a release waiting on the game looks at the process list: it
/// only watches a process stay alive (CLAUDE.md, process lifecycles).
const GAME_POLL: Duration = Duration::from_millis(500);

/// The processes that mean a game runs or is on its way: the game, and the
/// HLAE launcher starting it (as Launch Preview's process check counts them).
pub const GAME_PROCESSES: &[&str] = &["hl.exe", "hlae.exe"];

/// One read-modify-write of a record at a time within this Studio.
static RECORD_LOCK: Mutex<()> = Mutex::new(());
/// Makes two sessions started in the same nanosecond still differ.
static SESSION_COUNTER: AtomicU64 = AtomicU64::new(0);
/// Set when Studio closes: every wait for a game stops (`release_at_exit`).
static SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);
/// This Studio's sessions with builds still in place, and the leftovers it
/// is waiting to remove: what `release_at_exit` removes.
static ACTIVE: Mutex<Vec<(PathBuf, u64)>> = Mutex::new(Vec::new());

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

/// Studio's copy beside the slot while it is being written.
fn part_path(addon: &Path, map_name: &str) -> PathBuf {
    addon
        .join("maps")
        .join(format!("{map_name}.bsp{PART_SUFFIX}"))
}

fn library(addon: &Path) -> PathBuf {
    addon.join(LIBRARY_FOLDER)
}

fn record_path(addon: &Path) -> PathBuf {
    library(addon).join(RECORD_FILE)
}

/// Whether any game is running or starting.
pub fn games_running() -> bool {
    crate::sys::process::is_running(GAME_PROCESSES)
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

/// The record held for a change: this Studio's lock and the unshared file.
struct RecordGuard {
    _file: std::fs::File,
    _in_process: std::sync::MutexGuard<'static, ()>,
}

/// Takes the record for a change, waiting up to `wait` for another Studio.
fn lock_record_within(addon: &Path, wait: Duration) -> Result<RecordGuard, String> {
    let in_process = lock_or_recover(&RECORD_LOCK);
    let dir = library(addon);
    std::fs::create_dir_all(&dir).map_err(|e| msg::could_not_write(dir.display(), e))?;
    let path = dir.join(LOCK_FILE);
    let since = Instant::now();
    loop {
        match open_unshared(&path) {
            Ok(file) => {
                return Ok(RecordGuard {
                    _file: file,
                    _in_process: in_process,
                });
            }
            Err(_) if since.elapsed() < wait => std::thread::sleep(LOCK_RETRY),
            Err(_) => return Err(msg::record_busy(path.display(), wait.as_secs())),
        }
    }
}

#[cfg(windows)]
fn open_unshared(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .share_mode(0)
        .open(path)
}

#[cfg(not(windows))]
fn open_unshared(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
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

/// Whether the file at `slot` is still the build Studio put there: the same
/// bytes as the library file, or, with the library file gone, the build's
/// checksum. A slot only ever receives a whole copy (it is linked in after
/// the copy finished), so anything shorter is not Studio's.
fn holds_build(slot: &Path, library_file: &Path, checksum: u32) -> bool {
    let Ok(placed) = std::fs::read(slot) else {
        return false;
    };
    match std::fs::read(library_file) {
        Ok(build) => build == placed,
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
    SlotNotOurs { slot: PathBuf },
    /// The library doesn't hold the build.
    NotInLibrary,
    /// The record of placed builds can't be read, so nothing is placed.
    RecordUnreadable { record: PathBuf },
    /// The copy, the record, or its lock failed.
    Failed(String),
}

impl PlaceOutcome {
    /// Whether the build is in place for the launch.
    pub fn is_in_place(&self) -> bool {
        matches!(self, Self::Placed | Self::AlreadyInPlace)
    }
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
        forget_active(&self.addon, self.session);
        release_sessions(&self.addon, &[self.session])
    }

    /// Removes this launch's builds on a thread of their own once no game
    /// runs or starts: after waiting up to `start_wait` for one to show up,
    /// then for every one to close. Any game, not only this launch's: one
    /// Studio cannot tell apart may have loaded the build too. Stops without
    /// removing anything when Studio closes (`release_at_exit` decides then).
    pub fn release_after_game(self, start_wait: Duration) {
        if self.is_empty() {
            return;
        }
        let spawned = std::thread::Builder::new()
            .name("map_builds_release".into())
            .spawn(move || {
                let end = wait_for_games_to_end(start_wait, GAME_POLL, &SHUTTING_DOWN, &mut || {
                    games_running()
                });
                if end == WaitEnd::GamesClosed {
                    self.release();
                }
            });
        if let Err(e) = spawned {
            log_markdown(&msg::release_thread_failed(e));
        }
    }
}

/// How a wait for the games ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitEnd {
    /// No game runs or starts any more.
    GamesClosed,
    /// `cancel` was set (Studio is closing).
    Cancelled,
}

/// Waits up to `start_wait` for `games_running` to say a game runs, then
/// until it says none does, looking every `poll` and checking `cancel`
/// every time.
pub fn wait_for_games_to_end(
    start_wait: Duration,
    poll: Duration,
    cancel: &AtomicBool,
    games_running: &mut dyn FnMut() -> bool,
) -> WaitEnd {
    let since = Instant::now();
    let mut running = games_running();
    while !running && since.elapsed() < start_wait {
        if cancel.load(Ordering::Relaxed) {
            return WaitEnd::Cancelled;
        }
        std::thread::sleep(poll);
        running = games_running();
    }
    while running {
        if cancel.load(Ordering::Relaxed) {
            return WaitEnd::Cancelled;
        }
        std::thread::sleep(poll);
        running = games_running();
    }
    WaitEnd::GamesClosed
}

fn remember_active(addon: &Path, session: u64) {
    lock_or_recover(&ACTIVE).push((addon.to_path_buf(), session));
}

fn forget_active(addon: &Path, session: u64) {
    lock_or_recover(&ACTIVE).retain(|(a, s)| !(a == addon && *s == session));
}

/// Puts each of `wants` in place in `addon`'s maps folder for one new launch.
///
/// The caller makes sure no game runs or starts (see the module doc). A
/// build Studio already put there for an earlier launch is taken over,
/// another build of the same map Studio put there is replaced, and a file
/// Studio didn't put there is left alone. Logs each step to the activity
/// log. Returns the launch's [`Placement`] and what happened to each want.
pub fn place_builds(addon: &Path, wants: &[MapBuild]) -> (Placement, Vec<PlaceOutcome>) {
    let mut placement = Placement {
        addon: addon.to_path_buf(),
        session: new_session(),
        builds: Vec::new(),
    };
    let _guard = match lock_record_within(addon, LOCK_WAIT) {
        Ok(guard) => guard,
        Err(why) => {
            log_markdown(&msg::lock_failed(&why));
            return (placement, vec![PlaceOutcome::Failed(why); wants.len()]);
        }
    };
    let Some(mut record) = load_record(addon) else {
        let unreadable = PlaceOutcome::RecordUnreadable {
            record: record_path(addon),
        };
        return (placement, vec![unreadable; wants.len()]);
    };
    let outcomes: Vec<PlaceOutcome> = wants
        .iter()
        .map(|want| place_one(addon, &mut record, want, placement.session))
        .collect();
    placement.builds = wants
        .iter()
        .zip(&outcomes)
        .filter(|(_, outcome)| outcome.is_in_place())
        .map(|(want, _)| want.clone())
        .collect();
    if !placement.is_empty() {
        remember_active(addon, placement.session);
    }
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
            return PlaceOutcome::SlotNotOurs { slot };
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
    }
    // The slot is empty now: whatever the record still says about it (the
    // build just removed, or one the user deleted by hand) is stale, and an
    // old session's release must not take this launch's copy for its own.
    record.placed.retain(|p| p.map_name != want.map_name);
    copy_into_slot(addon, record, want, session, &library_file)
}

/// Records the build, then copies it into the empty slot. The record comes
/// first, so a Studio that dies mid-copy still knows what to clean up.
fn copy_into_slot(
    addon: &Path,
    record: &mut Record,
    want: &MapBuild,
    session: u64,
    library_file: &Path,
) -> PlaceOutcome {
    let started = Instant::now();
    let slot = slot_path(addon, &want.map_name);
    record.placed.push(PlacedBuild {
        map_name: want.map_name.clone(),
        checksum: want.checksum,
        session,
    });
    if let Err(e) = save_record(addon, record) {
        log_markdown(&msg::record_unwritable(record_path(addon).display(), &e));
        record.placed.pop();
        return PlaceOutcome::Failed(e.to_string());
    }
    let copied = link_copy(library_file, &part_path(addon, &want.map_name), &slot);
    if !matches!(copied, Ok(Linked::Linked)) {
        record.placed.pop();
        if let Err(save) = save_record(addon, record) {
            log_markdown(&msg::record_unwritable(record_path(addon).display(), save));
        }
    }
    match copied {
        Ok(Linked::Linked) => {
            log_markdown(&msg::placed(
                &want.map_name,
                want.checksum,
                slot.display(),
                started.elapsed().as_millis(),
            ));
            PlaceOutcome::Placed
        }
        Ok(Linked::SlotTaken) => {
            log_markdown(&msg::slot_not_ours(&want.map_name, slot.display()));
            PlaceOutcome::SlotNotOurs { slot }
        }
        Err(e) => {
            log_markdown(&msg::place_failed(
                &want.map_name,
                want.checksum,
                slot.display(),
                &e,
            ));
            PlaceOutcome::Failed(e.to_string())
        }
    }
}

/// How [`link_copy`] ended.
#[derive(Debug, PartialEq, Eq)]
enum Linked {
    Linked,
    /// Something appeared in the slot since it was checked; left alone.
    SlotTaken,
}

/// Copies `from` whole to `part` (Studio's own name), then links it in as
/// `slot`, which fails when anything is there, and removes `part`. A drive
/// without hard links (FAT, exFAT) gets a rename instead, made only while
/// the slot is still empty.
fn link_copy(from: &Path, part: &Path, slot: &Path) -> std::io::Result<Linked> {
    if let Some(parent) = slot.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // A part left by a Studio that died mid-copy: Studio's own name.
    let _ = std::fs::remove_file(part);
    let copied = std::fs::copy(from, part)
        .and_then(|_| std::fs::OpenOptions::new().write(true).open(part))
        .and_then(|file| file.sync_all());
    if let Err(e) = copied {
        let _ = std::fs::remove_file(part);
        return Err(e);
    }
    let linked = match std::fs::hard_link(part, slot) {
        Ok(()) => Ok(Linked::Linked),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(Linked::SlotTaken),
        Err(_) if slot.exists() => Ok(Linked::SlotTaken),
        Err(_) => std::fs::rename(part, slot).map(|()| Linked::Linked),
    };
    // Gone already after a rename; the slot keeps the bytes after a link.
    let _ = std::fs::remove_file(part);
    linked
}

/// Removes the builds of `sessions` from `addon`'s maps folder: each one
/// still holding its build is deleted, each one changed since is left alone
/// and forgotten. A delete that fails stays in the record for the next
/// start. Returns how many came out.
pub fn release_sessions(addon: &Path, sessions: &[u64]) -> usize {
    if !record_path(addon).exists() {
        return 0;
    }
    let _guard = match lock_record_within(addon, LOCK_WAIT) {
        Ok(guard) => guard,
        Err(why) => {
            log_markdown(&msg::remove_failed(addon.display(), why));
            return 0;
        }
    };
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
        // A copy a Studio that died mid-copy left under Studio's own name.
        let _ = std::fs::remove_file(part_path(addon, &placed.map_name));
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
    if !record_path(addon).exists() {
        return Vec::new();
    }
    let _guard = lock_or_recover(&RECORD_LOCK);
    let sessions: HashSet<u64> = load_record(addon)
        .map(|record| record.placed.iter().map(|p| p.session).collect())
        .unwrap_or_default();
    sessions.into_iter().collect()
}

/// The build Studio has in `addon`'s slot for `map_name`, or `None` when the
/// slot is empty or holds a file of the user's.
pub fn placed_build(addon: &Path, map_name: &str) -> Option<u32> {
    let slot = slot_path(addon, map_name);
    if !slot.is_file() || !record_path(addon).exists() {
        return None;
    }
    let _guard = lock_or_recover(&RECORD_LOCK);
    load_record(addon)?
        .placed
        .iter()
        .filter(|p| p.map_name == map_name)
        .find(|p| {
            holds_build(
                &slot,
                &build_path(&library(addon), &p.map_name, p.checksum),
                p.checksum,
            )
        })
        .map(|p| p.checksum)
}

/// Whether the file at `addon`'s slot for `map_name` is a build Studio put
/// there, rather than one of the user's.
pub fn is_studio_placement(addon: &Path, map_name: &str) -> bool {
    placed_build(addon, map_name).is_some()
}

/// At Studio's start: the builds an earlier Studio left in place (closed or
/// crashed before its game did) are removed on a thread of their own once no
/// game runs or starts; a game still running may have loaded them. Only the
/// builds in place now: one a launch from this Studio takes over in the
/// meantime stays.
pub fn release_leftovers_after_games(addon: &Path) {
    let sessions = placed_sessions(addon);
    if sessions.is_empty() {
        return;
    }
    log_markdown(&msg::leftovers_found(sessions.len(), addon.display()));
    for &session in &sessions {
        remember_active(addon, session);
    }
    let addon = addon.to_path_buf();
    let spawned = std::thread::Builder::new()
        .name("map_builds_leftovers".into())
        .spawn(move || {
            let end = wait_for_games_to_end(Duration::ZERO, GAME_POLL, &SHUTTING_DOWN, &mut || {
                games_running()
            });
            if end == WaitEnd::GamesClosed {
                for &session in &sessions {
                    forget_active(&addon, session);
                }
                release_sessions(&addon, &sessions);
            }
        });
    if let Err(e) = spawned {
        log_markdown(&msg::release_thread_failed(e));
    }
}

/// When Studio closes: stops every wait for a game, then removes this
/// Studio's builds (and the leftovers it was waiting on) now when no game
/// runs or starts, or leaves them, logged, to the next start's clean-up.
/// Returns how many came out.
pub fn release_at_exit() -> usize {
    SHUTTING_DOWN.store(true, Ordering::Relaxed);
    let active = std::mem::take(&mut *lock_or_recover(&ACTIVE));
    release_or_leave(&active, games_running())
}

/// `release_at_exit`'s choice for `active` (addon, session) pairs: removed
/// now with no game running, else left, logged, to the next start.
fn release_or_leave(active: &[(PathBuf, u64)], games_running: bool) -> usize {
    if active.is_empty() {
        return 0;
    }
    if games_running {
        log_markdown(&msg::left_at_exit(active.len()));
        return 0;
    }
    let removed = active
        .iter()
        .map(|(addon, session)| release_sessions(addon, &[*session]))
        .sum();
    log_markdown(&msg::released_at_exit(removed));
    removed
}

#[cfg(test)]
#[path = "build_placement_tests.rs"]
mod tests;
