//! Where a render writes until it is finished (#1439).
//!
//! FFmpeg, or the Skip copy, writes `<name>.partial<ext>` beside the finished
//! file's name, and the file takes its real name only once the render ends
//! cleanly. A render that never finishes (Studio closed, crashed, or the PC
//! lost power) so never leaves a file under a finished clip's name. The
//! extension stays last because FFmpeg picks its muxer from it.
//!
//! The renders in flight are listed, and the list is also how a render
//! reserves its name: one is picked and listed under one lock, so two renders
//! of one clip name started together can't pick the same file. Studio's way
//! out removes the listed files after it has ended their FFmpegs.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

/// What marks an unfinished render's file name, before its extension.
pub const PARTIAL_MARK: &str = ".partial";

/// How long a remove or rename keeps trying while Windows still holds the
/// file: it lets go only once the process that had it open has fully exited.
const RETRY_FOR: Duration = Duration::from_secs(2);
const RETRY_EVERY: Duration = Duration::from_millis(50);

/// The name a render writes `out_file` under until it is finished:
/// `clip.mov` → `clip.partial.mov`.
pub fn partial_path(out_file: &Path) -> PathBuf {
    let stem = out_file.file_stem().unwrap_or_default().to_string_lossy();
    let name = match out_file.extension() {
        Some(ext) => format!("{stem}{PARTIAL_MARK}.{}", ext.to_string_lossy()),
        None => format!("{stem}{PARTIAL_MARK}"),
    };
    out_file.with_file_name(name)
}

/// The renders in flight in this Studio: each one's unfinished file.
#[derive(Default)]
pub struct InFlight {
    paths: Mutex<HashSet<PathBuf>>,
}

/// One render's reserved name: where it writes and where it ends up. Keeps
/// the name taken, and the unfinished file on the list, until dropped.
pub struct Reserved<'a> {
    list: &'a InFlight,
    pub out_file: PathBuf,
    pub partial: PathBuf,
}

impl Drop for Reserved<'_> {
    fn drop(&mut self) {
        self.list.lock().remove(&self.partial);
    }
}

/// How removing the unfinished files went.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Removed {
    /// The renders that were in flight.
    pub in_flight: usize,
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

    /// Reserves `<stem><ext>` in `dir`, or the first free `<stem>_N<ext>`. A
    /// name is taken when a file has it or a render in flight reserved it. A
    /// leftover partial file of a free name is from a render that never
    /// finished: the new render overwrites it, and the activity log says so.
    pub fn reserve(&self, dir: &Path, stem: &str, ext: &str) -> Reserved<'_> {
        let mut paths = self.lock();
        let mut counter = 1;
        let mut out_file = dir.join(format!("{stem}{ext}"));
        while out_file.exists() || paths.contains(&partial_path(&out_file)) {
            out_file = dir.join(format!("{stem}_{counter}{ext}"));
            counter += 1;
        }
        let partial = partial_path(&out_file);
        paths.insert(partial.clone());
        drop(paths);
        if partial.exists() {
            crate::log_markdown(&crate::messages::render::leftover_partial_replaced(
                partial.display(),
            ));
        }
        Reserved {
            list: self,
            out_file,
            partial,
        }
    }

    /// Gives a finished render its real name and returns where it is. When a
    /// file took the reserved name meanwhile (something outside Studio), the
    /// render gets the next free name instead: a finished clip is never
    /// overwritten. Retries for a moment while Windows still holds the file.
    pub fn finish(&self, reserved: &Reserved) -> std::io::Result<PathBuf> {
        let mut target = reserved.out_file.clone();
        // Held until the rename is done, so the fallback name stays ours.
        let mut fallback = None;
        if target.exists() {
            let dir = target.parent().unwrap_or(Path::new("")).to_path_buf();
            let stem = target
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let ext = target
                .extension()
                .map(|e| format!(".{}", e.to_string_lossy()))
                .unwrap_or_default();
            let other = self.reserve(&dir, &stem, &ext);
            target = other.out_file.clone();
            fallback = Some(other);
        }
        retrying(|| std::fs::rename(&reserved.partial, &target))?;
        drop(fallback);
        Ok(target)
    }

    /// Removes every listed file that is on disk, retrying the ones their
    /// just-ended process still holds until one deadline for all of them. A
    /// file already gone counts as neither removed nor failed.
    pub fn remove_all(&self) -> Removed {
        let mut left: Vec<PathBuf> = self.lock().iter().cloned().collect();
        let mut result = Removed {
            in_flight: left.len(),
            ..Removed::default()
        };
        let deadline = Instant::now() + RETRY_FOR;
        let mut errors: Vec<(PathBuf, String)> = Vec::new();
        loop {
            errors.clear();
            left.retain(|path| match std::fs::remove_file(path) {
                Ok(()) => {
                    result.removed += 1;
                    false
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
                Err(e) => {
                    errors.push((path.clone(), e.to_string()));
                    true
                }
            });
            if left.is_empty() || Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(RETRY_EVERY);
        }
        result.failed = errors;
        result
    }
}

