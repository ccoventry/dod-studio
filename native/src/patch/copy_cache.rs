//! The folders of demo copies Studio plays in place of a demo the game can't
//! play as it is: trimmed copies (#207, `trimmed_copies`) and protocol-48
//! copies of protocol-47 demos (#1549, `protocol48_copies`).
//!
//! Each copy lives in a folder of its own under its cache's root, named for
//! the demo it was made from, with a [`USED_MARKER`] stamped beside it every
//! time it is played. [`prune`] keeps the most recently used folders, and a
//! copy a running batch plays is never removed, however many it plays
//! ([`Pins`], shared by every cache). A copy is written beside its final name
//! and renamed over ([`save_copy`]), so one that exists is always whole. A
//! new copy gets the original's analyzer cache entry ([`seed_analysis`]), so
//! the in-game DoD Studio window lists its highlights at once.

use lock_recover::lock_or_recover;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

/// Written beside each copy every time it is used, and when its folder is
/// made, for [`prune`].
pub const USED_MARKER: &str = "last_used";

/// How long after its last use a copy is never pruned, whatever the count:
/// another Studio (whose pins this one can't see) may have just handed it to
/// a game, or still be writing it.
pub const RECENT_USE: std::time::Duration = std::time::Duration::from_secs(15 * 60);

/// Copy folders that running batches play, one entry per [`Pins`] holding
/// one. [`prune`] never removes them.
static PINNED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

fn pinned() -> MutexGuard<'static, Vec<PathBuf>> {
    // The list stays whole if a holder panicked: each push or remove is one call.
    lock_or_recover(&PINNED)
}

/// Keeps copies from being pruned while it lives. A batch holds one from the
/// moment each of its copies is ready until its game is done with them, so a
/// Launch Preview started meanwhile can't remove a copy the batch has yet to
/// play, however many it plays.
#[derive(Debug, Default)]
pub struct Pins(Vec<PathBuf>);

impl Pins {
    /// Pins `copy`'s folder until this is dropped.
    pub fn add(&mut self, copy: &Path) {
        if let Some(dir) = copy.parent() {
            pinned().push(dir.to_path_buf());
            self.0.push(dir.to_path_buf());
        }
    }
}

impl Drop for Pins {
    fn drop(&mut self) {
        let mut all = pinned();
        for dir in self.0.drain(..) {
            if let Some(at) = all.iter().position(|d| *d == dir) {
                all.swap_remove(at);
            }
        }
    }
}

