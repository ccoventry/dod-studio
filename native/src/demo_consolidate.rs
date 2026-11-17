//! The Demo Auditor's Consolidate (#872): every demo an audit found moves
//! into one folder. Only each duplicate group's kept copy moves; the other
//! copies go to the Recycle Bin. A move logs itself so one Undo puts every
//! demo back.
//!
//! [`plan`] decides everything up front, with no writes: where each demo
//! goes, which names clash, what stays, what is copied between drives and
//! whether it fits. [`run`] re-plans from the same request (the frontend's
//! preview is never trusted as the plan) and does it.
//!
//! Demos inside an install's game folder stay where they are unless the
//! request asks for them, since the game plays demos from there (the audit's
//! `InUse::GameFolder`). A move never replaces a file. A move between drives
//! is a copy, checked for size, then a delete of the original.
//!
//! The audit pairs copies by size and first 64 KiB, so before a copy is
//! recycled it is compared byte for byte with the kept copy, wherever that
//! has moved to (`demo_compare`, as the Duplicates delete does). One that
//! differs stays where it is and is reported.

use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

/// Where the logs go under the app's data folder, beside Rename Demos'.
pub const LOG_FOLDER: &str = "demo_consolidate_logs";

/// How the moved demos sit under the destination.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Layout {
    /// Every demo straight in the destination.
    #[default]
    Flat,
    /// Each demo's folders below the audited folder it was found in, kept.
    Subfolders,
}

/// One demo the audit listed.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuditedDemo {
    pub path: String,
    pub size: u64,
    /// Inside an install's game folder (`<install>\dod`).
    #[serde(default)]
    pub game_folder: bool,
    /// For a copy to recycle: its group's kept copy, which it must match
    /// byte for byte. Empty for a demo that moves.
    #[serde(default)]
    pub keep: String,
}

/// What the frontend asks for.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Request {
    pub destination: String,
    #[serde(default)]
    pub layout: Layout,
    /// Move (and recycle) demos inside an install's game folder too.
    #[serde(default)]
    pub include_game_folders: bool,
    /// The folders the audit walked, for [`Layout::Subfolders`].
    #[serde(default)]
    pub roots: Vec<String>,
    /// Each group's kept copy, and every demo without copies.
    pub keep: Vec<AuditedDemo>,
    /// Every group's other copies.
    pub recycle: Vec<AuditedDemo>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PlannedMove {
    pub from: String,
    pub to: String,
    pub size: u64,
    /// To another drive: a copy, then a delete.
    pub cross_drive: bool,
    /// Given `_2`, `_3`… because the name was taken.
    pub renamed: bool,
}

/// A copy to recycle once it matches the kept copy.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PlannedRecycle {
    pub path: String,
    pub keep: String,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct Plan {
    pub destination: String,
    pub moves: Vec<PlannedMove>,
    pub recycle: Vec<PlannedRecycle>,
    /// Demos left where they are because they're in a game folder.
    pub stay_in_game_folders: usize,
    /// Copies not recycled because they're in a game folder.
    pub copies_in_game_folders: usize,
    /// Demos already in the destination, left as they are.
    pub already_there: usize,
    pub renamed: usize,
    pub move_bytes: u64,
    /// The part of `move_bytes` copied from another drive.
    pub copy_bytes: u64,
    /// Free on the destination's drive.
    pub free_bytes: u64,
    /// `copy_bytes` fits in `free_bytes`.
    pub enough_space: bool,
}

