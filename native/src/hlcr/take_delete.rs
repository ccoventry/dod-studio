//! Delete Take (#644): sends a finished take's folder to the Recycle Bin.
//!
//! Takes are big (a 120 fps frame sequence is gigabytes) and only needed again
//! to re-render. The Render tab's Delete Take button lands here.
//!
//! Two rules shape this module:
//!
//! - **Only a take folder is ever deleted.** The path comes from the frontend,
//!   so it is checked against what the scanner itself would hand out as a
//!   take, inside a Destinations folder, before anything moves.
//!   A wrong path must fail with a message, never delete a session, a block
//!   holding several takes, or a drive.
//! - **The take's history survives.** `dodstudio_take.json` (render history,
//!   #438, and the capture FPS) stays where it is: in the block folder above a
//!   `take0000`, which this never touches, or, when the take folder holds it
//!   itself, by sending everything else in the folder and leaving the file.

use std::path::{Path, PathBuf};

use crate::hlcr::{scanner, take_meta};
use crate::shared::paths::take_key;

/// One take, checked and measured, ready to go to the Recycle Bin.
#[derive(Debug, Clone, PartialEq)]
pub struct TakeDeletion {
    pub take_folder: PathBuf,
    /// `<session>/<block>`, the key the take index and the clip names use.
    pub take_key: String,
    /// What goes to the Recycle Bin: the folder itself, or everything in it
    /// but `dodstudio_take.json` when the folder holds that file.
    pub recycle: Vec<PathBuf>,
    /// The size on disk of everything in `recycle`.
    pub size_bytes: u64,
}

/// Checks that `take_folder` is a take folder inside one of `capture_dirs`,
/// and works out what deleting it moves and how much space that frees.
pub fn plan_take_deletion(
    take_folder: &Path,
    capture_dirs: &[PathBuf],
) -> Result<TakeDeletion, String> {
    let key = check_take_folder(take_folder, capture_dirs)?;
    let recycle = recycle_list(take_folder)?;
    let size_bytes = recycle.iter().map(|p| size_on_disk(p)).sum();
    Ok(TakeDeletion {
        take_folder: take_folder.to_path_buf(),
        take_key: key,
        recycle,
        size_bytes,
    })
}

/// The safety check: `Ok` with the take's key only for a real take folder
/// strictly inside a Destinations folder.
pub fn check_take_folder(take_folder: &Path, capture_dirs: &[PathBuf]) -> Result<String, String> {
    if !take_folder.is_absolute() {
        return Err(format!(
            "{} isn't a full path, so it was not deleted",
            take_folder.display()
        ));
    }
    let meta = std::fs::symlink_metadata(take_folder)
        .map_err(|e| format!("{} can't be read: {e}", take_folder.display()))?;
    // A link or junction would send its target's contents, which may be
    // anywhere, so only a real folder is accepted.
    if meta.file_type().is_symlink() {
        return Err(format!(
            "{} is a link, not a take folder, so it was not deleted",
            take_folder.display()
        ));
    }
    if !meta.is_dir() {
        return Err(format!("{} isn't a folder", take_folder.display()));
    }

    // Canonical on both sides, so `..`, a different case or a junction in the
    // capture path can't place the take somewhere it isn't.
    let canonical = take_folder
        .canonicalize()
        .map_err(|e| format!("{} can't be resolved: {e}", take_folder.display()))?;
    let inside_a_capture_dir = capture_dirs
        .iter()
        .filter_map(|dir| dir.canonicalize().ok())
        .any(|dir| canonical != dir && canonical.starts_with(&dir));
    if !inside_a_capture_dir {
        return Err(format!(
            "{} isn't inside a Destinations folder (Configuration → Destinations), so it was not deleted",
            take_folder.display()
        ));
    }

    if !scanner::is_take_folder(take_folder) {
        return Err(format!(
            "{} isn't a take folder (no captured streams in it), so it was not deleted",
            take_folder.display()
        ));
    }
    // One take per delete, as the confirm said: a folder that also holds
    // another take would take that one with it.
    let holds_another_take = std::fs::read_dir(take_folder)
        .map_err(|e| format!("{} can't be listed: {e}", take_folder.display()))?
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .any(|e| scanner::is_take_folder(&e.path()));
    if holds_another_take {
        return Err(format!(
            "{} holds other takes, so it was not deleted",
            take_folder.display()
        ));
    }
    take_key(take_folder).ok_or_else(|| {
        format!(
            "{} isn't inside a session folder, so it was not deleted",
            take_folder.display()
        )
    })
}

