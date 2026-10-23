//! Demo Auditor › Duplicates: one key pass over several folders, or every
//! fixed drive, so a copy on one drive pairs with its original on another
//! (#844). Groups list the copy to keep first; deletes go to the Recycle Bin
//! unless permanent deletion is asked for. A group is keyed by size and the
//! first 64 KiB only, so each copy is compared byte for byte with the kept
//! one before it is deleted.
//!
//! Each demo's key is remembered between scans (`audit_keys.json`, #888), so
//! a second scan costs the folder walk and opens only new or changed demos.
//! A copy the delete finds different has its whole-file hash remembered
//! there too, and later scans no longer group it with the kept copy.

use hl_demo_auditor::{AuditProgress, KeyCache, find_duplicates_cached, scan_roots};
use native::demo_compare::{Refusal, check_same_contents};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Instant;
use tauri::{AppHandle, Emitter};

pub struct AuditManager {
    pub is_running: Arc<AtomicBool>,
    pub cancel_token: Arc<AtomicBool>,
}

impl Default for AuditManager {
    fn default() -> Self {
        Self {
            is_running: Arc::new(AtomicBool::new(false)),
            cancel_token: Arc::new(AtomicBool::new(false)),
        }
    }
}

/// Why a copy should stay: the game or the open project reads it (#844).
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InUse {
    /// Inside an install's game folder (`dod`): the configured one, or any
    /// other `dod` beside an `hl.exe`.
    GameFolder,
    /// In the current project's queue.
    Queue,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct AuditFile {
    pub path: String,
    pub in_use: Option<InUse>,
}

#[derive(Serialize)]
pub struct SerializedDuplicateGroup {
    pub size: u64,
    pub header_hash: u64,
    /// The copy to keep first ([`order_for_keeping`]).
    pub files: Vec<AuditFile>,
}

/// A demo with no copies: Consolidate moves these too (#872).
#[derive(Serialize, Debug, PartialEq)]
pub struct SingleDemo {
    pub path: String,
    pub size: u64,
    pub in_use: Option<InUse>,
}

#[derive(Serialize)]
pub struct AuditOutcome {
    pub groups: Vec<SerializedDuplicateGroup>,
    /// Every demo without copies, by path.
    pub singles: Vec<SingleDemo>,
    /// The folders walked, drives included, for Consolidate's
    /// keep-subfolders layout.
    pub roots: Vec<String>,
    /// Demos keyed.
    pub demos: usize,
    /// Cancelled before the key pass ended: `groups` is empty, since a
    /// partial pass would list originals whose copies it never reached.
    pub cancelled: bool,
}

/// Every fixed drive's root (`C:\` and so on), sorted. Removable, network
/// and optical drives are left out: "All drives" means the PC's own disks.
pub fn fixed_drive_roots() -> Vec<String> {
    let disks = sysinfo::Disks::new_with_refreshed_list();
    let mut roots: Vec<String> = disks
        .list()
        .iter()
        .filter(|d| !d.is_removable())
        .map(|d| d.mount_point().to_string_lossy().into_owned())
        .collect();
    roots.sort();
    roots.dedup();
    roots
}

/// A path as text for comparing: no `\\?\` prefix, backslashes, lower case,
/// no trailing separator. Windows paths are case-insensitive, and the key
/// pass hands back canonical (`\\?\`) paths while the queue holds plain ones.
fn comparable(path: &Path) -> String {
    let text = path.to_string_lossy().replace('/', "\\");
    let text = match text.strip_prefix(r"\\?\UNC\") {
        Some(rest) => format!(r"\\{rest}"),
        None => text.strip_prefix(r"\\?\").unwrap_or(&text).to_string(),
    };
    text.trim_end_matches('\\').to_lowercase()
}

/// The canonical form when the path resolves, else the path as given.
fn resolved(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// The folders and files whose copies an audit should keep.
#[derive(Default)]
pub struct KeepRules {
    game_folders: Vec<String>,
    queued: HashSet<String>,
}

impl KeepRules {
    /// Resolves each path, so a queued `C:\demos\a.dem` matches the key
    /// pass's `\\?\C:\demos\a.dem` and a folder reached through a link
    /// matches its target.
    pub fn new(game_folders: &[PathBuf], queued: &[String]) -> Self {
        Self {
            game_folders: game_folders
                .iter()
                .map(|f| comparable(&resolved(f)))
                .collect(),
            queued: queued
                .iter()
                .map(|q| comparable(&resolved(Path::new(q))))
                .collect(),
        }
    }

    fn in_use(&self, file: &str) -> Option<InUse> {
        let in_folder = |folder: &String| {
            file.strip_prefix(folder.as_str())
                .is_some_and(|rest| rest.starts_with('\\'))
        };
        if self.game_folders.iter().any(in_folder) {
            Some(InUse::GameFolder)
        } else if self.queued.contains(file) {
            Some(InUse::Queue)
        } else {
            None
        }
    }
}

/// The game folders (`<install>\dod`, beside an `hl.exe`) that any of
/// `files` sits in, each checked on disk once.
pub fn install_game_folders<'a>(files: impl Iterator<Item = &'a PathBuf>) -> Vec<PathBuf> {
    let mut checked: HashMap<PathBuf, bool> = HashMap::new();
    let mut found = Vec::new();
    for file in files {
        for dir in file.ancestors().skip(1) {
            let is_dod = dir
                .file_name()
                .is_some_and(|n| n.eq_ignore_ascii_case(GAME_FOLDER_NAME));
            if !is_dod {
                continue;
            }
            let hit = *checked.entry(dir.to_path_buf()).or_insert_with(|| {
                let yes = dir.parent().is_some_and(|p| p.join("hl.exe").is_file());
                if yes {
                    found.push(dir.to_path_buf());
                }
                yes
            });
            if hit {
                break;
            }
        }
    }
    found
}

/// The game's own folder name in every install DoD Studio supports.
const GAME_FOLDER_NAME: &str = "dod";

/// One group's files, the copy to keep first: one the game folder or the
/// queue uses, then the shortest path, then by name so the pick is stable.
pub fn order_for_keeping(files: Vec<PathBuf>, rules: &KeepRules) -> Vec<AuditFile> {
    let mut ranked: Vec<(bool, usize, String, AuditFile)> = files
        .into_iter()
        .map(|path| {
            let shown = comparable(&path);
            let in_use = rules.in_use(&shown);
            let file = AuditFile {
                path: path.to_string_lossy().into_owned(),
                in_use,
            };
            (in_use.is_none(), shown.len(), shown, file)
        })
        .collect();
    ranked.sort_by(|a, b| (a.0, a.1, &a.2).cmp(&(b.0, b.1, &b.2)));
    ranked.into_iter().map(|(_, _, _, file)| file).collect()
}

/// The demos `find_duplicates` listed once that are in no group, with their
/// sizes. A demo it couldn't read is left out: there is nothing to move.
pub fn singles_of(
    unique: Vec<PathBuf>,
    groups: &[hl_demo_auditor::DuplicateGroup],
) -> Vec<(PathBuf, u64)> {
    let grouped: HashSet<&PathBuf> = groups.iter().flat_map(|g| g.files.iter()).collect();
    unique
        .into_iter()
        .filter(|p| !grouped.contains(p))
        .filter_map(|p| {
            let size = std::fs::metadata(&p).ok()?.len();
            Some((p, size))
        })
        .collect()
}

/// The folders to walk: the ones picked, plus every fixed drive when asked,
/// without blanks or repeats (case-insensitive).
pub fn audit_roots(folders: &[String], drives: &[String]) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    folders
        .iter()
        .chain(drives)
        .map(|f| f.trim())
        .filter(|f| !f.is_empty())
        .filter(|f| seen.insert(comparable(Path::new(f))))
        .map(PathBuf::from)
        .collect()
}

/// What `audit_progress` carries; the frontend words the line
/// (`STRINGS.AUDITOR`). `total` is 0 while folders are still being listed.
#[derive(Serialize, Clone, Default)]
struct AuditProgressEvent {
    folders: usize,
    total: usize,
    done: usize,
    current: String,
}

/// Where the audit remembers each demo's key between scans.
fn key_cache_file() -> PathBuf {
    native::shared::paths::get_appdata_dir().join("audit_keys.json")
}

/// What one audit did, for its end line in the activity log.
struct AuditTimes {
    cancelled: bool,
    walk_secs: f64,
    key_secs: f64,
    demos: usize,
    groups: usize,
    copies: usize,
    wasted_bytes: u64,
    reads: hl_demo_auditor::Reads,
}

/// The activity log's line when an audit ends: how long the folder walk and
/// the key pass took, what it found, and how many files it had to open (the
/// rest were remembered from an earlier scan, #888).
fn audit_end_log(t: &AuditTimes) -> String {
    let total = t.walk_secs + t.key_secs;
    if t.cancelled {
        return format!("[audit] cancelled after {total:.1} s");
    }
    format!(
        "[audit] finished after {total:.1} s (walk {:.1} s, keys {:.1} s): {} demo(s) checked, \
         {} group(s) with copies, {} copies, {:.2} GB; opened {} start(s), {} end(s), {} whole file(s), \
         the rest remembered",
        t.walk_secs,
        t.key_secs,
        t.demos,
        t.groups,
        t.copies,
        t.wasted_bytes as f64 / (1024.0 * 1024.0 * 1024.0),
        t.reads.heads,
        t.reads.tails,
        t.reads.fulls,
    )
}

/// The activity log's line when an audit starts: what it walks.
fn audit_start_log(folders: &[String], drives: &[String]) -> String {
    let mut what: Vec<String> = Vec::new();
    if !drives.is_empty() {
        what.push(format!("all drives ({})", drives.join(" ")));
    }
    if !folders.is_empty() {
        what.push(format!("{} folder(s)", folders.len()));
    }
    format!("[audit] started: {}", what.join(" and "))
}

/// Runs the audit off the main thread: lists every root's demos, keys them
/// all in one pass and orders each group for keeping.
pub async fn run_demo_audit_impl(
    app_handle: AppHandle,
    is_running: Arc<AtomicBool>,
    cancel_token: Arc<AtomicBool>,
    folders: Vec<String>,
    all_drives: bool,
    rules: (Vec<PathBuf>, Vec<String>),
) -> Result<AuditOutcome, String> {
    is_running.store(true, Ordering::SeqCst);
    cancel_token.store(false, Ordering::SeqCst);
    let is_running_end = Arc::clone(&is_running);

    let result = crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        let drives = if all_drives {
            fixed_drive_roots()
        } else {
            Vec::new()
        };
        let roots = audit_roots(&folders, &drives);
        let missing: Vec<&PathBuf> = roots.iter().filter(|r| !r.is_dir()).collect();
        if roots.is_empty() || missing.len() == roots.len() {
            let why = match missing.first() {
                Some(root) => crate::messages::audit_folder_not_found(root),
                None => crate::messages::NO_AUDIT_FOLDERS.to_string(),
            };
            native::log_markdown(&format!("[audit] failed: {why}"));
            return Err(why);
        }
        native::log_markdown(&audit_start_log(&folders, &drives));
        let (tx, rx) = mpsc::channel();
        let ah = app_handle.clone();
        // At most one event per ~33 ms, the first at once (CLAUDE.md).
        let progress_handle = std::thread::spawn(move || {
            let started = Instant::now();
            let last_emit = AtomicU32::new(u32::MAX);
            let mut event = AuditProgressEvent::default();
            while let Ok(msg) = rx.recv() {
                match msg {
                    AuditProgress::Scanning(p) => {
                        event.folders += 1;
                        event.current = p;
                    }
                    AuditProgress::Keying(total) => event.total = total,
                    AuditProgress::Hashing(p) => {
                        event.done += 1;
                        event.current = p;
                    }
                    _ => continue,
                }
                let now_ms = started.elapsed().as_millis().min(u32::MAX as u128 - 1) as u32;
                if crate::capture_manager::progress_emit_due(&last_emit, now_ms) {
                    let _ = ah.emit("audit_progress", event.clone());
                }
            }
            event
        });

        let walked: Vec<PathBuf> = roots.into_iter().filter(|r| r.is_dir()).collect();
        let walk_started = Instant::now();
        let files = scan_roots(&walked, &cancel_token, &Some(tx.clone()));
        let walk_secs = walk_started.elapsed().as_secs_f64();
        let demos = files.len();
        let key_started = Instant::now();
        let cache_file = key_cache_file();
        let mut cache = KeyCache::load(&cache_file);
        let audit = find_duplicates_cached(files, &cancel_token, &Some(tx), &mut cache);
        let key_secs = key_started.elapsed().as_secs_f64();
        let last = progress_handle.join().unwrap_or_default();
        let cancelled = cancel_token.load(Ordering::Relaxed);
        native::log_markdown(&audit_end_log(&AuditTimes {
            cancelled,
            walk_secs,
            key_secs,
            demos,
            groups: audit.groups.len(),
            copies: audit.copies,
            wasted_bytes: audit.wasted_bytes,
            reads: audit.reads,
        }));
        let duplicate_groups = audit.groups;
        let unique = audit.unique_files;
        if !cancelled {
            cache.prune_unseen_under(&walked);
        }
        // A cancelled pass still saves the keys it read. Only a cache: a
        // failed save costs the next scan time, nothing else.
        let _ = cache.save(&cache_file);

        let (groups, singles) = if cancelled {
            (Vec::new(), Vec::new())
        } else {
            let singles = singles_of(unique, &duplicate_groups);
            // Every install the demos sit in counts, not only the one the
            // hl.exe setting names: a PC with a stock, a PRE and a POST
            // install keeps each one's demos.
            let mut game_folders = rules.0.clone();
            game_folders.extend(install_game_folders(
                duplicate_groups
                    .iter()
                    .flat_map(|g| g.files.iter())
                    .chain(singles.iter().map(|(p, _)| p)),
            ));
            let keep_rules = KeepRules::new(&game_folders, &rules.1);
            let singles = singles
                .into_iter()
                .map(|(path, size)| {
                    let in_use = keep_rules.in_use(&comparable(&path));
                    SingleDemo {
                        path: path.to_string_lossy().into_owned(),
                        size,
                        in_use,
                    }
                })
                .collect();
            let groups = duplicate_groups
                .into_iter()
                .map(|g| SerializedDuplicateGroup {
                    size: g.key.size,
                    header_hash: g.key.header_hash,
                    files: order_for_keeping(g.files, &keep_rules),
                })
                .collect();
            (groups, singles)
        };
        Ok(AuditOutcome {
            groups,
            singles,
            roots: walked
                .iter()
                .map(|r| r.to_string_lossy().into_owned())
                .collect(),
            demos: last.done,
            cancelled,
        })
    }))
    .await;

    is_running_end.store(false, Ordering::SeqCst);
    result
}

