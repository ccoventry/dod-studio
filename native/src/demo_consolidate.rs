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

use std::collections::HashSet;
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

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct Plan {
    pub destination: String,
    pub moves: Vec<PlannedMove>,
    pub recycle: Vec<String>,
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
            out.recycle.push(copy.path.clone());
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
    let free = crate::sys::disk::get_available_bytes(&dest);
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
                error,
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
        let total = plan.recycle.len();
        let list: Vec<PathBuf> = plan.recycle.iter().map(PathBuf::from).collect();
        let mut report = Progress {
            step: Step::Recycling,
            files_done: 0,
            files_total: total,
            bytes_done: 0,
            bytes_total: 0,
            current: String::new(),
        };
        on_progress(&report);
        let errors = crate::sys::recycle_bin::recycle(list, |done| {
            report.files_done = done;
            on_progress(&report);
        });
        for (path, error) in plan.recycle.iter().zip(errors) {
            match error {
                None => outcome.recycled.push(path.clone()),
                Some(error) => outcome.recycle_failed.push(Failure {
                    path: path.clone(),
                    error,
                }),
            }
        }
    }
    outcome
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
                path: pair.to,
                error,
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
mod tests {
    use super::*;
    use crate::test_support::Scratch;

    fn demo(path: &str, size: u64) -> AuditedDemo {
        AuditedDemo {
            path: path.into(),
            size,
            game_folder: false,
        }
    }

    fn game(path: &str) -> AuditedDemo {
        AuditedDemo {
            game_folder: true,
            ..demo(path, 1)
        }
    }

    fn request(keep: Vec<AuditedDemo>, recycle: Vec<AuditedDemo>) -> Request {
        Request {
            destination: r"F:\Demos".into(),
            keep,
            recycle,
            ..Request::default()
        }
    }

    fn targets(plan: &Plan) -> Vec<(&str, &str)> {
        plan.moves
            .iter()
            .map(|m| (m.from.as_str(), m.to.as_str()))
            .collect()
    }

    fn nothing_exists(_: &Path) -> bool {
        false
    }

    #[test]
    fn two_different_demos_with_one_name_get_numbered() {
        let req = request(
            vec![
                demo(r"\\?\C:\a\match.dem", 10),
                demo(r"\\?\D:\b\match.dem", 20),
                demo(r"\\?\E:\c\MATCH.dem", 30),
            ],
            vec![],
        );
        let plan = plan(&req, &nothing_exists, u64::MAX).unwrap();
        assert_eq!(
            targets(&plan),
            [
                (r"\\?\C:\a\match.dem", r"F:\Demos\match.dem"),
                (r"\\?\D:\b\match.dem", r"F:\Demos\match_2.dem"),
                (r"\\?\E:\c\MATCH.dem", r"F:\Demos\MATCH_3.dem"),
            ]
        );
        assert_eq!(plan.renamed, 2);
        assert_eq!((plan.move_bytes, plan.copy_bytes), (60, 60));
    }

    #[test]
    fn a_file_already_in_the_destination_is_never_replaced() {
        let req = request(vec![demo(r"C:\x\anzio.dem", 1)], vec![]);
        let taken = |p: &Path| p == Path::new(r"F:\Demos\anzio.dem");
        let plan = plan(&req, &taken, u64::MAX).unwrap();
        assert_eq!(
            targets(&plan),
            [(r"C:\x\anzio.dem", r"F:\Demos\anzio_2.dem")]
        );
    }

