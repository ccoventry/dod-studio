//! Where FFmpeg writes a render until it is finished (#1439).
//!
//! FFmpeg writes `<name>.partial<ext>` beside the finished file's name and the
//! file takes its real name only once FFmpeg exits cleanly. A render that
//! never finishes (Studio closed, crashed, or the PC lost power) so never
//! leaves a file under a finished clip's name. The extension stays last
//! because FFmpeg picks its muxer from it.
//!
//! Renders in flight are listed, so Studio's way out can remove what they
//! were writing after it has ended their FFmpegs.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

/// What marks an unfinished render's file name, before its extension.
pub const PARTIAL_MARK: &str = ".partial";

/// How long Studio's way out keeps trying to remove a file whose FFmpeg was
/// just ended: Windows lets a file go only once its process has fully exited.
const REMOVE_RETRY_FOR: Duration = Duration::from_secs(2);
const REMOVE_RETRY_EVERY: Duration = Duration::from_millis(50);

/// The name FFmpeg writes `out_file` under until it is finished:
/// `clip.mov` → `clip.partial.mov`.
pub fn partial_path(out_file: &Path) -> PathBuf {
    let stem = out_file.file_stem().unwrap_or_default().to_string_lossy();
    let name = match out_file.extension() {
        Some(ext) => format!("{stem}{PARTIAL_MARK}.{}", ext.to_string_lossy()),
        None => format!("{stem}{PARTIAL_MARK}"),
    };
    out_file.with_file_name(name)
}

/// Gives a finished render its real name and returns where it is. When a
/// file took `out_file`'s name while FFmpeg ran, the render gets the next free
/// name instead: a finished clip is never overwritten.
pub fn finish(partial: &Path, out_file: &Path) -> std::io::Result<PathBuf> {
    let target = if out_file.exists() {
        let dir = out_file.parent().unwrap_or(Path::new(""));
        let stem = out_file.file_stem().unwrap_or_default().to_string_lossy();
        let ext = out_file
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default();
        dir.join(super::get_unique_filename(dir, &stem, &ext))
    } else {
        out_file.to_path_buf()
    };
    std::fs::rename(partial, &target)?;
    Ok(target)
}

/// The unfinished files of the renders running now.
#[derive(Default)]
pub struct InFlight {
    paths: Mutex<HashSet<PathBuf>>,
}

/// Keeps one render's unfinished file on the list until it is dropped.
pub struct InFlightEntry<'a> {
    list: &'a InFlight,
    path: PathBuf,
}

impl Drop for InFlightEntry<'_> {
    fn drop(&mut self) {
        self.list.lock().remove(&self.path);
    }
}

/// How removing the unfinished files went.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Removed {
    pub removed: usize,
    /// The files still there, with why.
    pub failed: Vec<(PathBuf, String)>,
}

impl InFlight {
    // The set stays valid after a panic mid-update: each change is one
    // insert or remove.
    fn lock(&self) -> std::sync::MutexGuard<'_, HashSet<PathBuf>> {
        self.paths.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Lists `path` until the returned entry is dropped.
    pub fn add(&self, path: &Path) -> InFlightEntry<'_> {
        self.lock().insert(path.to_path_buf());
        InFlightEntry {
            list: self,
            path: path.to_path_buf(),
        }
    }

    /// Removes every listed file that is on disk, retrying for a moment a
    /// file its just-ended FFmpeg still holds. A file already gone counts as
    /// neither removed nor failed.
    pub fn remove_all(&self) -> Removed {
        let paths: Vec<PathBuf> = self.lock().iter().cloned().collect();
        let mut result = Removed::default();
        for path in paths {
            match remove_retrying(&path) {
                Ok(true) => result.removed += 1,
                Ok(false) => {}
                Err(e) => result.failed.push((path, e.to_string())),
            }
        }
        result
    }
}

/// Removes `path`: `Ok(true)` once removed, `Ok(false)` when it was not
/// there, the last error when it is still held after `REMOVE_RETRY_FOR`.
fn remove_retrying(path: &Path) -> std::io::Result<bool> {
    let began = Instant::now();
    loop {
        match std::fs::remove_file(path) {
            Ok(()) => return Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(e) if began.elapsed() >= REMOVE_RETRY_FOR => return Err(e),
            Err(_) => std::thread::sleep(REMOVE_RETRY_EVERY),
        }
    }
}

/// The renders running in this Studio.
pub fn in_flight() -> &'static InFlight {
    static LIST: std::sync::OnceLock<InFlight> = std::sync::OnceLock::new();
    LIST.get_or_init(InFlight::default)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Scratch;

    #[test]
    fn the_partial_name_keeps_the_extension_last() {
        assert_eq!(
            partial_path(Path::new("D:/export/Kill streak.mov")),
            Path::new("D:/export/Kill streak.partial.mov")
        );
        assert_eq!(
            partial_path(Path::new("D:/export/a.b_hud.mp4")),
            Path::new("D:/export/a.b_hud.partial.mp4")
        );
        assert_eq!(
            partial_path(Path::new("D:/export/raw")),
            Path::new("D:/export/raw.partial")
        );
    }

    #[test]
    fn a_finished_render_takes_its_real_name() {
        let dir = Scratch::new("partial_finish");
        let out = dir.join("clip.mov");
        let partial = partial_path(&out);
        std::fs::write(&partial, b"frames").unwrap();
        assert_eq!(finish(&partial, &out).unwrap(), out);
        assert_eq!(std::fs::read(&out).unwrap(), b"frames");
        assert!(!partial.exists());
    }

    #[test]
    fn a_finished_render_never_overwrites_a_clip_that_took_its_name() {
        let dir = Scratch::new("partial_taken");
        let out = dir.join("clip.mov");
        std::fs::write(&out, b"earlier clip").unwrap();
        let partial = partial_path(&out);
        std::fs::write(&partial, b"frames").unwrap();
        let done = finish(&partial, &out).unwrap();
        assert_eq!(done, dir.join("clip_1.mov"));
        assert_eq!(std::fs::read(&out).unwrap(), b"earlier clip");
        assert_eq!(std::fs::read(&done).unwrap(), b"frames");
    }

    #[test]
    fn removing_in_flight_files_leaves_finished_ones() {
        let dir = Scratch::new("partial_in_flight");
        let list = InFlight::default();
        let running = dir.join("a.partial.mov");
        std::fs::write(&running, b"half").unwrap();
        let finished = dir.join("b.mov");
        std::fs::write(&finished, b"done").unwrap();
        let _running_entry = list.add(&running);
        // A render that finished and was renamed: its entry is gone with it.
        drop(list.add(&dir.join("b.partial.mov")));
        // A render listed whose file is not on disk yet.
        let _not_started = list.add(&dir.join("c.partial.mov"));

        let result = list.remove_all();
        assert_eq!(
            result,
            Removed {
                removed: 1,
                failed: vec![]
            }
        );
        assert!(!running.exists());
        assert!(finished.exists());
    }
}