/// One file the delete could not remove, and why.
#[derive(Serialize, Debug, PartialEq)]
pub struct DeleteFailure {
    pub path: String,
    pub error: String,
    /// It differs from the copy its group keeps: not a duplicate, so the
    /// table marks it rather than leaving it ticked.
    pub not_a_copy: bool,
}

/// A refused copy as the table reports it.
fn failure_of(refusal: Refusal, path: String) -> DeleteFailure {
    let (error, not_a_copy) = refusal.into_parts();
    DeleteFailure {
        path,
        error,
        not_a_copy,
    }
}

#[derive(Serialize, Debug, Default)]
pub struct DeleteOutcome {
    pub deleted: Vec<String>,
    pub failed: Vec<DeleteFailure>,
}

/// Only a demo file is ever deleted: the paths come from the frontend.
fn check_deletable(path: &Path) -> Result<(), String> {
    let is_demo = path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("dem"));
    let is_file = path.symlink_metadata().is_ok_and(|m| m.is_file());
    if is_demo && is_file {
        Ok(())
    } else {
        Err(crate::messages::not_a_demo_file(path))
    }
}

/// One copy to delete, and the copy its group keeps.
#[derive(Deserialize, Debug, Clone)]
pub struct DeleteCopy {
    pub path: String,
    pub keep: String,
}

