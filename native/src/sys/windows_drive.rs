//! "Keep free on the Windows drive" (#1430): captures and renders never write
//! to the drive Windows is installed on when that would leave it at or below
//! the setting, so the page file, updates and temp files keep their room.
//! Other drives keep their usual headroom (`MIN_DRIVE_HEADROOM_BYTES` for
//! captures, `EXPORT_SAFETY_MARGIN_BYTES` for renders).
//!
//! The setting is process-wide: Studio hands it over when settings load and on
//! every save ([`set_keep_free_gb`]), and every drive choice reads it again
//! ([`headroom_for`]), so a change applies to the next render or clip of a
//! batch that is already running.
//!
//! The Windows drive is found by volume, not by letter: a folder reached
//! through a junction (`<game>\dod_pool_0` to a folder on `C:`) or a disk
//! mounted as a folder counts as the volume it really is on.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// The setting's value for a settings file that has none: enough for a
/// Windows update and a grown page file on a typical machine.
pub const DEFAULT_KEEP_FREE_GB: u64 = 50;

/// The largest value the setting takes, so a typo can't overflow the byte
/// count or rule the Windows drive out for good by accident.
pub const MAX_KEEP_FREE_GB: u64 = 10_000;

/// Bytes in the setting's "GB": binary, like every other GB Studio shows
/// (`take_cleanup::format_gb`).
const BYTES_PER_GB: u64 = 1024 * 1024 * 1024;

/// Until Studio sets it: the default, so a CLI keeps the Windows drive free
/// too. 0 in this crate's own tests, which plan captures and render into the
/// temp folder (usually on the Windows drive) and must not depend on how full
/// that drive is; the tests of the rule pass its value in.
#[cfg(not(test))]
const INITIAL_KEEP_FREE_BYTES: u64 = DEFAULT_KEEP_FREE_GB * BYTES_PER_GB;
#[cfg(test)]
const INITIAL_KEEP_FREE_BYTES: u64 = 0;

static KEEP_FREE_BYTES: AtomicU64 = AtomicU64::new(INITIAL_KEEP_FREE_BYTES);

/// Sets what the Windows drive keeps free for the rest of this process, in
/// GB; clamped to [`MAX_KEEP_FREE_GB`]. 0 leaves only the usual headroom.
pub fn set_keep_free_gb(gb: u64) {
    KEEP_FREE_BYTES.store(gb.min(MAX_KEEP_FREE_GB) * BYTES_PER_GB, Ordering::Relaxed);
}

/// What the Windows drive keeps free, in bytes: the setting as last set.
pub fn keep_free_bytes() -> u64 {
    KEEP_FREE_BYTES.load(Ordering::Relaxed)
}

/// What a writer must leave free on `path`'s drive, given the `headroom` it
/// keeps on every drive: on the Windows drive, the setting when that is
/// larger. Reads the setting now, so a change made mid-batch counts.
pub fn headroom_for(path: &Path, headroom: u64) -> u64 {
    raised_headroom(headroom, is_on_windows_drive(path), keep_free_bytes())
}

/// `free` bytes on `path`'s drive, less what the Windows-drive setting keeps
/// back beyond `headroom`: what a planner that already keeps `headroom` on
/// every drive may count on there. Elsewhere, `free` unchanged.
pub fn free_for(path: &Path, headroom: u64, free: u64) -> u64 {
    free_less_raise(free, headroom, headroom_for(path, headroom))
}

/// `free` less how far `raised` lifts the usual `headroom`.
fn free_less_raise(free: u64, headroom: u64, raised: u64) -> u64 {
    free.saturating_sub(raised.saturating_sub(headroom))
}

/// The setting, when it raises `headroom` on any of `dirs`: what the Windows
/// drive keeps free, for a refusal that has to explain why a folder there
/// was passed over. `None` when no folder is on the Windows drive or the
/// setting is no larger than `headroom`.
pub fn raised_on_any(dirs: &[PathBuf], headroom: u64) -> Option<u64> {
    let keep_free = keep_free_bytes();
    (keep_free > headroom && dirs.iter().any(|d| is_on_windows_drive(d))).then_some(keep_free)
}

/// `headroom`, or `keep_free` when the drive is the Windows drive and that is
/// larger.
fn raised_headroom(headroom: u64, on_windows_drive: bool, keep_free: u64) -> u64 {
    if on_windows_drive {
        headroom.max(keep_free)
    } else {
        headroom
    }
}

/// Whether `path` is on the volume Windows is installed on. Links in the part
/// of `path` that exists are followed first. A share, a relative path or a
/// drive that isn't there is not.
pub fn is_on_windows_drive(path: &Path) -> bool {
    let Some(windows) = windows_volume() else {
        return false;
    };
    if !on_local_drive(path) {
        return false;
    }
    super::disk::volume_key(&resolve_links(path)).as_deref() == Some(windows)
}

/// Whether `path` names a drive letter. Only those are looked at: asking
/// about a share that's offline waits on the network, and Windows is never
/// installed on one.
fn on_local_drive(path: &Path) -> bool {
    matches!(
        path.components().next(),
        Some(std::path::Component::Prefix(prefix))
            if matches!(
                prefix.kind(),
                std::path::Prefix::Disk(_) | std::path::Prefix::VerbatimDisk(_)
            )
    )
}

