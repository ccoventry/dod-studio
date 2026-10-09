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
}