/// A path without the `\\?\` (or `\\?\UNC\`) prefix the audit's canonical
/// paths carry, with backslashes.
fn plain(path: &str) -> String {
    let text = path.replace('/', "\\");
    match text.strip_prefix(r"\\?\UNC\") {
        Some(rest) => format!(r"\\{rest}"),
        None => text.strip_prefix(r"\\?\").unwrap_or(&text).to_string(),
    }
}

/// For comparing: plain, ASCII lower case (same byte length, so a prefix
/// found here slices the plain form), no trailing separator.
fn comparable(path: &str) -> String {
    plain(path).trim_end_matches('\\').to_ascii_lowercase()
}

/// `c:` for `c:\x\y.dem`, `\\nas\share` for a UNC path.
fn drive_of(comparable: &str) -> String {
    if let Some(rest) = comparable.strip_prefix(r"\\") {
        let mut parts = rest.splitn(3, '\\');
        let server = parts.next().unwrap_or_default();
        let share = parts.next().unwrap_or_default();
        return format!(r"\\{server}\{share}");
    }
    comparable
        .split('\\')
        .next()
        .unwrap_or_default()
        .to_string()
}

fn is_inside(comparable: &str, folder: &str) -> bool {
    comparable
        .strip_prefix(folder)
        .is_some_and(|rest| rest.starts_with('\\'))
}

fn is_demo(path: &str) -> bool {
    Path::new(path)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("dem"))
}

/// Where `source` goes under `dest`.
fn target_for(source: &str, dest: &Path, layout: Layout, roots: &[String]) -> PathBuf {
    let shown = plain(source);
    let name = Path::new(&shown)
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    if layout == Layout::Flat {
        return dest.join(name);
    }
    let lower = shown.to_ascii_lowercase();
    // The deepest audited folder holding it: a folder picked inside another
    // keeps the shorter path.
    let root = roots
        .iter()
        .map(|r| comparable(r))
        .filter(|r| !r.is_empty() && is_inside(&lower, r))
        .max_by_key(|r| r.len());
    match root {
        Some(r) => dest.join(&shown[r.len() + 1..]),
        None => dest.join(name),
    }
}

/// `target`, or `name_2.dem`, `name_3.dem`… when `taken` says it's in use.
fn free_name(target: PathBuf, taken: impl Fn(&Path) -> bool) -> (PathBuf, bool) {
    if !taken(&target) {
        return (target, false);
    }
    let stem = target
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = target
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    let parent = target.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut n = 2;
    loop {
        let candidate = parent.join(format!("{stem}_{n}{ext}"));
        if !taken(&candidate) {
            return (candidate, true);
        }
        n += 1;
    }
}

/// Decides the whole consolidation without touching a file. `exists` says
/// whether a path is already taken on disk; `free_bytes` is the
/// destination drive's free space.
pub fn plan(
    req: &Request,
    exists: &dyn Fn(&Path) -> bool,
    free_bytes: u64,
) -> Result<Plan, String> {
    let dest_text = plain(req.destination.trim());
    if dest_text.trim().is_empty() {
        return Err(crate::messages::CONSOLIDATE_NO_DESTINATION.to_string());
    }
    // `F:\Demos\` → `F:\Demos`, but a drive root keeps its separator:
    // `F:` alone names the drive's current folder.
    let trimmed = dest_text.trim().trim_end_matches('\\');
    let dest = if trimmed.ends_with(':') {
        PathBuf::from(format!("{trimmed}\\"))
    } else {
        PathBuf::from(trimmed)
    };
    let dest_cmp = comparable(&dest.to_string_lossy());
    let dest_drive = drive_of(&dest_cmp);
    let mut out = Plan {
        destination: dest.to_string_lossy().into_owned(),
        free_bytes,
        ..Plan::default()
    };

    let mut seen = HashSet::new();
    let mut keep: Vec<&AuditedDemo> = req
        .keep
        .iter()
        .filter(|d| is_demo(&d.path) && seen.insert(comparable(&d.path)))
        .collect();
    keep.sort_by_key(|d| comparable(&d.path));

    // Names already held: every demo that stays in the destination, first,
    // so a moved demo never takes one of theirs.
    let mut claimed: HashSet<String> = HashSet::new();
    let mut to_move = Vec::new();
    for demo in keep {
        let cmp = comparable(&demo.path);
        if demo.game_folder && !req.include_game_folders {
            out.stay_in_game_folders += 1;
        } else if is_inside(&cmp, &dest_cmp) {
            out.already_there += 1;
            claimed.insert(cmp);
        } else {
            to_move.push(demo);
        }
    }
    for demo in to_move {
        let wanted = target_for(&demo.path, &dest, req.layout, &req.roots);
        let (to, renamed) = free_name(wanted, |p| {
            claimed.contains(&comparable(&p.to_string_lossy())) || exists(p)
        });
        let to_text = to.to_string_lossy().into_owned();
        claimed.insert(comparable(&to_text));
        let cross_drive = drive_of(&comparable(&demo.path)) != dest_drive;
        out.renamed += usize::from(renamed);
        out.move_bytes += demo.size;
        if cross_drive {
            out.copy_bytes += demo.size;
        }
        out.moves.push(PlannedMove {
            from: demo.path.clone(),
            to: to_text,
            size: demo.size,
            cross_drive,
            renamed,
        });
    }

    let mut recycle_seen = HashSet::new();
    for copy in &req.recycle {
        let cmp = comparable(&copy.path);
        // Never a demo that is also being kept: the lists come from the
        // frontend.
        if !is_demo(&copy.path) || seen.contains(&cmp) || !recycle_seen.insert(cmp) {
            continue;
        }
        if copy.game_folder && !req.include_game_folders {
            out.copies_in_game_folders += 1;
        } else {
            out.recycle.push(PlannedRecycle {
                path: copy.path.clone(),
                keep: copy.keep.clone(),
            });
        }
    }
    out.enough_space = out.copy_bytes <= free_bytes;
    Ok(out)
}

