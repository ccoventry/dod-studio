//! What the Duplicates audit remembers about each demo between scans (#888):
//! its key and tail hash, and its whole-file hash once the delete's
//! byte-for-byte check has read it. Keyed by the demo's canonical path and
//! valid while its size and modified time are unchanged, the way the
//! analyzer cache is, so a second scan opens only new or changed demos.
//!
//! Studio keeps it in `audit_keys.json` in the app data folder. It is only a
//! cache: a file that is missing, unreadable or from another version starts
//! it empty, and a failed save loses nothing but time.

use crate::{ContentHashes, Stamp};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

/// Bumped when [`KeyEntry`] changes meaning; an older file is dropped.
const VERSION: u32 = 1;

/// One demo as the audit last saw it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct KeyEntry {
    pub size: u64,
    pub modified_ns: u64,
    /// [`crate::FileKey::header_hash`].
    pub head: u64,
    /// [`crate::tail_hash`], once a group needed it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tail: Option<u64>,
    /// FNV-1a over the whole file, once something read all of it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full: Option<u64>,
}

impl KeyEntry {
    fn holds_for(&self, stamp: Stamp) -> bool {
        stamp.modified_ns != 0 && self.size == stamp.size && self.modified_ns == stamp.modified_ns
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct CacheFile {
    version: u32,
    entries: HashMap<String, KeyEntry>,
}

/// The remembered entries, plus what this run changed: [`KeyCache::save`]
/// writes only those changes over the file as it is then, so a delete that
/// saved while a scan ran keeps what it remembered.
#[derive(Debug, Default)]
pub struct KeyCache {
    entries: HashMap<String, KeyEntry>,
    changes: HashMap<String, Option<KeyEntry>>,
    seen: HashSet<String>,
}

/// One save at a time in this process; the file is replaced whole.
static SAVE_LOCK: Mutex<()> = Mutex::new(());

fn text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn read_entries(file: &Path) -> HashMap<String, KeyEntry> {
    std::fs::read(file)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<CacheFile>(&bytes).ok())
        .filter(|cache| cache.version == VERSION)
        .map(|cache| cache.entries)
        .unwrap_or_default()
}

impl KeyCache {
    /// The cache saved at `file`, or an empty one.
    pub fn load(file: &Path) -> Self {
        Self {
            entries: read_entries(file),
            ..Self::default()
        }
    }

    /// What is remembered about `path`, if it still holds for `stamp`.
    pub fn entry(&self, path: &Path, stamp: Stamp) -> Option<KeyEntry> {
        self.entries
            .get(&text(path))
            .filter(|e| e.holds_for(stamp))
            .copied()
    }

    fn set(&mut self, key: String, entry: KeyEntry) {
        if self.entries.get(&key) != Some(&entry) {
            self.entries.insert(key.clone(), entry);
            self.changes.insert(key, Some(entry));
        }
    }

    /// A fresh key for `path`: what was remembered under an older stamp goes.
    pub fn put(&mut self, path: &Path, stamp: Stamp, head: u64) {
        if stamp.modified_ns == 0 {
            return;
        }
        self.set(
            text(path),
            KeyEntry {
                size: stamp.size,
                modified_ns: stamp.modified_ns,
                head,
                tail: None,
                full: None,
            },
        );
    }

    /// `path`'s tail hash, beside a key that still holds.
    pub fn set_tail(&mut self, path: &Path, stamp: Stamp, tail: u64) {
        if let Some(entry) = self.entry(path, stamp) {
            self.set(
                text(path),
                KeyEntry {
                    tail: Some(tail),
                    ..entry
                },
            );
        }
    }

    /// Everything a read of all of `path` found, stamped as it was read.
    /// `path` is its canonical form, as the key pass names files.
    pub fn remember_contents(&mut self, path: &Path, stamp: Stamp, hashes: ContentHashes) {
        if stamp.modified_ns == 0 || stamp.size != hashes.size {
            return;
        }
        self.set(
            text(path),
            KeyEntry {
                size: stamp.size,
                modified_ns: stamp.modified_ns,
                head: hashes.head,
                tail: Some(hashes.tail),
                full: Some(hashes.full),
            },
        );
    }

    /// Drops `path`, e.g. once it has been deleted.
    pub fn forget(&mut self, path: &Path) {
        let key = text(path);
        self.entries.remove(&key);
        self.changes.insert(key, None);
    }

    /// Notes that this run's walk found `path`.
    pub fn mark_seen(&mut self, path: &Path) {
        self.seen.insert(text(path));
    }

    /// Drops every entry under one of `roots` that this run's walk did not
    /// find: it was deleted, moved or renamed. Call only after a walk that
    /// finished.
    pub fn prune_unseen_under(&mut self, roots: &[PathBuf]) {
        let roots: Vec<PathBuf> = roots
            .iter()
            .filter_map(|r| std::fs::canonicalize(r).ok())
            .collect();
        let gone: Vec<String> = self
            .entries
            .keys()
            .filter(|k| !self.seen.contains(*k))
            .filter(|k| roots.iter().any(|r| Path::new(k).starts_with(r)))
            .cloned()
            .collect();
        for key in gone {
            self.entries.remove(&key);
            self.changes.insert(key, None);
        }
    }

    pub fn has_changes(&self) -> bool {
        !self.changes.is_empty()
    }