    #[test]
    fn a_demo_already_in_the_destination_stays_and_keeps_its_name() {
        let req = request(
            vec![demo(r"F:\Demos\old\m.dem", 5), demo(r"C:\m.dem", 5)],
            vec![],
        );
        let plan = plan(
            &Request {
                layout: Layout::Subfolders,
                roots: vec![r"C:\".into(), r"F:\Demos".into()],
                ..req
            },
            &nothing_exists,
            u64::MAX,
        )
        .unwrap();
        assert_eq!(plan.already_there, 1);
        assert_eq!(targets(&plan), [(r"C:\m.dem", r"F:\Demos\m.dem")]);

        // Flat: the one already there claims its name first.
        let req = request(
            vec![demo(r"C:\anzio.dem", 1), demo(r"F:\Demos\anzio.dem", 1)],
            vec![],
        );
        let flat = super::plan(&req, &nothing_exists, u64::MAX).unwrap();
        assert_eq!(flat.already_there, 1);
        assert_eq!(targets(&flat), [(r"C:\anzio.dem", r"F:\Demos\anzio_2.dem")]);
    }

    #[test]
    fn subfolders_are_kept_below_the_deepest_audited_folder() {
        let req = Request {
            layout: Layout::Subfolders,
            roots: vec![r"C:\".into(), r"C:\Users\x\Demos".into(), r"E:\".into()],
            ..request(
                vec![
                    demo(r"\\?\C:\Users\x\Demos\wsod\day1\a.dem", 1),
                    demo(r"\\?\C:\other\b.dem", 1),
                    demo(r"\\?\E:\c.dem", 1),
                    demo(r"\\?\G:\stray\d.dem", 1),
                ],
                vec![],
            )
        };
        let plan = plan(&req, &nothing_exists, u64::MAX).unwrap();
        assert_eq!(
            targets(&plan),
            [
                (r"\\?\C:\other\b.dem", r"F:\Demos\other\b.dem"),
                (
                    r"\\?\C:\Users\x\Demos\wsod\day1\a.dem",
                    r"F:\Demos\wsod\day1\a.dem"
                ),
                (r"\\?\E:\c.dem", r"F:\Demos\c.dem"),
                // Under no audited folder: straight in the destination.
                (r"\\?\G:\stray\d.dem", r"F:\Demos\d.dem"),
            ]
        );
    }

    #[test]
    fn only_kept_copies_move_and_the_others_are_recycled() {
        let req = request(
            vec![demo(r"C:\keep.dem", 1)],
            vec![
                demo(r"D:\copy.dem", 1),
                // Listed as kept too: never recycled.
                demo(r"c:\KEEP.dem", 1),
                demo(r"D:\notes.txt", 1),
            ],
        );
        let plan = plan(&req, &nothing_exists, u64::MAX).unwrap();
        assert_eq!(targets(&plan), [(r"C:\keep.dem", r"F:\Demos\keep.dem")]);
        assert_eq!(plan.recycle, [r"D:\copy.dem"]);
    }

    #[test]
    fn game_folder_demos_stay_unless_asked_for() {
        let req = request(
            vec![game(r"C:\HL\dod\pov.dem"), demo(r"C:\x\b.dem", 1)],
            vec![game(r"C:\HL\dod\copy.dem")],
        );
        let plan_default = plan(&req, &nothing_exists, u64::MAX).unwrap();
        assert_eq!(targets(&plan_default), [(r"C:\x\b.dem", r"F:\Demos\b.dem")]);
        assert_eq!(plan_default.stay_in_game_folders, 1);
        assert_eq!(plan_default.copies_in_game_folders, 1);
        assert!(plan_default.recycle.is_empty());

        let ticked = Request {
            include_game_folders: true,
            ..req
        };
        let plan_ticked = super::plan(&ticked, &nothing_exists, u64::MAX).unwrap();
        assert_eq!(plan_ticked.moves.len(), 2);
        assert_eq!(plan_ticked.recycle, [r"C:\HL\dod\copy.dem"]);
    }

    #[test]
    fn only_moves_between_drives_need_space() {
        let req = request(
            vec![demo(r"F:\x\a.dem", 100), demo(r"C:\b.dem", 50)],
            vec![],
        );
        let plan = plan(&req, &nothing_exists, 60).unwrap();
        assert_eq!(plan.move_bytes, 150);
        assert_eq!(plan.copy_bytes, 50);
        assert!(plan.enough_space);
        assert!(
            !plan
                .moves
                .iter()
                .find(|m| m.from == r"F:\x\a.dem")
                .unwrap()
                .cross_drive
        );
        let tight = super::plan(&req, &nothing_exists, 49).unwrap();
        assert!(!tight.enough_space);
    }

    #[test]
    fn no_destination_is_refused() {
        let req = Request {
            destination: "  ".into(),
            ..request(vec![], vec![])
        };
        assert!(plan(&req, &nothing_exists, 0).is_err());
    }

    #[test]
    fn drives_compare_by_letter_or_share() {
        assert_eq!(drive_of(&comparable(r"\\?\C:\a\b.dem")), "c:");
        assert_eq!(drive_of(&comparable(r"\\?\UNC\nas\d\x.dem")), r"\\nas\d");
        assert_eq!(drive_of(&comparable(r"\\nas\d\y\z.dem")), r"\\nas\d");
    }

    #[test]
    fn a_batch_moves_logs_recycles_and_undoes() {
        let dir = Scratch::new("consolidate_batch");
        let logs = dir.path().join("logs");
        let src = dir.path().join("src");
        std::fs::create_dir_all(src.join("sub")).unwrap();
        std::fs::write(src.join("sub").join("a.dem"), b"aaaa").unwrap();
        std::fs::write(src.join("b.dem"), b"bb").unwrap();
        std::fs::write(src.join("copy.dem"), b"bb").unwrap();
        let dest = dir.path().join("all");
        let path = |p: PathBuf| p.to_string_lossy().into_owned();
        let req = Request {
            destination: path(dest.clone()),
            layout: Layout::Subfolders,
            roots: vec![path(src.clone())],
            keep: vec![
                demo(&path(src.join("sub").join("a.dem")), 4),
                demo(&path(src.join("b.dem")), 2),
            ],
            recycle: vec![],
            include_game_folders: false,
        };
        let mut reports = Vec::new();
        let outcome = run(&req, &logs, &AtomicBool::new(false), &mut |p| {
            reports.push(p.clone())
        })
        .unwrap();
        assert_eq!(outcome.moved.len(), 2, "{outcome:?}");
        assert!(outcome.failed.is_empty());
        assert_eq!(
            std::fs::read(dest.join("sub").join("a.dem")).unwrap(),
            b"aaaa"
        );
        assert!(!src.join("b.dem").exists());
        let last = reports.last().unwrap();
        assert_eq!((last.files_done, last.bytes_done), (2, 6));

        let batch = undoable(&logs).unwrap();
        assert_eq!(batch.count, 2);
        let undone = undo_last(&logs, &AtomicBool::new(false), &mut |_| {});
        assert_eq!(undone.moved.len(), 2);
        assert!(src.join("sub").join("a.dem").exists());
        assert!(src.join("b.dem").exists());
        assert_eq!(undoable(&logs), None);
        assert_eq!(
            undo_last(&logs, &AtomicBool::new(false), &mut |_| {}),
            Outcome::default()
        );
    }

    #[test]
    fn a_copy_keeps_the_modified_time_and_never_replaces() {
        let dir = Scratch::new("consolidate_copy");
        let from = dir.path().join("a.dem");
        let to = dir.path().join("b.dem");
        std::fs::write(&from, vec![7u8; COPY_CHUNK + 10]).unwrap();
        let old = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_600_000_000);
        std::fs::File::options()
            .write(true)
            .open(&from)
            .unwrap()
            .set_modified(old)
            .unwrap();
        let mut bytes = 0;
        assert!(copy_file(&from, &to, &AtomicBool::new(false), &mut |n| bytes += n).is_ok());
        assert_eq!(bytes, COPY_CHUNK as u64 + 10);
        assert_eq!(std::fs::metadata(&to).unwrap().modified().unwrap(), old);
        // A second copy onto it is refused.
        assert!(copy_file(&from, &to, &AtomicBool::new(false), &mut |_| {}).is_err());
        // A cancelled copy leaves nothing behind.
        let c = dir.path().join("c.dem");
        assert!(matches!(
            copy_file(&from, &c, &AtomicBool::new(true), &mut |_| {}),
            Err(MoveError::Cancelled)
        ));
        assert!(!c.exists());
    }

    #[test]
    fn a_move_never_replaces_and_reports_a_missing_source() {
        let dir = Scratch::new("consolidate_refuse");
        let a = dir.path().join("a.dem");
        let b = dir.path().join("b.dem");
        std::fs::write(&a, b"a").unwrap();
        std::fs::write(&b, b"b").unwrap();
        let no = AtomicBool::new(false);
        assert!(matches!(
            move_one(&a, &b, &no, &mut |_| {}),
            Err(MoveError::Failed(_))
        ));
        assert_eq!(std::fs::read(&b).unwrap(), b"b");
        let gone = dir.path().join("gone.dem");
        assert!(matches!(
            move_one(&gone, &dir.path().join("x.dem"), &no, &mut |_| {}),
            Err(MoveError::Failed(_))
        ));
    }

    #[test]
    fn a_cancelled_undo_keeps_what_it_did_not_reach() {
        let dir = Scratch::new("consolidate_undo_cancel");
        let logs = dir.path().join("logs");
        let log = ConsolidateLog {
            created_unix_secs: 1,
            destination: "x".into(),
            moves: vec![MovedPair {
                from: "a".into(),
                to: "b".into(),
            }],
            undone: false,
        };
        let p = new_log_path(&logs, 1).unwrap();
        save_log(&p, &log);
        let outcome = undo_last(&logs, &AtomicBool::new(true), &mut |_| {});
        assert!(outcome.cancelled);
        assert_eq!(undoable(&logs).unwrap().count, 1);
    }
}