/// [`plan`] against the real disk.
pub fn plan_on_disk(req: &Request) -> Result<Plan, String> {
    let dest = PathBuf::from(plain(req.destination.trim()));
    if dest.as_os_str().is_empty() {
        return Err(crate::messages::CONSOLIDATE_NO_DESTINATION.to_string());
    }
    if dest.exists() && !dest.is_dir() {
        return Err(crate::messages::consolidate_destination_bad(&dest));
    }
    // An unreadable drive never blocks the move (the pre-Option answer).
    let free = crate::sys::disk::get_available_bytes(&dest).unwrap_or(u64::MAX);
    plan(req, &|p| p.symlink_metadata().is_ok(), free)
}

/// One move done, as the log keeps it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MovedPair {
    pub from: String,
    pub to: String,
}

/// A move or a recycle that didn't happen, and why.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Failure {
    pub path: String,
    pub error: String,
    /// A copy whose bytes differ from the kept copy's: a different demo,
    /// marked "Not an exact copy" like the Duplicates delete marks it.
    pub not_a_copy: bool,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct Outcome {
    pub moved: Vec<MovedPair>,
    pub failed: Vec<Failure>,
    pub recycled: Vec<String>,
    pub recycle_failed: Vec<Failure>,
    pub cancelled: bool,
    /// The log that undoes this batch, when anything moved.
    pub log: Option<String>,
}

/// Which step a progress report is about.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    Moving,
    /// Comparing each copy with the kept one before recycling.
    Checking,
    Recycling,
    Undoing,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Progress {
    pub step: Step,
    pub files_done: usize,
    pub files_total: usize,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub current: String,
}

enum MoveError {
    Cancelled,
    Failed(String),
}

const COPY_CHUNK: usize = 1024 * 1024;

/// Copies `from` to the new file `to` (never replacing one) with its
/// modified time, so the analyzer cache's key still matches it. Reports
/// each chunk's bytes. A cancelled or failed copy is removed.
fn copy_file(
    from: &Path,
    to: &Path,
    cancel: &AtomicBool,
    on_bytes: &mut dyn FnMut(u64),
) -> Result<(), MoveError> {
    let fail = |e: std::io::Error| MoveError::Failed(e.to_string());
    let mut src = std::fs::File::open(from).map_err(fail)?;
    let meta = src.metadata().map_err(fail)?;
    let mut dst = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(to)
        .map_err(fail)?;
    let result = (|| {
        let mut buf = vec![0u8; COPY_CHUNK];
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(MoveError::Cancelled);
            }
            let n = src.read(&mut buf).map_err(fail)?;
            if n == 0 {
                break;
            }
            dst.write_all(&buf[..n]).map_err(fail)?;
            on_bytes(n as u64);
        }
        if let Ok(modified) = meta.modified() {
            let _ = dst.set_modified(modified);
        }
        dst.sync_all().map_err(fail)?;
        let copied = dst.metadata().map_err(fail)?.len();
        if copied != meta.len() {
            return Err(MoveError::Failed(
                crate::messages::consolidate_copy_mismatch(to),
            ));
        }
        Ok(())
    })();
    drop(dst);
    if result.is_err() {
        let _ = std::fs::remove_file(to);
    }
    result
}