    /// Writes this run's changes over `file` as it is now. A hash someone
    /// else saved for the same unchanged file is kept when this run did not
    /// learn it.
    pub fn save(&mut self, file: &Path) -> std::io::Result<()> {
        if self.changes.is_empty() {
            return Ok(());
        }
        let _one_at_a_time = SAVE_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
        let mut entries = read_entries(file);
        for (key, change) in self.changes.drain() {
            match change {
                Some(mut entry) => {
                    if let Some(old) = entries.get(&key)
                        && old.size == entry.size
                        && old.modified_ns == entry.modified_ns
                    {
                        entry.tail = entry.tail.or(old.tail);
                        entry.full = entry.full.or(old.full);
                    }
                    entries.insert(key, entry);
                }
                None => {
                    entries.remove(&key);
                }
            }
        }
        let json = serde_json::to_vec(&CacheFile {
            version: VERSION,
            entries,
        })
        .map_err(std::io::Error::other)?;
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = file.with_extension("json.tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, file)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STAMP: Stamp = Stamp {
        size: 10,
        modified_ns: 1_000,
    };

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("auditor_cache_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn an_entry_holds_only_for_the_same_size_and_modified_time() {
        let mut cache = KeyCache::default();
        let a = Path::new("a.dem");
        cache.put(a, STAMP, 7);
        assert_eq!(cache.entry(a, STAMP).map(|e| e.head), Some(7));
        let resized = Stamp { size: 11, ..STAMP };
        let touched = Stamp {
            modified_ns: 2_000,
            ..STAMP
        };
        assert_eq!(cache.entry(a, resized), None);
        assert_eq!(cache.entry(a, touched), None);
        assert_eq!(cache.entry(Path::new("b.dem"), STAMP), None);
        // A file the system gives no modified time is never remembered.
        cache.put(
            Path::new("c.dem"),
            Stamp {
                size: 1,
                modified_ns: 0,
            },
            1,
        );
        assert_eq!(
            cache.entry(
                Path::new("c.dem"),
                Stamp {
                    size: 1,
                    modified_ns: 0
                }
            ),
            None
        );
    }

    #[test]
    fn a_new_key_drops_the_old_stamps_hashes() {
        let mut cache = KeyCache::default();
        let a = Path::new("a.dem");
        let hashes = ContentHashes {
            size: 10,
            head: 1,
            tail: 2,
            full: 3,
        };
        cache.remember_contents(a, STAMP, hashes);
        assert_eq!(cache.entry(a, STAMP).and_then(|e| e.full), Some(3));
        let touched = Stamp {
            modified_ns: 2_000,
            ..STAMP
        };
        cache.put(a, touched, 9);
        let entry = cache.entry(a, touched).unwrap();
        assert_eq!((entry.head, entry.tail, entry.full), (9, None, None));
        // A tail for a stamp that no longer holds is not kept.
        cache.set_tail(a, STAMP, 5);
        assert_eq!(cache.entry(a, touched).unwrap().tail, None);
        cache.set_tail(a, touched, 5);
        assert_eq!(cache.entry(a, touched).unwrap().tail, Some(5));
    }

    #[test]
    fn it_survives_a_save_and_load_and_a_bad_file_starts_empty() {
        let dir = scratch("roundtrip");
        let file = dir.join("audit_keys.json");
        let mut cache = KeyCache::default();
        cache.put(Path::new(r"C:\demos\a.dem"), STAMP, 7);
        assert!(cache.has_changes());
        cache.save(&file).unwrap();
        assert!(!cache.has_changes());
        let loaded = KeyCache::load(&file);
        assert_eq!(
            loaded
                .entry(Path::new(r"C:\demos\a.dem"), STAMP)
                .map(|e| e.head),
            Some(7)
        );
        std::fs::write(&file, b"{not json").unwrap();
        assert!(KeyCache::load(&file).entries.is_empty());
        std::fs::write(&file, br#"{"version":999,"entries":{}}"#).unwrap();
        assert!(KeyCache::load(&file).entries.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_save_keeps_what_another_run_saved_meanwhile() {
        let dir = scratch("merge");
        let file = dir.join("audit_keys.json");
        let (a, b) = (Path::new("a.dem"), Path::new("b.dem"));
        // A scan loads the cache, then a delete remembers a's full hash.
        let mut scan = KeyCache::load(&file);
        let mut delete = KeyCache::load(&file);
        let hashes = ContentHashes {
            size: 10,
            head: 7,
            tail: 2,
            full: 3,
        };
        delete.remember_contents(a, STAMP, hashes);
        delete.save(&file).unwrap();
        // The scan saves its own key for a, without the full hash, and b.
        scan.put(a, STAMP, 7);
        scan.put(b, STAMP, 8);
        scan.save(&file).unwrap();
        let both = KeyCache::load(&file);
        assert_eq!(both.entry(a, STAMP).and_then(|e| e.full), Some(3));
        assert_eq!(both.entry(b, STAMP).map(|e| e.head), Some(8));
        // A forgotten file goes from the file too.
        let mut later = KeyCache::load(&file);
        later.forget(b);
        later.save(&file).unwrap();
        assert_eq!(KeyCache::load(&file).entry(b, STAMP), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn entries_under_a_walked_root_that_the_walk_missed_are_dropped() {
        let dir = std::fs::canonicalize(scratch("prune")).unwrap();
        let (here, gone) = (dir.join("here.dem"), dir.join("gone.dem"));
        let elsewhere = PathBuf::from(r"Z:\elsewhere\x.dem");
        let mut cache = KeyCache::default();
        for p in [&here, &gone, &elsewhere] {
            cache.put(p, STAMP, 1);
        }
        cache.mark_seen(&here);
        cache.prune_unseen_under(std::slice::from_ref(&dir));
        assert!(cache.entry(&here, STAMP).is_some());
        assert!(cache.entry(&gone, STAMP).is_none());
        assert!(cache.entry(&elsewhere, STAMP).is_some(), "not walked");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