/// The folder itself, unless it holds the take's `dodstudio_take.json`: then
/// everything in it but that file, so the history stays (#438).
fn recycle_list(take_folder: &Path) -> Result<Vec<PathBuf>, String> {
    let entries: Vec<_> = std::fs::read_dir(take_folder)
        .map_err(|e| format!("{} can't be listed: {e}", take_folder.display()))?
        .flatten()
        .collect();
    if !entries
        .iter()
        .any(|e| take_meta::is_metadata(&e.file_name()))
    {
        return Ok(vec![take_folder.to_path_buf()]);
    }
    Ok(entries
        .into_iter()
        .filter(|e| !take_meta::is_metadata(&e.file_name()))
        .map(|e| e.path())
        .collect())
}

/// The total size of the files at or under `path`. Links are not followed,
/// matching what the Recycle Bin moves.
pub fn size_on_disk(path: &Path) -> u64 {
    walkdir::WalkDir::new(path)
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum()
}

/// Sends a checked take to the Recycle Bin.
///
/// On a fresh thread: `trash` initialises COM apartment-threaded on whatever
/// thread calls it and panics if that thread already chose the other model,
/// which a reused worker thread may have. A panic aborts a release build.
pub fn recycle_take(deletion: &TakeDeletion) -> Result<(), String> {
    let paths = deletion.recycle.clone();
    let outcome = std::thread::spawn(move || trash::delete_all(&paths).map_err(|e| e.to_string()))
        .join()
        .unwrap_or_else(|_| Err("the Recycle Bin call stopped unexpectedly".to_string()));
    outcome.map_err(|e| {
        format!(
            "Couldn't move {} to the Recycle Bin: {e}",
            deletion.take_folder.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Scratch;

    /// `<capture>/<session>/<block>/take0000` with a wav and one BMP stream,
    /// the shape a frame-sequence capture leaves. Returns the capture dir and
    /// the take folder.
    fn capture_with_take(tag: &str) -> (Scratch, PathBuf) {
        let capture = Scratch::new(format_args!("take_delete_{tag}"));
        let take = capture
            .join("session_20261011_120000")
            .join("dodstudio_chain_01_b0")
            .join("take0000");
        std::fs::create_dir_all(take.join("all")).unwrap();
        std::fs::write(take.join("sound.wav"), b"wavdata").unwrap();
        std::fs::write(take.join("all").join("00000.bmp"), b"bmp").unwrap();
        std::fs::write(take.join("all").join("00001.bmp"), b"bmp").unwrap();
        (capture, take)
    }

    fn dirs(capture: &Scratch) -> Vec<PathBuf> {
        vec![capture.path().to_path_buf()]
    }

    #[test]
    fn a_take_inside_a_capture_folder_is_planned_whole_with_its_size() {
        let (capture, take) = capture_with_take("whole");
        let plan = plan_take_deletion(&take, &dirs(&capture)).expect("a real take");
        assert_eq!(
            plan.take_key,
            "session_20261011_120000/dodstudio_chain_01_b0"
        );
        assert_eq!(plan.recycle, vec![take.clone()]);
        assert_eq!(plan.size_bytes, 7 + 3 + 3);
    }

    #[test]
    fn the_history_file_in_the_block_folder_is_never_in_the_plan() {
        let (capture, take) = capture_with_take("block_history");
        let block = take.parent().unwrap().to_path_buf();
        take_meta::write(&block, &take_meta::SessionMeta::new("s", 120)).unwrap();
        let plan = plan_take_deletion(&take, &dirs(&capture)).unwrap();
        assert_eq!(plan.recycle, vec![take.clone()]);
        assert!(
            plan.recycle
                .iter()
                .all(|p| !block.join(take_meta::TAKE_FILE).starts_with(p))
        );
    }

    #[test]
    fn a_take_folder_holding_the_history_file_keeps_it() {
        let (capture, take) = capture_with_take("own_history");
        take_meta::write(&take, &take_meta::SessionMeta::new("s", 120)).unwrap();
        let plan = plan_take_deletion(&take, &dirs(&capture)).unwrap();
        let mut recycle = plan.recycle.clone();
        recycle.sort();
        assert_eq!(recycle, vec![take.join("all"), take.join("sound.wav")]);
        assert_eq!(plan.size_bytes, 7 + 3 + 3, "the history file was counted");
    }

    #[test]
    fn the_capture_folder_session_and_block_are_refused() {
        let (capture, take) = capture_with_take("parents");
        let block = take.parent().unwrap().to_path_buf();
        let session = block.parent().unwrap().to_path_buf();
        for folder in [capture.path().to_path_buf(), session, block] {
            let err = check_take_folder(&folder, &dirs(&capture)).unwrap_err();
            assert!(
                err.contains("not deleted"),
                "{folder:?} was accepted: {err}"
            );
        }
    }

    #[test]
    fn a_take_outside_every_capture_folder_is_refused() {
        let (_capture, take) = capture_with_take("outside");
        let elsewhere = Scratch::new("take_delete_outside_other");
        let err = check_take_folder(&take, &dirs(&elsewhere)).unwrap_err();
        assert!(err.contains("isn't inside a Destinations folder"), "{err}");
        assert!(
            check_take_folder(&take, &[]).is_err(),
            "no capture folders at all"
        );
    }

    #[test]
    fn dot_dot_cannot_walk_a_path_into_a_capture_folder() {
        let (_capture, take) = capture_with_take("dotdot_take");
        let other = Scratch::new("take_delete_dotdot_capture");
        // Starts with the capture folder as text, resolves outside it.
        let sneaky = other
            .join("..")
            .join(take.strip_prefix(std::env::temp_dir()).unwrap());
        assert!(sneaky.starts_with(other.path()));
        let err = check_take_folder(&sneaky, &dirs(&other)).unwrap_err();
        assert!(err.contains("isn't inside a Destinations folder"), "{err}");
    }

    #[test]
    fn a_folder_with_no_captured_streams_is_refused() {
        let (capture, take) = capture_with_take("no_streams");
        std::fs::remove_dir_all(take.join("all")).unwrap();
        let err = check_take_folder(&take, &dirs(&capture)).unwrap_err();
        assert!(err.contains("isn't a take folder"), "{err}");
    }

    #[test]
    fn a_folder_holding_another_take_is_refused() {
        let (capture, take) = capture_with_take("nested");
        let inner = take.join("take0001");
        std::fs::create_dir_all(inner.join("all")).unwrap();
        std::fs::write(inner.join("sound.wav"), b"w").unwrap();
        std::fs::write(inner.join("all").join("00000.bmp"), b"b").unwrap();
        let err = check_take_folder(&take, &dirs(&capture)).unwrap_err();
        assert!(err.contains("holds other takes"), "{err}");
    }

    #[test]
    fn relative_missing_and_file_paths_are_refused() {
        let (capture, take) = capture_with_take("bad_paths");
        let relative = PathBuf::from("session").join("block").join("take0000");
        assert!(
            check_take_folder(&relative, &dirs(&capture))
                .unwrap_err()
                .contains("isn't a full path")
        );
        assert!(
            check_take_folder(&take.join("missing"), &dirs(&capture))
                .unwrap_err()
                .contains("can't be read")
        );
        assert!(
            check_take_folder(&take.join("sound.wav"), &dirs(&capture))
                .unwrap_err()
                .contains("isn't a folder")
        );
    }
}
