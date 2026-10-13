//! The one folder walk for every scan in the app (#376).
//!
//! A junction inside a folder being walked can point back above it: a demo
//! folder linked into the game folder (`demo_links`) is refused when it would
//! do that, but a user can make such a link by hand, and a symlink can do
//! the same. A walk that followed it would never end. So every walk here:
//!
//! - **skips reparse points** (junctions and symlinks), except the root it
//!   was asked to open, which is read through whatever it is;
//! - **stops at a depth cap** ([`Walker::max_depth`]), and at an entry
//!   budget when the caller sets one, so a walk pointed at a drive root
//!   cannot turn into a hang.
//!
//! The entry's kind comes from the directory listing itself
//! (`DirEntry::file_type`, which never follows a link), so a junction is
//! never mistaken for a folder. `walkdir`, which the take scanner still
//! uses, does the same with `follow_links` off; `walkdir_does_not_follow_a_junction`
//! below keeps that checked.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

/// What an entry is, as the listing reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A real folder; the walk goes into it unless the visitor says not to.
    Dir,
    File,
    /// A reparse point (junction or symlink). Reported, never entered.
    Link,
}

/// One entry the walk found.
#[derive(Debug)]
pub struct Found {
    pub path: PathBuf,
    /// 1 for an entry directly in the root.
    pub depth: usize,
    pub kind: Kind,
}

impl Found {
    pub fn is_demo(&self) -> bool {
        self.kind == Kind::File
            && self
                .path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("dem"))
    }

    pub fn file_name(&self) -> std::borrow::Cow<'_, str> {
        self.path.file_name().unwrap_or_default().to_string_lossy()
    }
}

/// How a walk ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Outcome {
    /// False when the entry budget stopped it short.
    pub complete: bool,
    /// Entries read.
    pub entries: usize,
}

/// A bounded walk. Depth-first unless [`Walker::breadth_first`] is set.
#[derive(Debug, Clone)]
pub struct Walker {
    max_depth: usize,
    max_entries: usize,
    breadth_first: bool,
}

impl Default for Walker {
    fn default() -> Self {
        Self::new()
    }
}

impl Walker {
    /// Deeper than any demo library; shallow enough that a hand-made loop
    /// through a symlink the listing cannot see ends in a moment.
    pub const DEFAULT_MAX_DEPTH: usize = 32;

    pub fn new() -> Self {
        Self {
            max_depth: Self::DEFAULT_MAX_DEPTH,
            max_entries: usize::MAX,
            breadth_first: false,
        }
    }

    /// Entries at most this deep are reported (1 = the root's own entries),
    /// and no folder at that depth is entered.
    pub fn max_depth(mut self, depth: usize) -> Self {
        self.max_depth = depth;
        self
    }

    /// Stop after reading this many entries; the outcome says so.
    pub fn max_entries(mut self, entries: usize) -> Self {
        self.max_entries = entries;
        self
    }

    /// Every folder's own entries before any subfolder's.
    pub fn breadth_first(mut self) -> Self {
        self.breadth_first = true;
        self
    }

    /// Calls `visit` for every entry under `root`. For a [`Kind::Dir`] the
    /// return value says whether to go into it; for anything else it is
    /// ignored. `root` itself is read through even when it is a link: it is
    /// the folder the caller asked for.
    pub fn walk(&self, root: &Path, visit: &mut dyn FnMut(&Found) -> bool) -> Outcome {
        let mut pending: VecDeque<(PathBuf, usize)> = VecDeque::from([(root.to_path_buf(), 0)]);
        let mut entries = 0usize;
        while let Some((dir, depth)) = if self.breadth_first {
            pending.pop_front()
        } else {
            pending.pop_back()
        } {
            let Ok(listing) = std::fs::read_dir(&dir) else {
                continue;
            };
            // Depth-first keeps a listing's order by pushing its folders in
            // reverse; breadth-first appends them as they come.
            let mut found_dirs = Vec::new();
            for entry in listing.flatten() {
                entries += 1;
                if entries > self.max_entries {
                    return Outcome {
                        complete: false,
                        entries: entries - 1,
                    };
                }
                let Ok(file_type) = entry.file_type() else {
                    continue;
                };
                let kind = if file_type.is_symlink() {
                    Kind::Link
                } else if file_type.is_dir() {
                    Kind::Dir
                } else if file_type.is_file() {
                    Kind::File
                } else {
                    continue;
                };
                let found = Found {
                    path: entry.path(),
                    depth: depth + 1,
                    kind,
                };
                let enter = visit(&found);
                if kind == Kind::Dir && enter && found.depth < self.max_depth {
                    found_dirs.push((found.path, found.depth));
                }
            }
            if self.breadth_first {
                pending.extend(found_dirs);
            } else {
                pending.extend(found_dirs.into_iter().rev());
            }
        }
        Outcome {
            complete: true,
            entries,
        }
    }

    /// Every `.dem` file under `root`, sorted by path.
    pub fn demos(&self, root: &Path) -> Vec<PathBuf> {
        let mut demos = Vec::new();
        self.walk(root, &mut |found| {
            if found.is_demo() {
                demos.push(found.path.clone());
            }
            true
        });
        demos.sort();
        demos
    }

