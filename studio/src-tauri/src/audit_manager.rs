//! Demo Auditor › Duplicates: one key pass over several folders, or every
//! fixed drive, so a copy on one drive pairs with its original on another
//! (#844). Groups list the copy to keep first; deletes go to the Recycle Bin
//! unless permanent deletion is asked for.

use hl_demo_auditor::{AuditProgress, find_duplicates, scan_dir};
use serde::Serialize;
use std::collections::HashSet;
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
    /// Inside the configured install's game folder (`dod`).
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

#[derive(Serialize)]
pub struct AuditOutcome {
    pub groups: Vec<SerializedDuplicateGroup>,
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
            return Err(match missing.first() {
                Some(root) => crate::messages::audit_folder_not_found(root),
                None => crate::messages::NO_AUDIT_FOLDERS.to_string(),
            });
        }
        let keep_rules = KeepRules::new(&rules.0, &rules.1);

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

        let mut files = vec![];
        for root in roots.iter().filter(|r| r.is_dir()) {
            scan_dir(root, &mut files, &cancel_token, &Some(tx.clone()));
        }
        let (_, duplicate_groups, _, _) = find_duplicates(files, &cancel_token, &Some(tx));
        let last = progress_handle.join().unwrap_or_default();
        let cancelled = cancel_token.load(Ordering::Relaxed);

        let groups = if cancelled {
            Vec::new()
        } else {
            duplicate_groups
                .into_iter()
                .map(|g| SerializedDuplicateGroup {
                    size: g.key.size,
                    header_hash: g.key.header_hash,
                    files: order_for_keeping(g.files, &keep_rules),
                })
                .collect()
        };
        Ok(AuditOutcome {
            groups,
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

/// Deletes the picked copies: to the Recycle Bin, or for good when
/// `permanent`. Emits `audit_delete_progress` (`done`, `total`), throttled.
pub fn delete_audit_files_impl(
    app_handle: &AppHandle,
    paths: Vec<String>,
    permanent: bool,
) -> DeleteOutcome {
    let mut outcome = DeleteOutcome::default();
    let mut deletable = vec![];
    for path in paths {
        match check_deletable(Path::new(&path)) {
            Ok(()) => deletable.push(path),
            Err(error) => outcome.failed.push(DeleteFailure { path, error }),
        }
    }
    let total = deletable.len();
    let started = Instant::now();
    let last_emit = AtomicU32::new(u32::MAX);
    let mut report = |done: usize| {
        let now_ms = started.elapsed().as_millis().min(u32::MAX as u128 - 1) as u32;
        if done == total || crate::capture_manager::progress_emit_due(&last_emit, now_ms) {
            let _ = app_handle.emit(
                "audit_delete_progress",
                serde_json::json!({ "done": done, "total": total }),
            );
        }
    };

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
            None => outcome.deleted.push(path),
            Some(e) => outcome.failed.push(DeleteFailure {
                error: crate::messages::failed_to_delete_audit_file(Path::new(&path), e),
                path,
            }),
        }
    }
    outcome
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

    fn paths(list: &[&str]) -> Vec<PathBuf> {
        list.iter().map(PathBuf::from).collect()
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
    fn every_fixed_drive_root_is_a_folder() {
        for root in fixed_drive_roots() {
            assert!(Path::new(&root).is_dir(), "{root}");
        }
    }
}