/// `path` with its nearest existing folder replaced by where it really is,
/// junctions and symlinks followed; the rest (folders not made yet) is kept.
/// `path` itself when nothing of it exists or it can't be resolved.
fn resolve_links(path: &Path) -> PathBuf {
    let Some(existing) = path.ancestors().find(|p| p.is_dir()) else {
        return path.to_path_buf();
    };
    let Ok(real) = plain_canonical(existing) else {
        return path.to_path_buf();
    };
    match path.strip_prefix(existing) {
        Ok(rest) if !rest.as_os_str().is_empty() => real.join(rest),
        _ => real,
    }
}

#[cfg(windows)]
fn plain_canonical(path: &Path) -> std::io::Result<PathBuf> {
    super::junction::plain_canonical(path)
}

#[cfg(not(windows))]
fn plain_canonical(path: &Path) -> std::io::Result<PathBuf> {
    std::fs::canonicalize(path)
}

/// The Windows drive's volume key (`disk::volume_key`), read once.
fn windows_volume() -> Option<&'static str> {
    static VOLUME: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    VOLUME
        .get_or_init(|| super::disk::volume_key(&windows_dir()?))
        .as_deref()
}

/// The folder Windows is installed in (`C:\Windows`), from
/// `GetWindowsDirectoryW`, else `%SystemRoot%`.
#[cfg(windows)]
fn windows_dir() -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetWindowsDirectoryW(buffer: *mut u16, size: u32) -> u32;
    }

    // MAX_PATH covers the Windows folder, which is never a long path.
    let mut buffer = [0u16; 260];
    let len = unsafe { GetWindowsDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
    if len > 0 && len < buffer.len() {
        return Some(PathBuf::from(std::ffi::OsString::from_wide(&buffer[..len])));
    }
    std::env::var_os("SystemRoot").map(PathBuf::from)
}

/// No Windows folder off Windows, so no drive is the Windows drive.
#[cfg(not(windows))]
fn windows_dir() -> Option<PathBuf> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const GB: u64 = BYTES_PER_GB;

    #[test]
    fn the_windows_drive_keeps_the_larger_of_the_setting_and_the_headroom() {
        assert_eq!(raised_headroom(15 * GB, true, 50 * GB), 50 * GB);
        // A setting below the usual headroom never lowers it.
        assert_eq!(raised_headroom(15 * GB, true, 5 * GB), 15 * GB);
        assert_eq!(raised_headroom(15 * GB, true, 0), 15 * GB);
    }

    #[test]
    fn another_drive_keeps_only_the_headroom() {
        assert_eq!(raised_headroom(GB, false, 50 * GB), GB);
    }

    /// A planner that keeps 15 GB on every drive counts 50 GB keep-free as
    /// 35 GB less free on the Windows drive, and nothing below zero.
    #[test]
    fn a_raised_headroom_comes_off_the_free_space_a_planner_counts() {
        assert_eq!(free_less_raise(100 * GB, 15 * GB, 50 * GB), 65 * GB);
        assert_eq!(free_less_raise(20 * GB, 15 * GB, 50 * GB), 0);
        assert_eq!(free_less_raise(100 * GB, 15 * GB, 15 * GB), 100 * GB);
    }

    /// This crate's tests start with no keep-free, so planning and rendering
    /// into the temp folder never depends on how full its drive is.
    #[test]
    fn tests_start_with_nothing_kept_on_the_windows_drive() {
        assert_eq!(INITIAL_KEEP_FREE_BYTES, 0);
    }

    #[cfg(windows)]
    #[test]
    fn the_windows_folder_is_on_the_windows_drive() {
        let windows = windows_dir().expect("the Windows folder");
        assert!(is_on_windows_drive(&windows));
        // A folder not made yet counts as the drive it will be on.
        assert!(is_on_windows_drive(
            &windows.join("no-such-folder-1430").join("take")
        ));
    }

    #[cfg(windows)]
    #[test]
    fn a_missing_drive_a_share_and_a_relative_path_are_not_the_windows_drive() {
        let missing = (b'A'..=b'Z')
            .map(|c| format!("{}:\\", c as char))
            .find(|d| !Path::new(d).exists());
        if let Some(drive) = missing {
            assert!(!is_on_windows_drive(&Path::new(&drive).join("takes")));
        }
        assert!(!is_on_windows_drive(Path::new(r"\\nas\movies\takes")));
        assert!(!is_on_windows_drive(Path::new("relative/takes")));
    }

    /// A capture folder reached through a junction (`dod_pool_0`) is judged by
    /// the folder it points at, not where the junction sits.
    #[cfg(windows)]
    #[test]
    fn a_junction_is_followed_to_where_it_points() {
        let scratch = crate::test_support::Scratch::new("windows_drive_junction");
        let target = scratch.path().join("real takes");
        std::fs::create_dir_all(&target).unwrap();
        let link = scratch.path().join("dod_pool_0");
        crate::sys::junction::create(&link, &target).unwrap();

        let resolved = resolve_links(&link.join("session").join("clip_b0"));
        assert_eq!(
            resolved,
            plain_canonical(&target)
                .unwrap()
                .join("session")
                .join("clip_b0")
        );
        crate::sys::junction::remove(&link).unwrap();
    }

    #[test]
    fn a_path_with_nothing_on_disk_resolves_to_itself() {
        let path = Path::new("relative/never/made");
        assert_eq!(resolve_links(path), path.to_path_buf());
    }
}