    /// Every file under `root`, in walk order.
    pub fn files(&self, root: &Path) -> Vec<PathBuf> {
        let mut files = Vec::new();
        self.walk(root, &mut |found| {
            if found.kind == Kind::File {
                files.push(found.path.clone());
            }
            true
        });
        files
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("dodstudio_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("sub/deeper")).unwrap();
        for f in ["a.dem", "notes.txt", "sub/b.DEM", "sub/deeper/c.dem"] {
            std::fs::write(root.join(f), b"").unwrap();
        }
        root
    }

    fn relative(root: &Path, paths: &[PathBuf]) -> Vec<String> {
        paths
            .iter()
            .map(|p| {
                p.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect()
    }

    #[test]
    fn finds_every_demo_at_any_depth() {
        let root = scratch("walk_demos");
        let demos = Walker::new().demos(&root);
        assert_eq!(
            relative(&root, &demos),
            ["a.dem", "sub/b.DEM", "sub/deeper/c.dem"]
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_depth_cap_holds() {
        let root = scratch("walk_depth");
        let demos = Walker::new().max_depth(1).demos(&root);
        assert_eq!(relative(&root, &demos), ["a.dem"]);
        let demos = Walker::new().max_depth(2).demos(&root);
        assert_eq!(relative(&root, &demos), ["a.dem", "sub/b.DEM"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_entry_budget_stops_the_walk_and_says_so() {
        let root = scratch("walk_budget");
        let mut seen = 0;
        let outcome = Walker::new().max_entries(2).walk(&root, &mut |_| {
            seen += 1;
            true
        });
        assert_eq!(seen, 2);
        assert_eq!(
            outcome,
            Outcome {
                complete: false,
                entries: 2
            }
        );
        let outcome = Walker::new().walk(&root, &mut |_| true);
        assert!(outcome.complete);
        assert_eq!(outcome.entries, 6, "two folders, four files");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_visitor_can_keep_a_folder_out() {
        let root = scratch("walk_skip");
        let mut demos = Vec::new();
        Walker::new().walk(&root, &mut |found| {
            if found.is_demo() {
                demos.push(found.path.clone());
            }
            found.file_name() != "deeper"
        });
        demos.sort();
        assert_eq!(relative(&root, &demos), ["a.dem", "sub/b.DEM"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn breadth_first_lists_a_folder_before_its_subfolders() {
        let root = scratch("walk_bfs");
        std::fs::create_dir_all(root.join("aaa")).unwrap();
        std::fs::write(root.join("aaa/x.dem"), b"").unwrap();
        let mut order = Vec::new();
        Walker::new().breadth_first().walk(&root, &mut |found| {
            if found.is_demo() {
                order.push(found.path.clone());
            }
            true
        });
        let order = relative(&root, &order);
        assert_eq!(order[0], "a.dem");
        assert_eq!(order.last().unwrap(), "sub/deeper/c.dem");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_missing_root_finds_nothing() {
        let root = std::env::temp_dir().join("dodstudio_walk_missing_never_made");
        assert!(Walker::new().demos(&root).is_empty());
    }

    /// `loop` → its own grandparent: a walk that followed it would not end.
    #[cfg(windows)]
    fn scratch_with_loop(name: &str) -> PathBuf {
        let root = scratch(name);
        crate::sys::junction::create(&root.join("sub/loop"), &root).unwrap();
        root
    }

    #[cfg(windows)]
    #[test]
    fn a_junction_to_its_own_parent_is_reported_and_not_entered() {
        let root = scratch_with_loop("walk_loop");
        let mut links = Vec::new();
        let mut demos = Vec::new();
        let outcome = Walker::new().walk(&root, &mut |found| {
            match found.kind {
                Kind::Link => links.push(found.path.clone()),
                Kind::File if found.is_demo() => demos.push(found.path.clone()),
                _ => {}
            }
            true
        });
        assert!(outcome.complete);
        assert_eq!(relative(&root, &links), ["sub/loop"]);
        demos.sort();
        assert_eq!(
            relative(&root, &demos),
            ["a.dem", "sub/b.DEM", "sub/deeper/c.dem"]
        );
        let _ = crate::sys::junction::remove(&root.join("sub/loop"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(windows)]
    #[test]
    fn the_root_itself_is_read_through_when_it_is_a_junction() {
        let root = scratch_with_loop("walk_loop_root");
        let link = root.join("sub/loop");
        let demos = Walker::new().demos(&link);
        assert_eq!(
            relative(&link, &demos),
            ["a.dem", "sub/b.DEM", "sub/deeper/c.dem"],
            "the link's own contents once, and the loop inside it skipped"
        );
        let _ = crate::sys::junction::remove(&link);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The take scanner (`hlcr::scanner`) walks with `walkdir`; its listing
    /// reports a junction as a symlink, so with `follow_links` off it is not
    /// entered either.
    #[cfg(windows)]
    #[test]
    fn walkdir_does_not_follow_a_junction() {
        let root = scratch_with_loop("walk_walkdir");
        let entries: Vec<_> = walkdir::WalkDir::new(&root)
            .follow_links(false)
            .into_iter()
            .flatten()
            .collect();
        let link = entries
            .iter()
            .find(|e| e.file_name() == "loop")
            .expect("the junction is listed");
        assert!(link.file_type().is_symlink());
        assert!(!link.file_type().is_dir());
        let paths: Vec<PathBuf> = entries.iter().map(|e| e.path().to_path_buf()).collect();
        assert_eq!(
            paths.iter().filter(|p| p.ends_with("a.dem")).count(),
            1,
            "{paths:?}"
        );
        let _ = crate::sys::junction::remove(&root.join("sub/loop"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
