//! Finding the demos under the folders the user picked: the walk, the
//! folders it never enters (system folders, `.git`, `node_modules`, Windows),
//! and path forms for comparing and showing.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Condvar, Mutex, OnceLock, PoisonError};
use std::time::Duration;

use crate::lanes::{Readers, drive_of, plan_lanes};
use crate::{AuditProgress, archive};

/// Recursively scans a directory for `.dem` files with cancellation and progress reporting.
pub fn scan_dir(
    dir: &Path,
    files: &mut Vec<PathBuf>,
    cancel: &AtomicBool,
    progress: &Option<Sender<AuditProgress>>,
) {
    scan_dir_to_depth(dir, files, cancel, progress, 0);
}

/// Folder levels [`scan_dir`] reads below the folder it is given.
const SCAN_MAX_DEPTH: usize = 32;

/// Folders [`scan_dir`] never enters, by name at any depth: a demo in the
/// Recycle Bin is a deleted copy, not a duplicate to clean up (#843), System
/// Volume Information is usually unreadable, and `node_modules`, `.git`,
/// `__pycache__` and `site-packages` (Python's own) are tens of thousands of
/// folders that never hold a demo (#888). The same list as
/// `native::sys::folder_walk::SYSTEM_FOLDERS`, which can't be used here
/// (`native` depends on this crate); a test there checks they agree.
pub const SYSTEM_FOLDERS: [&str; 6] = [
    "$RECYCLE.BIN",
    "System Volume Information",
    "node_modules",
    ".git",
    "__pycache__",
    "site-packages",
];

fn is_system_folder(path: &Path) -> bool {
    path.file_name().is_some_and(|name| {
        let name = name.to_string_lossy();
        SYSTEM_FOLDERS.iter().any(|s| name.eq_ignore_ascii_case(s))
    })
}

/// A path as text for comparing: no `\\?\` prefix, backslashes, lower case,
/// no trailing separator.
pub fn comparable_path(path: &Path) -> String {
    let text = path.to_string_lossy().replace('/', "\\");
    let text = match text.strip_prefix(r"\\?\UNC\") {
        Some(rest) => format!(r"\\{rest}"),
        None => text.strip_prefix(r"\\?\").unwrap_or(&text).to_string(),
    };
    text.trim_end_matches('\\').to_lowercase()
}

/// A path for a message: a canonical path's `\\?\` prefix dropped
/// (`\\?\UNC\nas\d` is `\\nas\d`), as `audit_duplicates.js`'s `shownPath`.
pub fn shown_path(path: &Path) -> String {
    let text = path.display().to_string();
    match text.strip_prefix(r"\\?\UNC\") {
        Some(rest) => format!(r"\\{rest}"),
        None => text.strip_prefix(r"\\?\").unwrap_or(&text).to_string(),
    }
}

/// The Windows folder as [`comparable_path`] writes it, from `%SystemRoot%`
/// (what `GetWindowsDirectoryW` reports), so a PC that installed Windows
/// somewhere other than `C:\Windows` is still matched. `None` off Windows.
pub fn windows_folder() -> Option<&'static str> {
    static FOLDER: OnceLock<Option<String>> = OnceLock::new();
    FOLDER
        .get_or_init(|| {
            std::env::var_os("SystemRoot")
                .or_else(|| std::env::var_os("windir"))
                .map(|dir| comparable_path(Path::new(&dir)))
                .filter(|dir| !dir.is_empty())
        })
        .as_deref()
}

/// Whether `path` is the folder `windows` names ([`windows_folder`]'s form).
/// Only a folder found inside a walked one is ever checked, so a walk from a
/// drive root skips the Windows folder (about 5 s of C:'s 20 s, and no demos,
/// #888), while a folder the user picks inside it is still read.
pub fn is_windows_folder_in(path: &Path, windows: Option<&str>) -> bool {
    let Some(windows) = windows else {
        return false;
    };
    let last = windows.rsplit('\\').next().unwrap_or(windows);
    path.file_name()
        .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case(last))
        && comparable_path(path) == windows
}