/// Writes the copy beside its final name and renames it over, so a copy
/// that exists is always whole. A preview and a batch can make the same copy
/// at once: each writes its own partial file, and the one that renames second
/// takes the first one's copy (both write the same bytes). `Ok(false)` when
/// the rename couldn't replace a copy already there (the game has it open),
/// so that copy, not `bytes`, is what plays. `Err` when the folder or the
/// partial file can't be written, or the rename fails with no copy there.
pub fn save_copy(path: &Path, bytes: &[u8]) -> std::io::Result<bool> {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
        // Stamped before the slow write, so a prune meanwhile sees a folder
        // in use rather than one never used.
        mark_used(path);
    }
    let partial = path.with_extension(format!(
        "dem.{}-{}.part",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    // A write that fails partway (a full disk) leaves no partial file behind.
    if let Err(e) = std::fs::write(&partial, bytes) {
        let _ = std::fs::remove_file(&partial);
        return Err(e);
    }
    match std::fs::rename(&partial, path) {
        Ok(()) => Ok(true),
        Err(e) => {
            let _ = std::fs::remove_file(&partial);
            if path.is_file() { Ok(false) } else { Err(e) }
        }
    }
}

/// Gives the copy the original's analyzer cache entry under `root` (see the
/// module doc), with `peak_after` as the copy's peak (`None`: not known).
/// Best-effort: without it, the game asks Studio to read the copy.
pub fn seed_analysis(root: &Path, original: &Path, copy: &Path, peak_after: Option<u32>) {
    let Some((file_info, mut analysis)) = analysis::cache::load(root, original) else {
        return;
    };
    analysis.demo_info.peak_packet_entities = peak_after;
    let _ = analysis::cache::store(root, copy, &file_info, &analysis);
}

/// Stamps the copy's folder as just used. Best-effort: a copy whose stamp
/// couldn't be written is only removed sooner.
pub fn mark_used(copy: &Path) {
    if let Some(dir) = copy.parent() {
        let _ = std::fs::write(dir.join(USED_MARKER), b"");
    }
}

/// Removes all but the `keep` most recently used copy folders under `root`,
/// leaving any a running batch has pinned ([`Pins`]) and any used within
/// [`RECENT_USE`], whatever their number. A folder with no stamp yet counts
/// as used when it was made. Returns how many it removed.
pub fn prune(root: &Path, keep: usize) -> usize {
    let Ok(entries) = std::fs::read_dir(root) else {
        return 0;
    };
    let folders: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .map(|p| {
            let used = std::fs::metadata(p.join(USED_MARKER))
                .or_else(|_| std::fs::metadata(&p))
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            (used, p)
        })
        .collect();
    let pinned = pinned().clone();
    let recent = std::time::SystemTime::now()
        .checked_sub(RECENT_USE)
        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
    folders_to_prune(folders, keep, &pinned, recent)
        .iter()
        .filter(|p| std::fs::remove_dir_all(p).is_ok())
        .count()
}

/// [`prune`]'s rule: of `folders` (each with when it was last used), the ones
/// past the `keep` newest that aren't in `pinned` and weren't used after
/// `recent`.
fn folders_to_prune(
    mut folders: Vec<(std::time::SystemTime, PathBuf)>,
    keep: usize,
    pinned: &[PathBuf],
    recent: std::time::SystemTime,
) -> Vec<PathBuf> {
    folders.sort_by_key(|(used, _)| std::cmp::Reverse(*used));
    folders
        .into_iter()
        .skip(keep)
        .filter(|(used, p)| *used <= recent && !pinned.contains(p))
        .map(|(_, p)| p)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Scratch;

    #[test]
    fn prune_keeps_the_most_recently_used_copies() {
        let dir = Scratch::new("trimmed_copy_prune");
        for (i, name) in ["old", "middle", "new"].iter().enumerate() {
            let folder = dir.join(name);
            std::fs::create_dir_all(&folder).unwrap();
            let marker = std::fs::File::create(folder.join(USED_MARKER)).unwrap();
            let when = std::time::SystemTime::UNIX_EPOCH
                + std::time::Duration::from_secs(1_000_000 + i as u64 * 1000);
            marker.set_modified(when).unwrap();
        }
        assert_eq!(prune(&dir, 2), 1);
        assert!(!dir.join("old").exists());
        assert!(dir.join("middle").exists() && dir.join("new").exists());
    }

    #[test]
    fn a_copy_saved_twice_is_whole_and_leaves_no_partial_file() {
        let dir = Scratch::new("trimmed_copy_save");
        let path = dir.join("key").join("a_trimmed.dem");
        save_copy(&path, b"HLDEMO copy").unwrap();
        save_copy(&path, b"HLDEMO copy").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"HLDEMO copy");
        let mut names: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        names.sort();
        assert_eq!(
            names,
            vec![
                std::ffi::OsString::from("a_trimmed.dem"),
                std::ffi::OsString::from(USED_MARKER)
            ]
        );
    }

    /// `count` copy folders under `dir`, `copy0` the oldest used, each with a
    /// copy file; returns the copies' paths.
    fn copies_used_in_order(dir: &Path, count: usize) -> Vec<PathBuf> {
        (0..count)
            .map(|i| {
                let folder = dir.join(format!("copy{i}"));
                std::fs::create_dir_all(&folder).unwrap();
                let marker = std::fs::File::create(folder.join(USED_MARKER)).unwrap();
                let when = std::time::SystemTime::UNIX_EPOCH
                    + std::time::Duration::from_secs(1_000_000 + i as u64 * 1000);
                marker.set_modified(when).unwrap();
                folder.join("a_trimmed.dem")
            })
            .collect()
    }

    #[test]
    fn a_preview_during_a_batch_of_more_copies_than_are_kept_removes_none_of_the_batchs() {
        const KEEP: usize = 8;
        let dir = Scratch::new("trimmed_copy_prune_batch");
        let copies = copies_used_in_order(&dir, KEEP + 2);
        let mut batch = Pins::default();
        for copy in &copies {
            batch.add(copy);
        }
        assert_eq!(prune(&dir, KEEP), 0, "the batch's copies are pinned");
        assert!(copies.iter().all(|c| c.parent().unwrap().is_dir()));
        drop(batch);
        assert_eq!(prune(&dir, KEEP), 2, "once it ends they prune");
        assert!(!dir.join("copy0").exists() && !dir.join("copy1").exists());
    }

    #[test]
    fn a_copy_pinned_twice_stays_pinned_until_both_let_go() {
        let dir = Scratch::new("trimmed_copy_pin_twice");
        let copy = &copies_used_in_order(&dir, 1)[0];
        let mut first = Pins::default();
        let mut second = Pins::default();
        first.add(copy);
        second.add(copy);
        drop(first);
        assert_eq!(prune(&dir, 0), 0);
        drop(second);
        assert_eq!(prune(&dir, 0), 1);
    }

    #[test]
    fn prune_skips_pinned_folders_past_the_newest() {
        let at = |s: u64| std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(s);
        let folders = vec![
            (at(1), PathBuf::from("old")),
            (at(3), PathBuf::from("new")),
            (at(2), PathBuf::from("pinned")),
        ];
        assert_eq!(
            folders_to_prune(folders, 1, &[PathBuf::from("pinned")], at(100)),
            vec![PathBuf::from("old")]
        );
    }

    #[test]
    fn a_folder_used_recently_is_never_pruned_whatever_the_count() {
        let at = |s: u64| std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(s);
        let folders = vec![
            (at(1), PathBuf::from("old")),
            (at(50), PathBuf::from("just used by another Studio")),
            (at(60), PathBuf::from("newest")),
        ];
        assert_eq!(
            folders_to_prune(folders, 1, &[], at(10)),
            vec![PathBuf::from("old")]
        );
    }

    #[test]
    fn a_copy_folder_being_made_is_not_pruned() {
        let dir = Scratch::new("copy_cache_making");
        // A folder made a moment ago with no stamp yet counts as just used.
        std::fs::create_dir_all(dir.join("bare")).unwrap();
        assert_eq!(prune(&dir, 0), 0);
        assert!(dir.join("bare").is_dir());
    }

    #[test]
    fn copies_saved_while_another_thread_prunes_all_survive() {
        let dir = Scratch::new("copy_cache_concurrent");
        let root = dir.path().to_path_buf();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let pruner = {
            let (root, stop) = (root.clone(), stop.clone());
            std::thread::spawn(move || {
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    prune(&root, 0);
                }
            })
        };
        let copies: Vec<PathBuf> = (0..20)
            .map(|i| root.join(format!("key{i}")).join("a_p48.dem"))
            .collect();
        for copy in &copies {
            save_copy(copy, b"HLDEMO copy").unwrap();
        }
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        pruner.join().unwrap();
        assert!(copies.iter().all(|c| c.is_file()));
    }

    #[test]
    fn a_saved_copy_is_stamped_used() {
        let dir = Scratch::new("copy_cache_stamp");
        let copy = dir.join("key").join("a_p48.dem");
        save_copy(&copy, b"HLDEMO").unwrap();
        assert!(dir.join("key").join(USED_MARKER).is_file());
    }

    #[test]
    fn a_copy_gets_the_originals_cache_entry_under_the_originals_name() {
        let dir = Scratch::new("trimmed_copy_seed");
        let root = dir.join("cache");
        let original = dir.join("lennon2_hltv.dem");
        let copy = dir.join("lennon2_hltv_trimmed.dem");
        std::fs::write(&original, b"HLDEMO\0\0 original").unwrap();
        std::fs::write(&copy, b"HLDEMO\0\0 copy").unwrap();
        let mut analysis = analysis::Analysis::default();
        analysis.demo_info.peak_packet_entities = Some(300);
        let info = analysis::cache::FileInfo::of(&original).unwrap();
        analysis::cache::store(&root, &original, &info, &analysis).unwrap();

        seed_analysis(&root, &original, &copy, Some(246));

        let (seeded_info, seeded) = analysis::cache::load(&root, &copy).expect("seeded");
        assert_eq!(seeded.demo_info.peak_packet_entities, Some(246));
        assert_eq!(seeded_info.path, info.path, "listed once, by the original");
    }

    #[test]
    fn a_copy_of_a_demo_with_no_cache_entry_gets_none() {
        let dir = Scratch::new("trimmed_copy_seed_none");
        let copy = dir.join("copy.dem");
        std::fs::write(&copy, b"HLDEMO").unwrap();
        seed_analysis(
            &dir.join("cache"),
            &dir.join("missing.dem"),
            &copy,
            Some(246),
        );
        assert!(analysis::cache::load(&dir.join("cache"), &copy).is_none());
    }
}
