//! What a batch has made and started before its game is up, undone however
//! the batch stops short of the launch (#1212): the demos copied into
//! `dod/`, the `dod_pool_N` folder links, and the marker readers.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Owns everything a batch has made so far, from before its first link or
/// copy. Dropped before [`Leftovers::launched`], it removes the copies and
/// links and stops the readers, so a batch that ends early (a failed copy,
/// too little room, OBS refusing, a launch that fails) leaves the game
/// folder as it found it. Once the game is launched, the batch's own
/// teardown (`send_batch_end`) decides what stays; the readers still stop
/// when this drops.
#[derive(Default)]
pub(super) struct Leftovers {
    demo_copies: Vec<PathBuf>,
    pool_junctions: Vec<PathBuf>,
    /// The readers' stop flag, once they run.
    readers: Option<Arc<AtomicBool>>,
    launched: bool,
}

impl Leftovers {
    /// A demo copied into `dod/`.
    pub(super) fn add_demo_copy(&mut self, path: PathBuf) {
        self.demo_copies.push(path);
    }

    /// A `dod_pool_N` junction made beside hl.exe.
    pub(super) fn add_pool_junction(&mut self, path: PathBuf) {
        self.pool_junctions.push(path);
    }

    /// The marker readers run, and `stop` stops them.
    pub(super) fn add_readers(&mut self, stop: &Arc<AtomicBool>) {
        self.readers = Some(Arc::clone(stop));
    }

    /// The demos copied into `dod/` so far.
    pub(super) fn demo_copies(&self) -> &[PathBuf] {
        &self.demo_copies
    }

    /// The `dod_pool_N` junctions made so far.
    pub(super) fn pool_junctions(&self) -> &[PathBuf] {
        &self.pool_junctions
    }

    /// The game is launched: the copies and links are the teardown's now.
    pub(super) fn launched(&mut self) {
        self.launched = true;
    }
}

impl Drop for Leftovers {
    fn drop(&mut self) {
        if let Some(stop) = &self.readers {
            stop.store(true, Ordering::Relaxed);
        }
        if self.launched {
            return;
        }
        super::workspace::remove_demo_copies(&self.demo_copies);
        for junction in &self.pool_junctions {
            remove_junction(junction);
        }
    }
}

/// Unlinks `junction` without touching what it points at; one already gone
/// is fine, anything else goes in the activity log.
fn remove_junction(junction: &Path) {
    if let Err(e) = std::fs::remove_dir(junction)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        crate::log_markdown(&format!(
            "[cleanup] Failed to remove pool junction {}: {}",
            junction.display(),
            e
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Scratch;

    /// What a batch made before it stopped short of the launch.
    fn made_so_far(root: &Scratch) -> (Leftovers, PathBuf, PathBuf, Arc<AtomicBool>) {
        let copy = root.join("dodstudio_chain_01.dem");
        std::fs::write(&copy, b"x").unwrap();
        let pool = root.join("dod_pool_0");
        std::fs::create_dir_all(&pool).unwrap();
        let readers = Arc::new(AtomicBool::new(false));
        let mut leftovers = Leftovers::default();
        leftovers.add_demo_copy(copy.clone());
        leftovers.add_pool_junction(pool.clone());
        leftovers.add_readers(&readers);
        (leftovers, copy, pool, readers)
    }

    /// An early return removes the copies and links and stops the readers.
    #[test]
    fn a_batch_that_stops_before_the_launch_leaves_nothing_behind() {
        let root = Scratch::new("leftovers_early");
        let (leftovers, copy, pool, readers) = made_so_far(&root);
        drop(leftovers);
        assert!(!copy.exists());
        assert!(!pool.exists());
        assert!(readers.load(Ordering::Relaxed));
    }

    /// Once the game is launched the copies and links are the teardown's;
    /// the readers still stop.
    #[test]
    fn a_launched_batch_keeps_its_copies_and_links_for_the_teardown() {
        let root = Scratch::new("leftovers_launched");
        let (mut leftovers, copy, pool, readers) = made_so_far(&root);
        leftovers.launched();
        drop(leftovers);
        assert!(copy.exists());
        assert!(pool.exists());
        assert!(readers.load(Ordering::Relaxed));
    }

    /// Nothing made yet, or a link already gone, is not a failure.
    #[test]
    fn nothing_to_undo_is_fine() {
        drop(Leftovers::default());
        let root = Scratch::new("leftovers_gone");
        let mut leftovers = Leftovers::default();
        leftovers.add_pool_junction(root.join("dod_pool_9"));
        drop(leftovers);
    }
}
