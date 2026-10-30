//! The last free-space check before the game starts: every drive the batch
//! records to, and hl.exe's own drive for the patched demos.

use super::CaptureJob;
use std::path::Path;

const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

/// Why the batch can't start for want of room, or `Ok` when it can. The
/// capture drives are checked first, from the headroom `build_batch_queue`
/// already worked out; hl.exe's drive is only asked for its free space once
/// they pass.
pub(super) fn check_room(
    drive_headroom: &[crate::patch::DriveHeadroom],
    jobs: &[CaptureJob],
    save_local_patched_copy: bool,
    hl_exe_parent: &Path,
) -> Result<(), String> {
    if let Some(problem) = capture_drive_short_of_room(drive_headroom) {
        return Err(problem);
    }
    // hl.exe's own install drive is never in `drive_headroom` (that
    // list is only for drives an actual recording block routed to —
    // patched demo files land directly in `hl_exe_parent`'s own
    // `dod/` folder now, not on any capture_directories entry, since
    // that's the only place GoldSrc's `playdemo` can find them; see
    // issue #8). Every patched file already exists on disk by this
    // point — real sizes, not an estimate. Doubled when Save Local
    // Patched Copy duplicates each file into `<exe_dir>/demos` too.
    // See issue #11.
    let total_patched_bytes: u64 = jobs
        .iter()
        .map(|j| {
            std::fs::metadata(&j.patched_demo_path)
                .map(|m| m.len())
                .unwrap_or(0)
        })
        .sum();
    let required = patched_demos_need(total_patched_bytes, save_local_patched_copy);
    let free = crate::sys::disk::get_available_bytes(hl_exe_parent);
    match game_drive_short_of_room(hl_exe_parent, free, required) {
        Some(problem) => Err(problem),
        None => Ok(()),
    }
}

/// The first capture drive with less than the minimum headroom, as the
/// batch's error says it.
fn capture_drive_short_of_room(drive_headroom: &[crate::patch::DriveHeadroom]) -> Option<String> {
    let entry = drive_headroom
        .iter()
        .find(|entry| entry.free_bytes < crate::sys::disk::MIN_DRIVE_HEADROOM_BYTES)?;
    let required_gb = crate::sys::disk::MIN_DRIVE_HEADROOM_BYTES as f64 / GIB;
    Some(format!(
        "{} has less than {:.1} GB free space. Free some space or pick another capture directory.",
        entry.path.display(),
        required_gb
    ))
}

/// The room the patched demos take on hl.exe's drive: twice their size when
/// Save Local Patched Copy keeps a second copy of each.
fn patched_demos_need(total_patched_bytes: u64, save_local_patched_copy: bool) -> u64 {
    if save_local_patched_copy {
        total_patched_bytes.saturating_mul(2)
    } else {
        total_patched_bytes
    }
}

/// The error for hl.exe's drive, when its free space is short of `required`.
fn game_drive_short_of_room(hl_exe_parent: &Path, free: u64, required: u64) -> Option<String> {
    (free < required).then(|| {
        format!(
            "{} (hl.exe's own drive) has less than {:.1} GB free space for the patched demos.",
            hl_exe_parent.display(),
            required as f64 / GIB
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::patch::DriveHeadroom;
    use crate::sys::disk::MIN_DRIVE_HEADROOM_BYTES;
    use std::path::PathBuf;

    fn drive(path: &str, free_bytes: u64) -> DriveHeadroom {
        DriveHeadroom {
            path: PathBuf::from(path),
            free_bytes,
        }
    }

    #[test]
    fn the_first_capture_drive_under_the_minimum_is_named() {
        let enough = drive("C:/takes", MIN_DRIVE_HEADROOM_BYTES);
        let short = drive("D:/takes", MIN_DRIVE_HEADROOM_BYTES - 1);
        let empty = drive("E:/takes", 0);
        assert_eq!(
            capture_drive_short_of_room(std::slice::from_ref(&enough)),
            None
        );
        assert_eq!(capture_drive_short_of_room(&[]), None);
        assert_eq!(
            capture_drive_short_of_room(&[enough, short, empty]).as_deref(),
            Some(
                "D:/takes has less than 15.0 GB free space. Free some space or pick another capture directory."
            )
        );
    }

    #[test]
    fn a_local_copy_doubles_the_room_the_patched_demos_need() {
        assert_eq!(patched_demos_need(100, false), 100);
        assert_eq!(patched_demos_need(100, true), 200);
        assert_eq!(patched_demos_need(u64::MAX, true), u64::MAX);
    }

    #[test]
    fn hl_exes_drive_is_short_only_below_what_the_demos_need() {
        let game = Path::new("C:/Games/Half-Life");
        assert_eq!(game_drive_short_of_room(game, 100, 100), None);
        assert_eq!(
            game_drive_short_of_room(game, 99, 3 * 1024 * 1024 * 1024 / 2).as_deref(),
            Some(
                "C:/Games/Half-Life (hl.exe's own drive) has less than 1.5 GB free space for the patched demos."
            )
        );
    }

    /// The capture drives are checked before hl.exe's drive is asked: a
    /// short capture drive is the error even with a game folder that has
    /// no drive at all.
    #[test]
    fn a_short_capture_drive_is_reported_before_the_game_drive() {
        let short = drive("D:/takes", 0);
        assert!(
            check_room(&[short], &[], false, Path::new("Z:/no/such/game"))
                .unwrap_err()
                .starts_with("D:/takes has less than")
        );
    }
}