/// Deletes the picked copies: to the Recycle Bin, or for good when
/// `permanent`. Each is first compared byte for byte with the copy its group
/// keeps, and one that differs is not deleted. Emits `audit_delete_progress`
/// (`step` "checking" then "deleting", `done`, `total`), throttled.
pub fn delete_audit_files_impl(
    app_handle: &AppHandle,
    copies: Vec<DeleteCopy>,
    permanent: bool,
) -> DeleteOutcome {
    let mut outcome = DeleteOutcome::default();
    let started = Instant::now();
    let last_emit = AtomicU32::new(u32::MAX);
    let emit = |step: &str, done: usize, total: usize| {
        let now_ms = started.elapsed().as_millis().min(u32::MAX as u128 - 1) as u32;
        if done == total || crate::capture_manager::progress_emit_due(&last_emit, now_ms) {
            let _ = app_handle.emit(
                "audit_delete_progress",
                serde_json::json!({ "step": step, "done": done, "total": total }),
            );
        }
    };
    let checking = copies.len();
    let cache_file = key_cache_file();
    let mut cache = KeyCache::load(&cache_file);
    let mut deletable = vec![];
    for (i, copy) in copies.into_iter().enumerate() {
        let path = Path::new(&copy.path);
        match check_deletable(path)
            .map_err(Refusal::Other)
            .and_then(|()| check_same_contents(path, Path::new(&copy.keep)))
        {
            Ok(()) => deletable.push(copy.path),
            Err(refusal) => {
                if matches!(refusal, Refusal::NotACopy(_)) {
                    remember_differing(&mut cache, path, Path::new(&copy.keep));
                }
                outcome.failed.push(failure_of(refusal, copy.path))
            }
        }
        emit("checking", i + 1, checking);
    }
    let total = deletable.len();
    let mut report = |done: usize| emit("deleting", done, total);

    let errors: Vec<Option<String>> = if permanent {
        let mut errors = Vec::with_capacity(total);
        for (i, path) in deletable.iter().enumerate() {
            errors.push(std::fs::remove_file(path).err().map(|e| e.to_string()));
            report(i + 1);
        }
        errors
    } else {
        let list = deletable.iter().map(PathBuf::from).collect();
        native::sys::recycle_bin::recycle(list, &mut report)
    };
    for (path, error) in deletable.into_iter().zip(errors) {
        match error {
            None => {
                cache.forget(Path::new(&path));
                outcome.deleted.push(path)
            }
            Some(e) => outcome.failed.push(DeleteFailure {
                error: crate::messages::failed_to_delete_audit_file(Path::new(&path), e),
                path,
                not_a_copy: false,
            }),
        }
    }
    let _ = cache.save(&cache_file);
    outcome
}