/// [`is_windows_folder_in`] this PC's Windows folder.
pub fn is_windows_folder(path: &Path) -> bool {
    is_windows_folder_in(path, windows_folder())
}

/// A folder found inside a walked one that the walk leaves out.
fn skips_folder(path: &Path, windows: Option<&str>) -> bool {
    is_system_folder(path) || is_windows_folder_in(path, windows)
}

/// `file_type` never follows a junction or symlink, so a link is never
/// entered; with the depth cap, no folder layout can make this loop (#376).
fn scan_dir_to_depth(
    dir: &Path,
    files: &mut Vec<PathBuf>,
    cancel: &AtomicBool,
    progress: &Option<Sender<AuditProgress>>,
    depth: usize,
) {
    if cancel.load(Ordering::Relaxed) {
        return;
    }
    let mut subdirs = Vec::new();
    list_dir(
        dir,
        depth,
        windows_folder(),
        cancel,
        progress,
        &mut subdirs,
        files,
        None,
    );
    for (sub, sub_depth) in subdirs {
        scan_dir_to_depth(&sub, files, cancel, progress, sub_depth);
    }
}

fn has_extension(path: &Path, wanted: &str) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case(wanted))
}

/// One folder's listing: its demos into `demos`, its archives (`.zip`,
/// `.7z`, `.rar`) into `zips` when given, the folders to walk next into `subdirs`.
#[allow(clippy::too_many_arguments)]
fn list_dir(
    dir: &Path,
    depth: usize,
    windows: Option<&str>,
    cancel: &AtomicBool,
    progress: &Option<Sender<AuditProgress>>,
    subdirs: &mut Vec<(PathBuf, usize)>,
    demos: &mut Vec<PathBuf>,
    mut zips: Option<&mut Vec<PathBuf>>,
) {
    if let Some(tx) = progress {
        let _ = tx.send(AuditProgress::Scanning(dir.to_string_lossy().into_owned()));
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let path = entry.path();
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            if depth < SCAN_MAX_DEPTH && !skips_folder(&path, windows) {
                subdirs.push((path, depth + 1));
            }
        } else if has_extension(&path, "dem") {
            demos.push(path);
        } else if let Some(zips) = zips.as_deref_mut()
            && archive::is_archive(&path)
        {
            zips.push(path);
        }
    }
}

/// [`scan_dir`] over several roots at once, several folders listed at a
/// time (#888): every drive's walk runs alongside the others, and a big SSD
/// (C:) is shared out between the threads, while a hard disk is listed one
/// folder at a time (#941). Finds the same demos as calling [`scan_dir`] on
/// each root, in no particular order.
pub fn scan_roots(
    roots: &[PathBuf],
    cancel: &AtomicBool,
    progress: &Option<Sender<AuditProgress>>,
) -> Vec<PathBuf> {
    scan_roots_with(
        roots,
        &Readers::system(),
        windows_folder(),
        cancel,
        progress,
    )
}

/// [`scan_roots`] that also lists every archive it passes (`.zip`, `.7z`,
/// `.rar`, #929): the demos, then the archives.
pub fn scan_roots_and_zips(
    roots: &[PathBuf],
    cancel: &AtomicBool,
    progress: &Option<Sender<AuditProgress>>,
) -> (Vec<PathBuf>, Vec<PathBuf>) {
    walk_roots(
        roots,
        &Readers::system(),
        windows_folder(),
        true,
        cancel,
        progress,
    )
}

pub(crate) fn scan_roots_with(
    roots: &[PathBuf],
    readers: &Readers,
    windows: Option<&str>,
    cancel: &AtomicBool,
    progress: &Option<Sender<AuditProgress>>,
) -> Vec<PathBuf> {
    walk_roots(roots, readers, windows, false, cancel, progress).0
}