/// Runs `op` until it succeeds or `RETRY_FOR` has passed, returning its last
/// error then. For a file a process that just ended may still hold. A file
/// that isn't there fails at once: waiting won't bring it back.
fn retrying(mut op: impl FnMut() -> std::io::Result<()>) -> std::io::Result<()> {
    let began = Instant::now();
    loop {
        match op() {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(e),
            Err(e) if began.elapsed() >= RETRY_FOR => return Err(e),
            Err(_) => std::thread::sleep(RETRY_EVERY),
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
    fn a_name_takes_the_first_free_suffix_past_files_on_disk() {
        let dir = Scratch::new("partial_reserve_disk");
        std::fs::write(dir.join("Kill.mov"), b"done").unwrap();
        let list = InFlight::default();
        let reserved = list.reserve(&dir, "Kill", ".mov");
        assert_eq!(reserved.out_file, dir.join("Kill_1.mov"));
        assert_eq!(reserved.partial, dir.join("Kill_1.partial.mov"));
    }

    /// Two renders of one clip name started together: the second must not
    /// pick the name the first holds, though neither has written yet.
    #[test]
    fn two_renders_of_one_name_get_two_files() {
        let dir = Scratch::new("partial_reserve_two");
        let list = InFlight::default();
        let first = list.reserve(&dir, "Kill", ".mov");
        let second = list.reserve(&dir, "Kill", ".mov");
        assert_eq!(first.out_file, dir.join("Kill.mov"));
        assert_eq!(second.out_file, dir.join("Kill_1.mov"));
        drop(first);
        // Given back once the first render ends without a file.
        assert_eq!(
            list.reserve(&dir, "Kill", ".mov").out_file,
            dir.join("Kill.mov")
        );
    }

    /// A partial file a crashed Studio left: its name is free again, not
    /// skipped for good.
    #[test]
    fn a_leftover_partial_file_does_not_take_its_name() {
        let dir = Scratch::new("partial_reserve_leftover");
        std::fs::write(dir.join("Kill.partial.mov"), b"half").unwrap();
        let list = InFlight::default();
        assert_eq!(
            list.reserve(&dir, "Kill", ".mov").out_file,
            dir.join("Kill.mov")
        );
    }

    #[test]
    fn a_finished_render_takes_its_real_name() {
        let dir = Scratch::new("partial_finish");
        let list = InFlight::default();
        let reserved = list.reserve(&dir, "clip", ".mov");
        std::fs::write(&reserved.partial, b"frames").unwrap();
        assert_eq!(list.finish(&reserved).unwrap(), dir.join("clip.mov"));
        assert_eq!(std::fs::read(dir.join("clip.mov")).unwrap(), b"frames");
        assert!(!reserved.partial.exists());
    }

    #[test]
    fn a_finished_render_never_overwrites_a_clip_that_took_its_name() {
        let dir = Scratch::new("partial_taken");
        let list = InFlight::default();
        let reserved = list.reserve(&dir, "clip", ".mov");
        std::fs::write(&reserved.partial, b"frames").unwrap();
        std::fs::write(dir.join("clip.mov"), b"earlier clip").unwrap();
        let done = list.finish(&reserved).unwrap();
        assert_eq!(done, dir.join("clip_1.mov"));
        assert_eq!(
            std::fs::read(dir.join("clip.mov")).unwrap(),
            b"earlier clip"
        );
        assert_eq!(std::fs::read(&done).unwrap(), b"frames");
    }

    #[test]
    fn removing_in_flight_files_leaves_finished_ones() {
        let dir = Scratch::new("partial_in_flight");
        let list = InFlight::default();
        let running = list.reserve(&dir, "a", ".mov");
        std::fs::write(&running.partial, b"half").unwrap();
        std::fs::write(dir.join("b.mov"), b"done").unwrap();
        // A render that finished and was renamed: its entry is gone with it.
        drop(list.reserve(&dir, "b_done", ".mov"));
        // A render listed whose file is not on disk yet.
        let _not_started = list.reserve(&dir, "c", ".mov");

        let result = list.remove_all();
        assert_eq!(
            result,
            Removed {
                in_flight: 2,
                removed: 1,
                failed: vec![]
            }
        );
        assert!(!running.partial.exists());
        assert!(dir.join("b.mov").exists());
    }
}