/// Moves one demo, never over a file, and moves its analyzer cache entry
/// with it (the cache is keyed on the demo's path).
fn move_one(
    from: &Path,
    to: &Path,
    cancel: &AtomicBool,
    on_bytes: &mut dyn FnMut(u64),
) -> Result<(), MoveError> {
    if !from.is_file() {
        return Err(MoveError::Failed(
            crate::messages::CONSOLIDATE_SOURCE_GONE.to_string(),
        ));
    }
    if to.symlink_metadata().is_ok() {
        return Err(MoveError::Failed(
            crate::messages::CONSOLIDATE_TARGET_EXISTS.to_string(),
        ));
    }
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent).map_err(|e| MoveError::Failed(e.to_string()))?;
    }
    let old_cache = crate::analyzer_cache_path(from);
    let size = std::fs::metadata(from).map(|m| m.len()).unwrap_or(0);
    // A rename on one drive is instant. Between drives it fails, and the
    // demo is copied and the original deleted.
    if std::fs::rename(from, to).is_ok() {
        on_bytes(size);
    } else {
        copy_file(from, to, cancel, on_bytes)?;
        if let Err(e) = std::fs::remove_file(from) {
            // Two copies would be left: keep the original, drop the new one.
            let _ = std::fs::remove_file(to);
            return Err(MoveError::Failed(e.to_string()));
        }
    }
    if let Some(old_cache) = old_cache {
        crate::move_analyzer_cache_entry(&old_cache, to);
    }
    Ok(())
}

/// A batch's log file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct ConsolidateLog {
    created_unix_secs: u64,
    destination: String,
    moves: Vec<MovedPair>,
    #[serde(default)]
    undone: bool,
}

/// The batch the Undo button would put back.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct UndoableBatch {
    pub log: String,
    pub count: usize,
    pub destination: String,
    pub created_unix_secs: u64,
}

fn now_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn new_log_path(log_dir: &Path, created: u64) -> Option<PathBuf> {
    std::fs::create_dir_all(log_dir).ok()?;
    // Seconds plus a counter, so two batches in one second keep both logs.
    (0..1000)
        .map(|n| log_dir.join(format!("consolidate_{created}_{n:03}.json")))
        .find(|p| !p.exists())
}

fn save_log(path: &Path, log: &ConsolidateLog) {
    if let Ok(json) = serde_json::to_vec_pretty(log) {
        let _ = std::fs::write(path, json);
    }
}

/// How often the log is saved during a batch, in moves: a crash part-way
/// still leaves an Undo for what had moved.
const LOG_EVERY: usize = 25;

/// Does the consolidation `req` asks for: re-plans it, refuses it when the
/// copies don't fit, moves every demo, then recycles the copies (not after
/// a Cancel). `on_progress` gets every step; the caller throttles.
pub fn run(
    req: &Request,
    log_dir: &Path,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(&Progress),
) -> Result<Outcome, String> {
    let plan = plan_on_disk(req)?;
    if !plan.enough_space {
        return Err(crate::messages::consolidate_not_enough_space(
            plan.copy_bytes,
            plan.free_bytes,
            Path::new(&plan.destination),
        ));
    }
    Ok(run_plan(&plan, log_dir, cancel, on_progress))
}

