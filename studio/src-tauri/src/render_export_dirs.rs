//! The Render tab's export folders as Settings has them now (#1430): a
//! running render batch takes each change for the renders it starts after
//! it, instead of keeping the list it started with. Render While Capturing
//! keeps one render batch going for a whole capture batch, which can run for
//! days, so this is the only way to steer renders off a drive mid-batch.
//! "Keep free on the Windows drive" needs nothing here: every render reads
//! it as it picks its folder (`native::sys::windows_drive`).

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

/// The list Settings saved last, with how many times it has changed.
#[derive(Clone, Default)]
pub(crate) struct LiveExportDirs(Arc<RwLock<(u64, Vec<PathBuf>)>>);

impl LiveExportDirs {
    /// Settings were saved with `dirs`. Counts a change only when the list
    /// differs, blank entries left out. An emptied list is not taken: a
    /// running batch keeps the folders it has rather than renders failing
    /// for want of one (a new scan refuses without a folder anyway).
    pub fn saved(&self, dirs: &[String]) {
        let dirs: Vec<PathBuf> = dirs
            .iter()
            .filter(|d| !d.trim().is_empty())
            .map(PathBuf::from)
            .collect();
        if dirs.is_empty() {
            return;
        }
        let mut live = self.0.write().unwrap_or_else(|p| p.into_inner());
        if live.1 != dirs {
            live.0 += 1;
            live.1 = dirs;
        }
    }

    fn snapshot(&self) -> (u64, Vec<PathBuf>) {
        self.0.read().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// The list Settings saved last; empty before the first save.
    pub fn dirs(&self) -> Vec<PathBuf> {
        self.snapshot().1
    }

    /// Follows the list for one render batch that started with
    /// `started_with` (its payload's, which Settings had then).
    pub fn follow(&self, started_with: Vec<PathBuf>) -> ExportDirsInUse {
        ExportDirsInUse {
            live: self.clone(),
            seen: self.snapshot().0,
            dirs: started_with,
        }
    }
}

/// The export folders one render batch uses, following [`LiveExportDirs`].
pub(crate) struct ExportDirsInUse {
    live: LiveExportDirs,
    /// The change count last taken.
    seen: u64,
    dirs: Vec<PathBuf>,
}

impl ExportDirsInUse {
    /// The folders for renders starting now, and whether they differ from the
    /// ones before (for the activity log): a change saved since the batch
    /// started, or since the last look, is taken.
    pub fn current(&mut self) -> (&[PathBuf], bool) {
        let (count, dirs) = self.live.snapshot();
        let mut changed = false;
        if count != self.seen {
            self.seen = count;
            changed = dirs != self.dirs;
            self.dirs = dirs;
        }
        (&self.dirs, changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(list: &[&str]) -> Vec<PathBuf> {
        list.iter().map(PathBuf::from).collect()
    }

    /// #1430: removing a folder in Settings while a batch renders reaches
    /// the renders it starts next; before the change it keeps its own list.
    #[test]
    fn a_running_batch_takes_the_export_folders_saved_after_it_started() {
        let live = LiveExportDirs::default();
        live.saved(&["C:/clips".into(), "D:/clips".into()]);
        let mut batch = live.follow(paths(&["C:/clips", "D:/clips"]));
        assert_eq!(
            batch.current(),
            (&paths(&["C:/clips", "D:/clips"])[..], false)
        );

        live.saved(&["D:/clips".into(), "".into()]);
        assert_eq!(batch.current(), (&paths(&["D:/clips"])[..], true));
        // Said once: the next render starts with the same list, quietly.
        assert_eq!(batch.current(), (&paths(&["D:/clips"])[..], false));
    }

    /// A save of every other setting leaves the list, and the batch, alone;
    /// so does a save from before the batch started, even when the batch was
    /// given another list.
    #[test]
    fn only_a_change_after_the_batch_started_moves_it() {
        let live = LiveExportDirs::default();
        live.saved(&["E:/clips".into()]);
        let mut batch = live.follow(paths(&["F:/clips"]));
        live.saved(&["E:/clips".into()]);
        assert_eq!(batch.current(), (&paths(&["F:/clips"])[..], false));
    }

    #[test]
    fn an_emptied_list_keeps_the_folders_a_batch_has() {
        let live = LiveExportDirs::default();
        let mut batch = live.follow(paths(&["D:/clips"]));
        live.saved(&[]);
        live.saved(&["  ".into()]);
        assert_eq!(batch.current(), (&paths(&["D:/clips"])[..], false));
    }

    /// A change back to the list the batch already uses moves nothing and
    /// logs nothing.
    #[test]
    fn a_change_back_to_the_same_list_is_not_news() {
        let live = LiveExportDirs::default();
        live.saved(&["D:/clips".into()]);
        let mut batch = live.follow(paths(&["D:/clips"]));
        live.saved(&["E:/clips".into()]);
        live.saved(&["D:/clips".into()]);
        assert_eq!(batch.current(), (&paths(&["D:/clips"])[..], false));
    }
}