/// The roots walked in [`plan_lanes`]'s lanes, side by side: a folder found
/// inside a root is on the root's drive, since a junction is never entered.
pub(crate) fn walk_roots(
    roots: &[PathBuf],
    readers: &Readers,
    windows: Option<&str>,
    want_zips: bool,
    cancel: &AtomicBool,
    progress: &Option<Sender<AuditProgress>>,
) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let lanes = plan_lanes(
        roots.iter().map(|r| drive_of(r)),
        readers.threads,
        readers.seeks,
    );
    if let [lane] = lanes.as_slice() {
        return walk_pool(roots, lane.threads, windows, want_zips, cancel, progress);
    }
    let walked: Vec<(Vec<PathBuf>, Vec<PathBuf>)> = std::thread::scope(|scope| {
        let workers: Vec<_> = lanes
            .iter()
            .map(|lane| {
                let roots: Vec<PathBuf> = lane.items.iter().map(|&i| roots[i].clone()).collect();
                scope.spawn(move || {
                    walk_pool(&roots, lane.threads, windows, want_zips, cancel, progress)
                })
            })
            .collect();
        workers
            .into_iter()
            .map(|w| w.join().unwrap_or_default())
            .collect()
    });
    let mut found = (Vec::new(), Vec::new());
    for (demos, zips) in walked {
        found.0.extend(demos);
        found.1.extend(zips);
    }
    found
}

/// `roots` walked by `threads` threads sharing one queue of folders.
fn walk_pool(
    roots: &[PathBuf],
    threads: usize,
    windows: Option<&str>,
    want_zips: bool,
    cancel: &AtomicBool,
    progress: &Option<Sender<AuditProgress>>,
) -> (Vec<PathBuf>, Vec<PathBuf>) {
    /// Folders waiting to be listed, and how many are being listed now: the
    /// walk is over when both are none.
    struct Work {
        pending: Vec<(PathBuf, usize)>,
        busy: usize,
    }
    let work = Mutex::new(Work {
        pending: roots.iter().rev().map(|r| (r.clone(), 0)).collect(),
        busy: 0,
    });
    let ready = Condvar::new();
    let found = Mutex::new((Vec::new(), Vec::new()));
    std::thread::scope(|scope| {
        for _ in 0..threads.max(1) {
            scope.spawn(|| {
                let mut demos = Vec::new();
                let mut zips = Vec::new();
                loop {
                    let next = {
                        let mut w = work.lock().unwrap_or_else(PoisonError::into_inner);
                        loop {
                            if cancel.load(Ordering::Relaxed) {
                                break None;
                            }
                            if let Some(item) = w.pending.pop() {
                                w.busy += 1;
                                break Some(item);
                            }
                            if w.busy == 0 {
                                break None;
                            }
                            // Timed, so a cancel is seen while others list.
                            w = ready
                                .wait_timeout(w, Duration::from_millis(50))
                                .unwrap_or_else(PoisonError::into_inner)
                                .0;
                        }
                    };
                    let Some((dir, depth)) = next else {
                        break;
                    };
                    let mut subdirs = Vec::new();
                    list_dir(
                        &dir,
                        depth,
                        windows,
                        cancel,
                        progress,
                        &mut subdirs,
                        &mut demos,
                        want_zips.then_some(&mut zips),
                    );
                    let mut w = work.lock().unwrap_or_else(PoisonError::into_inner);
                    w.pending.extend(subdirs.into_iter().rev());
                    w.busy -= 1;
                    drop(w);
                    ready.notify_all();
                }
                ready.notify_all();
                let mut found = found.lock().unwrap_or_else(PoisonError::into_inner);
                found.0.extend(demos);
                found.1.extend(zips);
            });
        }
    });
    found.into_inner().unwrap_or_else(PoisonError::into_inner)
}

/// The demos named on a command line: each file as given, each folder walked
/// with [`scan_dir`]. Sorted, so a run over the same folder prints in the same
/// order. Paths that are neither come back in the second list.
pub fn collect_demos(paths: &[PathBuf]) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let cancel = AtomicBool::new(false);
    let mut demos = vec![];
    let mut missing = vec![];
    for path in paths {
        if path.is_file() {
            demos.push(path.clone());
        } else if path.is_dir() {
            scan_dir(path, &mut demos, &cancel, &None);
        } else {
            missing.push(path.clone());
        }
    }
    demos.sort();
    demos.dedup();
    (demos, missing)
}
