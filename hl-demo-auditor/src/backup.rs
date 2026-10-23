//! Which loose demos an archive already holds a copy of (#929). An archive
//! is a backup: its copy never makes a loose demo deletable and never forms
//! a duplicate group; it only marks the loose demo as backed up.
//!
//! The matching is kept apart from the Duplicates grouping so #983 (the
//! Archive tab) can feed it from its own record of archived demos, for
//! archives that have left the PC, as well as from archives read on disk.

use crate::{ContentHashes, FileKey, ZipDemoEntry, ZipRef};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

/// One archive and the `.dem` entries it lists: which demos each archive
/// holds, including demos with no loose copy (#983's consolidation reads
/// it). An entry's `hashes` are set only once it was read in full.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchiveListing {
    pub archive: PathBuf,
    pub demos: Vec<ZipDemoEntry>,
}

/// A demo inside an archive whose whole content is known: the archive and
/// entry it is at, and the hashes reading it in full gave.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchivedCopy {
    pub at: ZipRef,
    pub hashes: ContentHashes,
}

/// A loose demo that one or more archives hold a copy of.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Backup {
    pub demo: PathBuf,
    /// Every archive entry holding it, sorted.
    pub copies: Vec<ZipRef>,
}

/// Archived copies by size and first 64 KiB (the loose demos' [`FileKey`]),
/// to look loose demos up in.
#[derive(Debug, Default)]
pub struct BackupIndex {
    by_key: HashMap<FileKey, Vec<ArchivedCopy>>,
    sizes: HashSet<u64>,
}

impl BackupIndex {
    pub fn new(copies: impl IntoIterator<Item = ArchivedCopy>) -> Self {
        let mut index = Self::default();
        for copy in copies {
            index.sizes.insert(copy.hashes.size);
            index
                .by_key
                .entry(FileKey {
                    size: copy.hashes.size,
                    header_hash: copy.hashes.head,
                })
                .or_default()
                .push(copy);
        }
        index
    }

    pub fn is_empty(&self) -> bool {
        self.by_key.is_empty()
    }

    /// Whether any archived copy has `size` bytes: a loose demo of another
    /// size can't be backed up here, so it needn't be opened.
    pub fn has_size(&self, size: u64) -> bool {
        self.sizes.contains(&size)
    }

    /// Whether any archived copy has `key`: only such a loose demo needs its
    /// last 64 KiB read.
    pub fn has_key(&self, key: &FileKey) -> bool {
        self.by_key.contains_key(key)
    }

    /// Every archived copy of a loose demo with `key` and last-64-KiB hash
    /// `tail`, sorted. They must match its size, first and last 64 KiB, as
    /// copies in a duplicate group do. The whole-file hash decides only when
    /// it is already known (`known_full`), or when the archived candidates
    /// themselves differ in full; then `read_full` gives the loose demo's
    /// (`None` when it can't be read: then nothing matches).
    pub fn copies_of(
        &self,
        key: &FileKey,
        tail: u64,
        known_full: Option<u64>,
        read_full: impl FnOnce() -> Option<u64>,
    ) -> Vec<ZipRef> {
        let candidates: Vec<&ArchivedCopy> = self
            .by_key
            .get(key)
            .into_iter()
            .flatten()
            .filter(|c| c.hashes.tail == tail)
            .collect();
        if candidates.is_empty() {
            return Vec::new();
        }
        let distinct: HashSet<u64> = candidates.iter().map(|c| c.hashes.full).collect();
        let full = match known_full {
            Some(full) => Some(full),
            None if distinct.len() > 1 => match read_full() {
                Some(full) => Some(full),
                None => return Vec::new(),
            },
            None => None,
        };
        let mut copies: Vec<ZipRef> = candidates
            .into_iter()
            .filter(|c| full.is_none_or(|f| c.hashes.full == f))
            .map(|c| c.at.clone())
            .collect();
        copies.sort();
        copies
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn copy(zip: &str, entry: &str, head: u64, tail: u64, full: u64) -> ArchivedCopy {
        ArchivedCopy {
            at: ZipRef {
                zip: PathBuf::from(zip),
                entry: entry.into(),
            },
            hashes: ContentHashes {
                size: 100,
                head,
                tail,
                full,
            },
        }
    }

    const KEY: FileKey = FileKey {
        size: 100,
        header_hash: 1,
    };

    fn names(copies: &[ZipRef]) -> Vec<String> {
        copies
            .iter()
            .map(|c| format!("{}:{}", c.zip.display(), c.entry))
            .collect()
    }

    #[test]
    fn every_archive_holding_the_demo_is_listed() {
        let index = BackupIndex::new([
            copy("b.zip", "m.dem", 1, 2, 3),
            copy("a.7z", "x/m.dem", 1, 2, 3),
            copy("c.rar", "m.dem", 1, 9, 3),
        ]);
        assert!(index.has_size(100) && !index.has_size(99));
        assert!(index.has_key(&KEY));
        let found = index.copies_of(&KEY, 2, None, || panic!("no read needed"));
        assert_eq!(names(&found), ["a.7z:x/m.dem", "b.zip:m.dem"]);
        assert!(index.copies_of(&KEY, 5, None, || None).is_empty());
    }

    #[test]
    fn a_known_whole_file_hash_must_match() {
        let index = BackupIndex::new([copy("a.zip", "m.dem", 1, 2, 3)]);
        assert_eq!(index.copies_of(&KEY, 2, Some(3), || None).len(), 1);
        assert!(index.copies_of(&KEY, 2, Some(4), || None).is_empty());
    }

    #[test]
    fn archived_copies_that_differ_in_full_make_the_loose_demo_read() {
        let index = BackupIndex::new([
            copy("a.zip", "m.dem", 1, 2, 3),
            copy("b.zip", "m.dem", 1, 2, 4),
        ]);
        assert_eq!(
            names(&index.copies_of(&KEY, 2, None, || Some(4))),
            ["b.zip:m.dem"]
        );
        assert!(index.copies_of(&KEY, 2, None, || None).is_empty());
    }
}