/// After the check found `copy` and `keep` differ: both files' whole-file
/// hashes into `cache`, so the next scan no longer groups them (#888). The
/// check stops at the first difference, so this reads both again, but only
/// for a copy that turned out not to be one; the common case, a true copy,
/// costs nothing extra. Files of two sizes are never grouped, so they are
/// not read; a file that changes while it is read is not remembered.
fn remember_differing(cache: &mut KeyCache, copy: &Path, keep: &Path) {
    use hl_demo_auditor::{content_hashes, stamp};
    let sizes = (stamp(copy).map(|s| s.size), stamp(keep).map(|s| s.size));
    if sizes.0.is_none() || sizes.0 != sizes.1 {
        return;
    }
    for path in [copy, keep] {
        let Ok(canonical) = std::fs::canonicalize(path) else {
            continue;
        };
        if let (Some(before), Ok(hashes)) = (stamp(&canonical), content_hashes(&canonical))
            && stamp(&canonical) == Some(before)
        {
            cache.remember_contents(&canonical, before, hashes);
        }
    }
}

/// Reveals a file in the OS file explorer, selecting it if the platform
/// supports that. Mirrors dev's `views/auditor.rs` cross-platform match.
pub fn reveal_in_explorer_impl(path: String) -> Result<(), String> {
    let path_buf = PathBuf::from(&path);

    if !path_buf.exists() {
        return Err(crate::messages::path_no_longer_exists(&path));
    }

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg("/select,")
            .arg(&path_buf)
            .spawn()
            .map_err(crate::messages::failed_to_open_explorer)?;
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("-R")
            .arg(&path_buf)
            .spawn()
            .map_err(crate::messages::failed_to_open_finder)?;
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        // Linux fallback: no universal "select this file" behavior, so just
        // open the parent directory via the desktop's default file manager.
        let parent = path_buf
            .parent()
            .ok_or(crate::messages::NO_PARENT_DIRECTORY_FOR_PATH)?;
        std::process::Command::new("xdg-open")
            .arg(parent)
            .spawn()
            .map_err(crate::messages::failed_to_open_folder)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_audit_log_lines_say_what_ran_how_long_and_what_was_read() {
        assert_eq!(
            audit_start_log(&[], &["C:/".into(), "D:/".into()]),
            "[audit] started: all drives (C:/ D:/)"
        );
        assert_eq!(
            audit_start_log(&["E:/demos".into()], &[]),
            "[audit] started: 1 folder(s)"
        );
        let mut times = AuditTimes {
            cancelled: false,
            walk_secs: 5.04,
            key_secs: 1.55,
            demos: 3217,
            groups: 1007,
            copies: 1909,
            wasted_bytes: 129 * 1024 * 1024 * 1024,
            reads: hl_demo_auditor::Reads {
                heads: 2930,
                tails: 2919,
                fulls: 0,
            },
        };
        assert_eq!(
            audit_end_log(&times),
            "[audit] finished after 6.6 s (walk 5.0 s, keys 1.6 s): 3217 demo(s) checked, \
             1007 group(s) with copies, 1909 copies, 129.00 GB; opened 2930 start(s), 2919 end(s), \
             0 whole file(s), the rest remembered"
        );
        times.cancelled = true;
        assert_eq!(audit_end_log(&times), "[audit] cancelled after 6.6 s");
    }

    fn paths(list: &[&str]) -> Vec<PathBuf> {
        list.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn a_copy_with_other_bytes_is_marked_not_a_copy() {
        let failure = failure_of(Refusal::NotACopy("differs".into()), "copy.dem".into());
        assert!(failure.not_a_copy);
        assert!(!failure_of(Refusal::Other("gone".into()), "x".into()).not_a_copy);
    }

    /// Same size and first 64 KiB, so the audit groups them, but not the
    /// same demo past that (native::demo_compare's test helper is private).
    const COMPARE_CHUNK: usize = 1 << 20;
    fn demo_bytes(tail: u8) -> Vec<u8> {
        let mut bytes = vec![7u8; COMPARE_CHUNK + 70_000];
        *bytes.last_mut().unwrap() = tail;
        bytes
    }
    fn scratch_file(s: &crate::test_support::Scratch, name: &str, bytes: &[u8]) -> PathBuf {
        let path = s.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    /// A copy found different is remembered, and the next scan lists it
    /// apart from the copy it was grouped with (#888).
    #[test]
    fn a_copy_found_different_is_not_grouped_again() {
        let s = crate::test_support::Scratch::new("audit_remember");
        let keep = scratch_file(&s, "keep.dem", &demo_bytes(1));
        let copy = scratch_file(&s, "copy.dem", &demo_bytes(1));
        // Same first and last bytes as keep: only the middle differs.
        let mut middle = demo_bytes(1);
        middle[COMPARE_CHUNK / 2] = 9;
        let odd = scratch_file(&s, "odd.dem", &middle);
        let files = vec![keep.clone(), copy.clone(), odd.clone()];
        let never = AtomicBool::new(false);
        let mut cache = KeyCache::default();
        let before = find_duplicates_cached(files.clone(), &never, &None, &mut cache);
        assert_eq!(before.groups.len(), 1);
        assert_eq!(before.groups[0].files.len(), 3, "the ends match");

        assert!(matches!(
            check_same_contents(&odd, &keep),
            Err(Refusal::NotACopy(_))
        ));
        remember_differing(&mut cache, &odd, &keep);
        let after = find_duplicates_cached(files, &never, &None, &mut cache);
        assert_eq!(after.groups.len(), 1);
        let names: Vec<_> = after.groups[0]
            .files
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["copy.dem", "keep.dem"]);

        // Files of two sizes are never grouped: nothing to remember.
        let short = scratch_file(&s, "short.dem", b"abc");
        let mut untouched = KeyCache::default();
        remember_differing(&mut untouched, &short, &keep);
        assert!(!untouched.has_changes());
    }

    fn kept_order(files: &[&str], rules: &KeepRules) -> Vec<String> {
        order_for_keeping(paths(files), rules)
            .into_iter()
            .map(|f| f.path)
            .collect()
    }

    #[test]
    fn the_shortest_path_is_kept_when_nothing_uses_a_copy() {
        let rules = KeepRules::default();
        assert_eq!(
            kept_order(
                &[
                    r"F:\downloads\old\match.dem",
                    r"E:\match.dem",
                    r"D:\match.dem"
                ],
                &rules
            ),
            [
                r"D:\match.dem",
                r"E:\match.dem",
                r"F:\downloads\old\match.dem"
            ]
        );
    }

    #[test]
    fn a_copy_in_the_game_folder_is_kept_over_a_shorter_one() {
        let rules = KeepRules::new(&paths(&[r"Z:\no-such-install\Half-Life\dod"]), &[]);
        let files = order_for_keeping(
            paths(&[
                r"F:\m.dem",
                r"\\?\Z:\No-Such-Install\Half-Life\DOD\demos\m.dem",
            ]),
            &rules,
        );
        assert_eq!(
            files[0],
            AuditFile {
                path: r"\\?\Z:\No-Such-Install\Half-Life\DOD\demos\m.dem".into(),
                in_use: Some(InUse::GameFolder),
            }
        );
        assert_eq!(files[1].in_use, None);
    }

    #[test]
    fn every_install_the_copies_sit_in_is_a_game_folder() {
        let root =
            std::env::temp_dir().join(format!("dodstudio-audit-installs-{}", std::process::id()));
        let post = root.join("POST");
        let loose = root.join("loose");
        std::fs::create_dir_all(post.join("dod").join("sub")).unwrap();
        std::fs::create_dir_all(loose.join("dod")).unwrap();
        std::fs::write(post.join("hl.exe"), b"").unwrap();
        let files = [
            post.join("dod").join("sub").join("a.dem"),
            post.join("dod").join("b.dem"),
            // A folder named dod with no hl.exe beside it is not an install.
            loose.join("dod").join("c.dem"),
        ];
        let found = install_game_folders(files.iter());
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(found, vec![post.join("dod")]);
    }

    #[test]
    fn a_folder_that_only_starts_the_same_is_not_the_game_folder() {
        let rules = KeepRules::new(&paths(&[r"Z:\hl\dod"]), &[]);
        let files = order_for_keeping(paths(&[r"Z:\hl\dod_downloads\m.dem"]), &rules);
        assert_eq!(files[0].in_use, None);
    }

    #[test]
    fn a_queued_copy_is_kept_and_matches_its_canonical_form() {
        let rules = KeepRules::new(&[], &[r"Z:\projects\wsod\m.dem".to_string()]);
        let files = order_for_keeping(
            paths(&[r"\\?\A:\m.dem", r"\\?\Z:\Projects\WSOD\m.dem"]),
            &rules,
        );
        assert_eq!(files[0].path, r"\\?\Z:\Projects\WSOD\m.dem");
        assert_eq!(files[0].in_use, Some(InUse::Queue));
    }

    #[test]
    fn copies_both_in_use_fall_back_to_the_shortest_path() {
        let rules = KeepRules::new(&paths(&[r"Z:\hl\dod"]), &[r"Y:\q\m.dem".to_string()]);
        let files = order_for_keeping(paths(&[r"Z:\hl\dod\m.dem", r"Y:\q\m.dem"]), &rules);
        assert_eq!(files[0].path, r"Y:\q\m.dem");
        assert_eq!(files[1].in_use, Some(InUse::GameFolder));
    }

    #[test]
    fn roots_drop_blanks_and_repeats_and_add_the_drives() {
        let folders = [
            r"C:\Demos".to_string(),
            " ".to_string(),
            r"c:\demos\".to_string(),
        ];
        let drives = [r"C:\".to_string(), r"F:\".to_string()];
        assert_eq!(
            audit_roots(&folders, &drives),
            paths(&[r"C:\Demos", r"C:\", r"F:\"])
        );
    }

    #[test]
    fn comparable_strips_the_verbatim_prefix_and_case() {
        assert_eq!(comparable(Path::new(r"\\?\C:\A\b.dem")), r"c:\a\b.dem");
        assert_eq!(
            comparable(Path::new(r"\\?\UNC\nas\d\x.dem")),
            r"\\nas\d\x.dem"
        );
        assert_eq!(comparable(Path::new("C:/a/")), r"c:\a");
    }

    #[test]
    fn only_demo_files_are_deletable() {
        let dir = crate::test_support::Scratch::new("audit_deletable");
        let demo = dir.path().join("a.dem");
        let other = dir.path().join("a.txt");
        std::fs::write(&demo, b"x").unwrap();
        std::fs::write(&other, b"x").unwrap();
        assert!(check_deletable(&demo).is_ok());
        assert!(check_deletable(&other).is_err());
        assert!(check_deletable(dir.path()).is_err());
        let err = check_deletable(&dir.path().join("gone.dem")).unwrap_err();
        assert!(
            err.contains(&dir.path().join("gone.dem").display().to_string()),
            "{err}"
        );
    }

    #[test]
    fn singles_are_the_demos_in_no_group_with_their_sizes() {
        let dir = crate::test_support::Scratch::new("audit_singles");
        let one = dir.path().join("one.dem");
        let a = dir.path().join("a.dem");
        let b = dir.path().join("b.dem");
        std::fs::write(&one, b"12345").unwrap();
        std::fs::write(&a, b"x").unwrap();
        std::fs::write(&b, b"x").unwrap();
        let groups = [hl_demo_auditor::DuplicateGroup {
            key: hl_demo_auditor::FileKey {
                size: 1,
                header_hash: 0,
            },
            files: vec![a.clone(), b],
        }];
        let gone = dir.path().join("gone.dem");
        assert_eq!(singles_of(vec![a, one.clone(), gone], &groups), [(one, 5)]);
    }

    #[test]
    fn every_fixed_drive_root_is_a_folder() {
        for root in fixed_drive_roots() {
            assert!(Path::new(&root).is_dir(), "{root}");
        }
    }
}