fn run_plan(
    plan: &Plan,
    log_dir: &Path,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(&Progress),
) -> Outcome {
    let mut outcome = Outcome::default();
    let created = now_unix_secs();
    let mut log = ConsolidateLog {
        created_unix_secs: created,
        destination: plan.destination.clone(),
        moves: Vec::new(),
        undone: false,
    };
    let mut log_path: Option<PathBuf> = None;
    let mut progress = Progress {
        step: Step::Moving,
        files_done: 0,
        files_total: plan.moves.len(),
        bytes_done: 0,
        bytes_total: plan.move_bytes,
        current: String::new(),
    };
    for (i, m) in plan.moves.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            outcome.cancelled = true;
            break;
        }
        progress.current = plain(&m.from);
        on_progress(&progress);
        let start_bytes = progress.bytes_done;
        let result = move_one(Path::new(&m.from), Path::new(&m.to), cancel, &mut |n| {
            progress.bytes_done += n;
            on_progress(&progress);
        });
        // Whatever a demo's copy reported, it counts its own size once.
        progress.bytes_done = start_bytes + m.size;
        progress.files_done = i + 1;
        match result {
            Ok(()) => {
                let pair = MovedPair {
                    from: plain(&m.from),
                    to: m.to.clone(),
                };
                outcome.moved.push(pair.clone());
                log.moves.push(pair);
                if log.moves.len() % LOG_EVERY == 1 {
                    if log_path.is_none() {
                        log_path = new_log_path(log_dir, created);
                    }
                    if let Some(p) = &log_path {
                        save_log(p, &log);
                    }
                }
            }
            Err(MoveError::Cancelled) => {
                outcome.cancelled = true;
                break;
            }
            Err(MoveError::Failed(error)) => outcome.failed.push(Failure {
                path: m.from.clone(),
                error: crate::messages::consolidate_not_moved(Path::new(&m.from), &error),
                not_a_copy: false,
            }),
        }
    }
    on_progress(&progress);
    if !log.moves.is_empty() {
        if log_path.is_none() {
            log_path = new_log_path(log_dir, created);
        }
        if let Some(p) = &log_path {
            save_log(p, &log);
            outcome.log = Some(p.to_string_lossy().into_owned());
        }
    }

    if !outcome.cancelled && !plan.recycle.is_empty() {
        let recyclable = check_copies(plan, &mut outcome, cancel, on_progress);
        if !outcome.cancelled {
            recycle_copies(recyclable, &mut outcome, on_progress);
        }
    }
    outcome
}

/// The copies that match their kept copy byte for byte, wherever the kept
/// copy is now (moved this batch, or where it was). The others go into
/// `outcome.recycle_failed`. A Cancel stops it, and then nothing is recycled.
fn check_copies(
    plan: &Plan,
    outcome: &mut Outcome,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(&Progress),
) -> Vec<String> {
    let moved_to: HashMap<String, String> = outcome
        .moved
        .iter()
        .map(|m| (comparable(&m.from), m.to.clone()))
        .collect();
    let mut report = Progress {
        step: Step::Checking,
        files_done: 0,
        files_total: plan.recycle.len(),
        bytes_done: 0,
        bytes_total: 0,
        current: String::new(),
    };
    let mut recyclable = Vec::new();
    for copy in &plan.recycle {
        if cancel.load(Ordering::Relaxed) {
            outcome.cancelled = true;
            return Vec::new();
        }
        report.current = plain(&copy.path);
        on_progress(&report);
        let keep = moved_to.get(&comparable(&copy.keep)).unwrap_or(&copy.keep);
        match crate::demo_compare::check_same_contents(Path::new(&copy.path), Path::new(keep)) {
            Ok(()) => recyclable.push(copy.path.clone()),
            Err(refusal) => {
                let (error, not_a_copy) = refusal.into_parts();
                outcome.recycle_failed.push(Failure {
                    path: copy.path.clone(),
                    error,
                    not_a_copy,
                });
            }
        }
        report.files_done += 1;
    }
    report.current.clear();
    on_progress(&report);
    recyclable
}

/// Sends the checked copies to the Recycle Bin.
fn recycle_copies(
    recyclable: Vec<String>,
    outcome: &mut Outcome,
    on_progress: &mut dyn FnMut(&Progress),
) {
    if recyclable.is_empty() {
        return;
    }
    let mut report = Progress {
        step: Step::Recycling,
        files_done: 0,
        files_total: recyclable.len(),
        bytes_done: 0,
        bytes_total: 0,
        current: String::new(),
    };
    on_progress(&report);
    let list: Vec<PathBuf> = recyclable.iter().map(PathBuf::from).collect();
    let errors = crate::sys::recycle_bin::recycle(list, |done| {
        report.files_done = done;
        on_progress(&report);
    });
    for (path, error) in recyclable.into_iter().zip(errors) {
        match error {
            None => outcome.recycled.push(path),
            Some(error) => outcome.recycle_failed.push(Failure {
                error: crate::messages::consolidate_not_recycled(Path::new(&path), &error),
                path,
                not_a_copy: false,
            }),
        }
    }
}

