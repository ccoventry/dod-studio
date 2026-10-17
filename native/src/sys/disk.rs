pub fn calculate_raw_sequence_bytes(w: i32, h: i32, fps: i32, duration_secs: f32) -> u64 {
    (w * h * 3) as u64 * (fps as u64) * (duration_secs as f64).ceil() as u64
}

/// Bytes one recorded `mirv_agr` frame costs, rounded up. Measured on #403's
/// test take: 7.3 MB for 32.6 s at 30 fps is ~7.5 KB a frame. It grows with
/// how many models are on screen, so this is a planning figure, not a bound.
pub const AGR_BYTES_PER_FRAME: u64 = 8 * 1024;

/// What an AGR take of `duration_secs` at `fps` is expected to need on disk.
pub fn calculate_agr_bytes(fps: i32, duration_secs: f32) -> u64 {
    AGR_BYTES_PER_FRAME * fps.max(1) as u64 * (duration_secs as f64).ceil() as u64
}

/// Minimum free space a capture drive must retain, shared by
/// `patch::builder::build_batch_queue`'s AOT allocation (the only place that
/// decides whether a drive has room) and `capture_engine`'s pre-launch
/// re-validation of that same decision — previously two independent 15 GiB
/// literals that could silently drift apart.
pub const MIN_DRIVE_HEADROOM_BYTES: u64 = 15 * 1024 * 1024 * 1024;

/// The drive (`c:`) or share (`\\server\share`) a path is on, lowercased;
/// `None` for a path with no prefix. Folders with the same key share one
/// drive's free space, so planners budget per key, not per folder (#774).
pub fn volume_key(path: &std::path::Path) -> Option<String> {
    match path.components().next()? {
        std::path::Component::Prefix(prefix) => {
            Some(prefix.as_os_str().to_string_lossy().to_lowercase())
        }
        _ => None,
    }
}

/// Groups `dirs` by volume: returns the index of each dir's budget and one
/// starting budget per volume, from `free_of` (the smallest answer among the
/// volume's dirs). A dir with no volume key gets a budget of its own.
pub fn group_by_volume(
    dirs: &[std::path::PathBuf],
    mut free_of: impl FnMut(&std::path::Path) -> u64,
) -> (Vec<usize>, Vec<u64>) {
    let mut budget_of_dir = Vec::with_capacity(dirs.len());
    let mut budgets: Vec<u64> = Vec::new();
    let mut keys: Vec<Option<String>> = Vec::new();
    for dir in dirs {
        let key = volume_key(dir);
        let free = free_of(dir);
        let existing = key
            .is_some()
            .then(|| keys.iter().position(|k| *k == key))
            .flatten();
        match existing {
            Some(i) => {
                budgets[i] = budgets[i].min(free);
                budget_of_dir.push(i);
            }
            None => {
                budget_of_dir.push(budgets.len());
                budgets.push(free);
                keys.push(key);
            }
        }
    }
    (budget_of_dir, budgets)
}

// Disk query results are cached for TTL_MS milliseconds to avoid
// issuing blocking kernel disk-enumeration syscalls on every frame.
const TTL_MS: u128 = 2_000;

#[cfg(not(target_arch = "wasm32"))]
pub fn get_available_bytes(path: &std::path::Path) -> u64 {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    use std::time::Instant;
    use sysinfo::Disks;

    // Shared sysinfo disk list — only refreshed when cache is stale.
    static DISKS: OnceLock<Mutex<Disks>> = OnceLock::new();

    // Per-path TTL cache: path_key → (last_refresh, available_bytes)
    static CACHE: OnceLock<Mutex<HashMap<String, (Instant, u64)>>> = OnceLock::new();

    // Normalise path to a lowercase forward-slash key in a single pass.
    let path_key: String = path
        .to_string_lossy()
        .chars()
        .map(|c| {
            if c == '\\' {
                '/'
            } else {
                c.to_ascii_lowercase()
            }
        })
        .collect();

    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));

    // Fast path: return cached value if within TTL.
    if let Ok(guard) = cache.lock()
        && let Some(&(last, bytes)) = guard.get(&path_key)
        && last.elapsed().as_millis() < TTL_MS
    {
        return bytes;
    }

    // Slow path: refresh disk list and update cache. A mount-point prefix
    // match alone isn't enough — "C:/real/folder|garbage" prefix-matches
    // "c:/" just fine, which used to report all of C:'s free space for a
    // path that can never actually be written to. Reject anything that can
    // never resolve to a real writable directory (malformed syntax, a
    // relative path, or an existing non-directory file) before trusting the
    // mount match; a NotFound path still passes through, since many
    // capture/render output folders are legitimately auto-created at write
    // time — "doesn't exist yet" alone isn't disqualifying.
    let result = match diagnose_path(path) {
        PathStatus::Malformed | PathStatus::NotAbsolute | PathStatus::NotADirectory => u64::MAX,
        PathStatus::Ok | PathStatus::NotFound => {
            let disks_mutex = DISKS.get_or_init(|| Mutex::new(Disks::new_with_refreshed_list()));

            if let Ok(mut disks) = disks_mutex.lock() {
                // Re-lists too, so a drive plugged in since the last call counts.
                disks.refresh(true);

                let mut best_bytes = u64::MAX;
                let mut best_len = 0usize;

                for disk in disks.list() {
                    // Normalise mount point in-place without an extra owned String.
                    let mount_cow = disk.mount_point().to_string_lossy();
                    let mount_key: String = mount_cow
                        .chars()
                        .map(|c| {
                            if c == '\\' {
                                '/'
                            } else {
                                c.to_ascii_lowercase()
                            }
                        })
                        .collect();

                    if path_key.starts_with(&mount_key) && mount_key.len() > best_len {
                        best_len = mount_key.len();
                        best_bytes = disk.available_space();
                    }
                }
                best_bytes
            } else {
                u64::MAX
            }
        }
    };

    // Store result in cache.
    if let Ok(mut guard) = cache.lock() {
        guard.insert(path_key, (Instant::now(), result));
    }

    result
}