/// The logs in `log_dir`, newest first (their names sort by time).
fn logs(log_dir: &Path) -> Vec<(PathBuf, ConsolidateLog)> {
    let Ok(entries) = std::fs::read_dir(log_dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("consolidate_"))
                && p.extension().is_some_and(|e| e == "json")
        })
        .collect();
    paths.sort();
    paths.reverse();
    paths
        .into_iter()
        .filter_map(|p| {
            let log = serde_json::from_slice(&std::fs::read(&p).ok()?).ok()?;
            Some((p, log))
        })
        .collect()
}

/// The newest batch not undone yet.
pub fn undoable(log_dir: &Path) -> Option<UndoableBatch> {
    logs(log_dir)
        .into_iter()
        .find(|(_, log)| !log.undone && !log.moves.is_empty())
        .map(|(path, log)| UndoableBatch {
            log: path.to_string_lossy().into_owned(),
            count: log.moves.len(),
            destination: log.destination,
            created_unix_secs: log.created_unix_secs,
        })
}

/// Puts the newest batch not undone yet back, last move first. A demo that
/// has moved again since, or whose old place is taken, is reported and left
/// alone. A Cancel keeps the moves not reached in the log, so the next Undo
/// finishes them. Recycled copies stay in the Recycle Bin.
pub fn undo_last(
    log_dir: &Path,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(&Progress),
) -> Outcome {
    let Some((path, mut log)) = logs(log_dir)
        .into_iter()
        .find(|(_, log)| !log.undone && !log.moves.is_empty())
    else {
        return Outcome::default();
    };
    let sizes: Vec<u64> = log
        .moves
        .iter()
        .map(|m| std::fs::metadata(&m.to).map(|md| md.len()).unwrap_or(0))
        .collect();
    let mut progress = Progress {
        step: Step::Undoing,
        files_done: 0,
        files_total: log.moves.len(),
        bytes_done: 0,
        bytes_total: sizes.iter().sum(),
        current: String::new(),
    };
    let mut outcome = Outcome::default();
    let mut not_reached = Vec::new();
    let pending: Vec<(MovedPair, u64)> = log.moves.drain(..).zip(sizes).rev().collect();
    for (pair, size) in pending {
        if outcome.cancelled || cancel.load(Ordering::Relaxed) {
            outcome.cancelled = true;
            not_reached.push(pair);
            continue;
        }
        progress.current = pair.to.clone();
        on_progress(&progress);
        let start = progress.bytes_done;
        let result = move_one(
            Path::new(&pair.to),
            Path::new(&pair.from),
            cancel,
            &mut |n| {
                progress.bytes_done += n;
                on_progress(&progress);
            },
        );
        progress.bytes_done = start + size;
        progress.files_done += 1;
        match result {
            Ok(()) => outcome.moved.push(MovedPair {
                from: pair.to,
                to: pair.from,
            }),
            Err(MoveError::Cancelled) => {
                outcome.cancelled = true;
                not_reached.push(pair);
            }
            Err(MoveError::Failed(error)) => outcome.failed.push(Failure {
                error: crate::messages::consolidate_not_put_back(Path::new(&pair.to), &error),
                path: pair.to,
                not_a_copy: false,
            }),
        }
    }
    on_progress(&progress);
    // Back in the log's order (it was walked last first).
    not_reached.reverse();
    log.undone = not_reached.is_empty();
    log.moves = not_reached;
    save_log(&path, &log);
    outcome.log = Some(path.to_string_lossy().into_owned());
    outcome
}

#[cfg(test)]
#[path = "demo_consolidate_tests.rs"]
mod tests;