#[cfg(target_arch = "wasm32")]
pub fn get_available_bytes(_path: &std::path::Path) -> u64 {
    u64::MAX
}

/// Why a configured output path can't be used, for surfacing a specific
/// reason to the user instead of a generic "not configured" message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathStatus {
    Ok,
    /// No drive letter or UNC root (e.g. "a", "foo\\bar") — a relative path
    /// would silently resolve against the process's own working directory,
    /// not where the user thinks, so it's rejected before ever touching disk.
    NotAbsolute,
    /// The OS rejected the path syntax itself (illegal characters, malformed
    /// UNC, etc.) rather than reporting "not found".
    Malformed,
    /// Well-formed and absolute, but nothing exists there (or the drive
    /// letter isn't mounted).
    NotFound,
    /// Exists, but is a file, not a directory.
    NotADirectory,
}

#[cfg(not(target_arch = "wasm32"))]
pub fn diagnose_path(path: &std::path::Path) -> PathStatus {
    if !path.is_absolute() {
        return PathStatus::NotAbsolute;
    }
    match std::fs::metadata(path) {
        Ok(meta) => {
            if meta.is_dir() {
                PathStatus::Ok
            } else {
                PathStatus::NotADirectory
            }
        }
        Err(e) => match e.raw_os_error() {
            // ERROR_INVALID_NAME, ERROR_INVALID_PARAMETER, ERROR_BAD_PATHNAME
            Some(123) | Some(87) | Some(161) => PathStatus::Malformed,
            // ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, ERROR_INVALID_DRIVE, or unknown
            _ => PathStatus::NotFound,
        },
    }
}

#[cfg(target_arch = "wasm32")]
pub fn diagnose_path(_path: &std::path::Path) -> PathStatus {
    PathStatus::Ok
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    /// A real, existing folder resolves to its drive's free space; a path that
    /// can never be written to reports "unknown" (u64::MAX) instead of the
    /// free space of whatever drive it prefix-matches.
    #[test]
    fn free_space_of_a_real_folder_and_a_bad_path() {
        let dir = std::env::temp_dir();
        let bytes = get_available_bytes(&dir);
        assert!(bytes > 0 && bytes < u64::MAX, "{}: {bytes}", dir.display());
        // Cached for TTL_MS, so a second call agrees.
        assert_eq!(get_available_bytes(&dir), bytes);
        assert_eq!(
            get_available_bytes(std::path::Path::new("relative/folder")),
            u64::MAX
        );
    }

    #[cfg(windows)]
    #[test]
    fn folders_on_one_drive_share_a_key() {
        use std::path::Path;
        assert_eq!(
            volume_key(Path::new(r"C:\captures")),
            volume_key(Path::new(r"c:\Programs\HLAE"))
        );
        assert_ne!(
            volume_key(Path::new(r"C:\captures")),
            volume_key(Path::new(r"D:\captures"))
        );
        assert_eq!(
            volume_key(Path::new(r"\\nas\movies\a")),
            volume_key(Path::new(r"\\NAS\Movies\b"))
        );
        assert_eq!(volume_key(Path::new("relative")), None);
    }

    /// #774: two folders on C: get one budget, D: its own, and folders with
    /// no drive never merge.
    #[cfg(windows)]
    #[test]
    fn folders_on_one_drive_share_one_budget() {
        use std::path::PathBuf;
        let dirs: Vec<PathBuf> = [r"C:\a", r"D:\x", r"c:\b", "rel1", "rel2"]
            .iter()
            .map(PathBuf::from)
            .collect();
        let (budget_of_dir, budgets) =
            group_by_volume(&dirs, |p| if p.starts_with(r"D:\") { 50 } else { 100 });
        assert_eq!(budget_of_dir, vec![0, 1, 0, 2, 3]);
        assert_eq!(budgets, vec![100, 50, 100, 100]);
    }
}
